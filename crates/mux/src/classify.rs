//! What a pane is busy with, and which project it's in (M23): enough to
//! colour and group hundreds of panes without attaching to any.
//!
//! `kind` reads a command line: the foreground process's argv from the OS
//! first (it sees through aliases, so `c` reads as `claude`), else the text
//! the shell integration reported. S16 got 52 of 54 labelled commands right
//! this way. `project` is the nearest directory up from the working
//! directory with a `.git` in it, found without running git, and cached.
//! `branch` is that repository's HEAD, read from the file (M109).

use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::{LazyLock, Mutex},
    time::{Duration, Instant},
};

use arugula_proto::{Project, WorkKind};
use regex::Regex;

/// Agents run in terminals. Those with screen rules
/// ([`arugula_vt::detect`]) say whether they're working, blocked on you or
/// idle (#145); the rest are only known to be agents, and going quiet
/// doesn't mean they want you.
const AGENTS: &[&str] = &["claude", "codex", "aider", "gemini", "opencode", "goose", "amp", "cursor-agent"];
const EDITORS: &[&str] = &["vim", "nvim", "vi", "hx", "helix", "emacs", "nano", "micro", "kak"];
/// Follow-mode log readers (with `-f`), or always.
const LOGS_FOLLOW: &[&str] = &["tail", "less"];
const LOGS: &[&str] = &["journalctl", "stern", "lnav"];
const LOGS_TWO: &[&str] = &["kubectl logs", "docker logs", "fly logs", "flyctl logs"];
/// Commands that run another: look past them (`uv run pytest`, `sudo vim`).
/// A shell with a script is the script (`bash ./bin/deploy test`, which is
/// what /proc shows for a script with a `#!/bin/bash` line).
const WRAPPERS: &[&str] = &[
    "sudo", "time", "nice", "env", "nohup", "uv", "npx", "pnpm", "npm", "yarn", "bunx", "exec", "node", "python",
    "python3", "bun", "deno", "bash", "sh", "zsh",
];

static SERVERS: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"\b(run dev|serve|server|uvicorn|gunicorn|rails s|runserver|vite$|vite --|vite dev|next dev|arugulad --foreground|illogicald --foreground|caddy run|nginx|watch|cargo watch|air|npm start|pnpm dev|yarn dev|hugo server)\b",
    )
    .unwrap()
});
static TESTS: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"\b(test|tests|pytest|nextest|jest|vitest|playwright test|go test|rspec|bats|ctest)\b").unwrap()
});
static BUILDS: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(
        r"\b(build|make|ninja|cmake|clippy|check|tsc|webpack|esbuild|bazel|gradle|mvn|zig build|docker build|just build|cargo b)\b",
    )
    .unwrap()
});

/// A program's name from its path, either way it's written, less a
/// Windows `.exe`.
fn base(word: &str) -> &str {
    let name = word.rsplit(['/', '\\']).next().unwrap_or(word);
    match name.len().checked_sub(4) {
        Some(i) if name.is_char_boundary(i) && name[i..].eq_ignore_ascii_case(".exe") => &name[..i],
        _ => name,
    }
}

/// Programs that start another named after them (`npx`, `uv run`, `sudo`,
/// `nix run`): look past them, their flags and their subcommand.
const RUNNERS: &[&str] = &[
    "sudo", "time", "nice", "env", "nohup", "exec", "command", "npx", "bunx", "pnpm", "npm", "yarn", "uv", "uvx",
    "pipx", "nix",
];
const SUBCOMMANDS: &[&str] = &["run", "exec", "x", "dlx", "tool", "shell"];
/// Interpreters: the script they run is the program (`node …/claude`,
/// `python -m aider`), but code given inline (`python -c …`) isn't one.
/// By name without its version or case (`python3.14`, macOS's `Python`).
const INTERPRETERS: &[&str] = &["node", "python", "bun", "deno", "bash", "sh", "zsh", "ruby", "perl"];
const INLINE: &[&str] = &["-c", "-e", "--eval", "-p", "--print"];

