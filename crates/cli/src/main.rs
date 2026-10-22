//! `arugula`: drive arugulad from a shell or a script. Every command
//! talks to the daemon's HTTP API over its Unix socket (or another daemon's
//! URL, with `--host`); `--json` prints the API's answers as they are, for
//! programs.

#[cfg(test)]
mod answers;
mod ask;
mod attach;
mod cmd;
mod control;
mod fs;
mod hook;
mod hooks;
mod hosts;
mod http;
mod mcp;
// The daemon's named pipe as a stream (Windows).
#[cfg(windows)]
mod pipe;
mod ssh;
mod telemetry;
mod term;
mod util;
mod wake;

use std::path::PathBuf;

use arugula_proto::flags::{self, On};
use clap::{FromArgMatches, Parser, Subcommand};
use cmd::claude::ClaudeCmd;
use util::{Pane, REMOTE};

#[derive(Parser)]
#[command(version, about = "Drive arugulad: panes you can script")]
struct Cli {
    /// The daemon's socket [default: $ARUGULA_SOCK, else
    /// $XDG_STATE_HOME/arugula/sock].
    #[arg(long, global = true, env = "ARUGULA_SOCK")]
    socket: Option<PathBuf>,
    /// Talk to another daemon: a name from the local daemon's host list or
    /// from control's directory once this CLI is logged in (`arugula
    /// hosts` lists both), or a URL.
    #[arg(long, global = true, conflicts_with = "ssh")]
    host: Option<String>,
    /// Talk to the daemon on a box you can ssh into (`user@box`, or a Host
    /// from ~/.ssh/config), with your own ssh. Offers to install Arugula
    /// there if it's missing.
    #[arg(long, global = true, value_name = "DEST")]
    ssh: Option<String>,
    /// Print the API's JSON instead of a summary.
    #[arg(long, global = true)]
    json: bool,
    #[command(subcommand)]
    cmd: Command,
}

/// `mN`, `N` or `local`: a machine, not a daemon's name.
fn looks_like_machine(s: &str) -> bool {
    s == "local" || s.trim_start_matches('m').parse::<u32>().is_ok()
}

