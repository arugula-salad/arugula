//! `~/.local/bin` on the user's PATH (#883), where `arugulad install` and
//! the desktop app put `arugula` and `arugulad`. Stock macOS zsh doesn't
//! have it, nor do some Linux profiles, so a new terminal said `command not
//! found: arugula`. Like rustup, pipx and uv, the installer adds one marked
//! line to the login shell's startup file, once:
//!
//! - **Asks the login shell.** It runs the user's shell as a login shell,
//!   with a clean environment (a GUI app's or a pane's PATH says nothing
//!   about a new Terminal window's), and reads the PATH it ends up with.
//!   When that has the directory, nothing is written, but for the rc file
//!   on Linux (`~/.bashrc`, `~/.zshrc`): a terminal there has the desktop
//!   session's PATH, which `~/.profile` set before the directory existed
//!   (Ubuntu's adds it only if it does), until the next login.
//! - **One line, marked, once.** zsh: `~/.zprofile` on macOS (Terminal's
//!   shells are login shells), `~/.zshrc` elsewhere (terminals' aren't).
//!   bash: the login file it reads (`~/.bash_profile`, `~/.bash_login`, else
//!   `~/.profile`), and on Linux `~/.bashrc` too. fish:
//!   `conf.d/arugula.fish`. A file with the mark is left alone, and so is
//!   one that already names `.local/bin` (the user's own line). The line
//!   itself adds the directory only when PATH lacks it.
//! - **No sudo**: no `/usr/local/bin`, no `/etc/paths.d`. Other shells get
//!   the line to add printed instead.
//!
//! The binaries stay on `arugulad uninstall` (it removes only the service),
//! so the line does too.
//!
//! Like `service.rs`, this looks at the machine; the desktop app, the daemon
//! (the welcome card's command) and `arugulad install` share it.

use std::{
    fs,
    io::{self, Read},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    time::{Duration, Instant},
};

/// The comment that marks our line (and our fish file).
pub const MARK: &str = "# added by Arugula (arugula.io): ~/.local/bin on PATH";

/// How long the login shell may take to print its PATH.
const TIMEOUT: Duration = Duration::from_secs(5);

/// The shells we know how to set PATH for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Zsh,
    Bash,
    Fish,
}

impl Kind {
    pub fn of(shell: &Path) -> Option<Kind> {
        match shell.file_name()?.to_str()?.trim_start_matches('-') {
            "zsh" => Some(Kind::Zsh),
            "bash" => Some(Kind::Bash),
            "fish" => Some(Kind::Fish),
            _ => None,
        }
    }

    /// The line, with the mark.
    pub fn line(self) -> String {
        match self {
            Kind::Fish => {
                format!("if not contains -- $HOME/.local/bin $PATH; set -gx PATH $HOME/.local/bin $PATH; end  {MARK}")
            }
            _ => format!(
                "case \":$PATH:\" in *\":$HOME/.local/bin:\"*) ;; *) export PATH=\"$HOME/.local/bin:$PATH\" ;; esac  {MARK}"
            ),
        }
    }

    /// The files this shell reads at startup that could set PATH, for
    /// finding the user's own `.local/bin` line.
    fn startup_files(self, home: &Path) -> Vec<PathBuf> {
        let names: &[&str] = match self {
            Kind::Zsh => &[".zshenv", ".zprofile", ".zshrc", ".zlogin"],
            Kind::Bash => &[".bash_profile", ".bash_login", ".profile", ".bashrc"],
            Kind::Fish => return vec![fish_dir(home).join("config.fish")],
        };
        names.iter().map(|n| home.join(n)).collect()
    }
}

/// fish's config directory: `$XDG_CONFIG_HOME/fish` when `home` is this
/// user's own, else `home/.config/fish`.
fn fish_dir(home: &Path) -> PathBuf {
    let own = std::env::var_os("HOME").is_some_and(|h| Path::new(&h) == home);
    std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .filter(|d| own && d.is_absolute())
        .unwrap_or_else(|| home.join(".config"))
        .join("fish")
}