/// The agent a command line runs (`claude`, `codex`, …), seen through
/// wrappers: `node …/bin/claude`, `bunx @openai/codex`, `python -m aider`,
/// `nix run nixpkgs#claude-code`, a Nix wrapper (`.claude-wrapped`), `env
/// FOO=1 claude`. A word that only mentions one (`python -c codex`, `grep
/// claude`) doesn't count.
pub fn agent(command: &str) -> Option<&'static str> {
    let named = |word: &str| -> Option<&'static str> {
        let word = word.rsplit('#').next().unwrap_or(word);
        let name = base(word).trim_start_matches('.');
        let name = name.strip_suffix("-wrapped").unwrap_or(name);
        AGENTS.iter().find(|a| name == **a || name.starts_with(&format!("{a}-"))).copied()
    };
    let words: Vec<&str> = command.split_whitespace().map(|w| w.trim_matches(['\'', '"'])).collect();
    let mut i = 0;
    while let Some(w) = words.get(i) {
        let b = base(w);
        if RUNNERS.contains(&b) {
            i += 1;
            while words.get(i).is_some_and(|w| w.starts_with('-') || SUBCOMMANDS.contains(w) || is_assignment(w)) {
                i += 1;
            }
        } else if INTERPRETERS.contains(&interpreter(b).as_str()) {
            i += 1;
            while let Some(w) = words.get(i).filter(|w| w.starts_with('-') || SUBCOMMANDS.contains(w)) {
                if INLINE.contains(w) {
                    return None;
                }
                if *w == "-m" {
                    return words.get(i + 1).and_then(|m| named(m));
                }
                i += 1;
            }
        } else if is_assignment(w) {
            i += 1;
        } else {
            return named(w);
        }
    }
    None
}

/// An interpreter's name without its version or case: `python3.14` and
/// `Python` (Homebrew's, on macOS) are `python`.
fn interpreter(name: &str) -> String {
    name.trim_end_matches(|c: char| c.is_ascii_digit() || c == '.').to_ascii_lowercase()
}

/// `FOO=bar` before a command.
fn is_assignment(word: &str) -> bool {
    word.split_once('=').is_some_and(|(k, _)| !k.is_empty() && k.chars().all(|c| c.is_ascii_alphanumeric() || c == '_'))
}

/// What a command line is: an agent, an editor, logs, a server, tests, a
/// build, or anything else (`shell`, also for an empty line).
pub fn kind(command: &str) -> WorkKind {
    let words: Vec<&str> = command.split_whitespace().map(|w| w.trim_matches('\'')).collect();
    if words.is_empty() {
        return WorkKind::Shell;
    }
    // Look through wrappers (`uv run`, `npx`, `sudo`).
    let mut i = 0;
    while i + 1 < words.len() && WRAPPERS.contains(&base(words[i])) {
        i += 1;
        if words[i] == "run" {
            i = (i + 1).min(words.len() - 1);
        }
    }
    let head = base(words[i]);
    let two = words[i..].iter().take(2).map(|w| base(w)).collect::<Vec<_>>().join(" ");
    if agent(command).is_some() {
        return WorkKind::Agent;
    }
    if EDITORS.contains(&head) {
        return WorkKind::Editor;
    }
    let follows = words.iter().any(|w| *w == "-f" || *w == "-F" || w.starts_with("-f") && w.len() <= 4);
    if LOGS_TWO.contains(&two.as_str()) || LOGS.contains(&head) || (LOGS_FOLLOW.contains(&head) && follows) {
        return WorkKind::Logs;
    }
    // The rest read the line without its paths' directories.
    let text = words.iter().map(|w| base(w)).collect::<Vec<_>>().join(" ");
    if SERVERS.is_match(&text) {
        WorkKind::Server
    } else if TESTS.is_match(&text) {
        WorkKind::Test
    } else if BUILDS.is_match(&text) {
        WorkKind::Build
    } else {
        WorkKind::Shell
    }
}

/// Directories already looked at, so a pane's project costs a lookup.
static PROJECTS: LazyLock<Mutex<HashMap<String, Option<Project>>>> = LazyLock::new(Default::default);

/// The git repository `cwd` is in: the nearest directory up with a `.git`
/// (a directory, or a worktree's file). Cached per directory.
pub fn project(cwd: &str) -> Option<Project> {
    if let Some(p) = PROJECTS.lock().unwrap().get(cwd) {
        return p.clone();
    }
    let found =
        Path::new(cwd).ancestors().take_while(|p| *p != Path::new("/")).find(|p| p.join(".git").exists()).map(|root| {
            Project {
                root: root.display().to_string(),
                // The repository's own name, from its origin: a worktree's
                // folder or a clone's renamed folder isn't the project.
                name: origin_name(root)
                    .unwrap_or_else(|| root.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()),
            }
        });
    let mut cache = PROJECTS.lock().unwrap();
    // Bounded: a daemon that has seen this many directories starts over.
    if cache.len() > 4096 {
        cache.clear();
    }
    cache.insert(cwd.to_owned(), found.clone());
    found
}