#[derive(Subcommand)]
enum Command {
    /// List panes.
    Ls,
    /// Run a command in a new tab (or split); prints its pane.
    Run(cmd::run::Args),
    /// Machines that panes run on.
    #[command(hide = true)]
    Machines,
    /// Files on a host, read-only.
    ///
    /// `ls`, `stat`, `cat`, `watch`, `recent`. `%N:PATH` is on the host pane %N
    /// runs on.
    Fs {
        #[command(subcommand)]
        cmd: fs::FsCmd,
    },
    /// Type `cd DIR` into a pane's shell, if it's waiting at its prompt.
    Cd { pane: Pane, dir: String },
    /// A block's type, place and state (any type).
    ///
    /// With `--detection`, how the screen of the agent in a terminal pane reads:
    /// each rule, the text it looked at, and which one fired. `describe --agents`:
    /// the agents configured on this machine, which decide whose screen rules run
    /// here.
    Describe(cmd::describe::Args),
    /// Call one of a block's methods.
    ///
    /// For example `call %4 navigate '{"url":"…"}'`.
    Call(cmd::call::Args),
    /// Open a browser block on a port or a web page, or a project's issue.
    ///
    /// `:PORT[/path]` is a port on its machine, `https://…` a web page.
    /// `iss_…` or `#N` is a project's own issue on Arugula control, as an
    /// issue block that reads it there.
    Open(cmd::open::Args),
    /// Open VS Code on a folder or a file; prints its block.
    ///
    /// The editor runs on the machine this pane runs on. A file opens in its
    /// project.
    Edit(cmd::edit::Args),
    /// Show what changed in a git repository.
    ///
    /// Opens a diff block, prints it, then its files with +/−. No revisions: the
    /// working tree (staged, unstaged, untracked) against HEAD; one: against that;
    /// two: the range. `%N` first: the repository pane %N is in, on its machine.
    Diff(cmd::diff::Args),
    /// Show a file in a file block, read-only and followed live.
    ///
    /// `PATH[:LINE]` here, or `%N:PATH[:LINE]` on the host pane %N runs on
    /// (relative to its directory). Prints its block.
    View(cmd::view::Args),
    /// Show a chant workspace as a block.
    ///
    /// Its members as cards to open shells, agents and diffs on, its records, and
    /// the gates waiting for a person, which are attention you approve (`call %N
    /// approve`) or turn down (`call %N expire`: `chant approve --expire`). Read
    /// through the workspace's own chant. Prints the block, then its members and
    /// gates.
    #[command(hide = true)]
    Workspace(cmd::workspace::Args),
    /// The agents this machine offers to other people's agents.
    ///
    /// Each is a Claude Code subagent file (`.claude/agents/NAME.md` in a
    /// project, or `~/.claude/agents/`), offered with the project it works in,
    /// and listed as an A2A agent card. `arugula agent --as NAME` runs one here.
    #[command(hide = true)]
    Agents(cmd::agents::Args),
    /// Show a pull request as a block.
    ///
    /// Forgejo through your `tea` login, GitHub through `gh`'s, a GitLab merge
    /// request through your `glab` login (or read-only without one): its checks,
    /// reviews and timeline, and what it waits on you for. `URL`,
    /// `OWNER/REPO#N`, `GROUP/PROJECT!N`, or `N` in this directory's repository.
    /// Prints the block, then the PR as text. `pr comment|review|merge|rerun %N`
    /// write to it; run by an agent (CLAUDECODE or AI_AGENT set), a write is a
    /// draft that waits for a person to send it. On a repo linked to a project
    /// on Arugula control, with no block: `pr view|checks OWNER/REPO#N`, `pr
    /// list`, and `pr comment|review OWNER/REPO#N` (`--draft` to keep it a
    /// draft).
    #[command(args_conflicts_with_subcommands = true)]
    Pr(cmd::pr::Args),
    /// Show an issue as a block.
    ///
    /// Forgejo through your `tea` login, GitHub through `gh`'s: its labels,
    /// assignees, linked pull requests and timeline. `URL`, `OWNER/REPO#N`, or
    /// `N` in this directory's repository. `issue new` opens one (run by an
    /// agent, it's a draft a person sends); `issue comment %N` comments; `issue
    /// agent %N` starts an agent on it in a worktree and branch of its own, in a
    /// tab with the issue. On a repo linked on Arugula control, with no block:
    /// `issue view|comment OWNER/REPO#N`, `issue list OWNER/REPO` and `issue
    /// create OWNER/REPO`.
    ///
    /// A project's own issues on Arugula control (no block, no daemon; needs
    /// `arugula login`): `issue new|list|show|comment|close|reopen|edit|export`,
    /// and `issue agent`. `#N` or `N` is a number in the current project,
    /// `iss_…` any issue you can read. The current project is `--project
    /// NAME|ID` (or `$ARUGULA_PROJECT`; `personal` is yours), else the project
    /// this directory's GitHub repo is linked to, else your personal one; a
    /// GitHub clone that none of your projects links keeps GitHub's issues.
    /// Its issue home says whether they're native or GitHub's; `--native` and
    /// `--github` override it. `issue list` on a GitHub home lists the linked
    /// repo's issues from Arugula control.
    #[command(args_conflicts_with_subcommands = true)]
    Issue(cmd::issue::Args),
    /// Type a pane's failed command again.
    ///
    /// Once its shell is waiting at its prompt.
    Rerun(cmd::rerun::Args),
    /// Claude Code conversations on this machine.
    ///
    /// From a terminal or the desktop app's Code tab. `open` shows one as an agent
    /// block; `arugula agent --resume ID` continues one.
    Claude {
        #[command(subcommand)]
        cmd: cmd::claude::ClaudeCmd,
    },
    /// Your studio.
    ///
    /// `login URL` keeps a studio token in the daemon (read from stdin), `logout`
    /// forgets it, and `follower APP` keeps a hud follower link for an app's box.
    #[command(hide = true)]
    Studio {
        #[command(subcommand)]
        cmd: Option<cmd::studio::StudioCmd>,
    },
    /// Open a studio app's box as a block; prints its block.
    ///
    /// With no name, lists your apps.
    #[command(hide = true)]
    App(cmd::app::Args),
    /// Editors in the swarm: VS Code, Cursor or nvim that joined.
    ///
    /// Also editor blocks. `editors install` adds Arugula's extension to VS Code
    /// or Cursor here (in a Remote-SSH window's terminal: there). It's also on
    /// the Visual Studio Marketplace and Open VSX as `arugula.arugula-editor`.
    Editors {
        #[command(subcommand)]
        cmd: Option<cmd::editors::EditorsCmd>,
    },
    /// arugulad as Claude Code's IDE.
    ///
    /// Its port, and which IDE gets Claude Code's diffs (`--diffs arugula`, or
    /// another IDE's name as it registered, e.g. "Visual Studio Code").
    Ide(cmd::ide::Args),
    /// Standing permission rules for agent blocks.
    ///
    /// What agent blocks on this daemon allow without asking, made by "Always" for
    /// a directory or for every block. `--forget N` forgets one; `--forget-all`,
    /// all of them.
    Rules(cmd::rules::Args),
    /// The shell environment blocks that run your tools get.
    ///
    /// Your login shell's, read once: its PATH. `--refresh` reads it again, after
    /// you change an rc file.
    ShellEnv(cmd::shell_env::Args),
    /// Start an agent block and send it a prompt; prints its block.
    ///
    /// Claude Code by default. Then: `wait %N --idle`, `tail %N`, `call %N
    /// approve`.
    Agent(Box<cmd::agent::Args>),
    /// Type text into a pane (`-` reads stdin).
    ///
    /// With `--wait`, it's a prompt for the agent there (Claude Code or Codex in
    /// the terminal, or an agent block): sent with Enter, then waited through.
    /// Prints what it came to and exits 0 when the turn ended, 2 when it needs
    /// someone (or already did, so nothing was typed), 3 when it stalled (no sign
    /// of work), 4 still running at --timeout.
    Send(cmd::send::Args),
    /// Press named keys: C-c, M-x, Up, Enter, F5, Space, ...
    Keys(cmd::keys::Args),
    /// Copy files onto the pane's host and paste their paths into it.
    ///
    /// For an agent there to read: a screenshot into a `claude` on another
    /// machine, say. Only into a shell or an agent unless `--force`; exits 2 if it
    /// wasn't pasted, printing the paths.
    Upload(cmd::upload::Args),
    /// Click, press, release or drag at a cell (from 1,1).
    Mouse(cmd::mouse::Args),
    /// Print a pane's output.
    Tail(cmd::tail::Args),
    /// Wait for a command, an exit, a match, or an agent.
    ///
    /// For a command to finish, the program to exit, or output to match.
    /// Exits with the command's exit code; 124 on timeout.
    Wait(cmd::wait::Args),
    /// Use a pane from this terminal (Ctrl-] to detach).
    Attach { pane: Option<Pane> },
    /// Export a pane's history as an asciicast (`asciinema play`).
    Export(cmd::export::Args),
    /// The pane's foreground process.
    Process(cmd::process::Args),
    /// What a pane shows: the screen, the scrollback or the last command.
    Capture(cmd::capture::Args),
    /// Events as they happen (NDJSON).
    Events(cmd::events::Args),
    /// Close a pane (ending what runs in it); its output stays in history.
    Close(cmd::close::Args),
    /// Claude Code's hook for its questions (PreToolUse on AskUserQuestion).
    ///
    /// Shows its questions as a card beside this pane (every client, with a push),
    /// waits, and prints the answer for Claude Code. Outside an Arugula pane, or
    /// "Answer in terminal": no output, so Claude Code shows its picker.
    Ask,
    /// Claude Code's hook for permission prompts.
    ///
    /// `PermissionRequest` becomes an approval card anyone who may answer can allow
    /// or deny; other events close a card the terminal answered first. Outside an
    /// Arugula pane: nothing.
    Hook,
    /// Claude Code's background hook for follow-ups (`Stop`, `SessionStart`).
    ///
    /// Waits for a follow-up someone sends the agent, and wakes it with it (exit
    /// 2). A session nobody drives (`claude -p`, the SDK) isn't held: it exits 0 at
    /// once.
    Inbox,
    /// Put Claude Code's hooks in its settings.json, or say which are there.
    ///
    /// `install` adds them, `status` says which are there. Nothing else in the
    /// file is touched.
    Hooks {
        #[command(subcommand)]
        cmd: hooks::HooksCmd,
    },
    /// What wants you, and why.
    ///
    /// Or, given a state, tell Arugula whether this pane needs you (for agent
    /// hooks, which pass their JSON on stdin: its `message` becomes the headline).
    Attention(cmd::attention::Args),
    /// Commands run in any pane, including recently closed ones
    ///
    /// Also the answers and approvals given there, and what agent blocks did
    /// (`--kind command|answer|agent`).
    History(cmd::history::Args),
    /// A pane's commands and who ran each.
    ///
    /// `--who`: who typed in it over time (each handoff).
    Log(cmd::log::Args),
    /// Search the output of every pane.
    Search(cmd::search::Args),
    /// A read-only link to a pane.
    ///
    /// Whoever opens it on the tailnet sees it live and can't type, resize or see
    /// anything else.
    Share(cmd::share::Args),
    /// ssh invites that still work.
    ///
    /// Made with `share --guest`. `guests revoke ID` ends one and cuts off
    /// anyone using it.
    #[command(hide = true)]
    Guests {
        #[command(subcommand)]
        cmd: Option<cmd::share::SharesCmd>,
    },
    /// Who else can reach which sessions.
    ///
    /// `access` lists grants, `access grant SESSION WHO ROLE`, `access revoke
    /// SESSION WHO`, `access log`. WHO is a tailnet login (or `account:ID` from
    /// control).
    Access {
        #[command(subcommand)]
        cmd: Option<cmd::access::AccessCmd>,
    },
    /// Bring someone into a session.
    ///
    /// Shares it with them (as a viewer unless --role says otherwise) and
    /// notifies them alone, opening at a pane. WHO is a tailnet login, someone already shared with, or (when
    /// joined to Arugula control) a member of your teams, by name.
    /// Prints whether the notification reached them: sent, pending (they
    /// haven't accepted this machine yet) or unreachable, and why.
    Invite(cmd::invite::Args),
    /// Share links that still work.
    ///
    /// `shares revoke ID` ends one.
    Shares {
        #[command(subcommand)]
        cmd: Option<cmd::share::SharesCmd>,
    },
    /// History other hosts synced here (kept encrypted).
    Synced {
        #[command(subcommand)]
        cmd: Option<cmd::synced::SyncedCmd>,
    },
    /// Open this machine's page in your browser, signed in.
    ///
    /// Programs and browsers on this machine show the daemon's local token; this
    /// opens a sign-in link that gives your browser it (once: it stays signed in).
    /// `--print` prints the link instead (it holds the token).
    Web(cmd::web::Args),
    /// Install the daemon.
    ///
    /// `arugulad install` with these arguments.
    Install(cmd::install::Args),
    /// Add a machine to your account on Arugula control.
    ///
    /// So the web, the phone and other machines reach it through control. With
    /// `--ssh user@box`: that box, set up over ssh first (Arugula installed, its
    /// daemon kept running after you log out); its code shows here, to approve from
    /// a signed-in device. Without: this machine.
    Join(cmd::join::Args),
    /// Make this CLI one of your devices on Arugula control.
    ///
    /// So `--host NAME` reaches every machine on your account, directly or through
    /// control's relay. Shows a code to approve on a signed-in device.
    Login(cmd::login::Args),
    /// Forget this CLI's key for control (`arugula login` makes a new one).
    Logout,
    /// What your machines' agents may do with your things on Arugula control.
    ///
    /// Every machine you join gets `cloud.drive`: the agents on it drive your
    /// Cloud workspaces. `add` lets them read and post in a project's threads
    /// or your DMs, or write a project's issues (off until you do). `revoke`
    /// takes grants from one machine, `restore` gives `cloud.drive` back; `ls`
    /// (the default) lists them. Needs `arugula login`.
    Grants {
        #[command(subcommand)]
        cmd: Option<cmd::grants::GrantsCmd>,
    },
    /// A project's hosted workspaces on Arugula control.
    ///
    /// `create NAME` makes one (owner or editor on the project; `--repo`,
    /// `--package`, `--setup` and `--env` say what it's built from), `ls`
    /// lists them, `open` wakes one and says its machine, `rebuild` makes
    /// its box again from its spec, `delete` takes it and its box away.
    /// `--project` names a project by id or name; the default is your
    /// personal one. Needs `arugula login`.
    Cloud {
        #[command(subcommand)]
        cmd: cmd::cloud::CloudCmd,
    },
    /// A project's linked GitHub repos on Arugula control.
    ///
    /// `link OWNER/REPO` links one (you need admin or maintain on it on
    /// GitHub), `unlink` takes it out, `ls` lists them. `--project` names a
    /// project by id or name; the default is your personal one. Needs
    /// `arugula login`.
    Project {
        #[command(subcommand)]
        cmd: cmd::project::ProjectCmd,
    },
    /// Drafts on your linked repos' PRs and issues, on Arugula control.
    ///
    /// Comments, reviews and new issues that you (`--draft`) or your agents
    /// wrote, waiting for a person to send them: `ls` lists them (on
    /// `OWNER/REPO#N`, `OWNER/REPO`, a project, or your own), `show` shows
    /// one, `discard` drops it and `send` sends it as you. Needs `arugula
    /// login`.
    Draft {
        #[command(subcommand)]
        cmd: cmd::forge_ops::DraftCmd,
    },
    /// Your inbox of PRs and issues on linked repos, on Arugula control.
    ///
    /// What wants you there (a review requested, your checks failed,
    /// changes requested, a mention, assigned, merged or ready, a draft
    /// waiting): `inbox` lists it, `seen` and `done` mark an item's
    /// entries. Needs `arugula login`. (`arugula inbox` is Claude Code's
    /// follow-ups.)
    Forge {
        #[command(subcommand)]
        cmd: cmd::forge_ops::ForgeCmd,
    },
    /// How Arugula is doing on this machine.
    ///
    /// The daemon (its version, the service that runs it, its binary and
    /// log), whether and where it's joined to Arugula control
    /// (connected, or dropped by control), each agent's adapter and
    /// whether Claude Code has Arugula's MCP server. Exits 1 when the
    /// daemon doesn't answer or control dropped it.
    Status,
    /// Use Claude Code (or Codex) with arugula.
    ///
    /// Installs the ACP adapter agent blocks run it through, at the version
    /// this daemon pins (or updates an older one), and for Claude Code adds
    /// Arugula's MCP server, so it can start its helpers as panes. Says
    /// what changed. Getting started's Agents step does the same.
    Setup(cmd::setup::Args),
    /// Join stdin and stdout to this daemon's socket.
    ///
    /// On a box a client reaches over ssh (`--ssh`). Clients run it; people
    /// don't.
    #[command(hide = true)]
    Bridge {
        /// Print what's installed and whether the daemon answers, as JSON.
        #[arg(long)]
        probe: bool,
    },
    /// Other daemons to switch to.
    ///
    /// This daemon's host list.
    Hosts {
        #[command(subcommand)]
        cmd: Option<hosts::HostsCmd>,
    },
    /// The sandbox provider's sandboxes.
    ///
    /// This daemon's: open a shell on one (`run --sandbox`), or make a daemon
    /// resident there.
    #[command(hide = true)]
    Sandboxes {
        #[command(subcommand)]
        cmd: Option<hosts::SandboxesCmd>,
    },
    /// An MCP server on stdio, for agents that start one as a command.
    ///
    /// `claude mcp add arugula -- arugula mcp`: Arugula's tools, bridged to
    /// the daemon's `/mcp`. `mcp token` makes tokens for clients that reach `/mcp`
    /// over HTTP without a tailnet identity.
    Mcp(cmd::mcp::Args),
    /// Usage reports from this machine: on, off, or what would be sent.
    ///
    /// Off until you say yes. `status` prints the next report exactly as it
    /// would go (`--json` for the daemon's answer). Usage counts and crash
    /// reports only: never terminal contents, file names, commands or
    /// prompts.
    Telemetry {
        #[command(subcommand)]
        cmd: telemetry::TelemetryCmd,
    },
}