/// Where the line goes for `kind` (`macos`: Terminal's shells are login
/// shells), in the order written.
pub fn targets(kind: Kind, home: &Path, macos: bool) -> Vec<PathBuf> {
    match kind {
        Kind::Zsh if macos => vec![home.join(".zprofile")],
        Kind::Zsh => vec![home.join(".zshrc")],
        Kind::Bash => {
            // bash -l reads the first of these there is.
            let login = [".bash_profile", ".bash_login", ".profile"]
                .iter()
                .map(|n| home.join(n))
                .find(|p| p.exists())
                .unwrap_or_else(|| home.join(".profile"));
            if macos { vec![login] } else { vec![login, home.join(".bashrc")] }
        }
        Kind::Fish => vec![fish_dir(home).join("conf.d/arugula.fish")],
    }
}

/// `$SHELL`, else the user database's login shell (launchd doesn't always
/// set `$SHELL`), else `/bin/sh`.
pub fn login_shell() -> PathBuf {
    if let Some(s) = std::env::var_os("SHELL").filter(|s| !s.is_empty()) {
        return s.into();
    }
    let user = std::env::var("USER").ok().filter(|u| !u.is_empty()).or_else(|| {
        let o = Command::new("id").arg("-un").output().ok()?;
        Some(String::from_utf8_lossy(&o.stdout).trim().to_string())
    });
    let from_db = user.and_then(|u| {
        if cfg!(target_os = "macos") {
            let o = Command::new("dscl").args([".", "-read", &format!("/Users/{u}"), "UserShell"]).output().ok()?;
            let out = String::from_utf8_lossy(&o.stdout).into_owned();
            out.split_whitespace().nth(1).map(String::from)
        } else {
            let o = Command::new("getent").args(["passwd", &u]).output().ok()?;
            let out = String::from_utf8_lossy(&o.stdout).into_owned();
            out.trim().rsplit(':').next().map(String::from)
        }
    });
    from_db.filter(|s| s.starts_with('/')).map(PathBuf::from).unwrap_or_else(|| "/bin/sh".into())
}

/// The PATH a new login shell (`shell -l`) ends up with, starting from a
/// clean environment as a new terminal would; `None` when it fails, prints
/// none, or takes longer than [`TIMEOUT`].
pub fn login_path(shell: &Path, home: &Path) -> Option<Vec<PathBuf>> {
    const MARK_OUT: &str = "@ARUGULA_PATH=";
    let mut cmd = Command::new(shell);
    cmd.args(["-l", "-c", &format!("printf '\\n{MARK_OUT}%s\\n' \"$PATH\"")])
        .env_clear()
        .env("HOME", home)
        .env("SHELL", shell)
        .env("PATH", "/usr/bin:/bin:/usr/sbin:/sbin")
        .env("TERM", "dumb")
        .current_dir(home)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    for k in ["USER", "LOGNAME", "LANG", "TMPDIR", "XDG_CONFIG_HOME"] {
        if let Some(v) = std::env::var_os(k) {
            cmd.env(k, v);
        }
    }
    let mut child = cmd.spawn().ok()?;
    let mut out = child.stdout.take()?;
    let reader = std::thread::spawn(move || {
        let mut s = Vec::new();
        let _ = out.read_to_end(&mut s);
        s
    });
    let start = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(_)) => break,
            Ok(None) if start.elapsed() < TIMEOUT => std::thread::sleep(Duration::from_millis(20)),
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                return None;
            }
        }
    }
    // A background job the rc started may hold stdout open: don't wait on it.
    let start = Instant::now();
    while !reader.is_finished() {
        if start.elapsed() > Duration::from_millis(500) {
            return None;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    let text = String::from_utf8_lossy(&reader.join().ok()?).into_owned();
    let line = text.lines().rev().find_map(|l| l.strip_prefix(MARK_OUT))?;
    Some(line.split(':').filter(|d| !d.is_empty()).map(PathBuf::from).collect())
}

/// Whether `path` has `dir` (or `~/.local/bin` spelled with a trailing
/// slash).
fn has_dir(path: &[PathBuf], dir: &Path) -> bool {
    path.iter().any(|d| d == dir || d.as_os_str().to_string_lossy().trim_end_matches('/') == dir.to_string_lossy())
}

/// Whether `text` has our mark.
fn marked(text: &str) -> bool {
    text.lines().any(|l| l.contains(MARK))
}

/// Whether `text` has a line of the user's own (not a comment) that names
/// `.local/bin`.
fn mentions(text: &str) -> bool {
    text.lines().any(|l| {
        let l = l.trim_start();
        !l.starts_with('#') && !l.contains(MARK) && l.contains(".local/bin")
    })
}

/// Where `~/.local/bin` stands for a new terminal.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum State {
    /// The login shell has it.
    OnPath,
    /// Our line is in these files (written now, or by an earlier install).
    Marked(Vec<PathBuf>),
    /// The user's own startup file names `.local/bin`; left alone.
    Theirs(PathBuf),
    /// A shell we don't write for (or none could be written): the line to
    /// add by hand.
    Missing { shell: PathBuf },
}