/// The repository `origin` names (`…/arugula-salad/arugula.git` →
/// `arugula`), read from the git config, a worktree's through its common
/// directory.
fn origin_name(root: &Path) -> Option<String> {
    let d = git_dir(root)?;
    // A worktree's directory under the main repository names the main
    // repository's .git in its `commondir`.
    let common = match std::fs::read_to_string(d.join("commondir")) {
        Ok(c) => d.join(c.trim()),
        Err(_) => d,
    };
    origin_in(&std::fs::read_to_string(common.join("config")).ok()?)
}

/// The repository's own git directory: its `.git`, or for a worktree
/// (`.git` is a file, `gitdir: <path>`) the directory that names.
fn git_dir(root: &Path) -> Option<PathBuf> {
    let dot = root.join(".git");
    if dot.is_dir() {
        return Some(dot);
    }
    let text = std::fs::read_to_string(&dot).ok()?;
    let d = Path::new(text.strip_prefix("gitdir:")?.trim()).to_path_buf();
    Some(if d.is_absolute() { d } else { root.join(d) })
}

/// How long a repository's branch is believed: a checkout in a pane that
/// stays in its directory shows within this, and a busy screen costs one
/// small read per repository per period, not one per frame.
const BRANCH_FRESH: Duration = Duration::from_secs(2);

/// A repository's HEAD as read: its branch, or the short commit when
/// detached, and the commit it's at (none before the first commit).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Head {
    pub branch: String,
    pub commit: Option<String>,
}

/// A HEAD as read, and when.
type Read = (Instant, Option<Head>);

/// HEADs already read, by repository root.
static HEADS: LazyLock<Mutex<HashMap<String, Read>>> = LazyLock::new(Default::default);

/// The branch checked out in the repository at `root` (a [`Project`]'s
/// root), or the short commit when HEAD is detached.
pub fn branch(root: &str) -> Option<String> {
    head(root).map(|h| h.branch)
}

/// The repository at `root`'s HEAD: its branch and the commit it's at.
/// Read from the files, not by running git, and cached for
/// [`BRANCH_FRESH`]; a pane moving to another repository asks for another
/// root, so it's read afresh.
pub fn head(root: &str) -> Option<Head> {
    if let Some((at, h)) = HEADS.lock().unwrap().get(root)
        && at.elapsed() < BRANCH_FRESH
    {
        return h.clone();
    }
    let found = head_of(Path::new(root));
    let mut cache = HEADS.lock().unwrap();
    if cache.len() > 4096 {
        cache.clear();
    }
    cache.insert(root.to_owned(), (Instant::now(), found.clone()));
    found
}

/// A command ended: what it did to a repository is read afresh.
pub fn forget_heads() {
    HEADS.lock().unwrap().clear();
}

fn head_of(root: &Path) -> Option<Head> {
    let dir = git_dir(root)?;
    let text = std::fs::read_to_string(dir.join("HEAD")).ok()?;
    let branch = head_in(&text)?;
    let commit = match text.trim().strip_prefix("ref:") {
        Some(r) => ref_commit(&dir, r.trim()),
        None => Some(branch.clone()),
    };
    Some(Head { branch, commit })
}

/// The short commit `r` (`refs/heads/main`) names: its own file, in the
/// worktree's directory or the main repository's, else `packed-refs`.
fn ref_commit(dir: &Path, r: &str) -> Option<String> {
    let common = match std::fs::read_to_string(dir.join("commondir")) {
        Ok(c) => dir.join(c.trim()),
        Err(_) => dir.to_path_buf(),
    };
    let loose = [dir, common.as_path()].into_iter().find_map(|d| std::fs::read_to_string(d.join(r)).ok());
    let sha = match loose {
        Some(s) => s.trim().to_owned(),
        None => std::fs::read_to_string(common.join("packed-refs"))
            .ok()?
            .lines()
            .find_map(|l| l.strip_suffix(r)?.strip_suffix(' ').map(str::to_owned))?,
    };
    (sha.len() >= 7 && sha.bytes().all(|b| b.is_ascii_hexdigit())).then(|| sha[..7].to_owned())
}