impl Cli {
    /// Whether this command may ask the one-time telemetry question first:
    /// one a person runs on the local daemon and reads at a terminal. Not
    /// JSON for a program, MCP, `arugula telemetry` itself, or an agent's
    /// hooks.
    fn may_ask(&self) -> bool {
        !self.json
            && !matches!(
                self.cmd,
                Command::Telemetry { .. }
                    | Command::Mcp(_)
                    | Command::Events(_)
                    | Command::Bridge { .. }
                    | Command::Ask
                    | Command::Hook
                    | Command::Inbox
            )
    }
}

/// The one-time question (M105), before the command runs, when
/// `telemetry::should_ask` says so.
fn ask_about_telemetry(cli: &Cli) {
    use std::io::IsTerminal;
    let Some(dir) = state_dir() else { return };
    let facts = telemetry::Facts {
        tty: std::io::stdin().is_terminal() && std::io::stderr().is_terminal(),
        local: cli.host.is_none() && cli.ssh.is_none(),
        agent: http::agent(),
        command: cli.may_ask(),
        why: arugula_proto::telemetry::enabled(&dir).why,
    };
    if telemetry::should_ask(&facts) {
        telemetry::first_run(&http::Target::Socket(socket(cli)), &dir);
    }
}

/// The local daemon's state directory, resolved as the daemon and the desktop
/// app do: `ARUGULA_STATE_DIR`, then on Windows
/// `%LOCALAPPDATA%\arugula\state`, then `$XDG_STATE_HOME/arugula`, then
/// `~/.local/state/arugula`.
fn state_dir() -> Option<PathBuf> {
    arugula_proto::dirs::state_dir()
}

