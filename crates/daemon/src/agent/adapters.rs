//! The npm adapters Claude Code and Codex run through (#111): whether each
//! is installed on this host, and the `npm install` that installs it, made
//! from the pins in `defs.rs` (the README's commands are checked against
//! these in a test). An install older than the pin is out of date (#335):
//! the same install updates it.
//!
//! Programs are found the OS's way (#1175, `arugula_proto::exe`): on Windows
//! PATH is `;`-separated, Node is `node.exe`, npm is `npm.cmd`, and the
//! adapter is npm's `.cmd` shim in `node_modules\.bin`.

use std::path::{Path, PathBuf};
use std::process::Command;

use arugula_proto::exe::Exe;
use serde_json::{Value, json};

use super::defs::{CLAUDE_ACP, CODEX_ACP, Kind, agents_dir};

/// The oldest Node the adapters run on.
pub const NODE_MAJOR: u32 = 20;

pub struct Adapter {
    pub kind: Kind,
    pub label: &'static str,
    /// The agent's own CLI, whose being on this machine says it's used here
    /// (#335).
    pub cli: &'static str,
    /// Its directory under the agents directory.
    pub dir: &'static str,
    pub bin: &'static str,
    /// `name@version`.
    pub package: &'static str,
    /// Extra `npm install` flags. Not `--omit=optional` for Codex: its
    /// native binary is an optional dependency, which the adapter runs when
    /// there's no `codex` on this machine (#1447).
    pub flags: &'static str,
}

pub const ADAPTERS: &[Adapter] = &[
    Adapter {
        kind: Kind::Claude,
        label: "Claude Code",
        cli: "claude",
        dir: "claude",
        bin: "claude-agent-acp",
        package: CLAUDE_ACP,
        flags: "",
    },
    Adapter {
        kind: Kind::Codex,
        label: "Codex",
        cli: "codex",
        dir: "codex",
        bin: "codex-acp",
        package: CODEX_ACP,
        flags: "",
    },
];

pub fn of(kind: Kind) -> Option<&'static Adapter> {
    ADAPTERS.iter().find(|a| a.kind == kind)
}

impl Adapter {
    /// The package's name, without the version.
    pub fn name(&self) -> &'static str {
        self.package.rsplit_once('@').map_or(self.package, |(n, _)| n)
    }

    pub fn pinned(&self) -> &'static str {
        self.package.rsplit_once('@').map_or("", |(_, v)| v)
    }

    /// `npm install` for it, with `agents` the agents directory as written
    /// in a shell (`~/…`, or a quoted path).
    pub fn npm(&self, agents: &str) -> String {
        format!("npm install {}--prefix {agents}/{} {}", self.flags, self.dir, self.package)
    }

    /// Its executable in our agents directory.
    pub fn local_bin(&self, home: &Path) -> PathBuf {
        self.local_bin_in(home, &Exe::here())
    }

    /// Windows: npm's `.cmd` shim.
    fn local_bin_in(&self, home: &Path, x: &Exe) -> PathBuf {
        let dir = agents_dir(home).join(self.dir).join("node_modules").join(".bin");
        x.in_dir(&dir, self.bin).unwrap_or_else(|| dir.join(self.bin))
    }
}

/// The agents directory as a person would type it: `~/…` under home, else
/// the path, quoted if it needs it.
pub fn shown_dir(home: &Path) -> String {
    let dir = agents_dir(home);
    match dir.strip_prefix(home) {
        Ok(rest) if plain(&rest.display().to_string()) => format!("~/{}", rest.display()),
        _ => quote(&dir.display().to_string()),
    }
}

fn plain(s: &str) -> bool {
    !s.is_empty() && s.chars().all(|c| c.is_ascii_alphanumeric() || "/._-+@".contains(c))
}

fn quote(s: &str) -> String {
    if plain(s) { s.to_owned() } else { format!("'{}'", s.replace('\'', r"'\''")) }
}

