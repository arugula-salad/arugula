//! The command line: parsing and help text, with tests for labs and help formatting.

use std::{net::SocketAddr, path::PathBuf};

use arugula_proto::flags::{self, On};
use clap::{Parser, Subcommand};

#[derive(Parser, Debug)]
#[command(version, about = "Arugula daemon: owns terminals that clients attach to")]
pub(crate) struct Args {
    #[command(subcommand)]
    pub(crate) command: Option<Command>,

    #[command(flatten)]
    pub(crate) run: RunArgs,
}

#[derive(Subcommand, Debug)]
pub enum Command {
    /// Install as a service that starts at boot (a systemd user service) or
    /// at login (a launchd agent on macOS): copies this binary to
    /// ~/.local/bin, writes the unit or plist, enables and (re)starts it. On
    /// a Mac with no GUI login (reached over ssh) the agent runs in the
    /// background session: it outlives the ssh login but not a reboot;
    /// --system starts it at boot instead.
    Install {
        /// Write and enable the unit without starting it now.
        #[arg(long)]
        no_start: bool,
        /// Only put arugulad and arugula in ~/.local/bin, and that on your
        /// shell's PATH: no service (a machine without systemd).
        #[arg(long, conflicts_with_all = ["system", "no_start"])]
        no_service: bool,
        /// macOS: a LaunchDaemon that runs it as you from boot, with nobody
        /// logged in (/Library/LaunchDaemons/arugulad.USER.plist). Runs
        /// sudo, which may ask for your password. `arugulad uninstall`
        /// removes it.
        #[arg(long)]
        system: bool,
        /// Drop the daemon arguments an earlier install wrote, instead of
        /// keeping them when none are given.
        #[arg(long)]
        reset_args: bool,
        /// Arguments for the daemon in the unit, after `--`. With none, an
        /// earlier install's are kept, so upgrading doesn't drop them.
        #[arg(last = true)]
        daemon_args: Vec<String>,
    },
    /// Stop the service `install` set up and remove it (the LaunchAgent, the
    /// background agent or the --system LaunchDaemon, which needs sudo; on
    /// Linux the systemd user service). The binaries and panes' state stay.
    Uninstall,
    /// Add this machine to your account on Arugula cloud
    /// (`https://control.example.com`): prints a link to approve from a
    /// device that's signed in, and finishes once that device approves (an
    /// older control: then the account's fingerprint to check against that
    /// device). With `--token`, it joins at once, with no link to approve.
    /// A running daemon picks it up.
    Join {
        /// Control's URL [default: Arugula cloud].
        #[arg(env = "ARUGULA_CONTROL", default_value = crate::setup::CONTROL)]
        url: String,
        /// A join token (`ajt_…`) from a signed-in CLI, project settings
        /// or the API: joins at once, with no browser, on control's word.
        #[arg(long, env = "ARUGULA_JOIN_TOKEN", hide_env_values = true, conflicts_with_all = ["team", "account"])]
        token: Option<String>,
        /// This machine's name in the directory [default: the hostname].
        #[arg(long)]
        name: Option<String>,
        /// Join it to a team (its id, from the team's page), not your
        /// account alone: the team's members reach it by their team role.
        #[arg(long)]
        team: Option<String>,
        /// The account's fingerprint, as the approving device shows it
        /// (Devices and machines…): checked too. Only needed with an older
        /// control, whose link has no secret to answer.
        #[arg(long, value_name = "FINGERPRINT")]
        account: Option<String>,
        /// The daemon's state directory [default: as the daemon's].
        #[arg(long, env = "ARUGULA_STATE_DIR")]
        state_dir: Option<PathBuf>,
    },
    /// Update Arugula here to the latest release, after asking: download
    /// it, check it against the release's SHA256SUMS and run its `install`,
    /// which restarts the service (panes keep running) and keeps its flags.
    Update {
        /// Don't ask first.
        #[arg(long, short)]
        yes: bool,
    },
    /// Take this machine off the control it joined.
    Leave {
        #[arg(long, env = "ARUGULA_STATE_DIR")]
        state_dir: Option<PathBuf>,
    },
    /// List what can be switched on here, or switch one: `arugulad flags
    /// studio on` shows Studio apps, which a new install doesn't. It writes
    /// flags.json in the state directory, so it works with the daemon
    /// stopped, and a running one follows at once.
    Flags {
        /// The flag to show or set [default: list them all].
        name: Option<String>,
        /// Turn it on or off here, or unset it to follow what control
        /// says for the account and team [default: show it].
        #[arg(requires = "name", value_enum)]
        switch: Option<Switch>,
        /// The daemon's state directory [default: as the daemon's].
        #[arg(long, env = "ARUGULA_STATE_DIR")]
        state_dir: Option<PathBuf>,
    },
    /// Sharing settings: `arugulad sharing guest-ssh on` lets this
    /// machine's owner invite a guest with only OpenSSH to a pane (Guest
    /// SSH access, off by default); `off` ends every ssh invite. It writes
    /// sharing.json in the state directory; a running daemon follows at
    /// once. Control can't set it.
    Sharing {
        /// The setting to show or set [default: list them].
        #[arg(value_enum)]
        name: Option<SharingSetting>,
        /// Turn it on or off here [default: show it].
        #[arg(requires = "name", value_enum)]
        switch: Option<OnOff>,
        /// The daemon's state directory [default: as the daemon's].
        #[arg(long, env = "ARUGULA_STATE_DIR")]
        state_dir: Option<PathBuf>,
    },
    /// Whether this machine sends usage counts to Arugula, and what: `on`,
    /// `off`, or `status`, which shows the next report as it would be sent
    /// (docs/telemetry.md). It writes telemetry.json in the state
    /// directory; a running daemon follows within a minute.
    Telemetry {
        #[arg(value_enum, default_value = "status")]
        what: TelemetryCmd,
        /// Print the status as JSON.
        #[arg(long)]
        json: bool,
        /// The daemon's state directory [default: as the daemon's].
        #[arg(long, env = "ARUGULA_STATE_DIR")]
        state_dir: Option<PathBuf>,
    },
}