/// Commands and options that work but stay out of `--help` unless the machine
/// has turned their Labs flag on: they're for what a stranger doesn't have.
/// Others that are `hide = true` are internals, and stay hidden.
const LABS_COMMANDS: &[(&str, &str)] = &[
    ("studio", flags::STUDIO),
    ("app", flags::STUDIO),
    ("workspace", flags::WORKSPACES),
    ("agents", flags::AGENTS),
    ("guests", flags::GUEST_SSH),
    ("machines", flags::VMS),
    ("sandboxes", flags::VMS),
];
const LABS_OPTIONS: [(&str, &[&str], &str); 6] = [
    ("agent", &["recipe"], flags::AGENTS),
    ("agent", &["vm", "machine"], flags::VMS),
    ("run", &["vm", "vm_tab", "image", "sandbox"], flags::VMS),
    ("share", &["guest", "rw", "reusable", "relay", "addr", "name"], flags::GUEST_SSH),
    ("open", &["machine"], flags::VMS),
    ("edit", &["machine"], flags::VMS),
];

/// Machines (`mN`) are Labs too: with the `vms` flag, `fs` and `view` say
/// their paths take one.
fn labs_machine_paths(cmd: clap::Command) -> clap::Command {
    cmd.mut_subcommand("fs", |fs| {
        fs.long_about(
            "Files on a host, read-only.\n\n`ls`, `stat`, `cat`, `watch`, `recent`. `%N:PATH` is on the host pane %N \
             runs on, `mN:PATH` on machine N.",
        )
        .mut_subcommand("ls", |ls| {
            ls.mut_arg("path", |a| a.help("`PATH`, `%N:PATH` or `mN:PATH` [default: the home directory]."))
        })
        .mut_subcommand("recent", |r| r.about("Directories used lately on a host (`%N` or `mN`; default this one)."))
    })
    .mut_subcommand("view", |v| {
        v.long_about(
            "Show a file in a file block, read-only and followed live.\n\n`PATH[:LINE]` here, `%N:PATH[:LINE]` on \
             the host pane %N runs on (relative to its directory), `mN:PATH[:LINE]` on machine N. Prints its block.",
        )
    })
}