#[derive(Debug, Clone, PartialEq)]
pub enum State {
    /// `version` from its package.json, when it's in our directory.
    Installed {
        version: Option<String>,
        on_path: bool,
    },
    Missing,
    /// No Node that runs, or this one (too old).
    NoNode {
        found: Option<String>,
    },
}

pub struct Status {
    pub adapter: &'static Adapter,
    pub state: State,
    /// The command to show (and copy).
    pub npm: String,
    /// Where a Node was found that isn't on the PATH a shell gets (mise's),
    /// to put first when installing.
    pub node_bin: Option<PathBuf>,
}

impl Status {
    pub fn ok(&self) -> bool {
        matches!(self.state, State::Installed { .. })
    }

    /// Installed in our directory, but not the version the pin says (#335):
    /// it still runs, and installing again updates it.
    pub fn outdated(&self) -> bool {
        matches!(&self.state, State::Installed { version: Some(v), .. } if v != self.adapter.pinned())
    }

    /// Nothing to install: it's there, at the pin (or on PATH, which is
    /// the person's own).
    pub fn current(&self) -> bool {
        self.ok() && !self.outdated()
    }

    /// Why the agent can't start, for its block.
    pub fn why(&self) -> String {
        match &self.state {
            State::Installed { .. } => String::new(),
            State::Missing => format!("{}'s adapter isn't installed", self.adapter.label),
            State::NoNode { found: Some(v) } => {
                format!("{}'s adapter needs Node {NODE_MAJOR}+ (this machine has {v})", self.adapter.label)
            }
            State::NoNode { found: None } => format!("{}'s adapter needs Node {NODE_MAJOR}+", self.adapter.label),
        }
    }

    pub fn json(&self) -> Value {
        let a = self.adapter;
        let mut v = json!({
            "kind": a.kind,
            "label": a.label,
            "package": a.name(),
            "pinned": a.pinned(),
            "npm": self.npm,
            "node_major": NODE_MAJOR,
        });
        match &self.state {
            State::Installed { version, on_path } => {
                v["state"] = json!("installed");
                v["version"] = json!(version);
                v["on_path"] = json!(on_path);
                v["outdated"] = json!(self.outdated());
            }
            State::Missing => v["state"] = json!("missing"),
            State::NoNode { found } => {
                v["state"] = json!("no_node");
                v["node"] = json!(found);
            }
        }
        if !self.ok() {
            v["why"] = json!(self.why());
        }
        v
    }
}

/// Where `env`'s PATH (else ours) is.
fn path_of(env: &[(String, String)]) -> String {
    env.iter()
        .rev()
        .find(|(k, _)| k == "PATH")
        .map(|(_, v)| v.clone())
        .or_else(|| std::env::var("PATH").ok())
        .unwrap_or_default()
}

/// The Node an adapter would run with: its `--version`, if it runs.
fn node_version(path: &str, x: &Exe) -> Option<String> {
    let node = x.find("node", path)?;
    let out = Command::new(node).arg("--version").env("PATH", path).output().ok()?;
    let v = String::from_utf8_lossy(&out.stdout).trim().to_owned();
    (out.status.success() && v.starts_with('v')).then_some(v)
}

fn major(v: &str) -> Option<u32> {
    v.trim_start_matches('v').split('.').next()?.parse().ok()
}

/// Whether `a` can start on this host, with `env` as an agent block gets.
pub fn status(a: &'static Adapter, home: &Path, env: &[(String, String)]) -> Status {
    status_in(a, home, env, &Exe::here())
}