/// What `arugulad telemetry` is told to do.
#[derive(clap::ValueEnum, Clone, Copy, Debug, PartialEq, Eq)]
pub enum TelemetryCmd {
    On,
    Off,
    Status,
}

/// A sharing setting (`arugulad sharing NAME`, native #205).
#[derive(clap::ValueEnum, Clone, Copy, Debug, PartialEq, Eq)]
pub enum SharingSetting {
    /// Guest SSH access.
    GuestSsh,
    /// Teammates' messages in a thread reaching this machine's agents.
    AgentMentions,
    /// What a teammate's agent may learn of this machine's sessions:
    /// `work`, `names` or `none`.
    AgentSessions,
    /// Whether people reaching this machine through control may type and
    /// act here (aru-141). Off, they can only watch. Only set here, on the
    /// machine.
    RemoteInput,
}

/// What `arugulad sharing NAME` is told: `on` or `off`, or for
/// `agent-sessions`, `work`, `names` or `none`.
#[derive(clap::ValueEnum, Clone, Copy, Debug, PartialEq, Eq)]
pub enum OnOff {
    On,
    Off,
    /// An agent-sessions setting: names and what each works on.
    Work,
    /// An agent-sessions setting: names only.
    Names,
    /// An agent-sessions setting: nothing.
    None,
}

/// What `arugulad flags NAME` is told to do.
#[derive(clap::ValueEnum, Clone, Copy, Debug, PartialEq, Eq)]
pub enum Switch {
    On,
    Off,
    /// This machine says nothing: control's word, or the default.
    // #347.
    Unset,
}

#[derive(clap::Args, Debug)]
pub(crate) struct RunArgs {
    /// `ARUGULA_LOG_FILE`, as a flag: where Windows' logon task (which
    /// sets no environment) puts the log. Read before parsing (`log_to_file`).
    #[arg(long, hide = true)]
    pub(crate) log_file: Option<PathBuf>,

    /// Address to listen on. Keep it loopback; `tailscale serve` exposes it.
    /// Port 0 picks a free one, recorded in `listen` in the state directory.
    #[arg(long, default_value = "127.0.0.1:7681", env = "ARUGULA_LISTEN")]
    pub(crate) listen: SocketAddr,