impl State {
    /// Whether a new terminal finds `arugula` by name.
    pub fn found(&self) -> bool {
        matches!(self, State::OnPath | State::Marked(_) | State::Theirs(_))
    }
}

/// What [`ensure`] did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Outcome {
    pub state: State,
    /// The files it added the line to just now.
    pub added: Vec<PathBuf>,
}

impl Outcome {
    /// What to tell the person, if anything: what was added where, or what
    /// to add.
    pub fn note(&self, home: &Path) -> Option<String> {
        let tilde = |p: &Path| match p.strip_prefix(home) {
            Ok(rest) => format!("~/{}", rest.display()),
            Err(_) => p.display().to_string(),
        };
        if !self.added.is_empty() {
            let files: Vec<String> = self.added.iter().map(|p| tilde(p)).collect();
            return Some(format!(
                "added ~/.local/bin to your PATH in {} (for `arugula`): new terminal windows have it; in one already \
                 open, run ~/.local/bin/arugula, or open a new one",
                files.join(" and ")
            ));
        }
        match &self.state {
            State::Theirs(f) => Some(format!(
                "note: your login shell doesn't have ~/.local/bin on PATH, though {} names it; left as it is. \
                 Until it does, run ~/.local/bin/arugula",
                tilde(f)
            )),
            State::Missing { shell } => Some(format!(
                "note: ~/.local/bin isn't on the PATH of new terminals ({}): add it in your shell's startup file \
                 for `arugula`, or run ~/.local/bin/arugula",
                shell.display()
            )),
            _ => None,
        }
    }
}

/// Where `~/.local/bin` stands for `shell`, without writing anything.
pub fn check(home: &Path, shell: &Path) -> State {
    plan(home, shell, Kind::of(shell), cfg!(target_os = "macos"), login_path(shell, home)).0
}

/// `~/.local/bin` on the PATH of `shell`'s new terminals: the marked line
/// added where it's missing. Errors writing a file are notes, not
/// failures: the binaries are installed either way.
pub fn ensure(home: &Path, shell: &Path) -> Outcome {
    ensure_with(home, shell, Kind::of(shell), cfg!(target_os = "macos"), login_path(shell, home))
}

/// `ensure`, with what it asks the machine given: the shell's kind, macOS
/// or not, and the login shell's PATH (`None`: it couldn't tell).
pub fn ensure_with(home: &Path, shell: &Path, kind: Option<Kind>, macos: bool, login: Option<Vec<PathBuf>>) -> Outcome {
    let (state, write) = plan(home, shell, kind, macos, login.clone());
    let Some(kind) = kind.filter(|_| !write.is_empty()) else { return Outcome { state, added: vec![] } };
    let added: Vec<PathBuf> = write.into_iter().filter(|f| append(f, &kind.line()).is_ok()).collect();
    // Where it stands now: marked where the writes went.
    let state = plan(home, shell, Some(kind), macos, login).0;
    Outcome { state, added }
}