/// The command line, listing in `--help` what the flags that are `on`
/// turn on.
fn labs_command(on: On) -> clap::Command {
    use clap::CommandFactory;
    let mut cmd = Cli::command();
    for (name, _) in LABS_COMMANDS.iter().filter(|(_, f)| on.has(f)) {
        cmd = cmd.mut_subcommand(*name, |s| s.hide(false));
    }
    for (name, opts, _) in LABS_OPTIONS.iter().filter(|(_, _, f)| on.has(f)) {
        cmd = cmd.mut_subcommand(*name, |s| opts.iter().fold(s, |s, o| s.mut_arg(*o, |a| a.hide(false))));
    }
    if on.has(flags::VMS) {
        cmd = labs_machine_paths(cmd);
    }
    cmd
}

fn socket(cli: &Cli) -> PathBuf {
    cli.socket.clone().unwrap_or_else(default_socket)
}

fn default_socket() -> PathBuf {
    if let Some(s) = std::env::var_os("ARUGULA_SOCK") {
        return PathBuf::from(s);
    }
    // The daemon records its socket beside its state: Windows' named pipe,
    // or one put elsewhere because the directory is too deep for a socket.
    let state = arugula_proto::dirs::default_state_dir().unwrap_or_else(|| PathBuf::from(".local/state/arugula"));
    match std::fs::read_to_string(state.join("sock.path")) {
        Ok(p) if !p.trim().is_empty() => PathBuf::from(p.trim()),
        _ => state.join("sock"),
    }
}

/// The version, findable in the binary's bytes: the testnet tests read it
/// from a box's static build they can't run here (#259).
#[used]
static VERSION_MARK: &str = concat!("\0arugula-version=", env!("CARGO_PKG_VERSION"), "\0");

fn main() {
    // A closed pipe (`| head`) ends the process quietly, not with a panic
    // from `println!` (#682): Rust starts with SIGPIPE ignored.
    // SAFETY: nothing else runs yet, and SIG_DFL is no handler.
    #[cfg(unix)]
    unsafe {
        let _ = nix::sys::signal::signal(nix::sys::signal::Signal::SIGPIPE, nix::sys::signal::SigHandler::SigDfl);
    }
    // A panic is written down as a crash, in the local daemon's state, if
    // telemetry is on there (M102): the daemon sends it.
    if let Some(dir) = state_dir() {
        use arugula_proto::telemetry::crash;
        crash::install_hook(crash::Process::Cli, env!("CARGO_PKG_VERSION"), &dir);
    }
    // Run as `tmux` (a link left from when the CLI was tmux's control mode
    // for iTerm2, removed 2026-10-18): say so rather than parse tmux's
    // arguments as ours.
    let argv0 = std::env::args_os().next().map(PathBuf::from);
    let name = argv0.as_deref().and_then(|p| p.file_name());
    if name.is_some_and(|n| n == "tmux" || cfg!(windows) && n.eq_ignore_ascii_case("tmux.exe")) {
        eprintln!(
            "tmux (arugula): Arugula is no longer a tmux control-mode server; remove this link to the arugula CLI ({})",
            argv0.as_deref().map(|p| p.display().to_string()).unwrap_or_default()
        );
        std::process::exit(1);
    }
    // What a stranger doesn't get shows in `--help` where this machine has
    // turned its flag on; everything works either way.
    let on = state_dir().as_deref().map(flags::on).unwrap_or_default();
    let cli = Cli::from_arg_matches(&labs_command(on).get_matches()).unwrap_or_else(|e| e.exit());
    match real_main(cli) {
        Ok(code) => std::process::exit(code),
        Err(e) => {
            eprintln!("arugula: {e:#}");
            std::process::exit(1);
        }
    }
}