    /// Tailscale login allowed through `tailscale serve` [default: the login
    /// that owns this node].
    #[arg(long, env = "ARUGULA_OWNER")]
    pub(crate) owner: Option<String>,

    /// Extra Host names to accept (the MagicDNS name is detected).
    #[arg(long = "public-host")]
    pub(crate) public_hosts: Vec<String>,

    /// Don't keep a socket open to control's relay: clients reach this
    /// daemon only at its own addresses.
    #[arg(long, hide = true)]
    pub(crate) no_relay: bool,

    /// A Cloud workspace's hosted machine (M180): headless, enrolls itself with the
    /// ticket in ARUGULA_HOSTED_TICKET or its sandbox id, at
    /// ARUGULA_CONTROL_URL (else --control), and tells control when it's
    /// working.
    #[arg(long, env = "ARUGULA_HOSTED", hide = true)]
    pub(crate) hosted: bool,

    /// A URL clients can reach this daemon at directly, for control's
    /// directory (`https://box.lan:7681`); its host is accepted too. The
    /// tailnet name, if any, is listed without this. Port 0 is the port
    /// --listen got.
    #[arg(long = "direct-url", env = "ARUGULA_DIRECT_URL", value_delimiter = ',')]
    pub(crate) direct_urls: Vec<String>,

    /// The control Getting started's *Cloud* step joins: your own,
    /// say. `arugulad join URL` takes any control regardless.
    #[arg(long = "control", env = "ARUGULA_CONTROL", value_name = "URL", default_value = crate::setup::CONTROL)]
    pub(crate) control_url: String,

    /// Extra origins whose pages may use this daemon (WebSocket and API),
    /// exactly as the browser sends them: the Vite dev server, or the home
    /// daemon whose host list this daemon is on (`https://home.example.ts.net`).
    /// Through `tailscale serve` the tailnet says who is asking. Another
    /// daemon's page on this same machine (`http://127.0.0.1:OTHER`) can't
    /// sign in here: its page key is only for its own daemon, and no cookie
    /// counts. That works in tests only, where every request carries the key.
    #[arg(long = "allow-origin")]
    pub(crate) allow_origins: Vec<String>,

    /// This daemon's name in host lists [default: its tailnet name, else
    /// the hostname].
    #[arg(long, env = "ARUGULA_NAME")]
    pub(crate) name: Option<String>,

    /// tailscaled's socket, for its name and for asking who is connecting
    /// [default: tailscaled's usual one, if present].
    #[arg(long, env = "ARUGULA_TAILSCALE_SOCKET")]
    pub(crate) tailscale_socket: Option<PathBuf>,

    /// How many VMs each guest (someone a session is shared with) may have
    /// at once; their panes run on VMs, never this machine.
    #[arg(long, default_value_t = 3, env = "ARUGULA_GUEST_MACHINES", hide = true)]
    pub(crate) guest_machines: usize,

    /// Where the ssh server for invited guests listens (`arugula
    /// share --guest`, with Guest SSH access on: `arugulad sharing`), only
    /// while an invite exists; `off` turns it off. Port 0 picks a free one.
    #[arg(long, env = "ARUGULA_GUEST_SSH", default_value = crate::lab_hooks::GUEST_SSH_LISTEN)]
    pub(crate) guest_ssh: String,

    /// The address guests are told to ssh to [default: the hostname].
    #[arg(long, env = "ARUGULA_GUEST_SSH_HOST")]
    pub(crate) guest_ssh_host: Option<String>,

    /// Command line for panes, split on whitespace [default: $SHELL -l].
    #[arg(long)]
    pub(crate) shell: Option<String>,

    /// Where layout, scrollback and checkpoints live [default:
    /// $XDG_STATE_HOME/arugula, else ~/.local/state/arugula].
    #[arg(long, env = "ARUGULA_STATE_DIR")]
    pub(crate) state_dir: Option<PathBuf>,

    /// Don't merge the systemd user manager's environment into new panes.
    #[arg(long)]
    pub(crate) no_manager_env: bool,

    /// Without systemd (macOS, containers): keep panes' programs running
    /// while the daemon restarts, by having each pane's shim hold its
    /// terminal. If no daemon comes back within a minute they end as before.
    /// `arugulad install` sets it on macOS.
    #[arg(long, env = "ARUGULA_KEEP_PANES")]
    pub(crate) keep_panes: bool,