/// Where it stands, and the files to add the line to.
fn plan(
    home: &Path,
    shell: &Path,
    kind: Option<Kind>,
    macos: bool,
    login: Option<Vec<PathBuf>>,
) -> (State, Vec<PathBuf>) {
    let dir = home.join(".local/bin");
    let on_login = login.as_deref().is_some_and(|p| has_dir(p, &dir));
    let none = || if on_login { State::OnPath } else { State::Missing { shell: shell.to_path_buf() } };
    let Some(kind) = kind else { return (none(), vec![]) };
    let mut targets = targets(kind, home, macos);
    if on_login {
        // Linux: a terminal's shell isn't a login shell. It has the desktop
        // session's PATH, from ~/.profile when the session began (before
        // ~/.local/bin was there, on a first install: Ubuntu's adds it only
        // if it exists), and its rc file, which gets the line still.
        targets.retain(|f| !macos && [".bashrc", ".zshrc"].iter().any(|n| *f == home.join(n)));
    }
    let read = |p: &Path| fs::read_to_string(p).unwrap_or_default();
    // Ours, from an earlier install.
    let have: Vec<PathBuf> = targets.iter().filter(|f| marked(&read(f))).cloned().collect();
    // The user's own line: in a file we'd write, or (when the login shell
    // couldn't say) in any file the shell reads.
    let theirs = if login.is_some() {
        targets.iter().find(|f| mentions(&read(f))).cloned()
    } else {
        kind.startup_files(home).into_iter().chain(targets.clone()).find(|f| mentions(&read(f)))
    };
    if let Some(f) = theirs {
        return (if on_login { State::OnPath } else { State::Theirs(f) }, vec![]);
    }
    let write: Vec<PathBuf> = targets.into_iter().filter(|f| !have.contains(f)).collect();
    let state = if have.is_empty() { none() } else { State::Marked(have) };
    (state, write)
}