fn status_in(a: &'static Adapter, home: &Path, env: &[(String, String)], x: &Exe) -> Status {
    let mut env = env.to_vec();
    let before = path_of(&env);
    with_node_on_path_in(&mut env, home, x);
    let path = path_of(&env);
    let node_bin = (path != before).then(|| x.dirs(&path).into_iter().next()).flatten();
    let npm = a.npm(&shown_dir(home));
    let state = match node_version(&path, x) {
        None => State::NoNode { found: None },
        Some(v) if major(&v).is_none_or(|m| m < NODE_MAJOR) => State::NoNode { found: Some(v) },
        Some(_) if a.local_bin_in(home, x).exists() => {
            let pkg = agents_dir(home).join(a.dir).join("node_modules").join(a.name()).join("package.json");
            let version = std::fs::read(pkg)
                .ok()
                .and_then(|b| serde_json::from_slice::<Value>(&b).ok())
                .and_then(|v| v["version"].as_str().map(str::to_owned));
            State::Installed { version, on_path: false }
        }
        Some(_) if x.find(a.bin, &path).is_some() => State::Installed { version: None, on_path: true },
        Some(_) => State::Missing,
    };
    Status { adapter: a, state, npm, node_bin }
}

/// Every adapter's status.
pub fn all(home: &Path, env: &[(String, String)]) -> Vec<Value> {
    ADAPTERS.iter().map(|a| status(a, home, env).json()).collect()
}

/// The PATH an install runs with: `env`'s, with the Node an agent block
/// would get put first.
pub fn node_path(home: &Path, env: &[(String, String)]) -> String {
    let mut env = env.to_vec();
    with_node_on_path(&mut env, home);
    path_of(&env)
}

/// The install, to run here and wait for (Getting started's one click and
/// `arugula setup`, #335): the same `npm install` an *Install* pane runs,
/// into the real directory, with `path` (`node_path`). `ARUGULA_NPM`
/// replaces npm. `None` when there's no npm on that PATH.
pub fn npm_install(a: &Adapter, home: &Path, path: &str) -> Option<Command> {
    npm_install_in(a, home, path, &Exe::here())
}

/// Windows: `npm.cmd`, which `Command` runs through cmd.exe.
fn npm_install_in(a: &Adapter, home: &Path, path: &str, x: &Exe) -> Option<Command> {
    let npm = match std::env::var("ARUGULA_NPM") {
        Ok(other) if !other.is_empty() => PathBuf::from(other),
        _ => x.find("npm", path)?,
    };
    let mut cmd = Command::new(npm);
    cmd.arg("install")
        .args(a.flags.split_whitespace())
        .arg("--prefix")
        .arg(agents_dir(home).join(a.dir))
        .arg(a.package)
        .env("PATH", path)
        .stdin(std::process::Stdio::null());
    Some(cmd)
}

/// What an *Install* pane runs: the shown command, but into the real
/// directory and with the Node we found first on PATH. `ARUGULA_NPM`
/// replaces npm (the browser tests' stand-in). Windows: for PowerShell,
/// which a pane's command runs in there.
pub fn install_command(s: &Status, home: &Path) -> String {
    if cfg!(windows) { install_command_powershell(s, home) } else { install_command_sh(s, home) }
}

fn install_command_sh(s: &Status, home: &Path) -> String {
    let npm = s.adapter.npm(&quote(&agents_dir(home).display().to_string()));
    let npm = match std::env::var("ARUGULA_NPM") {
        Ok(other) if !other.is_empty() => format!("{}{}", quote(&other), npm.trim_start_matches("npm")),
        _ => npm,
    };
    let npm = match &s.node_bin {
        // Quoted, `$PATH` is colon-joined in fish too.
        Some(bin) => format!("env \"PATH={}:$PATH\" {npm}", bin.display()),
        None => npm,
    };
    format!("{npm} && echo && echo 'Installed {}. Start the agent again.'", s.adapter.package)
}

/// #1175: `npm.cmd`, as `npm` may be npm's `.ps1`, which Windows
/// PowerShell's script policy can refuse; the package quoted, as `@name` is
/// splatting there; `$env:PATH`; and `if ($?)`, as Windows PowerShell 5.1
/// has no `&&`.
fn install_command_powershell(s: &Status, home: &Path) -> String {
    let a = s.adapter;
    let ps = |s: &str| format!("'{}'", s.replace('\'', "''"));
    let npm = match std::env::var("ARUGULA_NPM") {
        Ok(other) if !other.is_empty() => format!("& {}", ps(&other)),
        _ => "npm.cmd".into(),
    };
    let dir = agents_dir(home).join(a.dir);
    let mut cmd = format!("{npm} install {}--prefix {} {}", a.flags, ps(&dir.display().to_string()), ps(a.package));
    if let Some(bin) = &s.node_bin {
        cmd = format!("$env:PATH = {} + ';' + $env:PATH; {cmd}", ps(&bin.display().to_string()));
    }
    format!("{cmd}; if ($?) {{ echo ''; echo {} }}", ps(&format!("Installed {}. Start the agent again.", a.package)))
}