fn real_main(mut cli: Cli) -> anyhow::Result<i32> {
    ask_about_telemetry(&cli);
    if let Command::Join(args) = &cli.cmd {
        return cmd::join::run(args, &cli);
    }
    if let Command::Login(args) = &cli.cmd {
        return cmd::login::run(args, &cli);
    }
    if let Command::Logout = cli.cmd {
        control::logout()?;
        return Ok(0);
    }
    if let Command::Grants { cmd } = cli.cmd {
        return cmd::grants::run(cmd, cli.json);
    }
    if let Command::Cloud { cmd } = cli.cmd {
        return cmd::cloud::run(cmd, cli.json);
    }
    if let Command::Project { cmd } = cli.cmd {
        return cmd::project::run(cmd, cli.json);
    }
    if let Command::Draft { cmd } = cli.cmd {
        return cmd::forge_ops::draft(cmd, cli.json);
    }
    if let Command::Forge { cmd } = cli.cmd {
        return cmd::forge_ops::forge(cmd, cli.json);
    }
    // A linked repo's PRs and issues on control (M169): no daemon.
    if let Command::Pr(args) = &cli.cmd
        && let Some(done) = cmd::pr::on_control(args, cli.json)
    {
        return done;
    }
    // A linked repo's issues on control (M169), and a project's own on the
    // platform (M199): no daemon.
    if let Command::Issue(args) = &mut cli.cmd
        && let Some(done) = cmd::issue::on_control(args, cli.json).or_else(|| cmd::issue::on_platform(args, cli.json))
    {
        return done;
    }
    if let Command::Bridge { probe } = cli.cmd {
        // On a box, for a client that ssh'd in: this daemon's socket on
        // stdin and stdout.
        return ssh::bridge(&socket(&cli), probe);
    }
    if let Command::Install(args) = &cli.cmd {
        return cmd::install::run(args);
    }
    if let Command::Ask = cli.cmd {
        // A hook: the local daemon only, and never an error.
        return Ok(ask::run(http::Target::Socket(socket(&cli))));
    }
    if let Command::Hook = cli.cmd {
        return Ok(hook::run(http::Target::Socket(socket(&cli))));
    }
    if let Command::Web(args) = &cli.cmd
        && cli.ssh.is_none()
    {
        // The local daemon's own link, over its socket (only ours); with
        // --ssh, the box's, below.
        return cmd::web::web(&http::Target::Socket(socket(&cli)), args.print);
    }
    if let Command::Inbox = cli.cmd {
        return Ok(hook::inbox(http::Target::Socket(socket(&cli))));
    }
    if let Command::Hooks { cmd } = cli.cmd {
        // A settings file: no daemon involved.
        return hooks::run(cmd, cli.json);
    }
    // `claude ls --host all` (#78): every host's, as each answers.
    if cli.host.as_deref() == Some("all")
        && let Command::Claude { cmd: ClaudeCmd::Ls { all, live, cwd, limit, words } } = &cli.cmd
    {
        return cmd::claude::claude_ls_all(
            socket(&cli),
            cmd::claude::conversations_path(*all, *live, cwd.clone(), *limit, words),
            cli.json,
        );
    }
    let reads_history = matches!(cli.cmd, Command::History(_) | Command::Search(_) | Command::Tail(_));
    let resolved = match &cli.ssh {
        Some(dest) => ssh::Remote::parse(dest).map(http::Target::Ssh),
        None => hosts::target(socket(&cli), cli.host.as_deref()),
    };
    let (sock, gone) = match resolved {
        Ok(t) => (t, None),
        // A host that's gone (deleted, unreachable) may have left its
        // history here.
        Err(e) if reads_history && cli.host.is_some() => {
            eprintln!("arugula: {e:#}; reading what it synced here instead");
            (http::Target::Socket(socket(&cli)), cli.host.clone())
        }
        // `open --host m2` meant a machine: the flag is `--machine` (#61).
        Err(e)
            if cli.host.as_deref().is_some_and(looks_like_machine)
                && matches!(cli.cmd, Command::Open(_) | Command::Edit(_) | Command::Agent(_)) =>
        {
            let m = cli.host.as_deref().unwrap_or_default();
            return Err(e.context(format!("--host is another daemon; for machine {m}, use --machine {m}")));
        }
        Err(e) => return Err(e),
    };
    REMOTE.store(!matches!(sock, http::Target::Socket(_)), std::sync::atomic::Ordering::Relaxed);
    // Over ssh: the master, Arugula installed there, its daemon up.
    if let http::Target::Ssh(r) = &sock {
        r.prepare()?;
    }
    let json_out = cli.json;
    // `run --home`: the local daemon too, and the host's name in its list.
    let (local_sock, host_name) = (socket(&cli), cli.host.clone());
    let ctx = cmd::Ctx { sock, json_out, local_sock, host_name, gone };
    match cli.cmd {
        Command::Share(args) => cmd::share::run(args, ctx),
        Command::Guests { cmd } => cmd::share::guests(cmd, ctx),
        Command::Access { cmd } => cmd::access::run(cmd, ctx),
        Command::Invite(args) => cmd::invite::run(args, ctx),
        Command::Shares { cmd } => cmd::share::shares(cmd, ctx),
        Command::Mcp(args) => cmd::mcp::run(args, ctx),
        Command::Synced { cmd } => cmd::synced::run(cmd, ctx),
        Command::Hosts { cmd } => {
            hosts::run(&ctx.sock, cmd, ctx.json_out, util::duration)?;
            Ok(0)
        }
        Command::Sandboxes { cmd } => {
            hosts::sandboxes(&ctx.sock, cmd, ctx.json_out)?;
            Ok(0)
        }
        // Only with --ssh: the box daemon's link, over the bridge to its
        // socket.
        Command::Web(args) if matches!(ctx.sock, http::Target::Ssh(_)) => cmd::web::web(&ctx.sock, args.print),
        Command::Install(_)
        | Command::Web(_)
        | Command::Bridge { .. }
        | Command::Join(_)
        | Command::Login(_)
        | Command::Logout
        | Command::Grants { .. }
        | Command::Cloud { .. }
        | Command::Project { .. }
        | Command::Draft { .. }
        | Command::Forge { .. } => {
            unreachable!("handled before connecting")
        }
        Command::Ls => cmd::ls::run(ctx),
        Command::Status => cmd::status::run(ctx),
        Command::Setup(args) => cmd::setup::run(args, ctx),
        Command::Describe(args) => cmd::describe::run(args, ctx),
        Command::Call(args) => cmd::call::run(args, ctx),
        Command::Open(args) => cmd::open::run(args, ctx),
        Command::Edit(args) => cmd::edit::run(args, ctx),
        Command::Diff(args) => cmd::diff::run(args, ctx),
        Command::View(args) => cmd::view::run(args, ctx),
        Command::Workspace(args) => cmd::workspace::run(args, ctx),
        Command::Agents(args) => cmd::agents::run(args, ctx),
        Command::Pr(args) => cmd::pr::run(args, ctx),
        Command::Issue(args) => cmd::issue::run(args, ctx),
        Command::Rerun(args) => cmd::rerun::run(args, ctx),
        Command::Agent(args) => cmd::agent::run(*args, ctx),
        Command::Claude { cmd } => cmd::claude::run(cmd, ctx),
        Command::Studio { cmd } => cmd::studio::run(cmd, ctx),
        Command::App(args) => cmd::app::run(args, ctx),
        Command::Editors { cmd } => cmd::editors::run(cmd, ctx),
        Command::Rules(args) => cmd::rules::run(args, ctx),
        Command::Ide(args) => cmd::ide::run(args, ctx),
        Command::ShellEnv(args) => cmd::shell_env::run(args, ctx),
        Command::Machines => cmd::machines::run(ctx),
        Command::Fs { cmd } => fs::run(&ctx.sock, cmd, ctx.json_out, REMOTE.load(std::sync::atomic::Ordering::Relaxed)),
        Command::Cd { pane, dir } => fs::cd(&ctx.sock, pane.0, &dir),
        Command::Run(args) => cmd::run::run(args, ctx),
        Command::Send(args) => cmd::send::run(args, ctx),
        Command::Upload(args) => cmd::upload::run(args, ctx),
        Command::Keys(args) => cmd::keys::run(args, ctx),
        Command::Mouse(args) => cmd::mouse::run(args, ctx),
        Command::Tail(args) => cmd::tail::run(args, ctx),
        Command::Wait(args) => cmd::wait::run(args, ctx),
        Command::Attach { pane } => attach::run(&ctx.sock, util::here(pane)?),
        Command::Telemetry { cmd } => telemetry::run(cmd, ctx, state_dir().as_deref()),
        Command::Export(args) => cmd::export::run(args, ctx),
        Command::Process(args) => cmd::process::run(args, ctx),
        Command::Capture(args) => cmd::capture::run(args, ctx),
        Command::Events(args) => cmd::events::run(args, ctx),
        Command::Close(args) => cmd::close::run(args, ctx),
        Command::Ask | Command::Hook | Command::Inbox | Command::Hooks { .. } => {
            unreachable!("handled first")
        }
        Command::Attention(args) => cmd::attention::run(args, ctx),
        Command::History(args) => cmd::history::run(args, ctx),
        Command::Log(args) => cmd::log::run(args, ctx),
        Command::Search(args) => cmd::search::run(args, ctx),
    }
}