/// `line` at the end of `file` (created, with its directory, if need be),
/// on a line of its own.
fn append(file: &Path, line: &str) -> io::Result<()> {
    if let Some(dir) = file.parent() {
        fs::create_dir_all(dir)?;
    }
    let text = match fs::read_to_string(file) {
        Ok(t) => t,
        Err(e) if e.kind() == io::ErrorKind::NotFound => String::new(),
        Err(e) => return Err(e),
    };
    let sep = if text.is_empty() || text.ends_with('\n') { "" } else { "\n" };
    let blank = if text.is_empty() { "" } else { "\n" };
    // Appended, not rewritten: a symlinked dotfile stays a link.
    use std::io::Write;
    let mut f = fs::OpenOptions::new().create(true).append(true).open(file)?;
    writeln!(f, "{sep}{blank}{line}")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A fresh temporary HOME.
    fn home(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("arugula-shell-path-{}-{name}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        fs::create_dir_all(&d).unwrap();
        d
    }

    fn ensure(h: &Path, shell: &str, macos: bool, login: Option<Vec<PathBuf>>) -> Outcome {
        let shell = Path::new(shell);
        ensure_with(h, shell, Kind::of(shell), macos, login)
    }

    fn sys() -> Option<Vec<PathBuf>> {
        Some(vec!["/usr/bin".into(), "/bin".into()])
    }

    fn count(file: &Path) -> usize {
        fs::read_to_string(file).unwrap_or_default().matches(MARK).count()
    }

    #[test]
    fn zsh_on_a_mac_gets_zprofile_once() {
        let h = home("zsh-mac");
        let o = ensure(&h, "/bin/zsh", true, sys());
        assert_eq!(o.added, [h.join(".zprofile")]);
        assert_eq!(o.state, State::Marked(vec![h.join(".zprofile")]));
        assert!(o.note(&h).unwrap().contains("~/.zprofile"));
        assert!(!h.join(".zshrc").exists());
        // Again: nothing more.
        let o = ensure(&h, "/bin/zsh", true, sys());
        assert!(o.added.is_empty());
        assert_eq!(o.state, State::Marked(vec![h.join(".zprofile")]));
        assert_eq!(o.note(&h), None);
        assert_eq!(count(&h.join(".zprofile")), 1);
    }

    #[test]
    fn on_the_login_path_writes_nothing() {
        let h = home("on-path");
        let o = ensure(&h, "/bin/zsh", true, Some(vec!["/usr/bin".into(), h.join(".local/bin/")]));
        assert_eq!(o, Outcome { state: State::OnPath, added: vec![] });
        assert!(!h.join(".zprofile").exists());
        let fish = ensure(&h, "/usr/bin/fish", false, Some(vec![h.join(".local/bin")]));
        assert_eq!(fish, Outcome { state: State::OnPath, added: vec![] });
    }

    #[test]
    fn on_linux_the_rc_file_gets_it_even_so() {
        // Ubuntu's ~/.profile: a login shell has it, a terminal's session
        // may not yet.
        let h = home("linux-rc");
        let profile = "if [ -d \"$HOME/.local/bin\" ] ; then\n    PATH=\"$HOME/.local/bin:$PATH\"\nfi\n";
        fs::write(h.join(".profile"), profile).unwrap();
        let login = || Some(vec![h.join(".local/bin"), "/usr/bin".into()]);
        let o = ensure(&h, "/bin/bash", false, login());
        assert_eq!(o, Outcome { state: State::Marked(vec![h.join(".bashrc")]), added: vec![h.join(".bashrc")] });
        assert_eq!(fs::read_to_string(h.join(".profile")).unwrap(), profile);
        assert!(ensure(&h, "/bin/bash", false, login()).added.is_empty());
        // Fedora's ~/.bashrc has its own line: nothing.
        let h = home("linux-rc-fedora");
        fs::write(h.join(".bashrc"), "if ! [[ \"$PATH\" =~ \"$HOME/.local/bin:$HOME/bin:\" ]]; then\n").unwrap();
        let o = ensure(&h, "/bin/bash", false, Some(vec![h.join(".local/bin")]));
        assert_eq!(o, Outcome { state: State::OnPath, added: vec![] });
    }

    #[test]
    fn appends_after_what_is_there() {
        let h = home("append");
        fs::write(h.join(".zprofile"), "eval \"$(/opt/homebrew/bin/brew shellenv)\"").unwrap();
        ensure(&h, "/bin/zsh", true, sys());
        let text = fs::read_to_string(h.join(".zprofile")).unwrap();
        assert!(text.starts_with("eval \"$(/opt/homebrew/bin/brew shellenv)\"\n\ncase "), "{text}");
        assert!(text.ends_with(&format!("{MARK}\n")));
    }

    #[test]
    fn a_users_own_line_is_left_alone() {
        let h = home("theirs");
        let mine = "export PATH=$HOME/.local/bin:$PATH\n";
        fs::write(h.join(".zprofile"), mine).unwrap();
        let o = ensure(&h, "/bin/zsh", true, sys());
        assert_eq!(o.state, State::Theirs(h.join(".zprofile")));
        assert!(o.added.is_empty());
        assert_eq!(fs::read_to_string(h.join(".zprofile")).unwrap(), mine);
        // A comment naming it isn't a line that sets it.
        let h = home("theirs-comment");
        fs::write(h.join(".zprofile"), "# maybe ~/.local/bin one day\n").unwrap();
        assert_eq!(ensure(&h, "/bin/zsh", true, sys()).added, [h.join(".zprofile")]);
    }

    #[test]
    fn when_the_shell_cant_say_any_startup_file_counts() {
        let h = home("unknown");
        fs::write(h.join(".zshrc"), "path+=(~/.local/bin)\n").unwrap();
        let o = ensure(&h, "/bin/zsh", true, None);
        assert_eq!(o.state, State::Theirs(h.join(".zshrc")));
        assert!(!h.join(".zprofile").exists());
        // Nothing anywhere: added.
        let h = home("unknown-none");
        assert_eq!(ensure(&h, "/bin/zsh", true, None).added, [h.join(".zprofile")]);
        // Again: our own line, not the user's.
        let o = ensure(&h, "/bin/zsh", true, None);
        assert_eq!(o, Outcome { state: State::Marked(vec![h.join(".zprofile")]), added: vec![] });
    }

    #[test]
    fn zsh_elsewhere_gets_zshrc() {
        let h = home("zsh-linux");
        assert_eq!(ensure(&h, "/usr/bin/zsh", false, sys()).added, [h.join(".zshrc")]);
    }

    #[test]
    fn bash_picks_the_login_file_it_reads() {
        // None: ~/.profile (and ~/.bashrc on Linux).
        let h = home("bash-none");
        let o = ensure(&h, "/bin/bash", false, sys());
        assert_eq!(o.added, [h.join(".profile"), h.join(".bashrc")]);
        assert!(o.note(&h).unwrap().contains("~/.profile and ~/.bashrc"));
        assert_eq!(ensure(&h, "/bin/bash", false, sys()).added, Vec::<PathBuf>::new());
        assert_eq!(count(&h.join(".profile")) + count(&h.join(".bashrc")), 2);
        // ~/.bash_profile there: that one, which hides ~/.profile.
        let h = home("bash-profile");
        fs::write(h.join(".bash_profile"), "source ~/.bashrc\n").unwrap();
        fs::write(h.join(".profile"), "").unwrap();
        assert_eq!(ensure(&h, "/bin/bash", true, sys()).added, [h.join(".bash_profile")]);
        assert_eq!(count(&h.join(".profile")), 0);
        // ~/.bash_login before ~/.profile.
        let h = home("bash-login");
        fs::write(h.join(".bash_login"), "").unwrap();
        fs::write(h.join(".profile"), "").unwrap();
        assert_eq!(ensure(&h, "/bin/bash", true, sys()).added, [h.join(".bash_login")]);
    }

    #[test]
    fn bash_on_linux_adds_the_missing_one_only() {
        // An earlier install's line in ~/.profile; ~/.bashrc lacks it.
        let h = home("bash-half");
        fs::write(h.join(".profile"), format!("{}\n", Kind::Bash.line())).unwrap();
        let o = ensure(&h, "/bin/bash", false, sys());
        assert_eq!(o.added, [h.join(".bashrc")]);
        assert_eq!(o.state, State::Marked(vec![h.join(".profile"), h.join(".bashrc")]));
        assert_eq!(count(&h.join(".profile")), 1);
    }

    #[test]
    fn fish_gets_a_conf_d_file() {
        let h = home("fish");
        let o = ensure(&h, "/opt/homebrew/bin/fish", true, sys());
        let f = fish_dir(&h).join("conf.d/arugula.fish");
        assert_eq!(o.added, std::slice::from_ref(&f));
        let text = fs::read_to_string(&f).unwrap();
        assert!(text.starts_with("if not contains -- $HOME/.local/bin $PATH;"), "{text}");
        assert!(ensure(&h, "/opt/homebrew/bin/fish", true, sys()).added.is_empty());
        assert_eq!(count(&f), 1);
    }

    #[test]
    fn other_shells_get_told() {
        let h = home("tcsh");
        let o = ensure(&h, "/bin/tcsh", true, sys());
        assert_eq!(o.state, State::Missing { shell: "/bin/tcsh".into() });
        assert!(o.added.is_empty());
        assert!(o.note(&h).unwrap().contains("/bin/tcsh"));
        assert!(!o.state.found());
    }

    /// The line itself, run by sh: adds the directory once, however often
    /// it runs.
    #[cfg(unix)]
    #[test]
    fn the_line_adds_the_dir_once() {
        let line = Kind::Zsh.line();
        let script = format!("{line}\n{line}\nprintf %s \"$PATH\"");
        let out = Command::new("/bin/sh").args(["-c", &script]).env("HOME", "/h").env("PATH", "/usr/bin:/bin").output();
        let out = String::from_utf8(out.unwrap().stdout).unwrap();
        assert_eq!(out, "/h/.local/bin:/usr/bin:/bin");
        let out = Command::new("/bin/sh")
            .args(["-c", &script])
            .env("HOME", "/h")
            .env("PATH", "/usr/bin:/h/.local/bin")
            .output();
        assert_eq!(String::from_utf8(out.unwrap().stdout).unwrap(), "/usr/bin:/h/.local/bin");
    }

    /// The real login shell reads the file we write (sh is everywhere; bash
    /// and zsh when they're here).
    #[cfg(unix)]
    #[test]
    fn a_login_shell_sees_the_line() {
        for (shell, name) in [("/bin/bash", "bash"), ("/bin/zsh", "zsh"), ("/usr/bin/zsh", "zsh")] {
            if !Path::new(shell).exists() {
                continue;
            }
            let h = home(&format!("login-{name}-{}", shell.len()));
            let before = login_path(Path::new(shell), &h);
            let o = ensure_with(&h, Path::new(shell), Kind::of(Path::new(shell)), cfg!(target_os = "macos"), before);
            assert!(!o.added.is_empty(), "{shell}: {o:?}");
            let after = check(&h, Path::new(shell));
            assert!(after.found(), "{shell}: {after:?}");
            // zsh's ~/.zshrc (Linux) isn't read by `zsh -l -c`.
            if !(name == "zsh" && !cfg!(target_os = "macos")) {
                let path = login_path(Path::new(shell), &h).unwrap();
                assert!(has_dir(&path, &h.join(".local/bin")), "{shell}: {path:?}");
            }
        }
    }
}