/// npm adapters are Node scripts: make sure a Node is on PATH even when the
/// daemon runs as a service with a bare one. mise's shims won't do: outside
/// a directory that pins Node they refuse to pick a version. So: a real
/// `node` already on PATH, else one mise installed (22, the version the
/// adapters were tested with, else the newest), put first.
pub fn with_node_on_path(env: &mut Vec<(String, String)>, home: &Path) {
    with_node_on_path_in(env, home, &Exe::here())
}

fn with_node_on_path_in(env: &mut Vec<(String, String)>, home: &Path, x: &Exe) {
    let path = path_of(env);
    let mise = mise_dir(env, home, x);
    let shims = mise.join("shims");
    let real = x.dirs(&path).iter().any(|d| *d != shims && x.in_dir(d, "node").is_some());
    if real {
        return;
    }
    let Some(bin) = mise_node_in(&mise.join("installs").join("node"), x) else { return };
    env.retain(|(k, _)| k != "PATH");
    env.push(("PATH".into(), x.prepend(&bin, &path)));
}

/// Where mise keeps its shims and installs: `~/.local/share/mise`, and on
/// Windows `%LOCALAPPDATA%\mise` (#1178).
fn mise_dir(env: &[(String, String)], home: &Path, x: &Exe) -> PathBuf {
    if x.sep != ';' {
        return home.join(".local/share/mise");
    }
    let local = env
        .iter()
        .rev()
        .find(|(k, _)| k.eq_ignore_ascii_case("LOCALAPPDATA"))
        .map(|(_, v)| v.clone())
        .or_else(|| std::env::var("LOCALAPPDATA").ok())
        .filter(|v| !v.is_empty())
        .map_or_else(|| home.join("AppData").join("Local"), PathBuf::from);
    local.join("mise")
}

/// The `bin` of a Node that mise installed: 22's if there, else the newest.
pub fn mise_node(installs: &Path) -> Option<std::path::PathBuf> {
    mise_node_in(installs, &Exe::here())
}