#[cfg(test)]
mod tests {
    /// The command line with every Labs flag on, or none.
    fn labs_command(labs: bool) -> clap::Command {
        use arugula_proto::flags::On;
        super::labs_command(if labs { On::all() } else { On::none() })
    }

    #[test]
    fn machine_is_not_the_global_host() {
        use clap::Parser;
        let parse = |a: &[&str]| super::Cli::try_parse_from(a).unwrap();
        let c = parse(&["arugula", "--host", "box", "open", "--machine", "m2", ":3000"]);
        assert_eq!(c.host.as_deref(), Some("box"));
        assert!(matches!(c.cmd, super::Command::Open(ref a) if a.machine.as_deref() == Some("m2")));
        let c = parse(&["arugula", "agent", "--machine", "3", "hi"]);
        assert!(c.host.is_none());
        assert!(matches!(c.cmd, super::Command::Agent(ref a) if a.machine.as_deref() == Some("3")));
        assert!(super::Cli::try_parse_from(["arugula", "agent", "--vm", "--machine", "m3"]).is_err());
        assert!(super::looks_like_machine("m2") && super::looks_like_machine("local"));
        assert!(!super::looks_like_machine("box"));
    }

    /// What a newcomer can't use stays out of the help, and still works for
    /// anyone who knows the command.
    #[test]
    fn what_a_stranger_cant_use_is_hidden_but_there() {
        use clap::{CommandFactory, Parser};
        let mut root = super::Cli::command();
        let listed = root.render_long_help().to_string();
        let listed: Vec<&str> =
            listed.lines().filter_map(|l| l.strip_prefix("  ")?.split_whitespace().next()).collect();
        for hidden in super::LABS_COMMANDS.iter().map(|(c, _)| *c) {
            assert!(root.find_subcommand(hidden).unwrap().is_hide_set(), "{hidden} isn't hidden");
            assert!(!listed.contains(&hidden), "{hidden} is in `arugula --help`");
            // Asking for it by name still gives its help.
            let e = super::Cli::try_parse_from(["arugula", hidden, "--help"]).err().expect("help is an early exit");
            assert_eq!(e.kind(), clap::error::ErrorKind::DisplayHelp, "{hidden}");
        }
        assert!(listed.contains(&"run") && listed.contains(&"pr"), "{listed:?}");
        // Options: out of the subcommand's help, still parsed.
        for (cmd, opts) in [
            ("agent", &["recipe", "vm", "machine"][..]),
            ("run", &["vm", "vm_tab", "image", "sandbox"][..]),
            ("share", &["guest", "rw", "reusable", "relay", "addr", "name"][..]),
            ("open", &["machine"][..]),
            ("edit", &["machine"][..]),
        ] {
            let sub = root.find_subcommand(cmd).unwrap();
            for o in opts {
                let arg = sub.get_arguments().find(|a| a.get_id() == o).unwrap_or_else(|| panic!("{cmd} has no {o}"));
                assert!(arg.is_hide_set(), "{cmd} {o} isn't hidden");
            }
        }
        // Daemon-side Managoat was removed: its command and options are gone.
        assert!(super::Cli::try_parse_from(["arugula", "agent", "--managoat", "x", "hi"]).is_err());
        assert!(super::Cli::try_parse_from(["arugula", "managoat", "status"]).is_err());
        assert!(super::Cli::try_parse_from(["arugula", "run", "--vm", "--", "make"]).is_ok());
        assert!(super::Cli::try_parse_from(["arugula", "share", "--guest", "--rw", "%3"]).is_ok());
        assert!(super::Cli::try_parse_from(["arugula", "guests"]).is_ok());
    }