    /// Start shells without the integration that marks prompts, commands
    /// and exit codes.
    #[arg(long)]
    pub(crate) no_shell_integration: bool,
    /// The loam API VM panes get their machines from
    /// (`https://api.loam.inevitable.fyi`, say). VM panes are off without it.
    #[arg(long, env = "LOAM_API_URL", hide = true)]
    pub(crate) loam_url: Option<String>,
    /// The loam project key it uses (`loam_…`), in a file of its own [default:
    /// ~/.config/arugula/loam-key].
    #[arg(long, env = "ARUGULA_LOAM_KEY_FILE", hide = true)]
    pub(crate) loam_key_file: Option<PathBuf>,
    /// An Anthropic API key for Claude Code agents in VMs, passed to them as
    /// ANTHROPIC_API_KEY [default: ~/.config/arugula/anthropic-key].
    #[arg(long, env = "ARUGULA_ANTHROPIC_KEY_FILE", hide = true)]
    pub(crate) anthropic_key_file: Option<PathBuf>,
    /// A Claude Code token (`claude setup-token`) for agents in VMs when
    /// there's no API key, passed as CLAUDE_CODE_OAUTH_TOKEN [default:
    /// ~/.config/arugula/claude-oauth-token].
    #[arg(long, env = "ARUGULA_CLAUDE_TOKEN_FILE", hide = true)]
    pub(crate) claude_token_file: Option<PathBuf>,
    /// Where the studio token is kept (`arugula studio login`),
    /// mode 0600, never sent to a client [default: studio.json in the
    /// state directory].
    #[arg(long, env = "ARUGULA_STUDIO_FILE", hide = true)]
    pub(crate) studio_file: Option<PathBuf>,
    /// Removed with daemon-side Managoat: still parsed, so an old service
    /// file starts, and does nothing but say so.
    #[arg(
        long = "managoat-account-stream",
        hide = true,
        alias = "fountain-account-stream",
        env = "ARUGULA_MANAGOAT_ACCOUNT_STREAM"
    )]
    pub(crate) managoat_account_stream: bool,
    /// Don't be Claude Code's IDE. By default Claude Code in a pane
    /// connects to arugulad (`CLAUDE_CODE_SSE_PORT`) and its edits wait
    /// as diff cards beside the terminal's own prompt.
    #[arg(long, env = "ARUGULA_NO_CLAUDE_IDE")]
    pub(crate) no_claude_ide: bool,
    /// Where Claude Code looks for IDEs [default: $CLAUDE_CONFIG_DIR/ide,
    /// else ~/.claude/ide].
    #[arg(long, env = "ARUGULA_CLAUDE_IDE_DIR", hide = true)]
    pub(crate) claude_ide_dir: Option<PathBuf>,
    /// Don't check for a newer release. Otherwise, at most twice a day,
    /// the daemon asks GitHub which release is the latest (nothing else is
    /// sent) and the web client offers the command that updates.
    #[arg(long, env = "ARUGULA_NO_UPDATE_CHECK")]
    pub(crate) no_update_check: bool,
    /// Where the latest release is looked up (tests point it at a fake).
    #[arg(long, env = "ARUGULA_UPDATE_URL", default_value = crate::update::LATEST, hide = true)]
    pub(crate) update_url: String,

    #[command(flatten)]
    pub(crate) blocks: BlockArgs,

    #[command(flatten)]
    pub(crate) editors: EditorArgs,
}

/// Editor blocks: VS Code as code-server, served like browser blocks
/// on ports (so they need --block-listen).
#[derive(clap::Args, Debug)]
pub(crate) struct EditorArgs {
    /// A code-server to run [default: the release Arugula pins, downloaded
    /// to ~/.cache/arugula/code-server the first time an editor opens].
    #[arg(long, env = "ARUGULA_CODE_SERVER")]
    pub(crate) code_server: Option<PathBuf>,
    /// Stop an editor server after this many seconds with no editor open
    /// (at least 60).
    #[arg(long, env = "ARUGULA_EDITOR_IDLE", default_value_t = 900)]
    pub(crate) editor_idle: u64,
    /// Where code-server releases are downloaded from.
    #[arg(long, env = "ARUGULA_CODE_SERVER_RELEASES", default_value = crate::editors::RELEASES, hide = true)]
    pub(crate) code_server_releases: String,
}