/// Windows: `node.exe` is in the version's own directory, not a `bin`.
fn mise_node_in(installs: &Path, x: &Exe) -> Option<std::path::PathBuf> {
    let mut versions: Vec<(Vec<u32>, std::path::PathBuf)> = std::fs::read_dir(installs)
        .ok()?
        .flatten()
        .filter_map(|e| {
            let name = e.file_name().to_string_lossy().into_owned();
            let v: Vec<u32> = name.split('.').map(|p| p.parse().ok()).collect::<Option<_>>()?;
            let bin = if x.sep == ';' { e.path() } else { e.path().join("bin") };
            (v.len() == 3 && x.in_dir(&bin, "node").is_some()).then_some((v, bin))
        })
        .collect();
    versions.sort();
    versions.iter().rev().find(|(v, _)| v[0] == 22).or_else(|| versions.last()).map(|(_, b)| b.clone())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn commands_come_from_the_pins() {
        let claude = of(Kind::Claude).unwrap();
        assert_eq!(claude.name(), "@agentclientprotocol/claude-agent-acp");
        assert_eq!(claude.pinned(), CLAUDE_ACP.rsplit_once('@').unwrap().1);
        assert_eq!(
            claude.npm("~/.local/share/arugula/agents"),
            format!("npm install --prefix ~/.local/share/arugula/agents/claude {CLAUDE_ACP}")
        );
        assert_eq!(of(Kind::Codex).unwrap().npm("~/x"), format!("npm install --prefix ~/x/codex {CODEX_ACP}"));
        assert!(of(Kind::Managoat).is_none());
        assert_eq!(quote("/a b/it's"), r"'/a b/it'\''s'");
    }

    /// A hosted workspace's image (M177) installs the adapters this daemon
    /// pins, where it looks first.
    #[test]
    fn the_hosted_image_has_the_pins() {
        let image = include_str!("../../../../packaging/hosted/Dockerfile");
        for (arg, pin) in [("CLAUDE_ACP", CLAUDE_ACP), ("CODEX_ACP", CODEX_ACP)] {
            assert!(
                image.lines().any(|l| l == format!("ARG {arg}={pin}")),
                "packaging/hosted/Dockerfile: ARG {arg}={pin}"
            );
        }
        // M255: in /opt/arugula/agents, which session users can read,
        // linked from where the daemon looks first.
        assert!(image.contains("--prefix /opt/arugula/agents/claude"));
        // It has the `codex` CLI on PATH (the Dockerfile's `CODEX`), which
        // the adapter runs, so it leaves out the native copy it bundles.
        assert!(image.contains("--omit=optional --prefix /opt/arugula/agents/codex"));
        assert!(image.contains("npm install -g --no-fund --no-audit \"$CODEX\""));
        assert!(image.contains("ln -s /opt/arugula/agents /home/user/.local/share/arugula/agents"));
        assert_eq!(major("v22.23.2"), Some(22));
        assert_eq!(major("v8.1.0"), Some(8));
    }

    // Unix: a fake node made executable by its mode.
    #[cfg(unix)]
    #[test]
    fn reports_missing_and_installed() {
        // The agents directory is under home unless this is set.
        if std::env::var_os("ARUGULA_AGENTS_DIR").is_some() {
            return;
        }
        let dir = std::env::temp_dir().join(format!("arugula-adapters-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let home = &dir;
        let claude = of(Kind::Claude).unwrap();
        // No Node at all on this PATH.
        let bare = vec![("PATH".to_owned(), home.join("nothing").display().to_string())];
        let s = status(claude, home, &bare);
        assert_eq!(s.state, State::NoNode { found: None });
        assert!(s.why().contains("needs Node 20+"));
        assert_eq!(s.npm, format!("npm install --prefix ~/.local/share/arugula/agents/claude {CLAUDE_ACP}"));

        // A Node that says it's 22.
        let bin = home.join("bin");
        std::fs::create_dir_all(&bin).unwrap();
        let node = bin.join("node");
        std::fs::write(&node, "#!/bin/sh\necho v22.1.0\n").unwrap();
        std::fs::set_permissions(&node, std::os::unix::fs::PermissionsExt::from_mode(0o755)).unwrap();
        let env = vec![("PATH".to_owned(), bin.display().to_string())];
        let s = status(claude, home, &env);
        assert_eq!(s.state, State::Missing);
        assert_eq!(s.why(), "Claude Code's adapter isn't installed");
        assert_eq!(s.json()["state"], "missing");
        assert_eq!(s.json()["why"], "Claude Code's adapter isn't installed");

        // Installed in our directory, with its version.
        let pkg = agents_dir(home).join("claude/node_modules");
        std::fs::create_dir_all(pkg.join(".bin")).unwrap();
        std::fs::write(pkg.join(".bin/claude-agent-acp"), "").unwrap();
        std::fs::create_dir_all(pkg.join(claude.name())).unwrap();
        std::fs::write(pkg.join(claude.name()).join("package.json"), r#"{"version":"0.85.0"}"#).unwrap();
        let s = status(claude, home, &env);
        assert_eq!(s.state, State::Installed { version: Some("0.85.0".into()), on_path: false });
        assert_eq!(s.outdated(), claude.pinned() != "0.85.0");

        // #335: an older version than the pin is out of date, and says so.
        std::fs::write(pkg.join(claude.name()).join("package.json"), r#"{"version":"0.1.0"}"#).unwrap();
        let s = status(claude, home, &env);
        assert!(s.ok() && s.outdated() && !s.current());
        assert_eq!(s.json()["state"], "installed");
        assert_eq!(s.json()["outdated"], true);
        std::fs::write(pkg.join(claude.name()).join("package.json"), format!(r#"{{"version":"{}"}}"#, claude.pinned()))
            .unwrap();
        let s = status(claude, home, &env);
        assert!(s.current() && s.json()["outdated"] == false);

        // The install runs npm with the pin, into our directory.
        std::fs::write(bin.join("npm"), "").unwrap();
        let cmd = npm_install(claude, home, &bin.display().to_string()).unwrap();
        let args: Vec<_> = cmd.get_args().map(|a| a.to_string_lossy().into_owned()).collect();
        assert_eq!(args, ["install", "--prefix", &agents_dir(home).join("claude").display().to_string(), CLAUDE_ACP]);
        assert!(npm_install(claude, home, &home.join("nothing").display().to_string()).is_none());

        // Too old a Node.
        std::fs::write(&node, "#!/bin/sh\necho v18.2.0\n").unwrap();
        let s = status(claude, home, &env);
        assert_eq!(s.state, State::NoNode { found: Some("v18.2.0".into()) });
        assert_eq!(s.why(), "Claude Code's adapter needs Node 20+ (this machine has v18.2.0)");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// #1175: Windows's rules, here: a `;` PATH, `node.exe`, `npm.cmd` and
    /// npm's `.cmd` shim. (Unix: the fake `node.exe` runs by its mode.)
    #[cfg(unix)]
    #[test]
    fn finds_node_the_windows_way() {
        if std::env::var_os("ARUGULA_AGENTS_DIR").is_some() || std::env::var_os("ARUGULA_NPM").is_some() {
            return;
        }
        let dir = std::env::temp_dir().join(format!("arugula-adapters-win-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let home = &dir;
        let (nodejs, roaming) = (home.join("Program Files/nodejs"), home.join("AppData/Roaming/npm"));
        std::fs::create_dir_all(&nodejs).unwrap();
        std::fs::create_dir_all(&roaming).unwrap();
        let node = nodejs.join("node.exe");
        std::fs::write(&node, "#!/bin/sh\necho v22.20.0\n").unwrap();
        std::fs::set_permissions(&node, std::os::unix::fs::PermissionsExt::from_mode(0o755)).unwrap();
        // npm's sh script, beside the .cmd and .ps1 it makes on Windows.
        for f in ["npm", "npm.cmd", "npm.ps1"] {
            std::fs::write(nodejs.join(f), "").unwrap();
        }
        let path = format!("{};{}", roaming.display(), nodejs.display());
        let env = vec![("PATH".to_owned(), path.clone())];
        let win = Exe::windows(Some(".COM;.EXE;.BAT;.CMD;.VBS;.JS;.PS1"));
        let claude = of(Kind::Claude).unwrap();

        // What 0.70.4 did on Windows: no Node.
        assert_eq!(status_in(claude, home, &env, &Exe::unix()).state, State::NoNode { found: None });
        let s = status_in(claude, home, &env, &win);
        assert_eq!(s.state, State::Missing);
        assert_eq!(s.node_bin, None, "Node was on PATH: nothing to put first");

        // `arugula setup claude`'s install runs npm.cmd.
        let cmd = npm_install_in(claude, home, &path, &win).unwrap();
        assert_eq!(Path::new(cmd.get_program()), nodejs.join("npm.cmd"));

        // On PATH (npm's global shims are .cmd files).
        std::fs::write(roaming.join("claude-agent-acp.cmd"), "").unwrap();
        let s = status_in(claude, home, &env, &win);
        assert_eq!(s.state, State::Installed { version: None, on_path: true });
        std::fs::remove_file(roaming.join("claude-agent-acp.cmd")).unwrap();

        // Installed in our directory: npm made a .cmd shim in .bin.
        let pkg = agents_dir(home).join("claude/node_modules");
        std::fs::create_dir_all(pkg.join(".bin")).unwrap();
        std::fs::write(pkg.join(".bin/claude-agent-acp.cmd"), "").unwrap();
        std::fs::create_dir_all(pkg.join(claude.name())).unwrap();
        std::fs::write(pkg.join(claude.name()).join("package.json"), r#"{"version":"0.85.0"}"#).unwrap();
        assert_eq!(claude.local_bin_in(home, &win), pkg.join(".bin/claude-agent-acp.cmd"));
        let s = status_in(claude, home, &env, &win);
        assert_eq!(s.state, State::Installed { version: Some("0.85.0".into()), on_path: false });

        // A Node mise installed goes first, after a `;`. #1178: mise is in
        // %LOCALAPPDATA% there, `node.exe` in the version's directory.
        let bare = home.join("nothing").display().to_string();
        let unix = home.join(".local/share/mise/installs/node/22.20.0/bin");
        std::fs::create_dir_all(&unix).unwrap();
        std::fs::write(unix.join("node"), "").unwrap();
        let mut env = vec![("PATH".to_owned(), bare.clone())];
        with_node_on_path_in(&mut env, home, &win);
        assert_eq!(path_of(&env), bare, "a Unix mise isn't Windows's");
        let local = home.join("AppData/Local");
        let mise = local.join("mise/installs/node/22.20.0");
        std::fs::create_dir_all(&mise).unwrap();
        std::fs::write(mise.join("node.exe"), "").unwrap();
        // Its shims (.cmd files) aren't a Node.
        std::fs::create_dir_all(local.join("mise/shims")).unwrap();
        std::fs::write(local.join("mise/shims/node.cmd"), "").unwrap();
        let shims = format!("{};{bare}", local.join("mise/shims").display());
        for (local_env, path) in [(None, bare.clone()), (Some(local.display().to_string()), shims.clone())] {
            let mut env = vec![("PATH".to_owned(), path.clone())];
            env.extend(local_env.map(|l| ("LOCALAPPDATA".to_owned(), l)));
            with_node_on_path_in(&mut env, home, &win);
            assert_eq!(path_of(&env), format!("{};{path}", mise.display()));
        }
        // And Unix's, as it was.
        let mut env = vec![("PATH".to_owned(), bare.clone())];
        with_node_on_path_in(&mut env, home, &Exe::unix());
        assert_eq!(path_of(&env), format!("{}:{bare}", unix.display()));
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// #1175: the *Install* pane's command for PowerShell, a Windows pane's
    /// shell.
    #[test]
    fn the_install_pane_speaks_powershell() {
        if std::env::var_os("ARUGULA_NPM").is_some() {
            return;
        }
        let home = Path::new("/home/it's me");
        let claude = of(Kind::Claude).unwrap();
        let mut s = Status { adapter: claude, state: State::Missing, npm: String::new(), node_bin: None };
        let dir = agents_dir(home).join("claude").display().to_string().replace('\'', "''");
        assert_eq!(
            install_command_powershell(&s, home),
            format!(
                "npm.cmd install --prefix '{dir}' '{CLAUDE_ACP}'; if ($?) {{ echo ''; echo 'Installed {CLAUDE_ACP}. Start the agent again.' }}"
            )
        );
        s.node_bin = Some(PathBuf::from(r"C:\mise\node"));
        assert!(
            install_command_powershell(&s, home)
                .starts_with(r"$env:PATH = 'C:\mise\node' + ';' + $env:PATH; npm.cmd install ")
        );
        let codex = Status { adapter: of(Kind::Codex).unwrap(), ..s };
        let ps = install_command_powershell(&codex, home);
        assert!(ps.contains("npm.cmd install --prefix ") && !ps.contains("--omit=optional"), "{ps}");
        // The sh one is as it was.
        assert!(
            install_command_sh(&codex, home).starts_with("env \"PATH=C:\\mise\\node:$PATH\" npm install --prefix ")
        );
    }
}