/// A HEAD file's branch (`ref: refs/heads/main` → `main`), or its
/// commit's first seven hex digits when detached.
fn head_in(head: &str) -> Option<String> {
    let head = head.trim();
    if let Some(r) = head.strip_prefix("ref:") {
        let r = r.trim();
        let name = r.strip_prefix("refs/heads/").unwrap_or(r);
        return (!name.is_empty()).then(|| name.to_owned());
    }
    (head.len() >= 7 && head.bytes().all(|b| b.is_ascii_hexdigit())).then(|| head[..7].to_owned())
}

/// `[remote "origin"]`'s url's last part, without `.git`.
fn origin_in(config: &str) -> Option<String> {
    let mut in_origin = false;
    for line in config.lines().map(str::trim) {
        if line.starts_with('[') {
            in_origin = line.replace(' ', "") == "[remote\"origin\"]";
        } else if in_origin
            && let Some(url) = line.strip_prefix("url").map(str::trim_start).and_then(|l| l.strip_prefix('='))
        {
            let last = url.trim().trim_end_matches('/').rsplit(['/', ':']).next()?;
            let name = last.strip_suffix(".git").unwrap_or(last);
            return (!name.is_empty()).then(|| name.to_owned());
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use WorkKind::*;

    /// S16's labelled fixture, plus what panes run in this repo.
    #[test]
    fn kinds_of_real_commands() {
        let fixture = [
            ("cargo build --release", Build),
            ("cargo build -p arugula-control", Build),
            ("cargo clippy --all-targets -- -D warnings", Build),
            ("just build", Build),
            ("make -j32", Build),
            ("npm run build", Build),
            ("pnpm run build", Build),
            ("vite build", Build),
            ("tsc --noEmit", Build),
            ("docker build -t api .", Build),
            ("zig build", Build),
            ("cargo test", Test),
            ("cargo test -p arugulad --test questions", Test),
            ("cargo nextest run", Test),
            ("npx playwright test passkey", Test),
            ("pytest -x", Test),
            ("uv run pytest -k slots", Test),
            ("go test ./...", Test),
            ("pnpm vitest", Test),
            ("just test", Test),
            ("bats tests/", Test),
            ("claude", Agent),
            ("claude --continue", Agent),
            ("claude --dangerously-skip-permissions", Agent),
            ("codex", Agent),
            ("aider --model sonnet", Agent),
            ("/home/user/.local/bin/claude", Agent),
            // What /proc shows for `c`, an alias for it.
            ("claude --dangerously-skip-permissions", Agent),
            ("node /home/user/.local/share/agents/node_modules/.bin/claude-agent-acp", Agent),
            ("npm run dev", Server),
            ("pnpm dev", Server),
            ("npm start", Server),
            ("uvicorn app:main --reload", Server),
            ("python manage.py runserver", Server),
            ("arugulad --foreground", Server),
            ("illogicald --foreground", Server),
            ("hugo server", Server),
            ("cargo watch -x check", Server),
            ("caddy run", Server),
            ("node /work/app/node_modules/.bin/vite", Server),
            ("tail -f /var/log/caddy.log", Logs),
            ("tail -F app.log", Logs),
            ("journalctl -fu arugulad", Logs),
            ("journalctl -f", Logs),
            ("kubectl logs -f deploy/api", Logs),
            ("fly logs -a arugula-control", Logs),
            ("docker logs -f web", Logs),
            ("nvim src/main.rs", Editor),
            ("vim /tmp/x.txt", Editor),
            ("sudo vim /etc/hosts", Editor),
            ("hx .", Editor),
            ("emacs -nw", Editor),
            ("", Shell),
            ("git status", Shell),
            ("ls -la", Shell),
            ("ssh box", Shell),
            ("htop", Shell),
            ("git push origin main", Shell),
            ("sudo apt upgrade", Shell),
            ("python3", Shell),
            ("tail -n 20 app.log", Shell),
            // Scripts, as /proc shows them: their interpreter, then them.
            ("/bin/bash /home/user/bin/cargo test", Test),
            ("bash /opt/tools/journalctl -f", Logs),
            ("/bin/bash /home/user/.local/bin/claude perm", Agent),
            ("bash", Shell),
            ("bash -l", Shell),
            ("bash --norc --noprofile", Shell),
        ];
        let wrong: Vec<_> =
            fixture.iter().filter(|(c, want)| kind(c) != *want).map(|(c, want)| (c, want, kind(c))).collect();
        assert!(wrong.is_empty(), "misclassified: {wrong:?}");
    }

    #[test]
    fn agents_through_wrappers() {
        for (command, want) in [
            ("claude", Some("claude")),
            ("claude --resume 0f3c", Some("claude")),
            ("/home/me/.local/bin/claude", Some("claude")),
            (r"C:\Users\me\AppData\Roaming\npm\node_modules\@anthropic-ai\claude-code\bin\claude.exe", Some("claude")),
            (r"C:\nodejs\node.EXE C:\npm\node_modules\.bin\codex", Some("codex")),
            ("node /usr/lib/node_modules/.bin/claude", Some("claude")),
            ("/usr/bin/python3 /tmp/bin/claude", Some("claude")),
            (
                "/opt/homebrew/Cellar/python@3.14/3.14.6/Frameworks/Python.framework/Versions/3.14/Resources/Python.app/Contents/MacOS/Python /tmp/bin/codex",
                Some("codex"),
            ),
            ("python3.12 -m aider", Some("aider")),
            ("bunx @openai/codex", Some("codex")),
            ("npx -y @anthropic-ai/claude-code", Some("claude")),
            ("pnpm dlx @openai/codex --full-auto", Some("codex")),
            ("python -m aider --model sonnet", Some("aider")),
            ("uv tool run aider", Some("aider")),
            ("nix run nixpkgs#claude-code", Some("claude")),
            ("/nix/store/abc-claude-code/bin/.claude-wrapped", Some("claude")),
            ("env FOO=1 codex", Some("codex")),
            ("ANTHROPIC_MODEL=x claude", Some("claude")),
            ("sudo -E claude", Some("claude")),
            ("/bin/bash /home/me/bin/codex perm", Some("codex")),
            // Mentioned, not run.
            ("python -c codex", None),
            ("python3 -c 'import codex'", None),
            ("node -e claude", None),
            ("grep claude notes.txt", None),
            ("vim claude.md", None),
            ("echo codex", None),
            ("python3", None),
            ("bash -l", None),
            ("", None),
        ] {
            assert_eq!(agent(command), want, "{command}");
        }
    }

    /// The typed text hides an alias; the process's argv doesn't.
    #[test]
    fn argv_before_typed_text() {
        assert_eq!(kind("c"), Shell);
        assert_eq!(kind("claude"), Agent);
    }

    #[test]
    fn projects_are_git_roots() {
        let dir = std::env::temp_dir().join(format!("arugula-classify-{}", std::process::id()));
        let repo = dir.join("myrepo");
        std::fs::create_dir_all(repo.join(".git")).unwrap();
        std::fs::create_dir_all(repo.join("crates/core/src")).unwrap();
        std::fs::create_dir_all(dir.join("loose")).unwrap();
        let inner = repo.join("crates/core/src").display().to_string();
        let p = project(&inner).unwrap();
        assert_eq!((p.name.as_str(), p.root), ("myrepo", repo.display().to_string()));
        assert_eq!(project(&repo.display().to_string()).unwrap().name, "myrepo");
        assert_eq!(project(&dir.join("loose").display().to_string()), None);
        // A worktree's .git is a file.
        let wt = dir.join("wt");
        std::fs::create_dir_all(&wt).unwrap();
        std::fs::write(wt.join(".git"), "gitdir: elsewhere").unwrap();
        assert_eq!(project(&wt.display().to_string()).unwrap().name, "wt");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_project_is_named_by_its_origin() {
        let cfg = "[core]\n\tbare = false\n[remote \"upstream\"]\n\turl = git@github.com:x/other.git\n[remote \"origin\"]\n\turl = https://github.com/arugula-salad/arugula.git\n";
        assert_eq!(origin_in(cfg).as_deref(), Some("arugula"));
        assert_eq!(origin_in("[remote \"origin\"]\n url = git@github.com:a/b\n").as_deref(), Some("b"));
        assert_eq!(origin_in("[core]\n"), None);
        // A worktree reads its main repository's config.
        let dir = std::env::temp_dir().join(format!("arugula-origin-{}", std::process::id()));
        let main = dir.join("main");
        let wt = dir.join("wt");
        std::fs::create_dir_all(main.join(".git/worktrees/wt")).unwrap();
        std::fs::create_dir_all(&wt).unwrap();
        std::fs::write(main.join(".git/config"), cfg).unwrap();
        std::fs::write(main.join(".git/worktrees/wt/commondir"), "../..\n").unwrap();
        std::fs::write(wt.join(".git"), format!("gitdir: {}\n", main.join(".git/worktrees/wt").display())).unwrap();
        assert_eq!(origin_name(&wt).as_deref(), Some("arugula"));
        assert_eq!(origin_name(&main).as_deref(), Some("arugula"));
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn branches_from_head() {
        let dir = std::env::temp_dir().join(format!("arugula-branch-{}", std::process::id()));
        let s = |p: &Path| p.display().to_string();
        // On a branch, slashes and all.
        let repo = dir.join("repo");
        std::fs::create_dir_all(repo.join(".git")).unwrap();
        std::fs::write(repo.join(".git/HEAD"), "ref: refs/heads/m109/layout\n").unwrap();
        assert_eq!(branch(&s(&repo)).as_deref(), Some("m109/layout"));
        // Detached: the short commit.
        let detached = dir.join("detached");
        std::fs::create_dir_all(detached.join(".git")).unwrap();
        std::fs::write(detached.join(".git/HEAD"), "bb5b2f16a0c1d2e3f4a5b6c7d8e9f00112233445\n").unwrap();
        assert_eq!(branch(&s(&detached)).as_deref(), Some("bb5b2f1"));
        // A worktree reads its own HEAD, through `gitdir:`, relative or not.
        std::fs::create_dir_all(repo.join(".git/worktrees/wt")).unwrap();
        std::fs::write(repo.join(".git/worktrees/wt/HEAD"), "ref: refs/heads/feature\n").unwrap();
        let wt = dir.join("wt");
        std::fs::create_dir_all(&wt).unwrap();
        std::fs::write(wt.join(".git"), format!("gitdir: {}\n", s(&repo.join(".git/worktrees/wt")))).unwrap();
        assert_eq!(branch(&s(&wt)).as_deref(), Some("feature"));
        let rel = dir.join("rel");
        std::fs::create_dir_all(&rel).unwrap();
        std::fs::write(rel.join(".git"), "gitdir: ../repo/.git/worktrees/wt\n").unwrap();
        assert_eq!(head_of(&rel).map(|h| h.branch).as_deref(), Some("feature"));
        // No repository, or one without a readable HEAD.
        let loose = dir.join("loose");
        std::fs::create_dir_all(&loose).unwrap();
        assert_eq!(branch(&s(&loose)), None);
        assert_eq!(head_in("garbage"), None);
        // A checkout shows once the cache is stale; a cwd in a pane's info
        // goes through `project` first, so a subdirectory is its root's.
        std::fs::write(repo.join(".git/HEAD"), "ref: refs/heads/main\n").unwrap();
        assert_eq!(head_of(&repo), Some(Head { branch: "main".into(), commit: None }), "no commit yet");
        std::fs::create_dir_all(repo.join(".git/refs/heads")).unwrap();
        // The commit: the branch's own file, else packed-refs; a worktree's
        // branch is in the main repository's.
        let packed = "# pack-refs with: peeled\n0123456789abcdef0123456789abcdef01234567 refs/heads/feature\n";
        std::fs::write(repo.join(".git/packed-refs"), packed).unwrap();
        std::fs::write(repo.join(".git/worktrees/wt/commondir"), "../..\n").unwrap();
        assert_eq!(head_of(&wt).and_then(|h| h.commit).as_deref(), Some("0123456"));
        std::fs::write(repo.join(".git/refs/heads/main"), "fedcba9876543210fedcba9876543210fedcba98\n").unwrap();
        assert_eq!(head_of(&repo).and_then(|h| h.commit).as_deref(), Some("fedcba9"));
        assert_eq!(head_of(&detached).and_then(|h| h.commit).as_deref(), Some("bb5b2f1"));
        let inner = repo.join("src");
        std::fs::create_dir_all(&inner).unwrap();
        assert_eq!(project(&s(&inner)).map(|p| p.root), Some(s(&repo)));
        std::fs::remove_dir_all(&dir).ok();
    }
}