/// Browser blocks on ports: each is served on its own origin by a listener
/// of ours (see `sites.rs`).
#[derive(clap::Args, Debug)]
pub(crate) struct BlockArgs {
    /// Serve browser blocks on ports here [default: off]. Without
    /// --block-domain this must be loopback, and blocks are
    /// `http://b-<id>-<key>.localhost:<port>`. Port 0 picks a free one,
    /// recorded in `block-listen` in the state directory. Not needed to
    /// show blocks in Control, which serves their origins
    /// itself when it has a block domain.
    #[arg(long, env = "ARUGULA_BLOCK_LISTEN")]
    pub(crate) block_listen: Option<SocketAddr>,
    /// Name blocks `b-<id>.<DOMAIN>`, over HTTPS, for the owner on the
    /// tailnet. `*.<DOMAIN>` must resolve to --block-listen's address.
    #[arg(long, env = "ARUGULA_BLOCK_DOMAIN", requires = "block_listen")]
    pub(crate) block_domain: Option<String>,
    /// A certificate for `*.<DOMAIN>` (PEM, with its chain); replaced files
    /// are picked up.
    #[arg(long, requires = "block_key", requires = "block_domain")]
    pub(crate) block_cert: Option<PathBuf>,
    /// The certificate's key (PEM).
    #[arg(long, requires = "block_cert")]
    pub(crate) block_key: Option<PathBuf>,
    /// Get and renew the certificate from an ACME CA with a DNS-01
    /// challenge, through Cloudflare with this API token (Zone:Read and
    /// DNS:Edit).
    #[arg(long, env = "ARUGULA_BLOCK_ACME_TOKEN_FILE", conflicts_with = "block_cert", requires = "block_domain")]
    pub(crate) block_acme_cloudflare_token_file: Option<PathBuf>,
    /// The ACME account's contact.
    #[arg(long, env = "ARUGULA_BLOCK_ACME_EMAIL")]
    pub(crate) block_acme_email: Option<String>,
    /// The ACME directory URL, or `staging` for Let's Encrypt's test CA.
    #[arg(long, env = "ARUGULA_BLOCK_ACME_DIRECTORY", default_value = crate::tls::LETS_ENCRYPT)]
    pub(crate) block_acme_directory: String,
}

/// Options that work but stay out of `--help` unless the machine has turned
/// their Labs flag on: the studio file and the sandbox provider. (Guest
/// ssh's left Labs, native #205: always listed.) The others that are
/// `hide = true` are internals, and stay hidden.
const LABS_OPTIONS: [(&str, &str); 3] = [
    ("studio_file", flags::STUDIO),
    ("loam_url", flags::SANDBOX_PROVIDERS),
    ("loam_key_file", flags::SANDBOX_PROVIDERS),
];

/// The command line, listing in `--help` the options `flags` turns on.
/// Named and versioned as the binary: this crate is `arugulad-base` 0.0.0,
/// and `arugulad update` reads `--version` from what it downloads (#279).
pub(crate) fn labs_command(on: On) -> clap::Command {
    use clap::CommandFactory;
    let mut cmd = Args::command().name("arugulad").version(arugula_core::version::get());
    for (opt, _) in LABS_OPTIONS.iter().filter(|(_, f)| on.has(f)) {
        cmd = cmd.mut_arg(*opt, |a| a.hide(false));
    }
    cmd
}

/// This machine's Labs flags, in the state dir it was told to use
/// (`--state-dir`, read from the command line before it's parsed, or
/// `ARUGULA_STATE_DIR`) or the default one.
pub(crate) fn labs_here() -> On {
    let mut given = None;
    let mut argv = std::env::args().skip(1);
    while let Some(a) = argv.next() {
        if a == "--state-dir" {
            given = argv.next().map(PathBuf::from);
        } else if let Some(v) = a.strip_prefix("--state-dir=") {
            given = Some(PathBuf::from(v));
        }
    }
    let dir = given.or_else(|| std::env::var_os("ARUGULA_STATE_DIR").map(PathBuf::from));
    arugula_proto::flags::on(&dir.unwrap_or_else(crate::default_state_dir))
}