    /// With the `labs` file the help lists what #342 hid, all of it and
    /// nothing more: the internals stay hidden, and everything parses either
    /// way. The test passes the bool; it reads neither the filesystem nor the
    /// environment.
    #[test]
    fn labs_unhides_the_labs_set_and_only_that() {
        // What `--help` hides, as `command` or `command --option`.
        fn hidden(c: &clap::Command) -> Vec<String> {
            let mut out = vec![];
            for a in c.get_arguments().filter(|a| a.is_hide_set()) {
                out.push(format!("--{}", a.get_id()));
            }
            for sub in c.get_subcommands() {
                if sub.is_hide_set() {
                    out.push(sub.get_name().to_owned());
                }
                for h in hidden(sub) {
                    out.push(format!("{} {h}", sub.get_name()));
                }
            }
            out.sort();
            out
        }
        let (off, on) = (hidden(&labs_command(false)), hidden(&labs_command(true)));
        let mut set: Vec<String> = super::LABS_COMMANDS.iter().map(|(c, _)| (*c).to_owned()).collect();
        for (cmd, opts) in [
            ("agent", &["recipe", "vm", "machine"][..]),
            ("run", &["vm", "vm_tab", "image", "sandbox"][..]),
            ("share", &["guest", "rw", "reusable", "relay", "addr", "name"][..]),
            ("open", &["machine"][..]),
            ("edit", &["machine"][..]),
        ] {
            set.extend(opts.iter().map(|o| format!("{cmd} --{o}")));
        }
        for h in &set {
            assert!(off.contains(h), "{h} isn't hidden without labs");
            assert!(!on.contains(h), "{h} is still hidden with labs");
        }
        // Only those: the rest of what is hidden is hidden both ways.
        let rest: Vec<&String> = off.iter().filter(|h| !set.contains(h)).collect();
        assert!(rest.iter().any(|h| *h == "bridge"), "{rest:?}");
        assert_eq!(on.iter().collect::<Vec<_>>(), rest, "labs unhides something that isn't in its set");
        assert_eq!(off.len(), set.len() + rest.len());

        // The help lists them with labs and not without.
        let listed = |labs: bool| {
            let help = labs_command(labs).render_long_help().to_string();
            help.lines()
                .filter_map(|l| l.strip_prefix("  ")?.split_whitespace().next().map(str::to_owned))
                .collect::<Vec<_>>()
        };
        let (without, with) = (listed(false), listed(true));
        for name in super::LABS_COMMANDS.iter().map(|(c, _)| *c) {
            assert!(!without.iter().any(|l| l == name), "{name} in `arugula --help` without labs");
            assert!(with.iter().any(|l| l == name), "{name} isn't in `arugula --help` with labs");
        }
        assert!(!with.iter().any(|l| l == "bridge"), "an internal is listed with labs");
        let sub_help = |labs: bool, cmd: &str| {
            let mut root = labs_command(labs);
            root.find_subcommand_mut(cmd).unwrap().render_long_help().to_string()
        };
        for (cmd, flag) in [
            ("run", "--vm-tab"),
            ("share", "--guest"),
            ("agent", "--machine"),
            ("open", "--machine"),
            ("edit", "--machine"),
            // Machines' paths.
            ("fs", "mN:PATH"),
            ("view", "mN:PATH"),
        ] {
            assert!(!sub_help(false, cmd).contains(flag), "{cmd} {flag} without labs");
            assert!(sub_help(true, cmd).contains(flag), "{cmd} {flag} with labs");
        }
        let fs_help = |labs: bool, sub: &str| {
            let mut root = labs_command(labs);
            root.find_subcommand_mut("fs").unwrap().find_subcommand_mut(sub).unwrap().render_long_help().to_string()
        };
        for sub in ["ls", "recent"] {
            assert!(!fs_help(false, sub).contains("mN"), "fs {sub}'s mN without labs");
            assert!(fs_help(true, sub).contains("mN"), "fs {sub}'s mN with labs");
        }
        // And either way they parse.
        for labs in [false, true] {
            use clap::FromArgMatches;
            let parse =
                |a: &[&str]| super::Cli::from_arg_matches(&labs_command(labs).try_get_matches_from(a).unwrap()).is_ok();
            assert!(parse(&["arugula", "run", "--vm", "--", "make"]));
            assert!(parse(&["arugula", "guests"]));
        }
    }

    /// The help is for strangers: no milestone or issue numbers, no names of
    /// our own machines, and each command's summary is one short line.
    #[test]
    fn help_has_no_internal_numbers_or_names_and_short_summaries() {
        use clap::CommandFactory;
        fn walk(c: &mut clap::Command, path: &str, bad: &mut Vec<String>) {
            let help = c.render_long_help().to_string();
            let bytes = help.as_bytes();
            for (at, ch) in help.char_indices() {
                let before_ok = at == 0 || !bytes[at - 1].is_ascii_alphanumeric();
                let digits = |from: usize| help[from..].chars().take_while(|c| c.is_ascii_digit()).count();
                // M36, S18 (a letter and two digits) and #166.
                let milestone = matches!(ch, 'M' | 'S') && before_ok && digits(at + 1) >= 2;
                let issue = ch == '#' && digits(at + 1) >= 2;
                if milestone || issue {
                    bad.push(format!("{path}: {}", &help[at..(at + 12).min(help.len())]));
                }
            }
            for name in ["geek", "jake-mini", "arugula-salad"] {
                if help.contains(name) {
                    bad.push(format!("{path}: {name}"));
                }
            }
            for sub in c.get_subcommands_mut() {
                if let Some(about) = sub.get_about() {
                    let about = about.to_string();
                    if about.contains('\n') || about.len() > 90 {
                        bad.push(format!("{path} {}: summary {about:?}", sub.get_name()));
                    }
                }
                let name = format!("{path} {}", sub.get_name());
                walk(sub, &name, bad);
            }
        }
        let mut bad = vec![];
        walk(&mut super::Cli::command(), "arugula", &mut bad);
        assert!(bad.is_empty(), "{bad:#?}");
    }

    /// The README doesn't carry the milestone log, and opens with the agents
    /// pitch, as the site does. The docs for users are checked the same way
    /// where they live now (arugula-salad/site, scripts/check-code.mjs).
    #[test]
    fn the_readme_has_no_milestone_numbers() {
        let docs = [("README.md", include_str!("../../../README.md"))];
        for (name, text) in docs {
            let b = text.as_bytes();
            for (at, _) in text.match_indices('M') {
                let word_start = at == 0 || !b[at - 1].is_ascii_alphanumeric();
                let digits = b[at + 1..].iter().take_while(|c| c.is_ascii_digit()).count();
                assert!(!(word_start && digits >= 2), "{name}: {}", &text[at..(at + 20).min(text.len())]);
            }
            for hash in text.match_indices(" (#") {
                let n = text[hash.0 + 3..].chars().take_while(|c| c.is_ascii_digit()).count();
                assert!(n == 0, "{name}: {}", &text[hash.0..(hash.0 + 20).min(text.len())]);
            }
        }
        let readme = docs[0].1;
        let first = readme.lines().skip(2).take_while(|l| !l.is_empty()).collect::<Vec<_>>().join(" ");
        assert!(first.starts_with("Keep track of your agents without checking every session."), "{first}");
    }
}