#[cfg(test)]
mod tests {
    /// The command line with every Labs flag on, or none.
    fn labs_command(labs: bool) -> clap::Command {
        use arugula_proto::flags::On;
        super::labs_command(if labs { On::all() } else { On::none() })
    }

    use clap::CommandFactory;

    /// `arugulad --help` is for strangers: no milestone or issue numbers, no
    /// names of our own machines, and what needs loam or the studio isn't
    /// listed. Guest ssh's options are: it left Labs (native #205).
    #[test]
    fn help_has_no_internal_numbers_or_names_and_hides_what_a_stranger_cant_use() {
        let mut cmd = super::Args::command();
        let mut texts = vec![cmd.render_long_help().to_string()];
        for sub in cmd.get_subcommands_mut() {
            texts.push(sub.render_long_help().to_string());
        }
        for help in &texts {
            let b = help.as_bytes();
            for (at, ch) in help.char_indices() {
                if ch != 'M' && ch != '#' {
                    continue;
                }
                let digits = help[at + 1..].chars().take_while(|c| c.is_ascii_digit()).count();
                let word_start = at == 0 || !b[at - 1].is_ascii_alphanumeric();
                let shown: String = help[at..].chars().take(12).collect();
                assert!(!((ch == 'M' && word_start && digits >= 2) || (ch == '#' && digits >= 2)), "{shown}");
            }
            for name in ["geek", "jake-mini", "arugula-salad"] {
                assert!(!help.contains(name), "{name} in the help");
            }
        }
        let top = &texts[0];
        for flag in ["--loam-url", "--loam-key-file", "--studio-file", "--guest-machines"] {
            assert!(!top.contains(flag), "{flag} is in `arugulad --help`");
        }
        assert!(top.contains("--guest-ssh ") && top.contains("--guest-ssh-host"), "{top}");
        // Retired options still parse old service files but are not advertised.
        assert!(!top.contains("--managoat-account-stream") && !top.contains("fountain"), "{top}");
        // Its name before the rename to Managoat still parses too.
        let old = <super::Args as clap::Parser>::try_parse_from(["arugulad", "--fountain-account-stream"]).unwrap();
        assert!(old.run.managoat_account_stream);
        // Still there for whoever knows them.
        assert!(super::Args::command().get_arguments().any(|a| a.get_long() == Some("studio-file")));
    }

    /// With the `labs` file `arugulad --help` lists the sandbox provider
    /// and the studio file, and no other hidden option. The test
    /// passes the bool; it reads neither the filesystem nor the environment.
    #[test]
    fn labs_unhides_its_options_and_only_those() {
        let labs = ["--loam-url", "--loam-key-file", "--studio-file"];
        let hidden_internals = ["--guest-machines", "--log-file", "--no-relay"];
        let help = |on: bool| labs_command(on).render_long_help().to_string();
        let (off, on) = (help(false), help(true));
        for flag in labs {
            assert!(!off.contains(&format!("{flag} ")) && !off.contains(&format!("{flag}\n")), "{flag} without labs");
            assert!(on.contains(flag), "{flag} isn't in `arugulad --help` with labs");
        }
        for flag in hidden_internals {
            assert!(!off.contains(flag) && !on.contains(flag), "{flag} is listed");
        }
        // The labs set is all that differs, and the hidden options still parse.
        let hidden = |on: bool| {
            let mut ids: Vec<String> =
                labs_command(on).get_arguments().filter(|a| a.is_hide_set()).map(|a| a.get_id().to_string()).collect();
            ids.sort();
            ids
        };
        let (h_off, h_on) = (hidden(false), hidden(true));
        assert_eq!(h_off.len() - h_on.len(), labs.len(), "{h_off:?} vs {h_on:?}");
        assert!(h_on.iter().all(|i| h_off.contains(i)));
        for on in [false, true] {
            assert!(
                labs_command(on)
                    .try_get_matches_from(["arugulad", "--studio-file", "/x", "--guest-ssh", "off"])
                    .is_ok()
            );
        }
    }
}
