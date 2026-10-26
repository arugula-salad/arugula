//! Agents on a hosted workspace's box (M185): whose inference key they
//! run on is the platform's to say, and the key itself never enters the
//! box.
//!
//! - **A placeholder key.** Claude Code and Codex get
//!   `ANTHROPIC_API_KEY`/`OPENAI_API_KEY` set to [`PLACEHOLDER`]. The
//!   compute provider's egress proxy writes the real key over it on the
//!   way out (E2B Secrets), from the project's key or the platform's. Its
//!   proxy re-signs TLS with its own CA, which Node doesn't read from the
//!   system store, so `NODE_EXTRA_CA_CERTS` points at it when it's there.
//! - **Or a Claude subscription.** A project's Anthropic key may be a
//!   `claude setup-token` token, which the proxy sets as a bearer: Claude
//!   Code then holds the placeholder as `CLAUDE_CODE_OAUTH_TOKEN`, so it
//!   sends a bearer and the OAuth beta ([`Auth`]). Each turn says which; an
//!   agent started holding the other restarts on its session first.
//! - **Each turn asks first** ([`Gate::start`]): the platform says whose
//!   key pays, or why not (no key, with how to add one; a cap), which the
//!   block shows instead of sending the prompt. The turn's end is said
//!   too ([`Gate::end`]), for the caps on the platform's key.
//!
//! Only on a hosted box: [`init`] turns it on for a daemon started with
//! `--hosted` (M180) or with `ARUGULA_HOSTED` set. Anywhere else nothing
//! here happens. The calls are signed as the daemon's control identity,
//! which on a hosted box is its hosted machine (M178/M180): the hub takes
//! only that signature on these routes.

use std::sync::{
    Arc, OnceLock,
    atomic::{AtomicBool, Ordering},
};

use futures_util::future::BoxFuture;

use super::defs::Kind;

/// What a box's agents hold for a key (the platform's
/// `hosted::inference::PLACEHOLDER`).
pub const PLACEHOLDER: &str = "arugula-brokered";

/// E2B's proxy CA, as its boxes have it.
const PROXY_CA: &str = "/usr/local/share/ca-certificates/e2b-ca.crt";

/// What kind of credential the proxy fills in for Claude Code: an API key
/// (`x-api-key`), or a Claude subscription's token (a bearer).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Auth {
    #[default]
    Key,
    Subscription,
}

impl Auth {
    /// As a turn's answer says it (`auth`): anything but `subscription`,
    /// or nothing (an older control), is a key.
    pub fn parse(s: Option<&str>) -> Auth {
        if s == Some("subscription") { Auth::Subscription } else { Auth::Key }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Auth::Key => "key",
            Auth::Subscription => "subscription",
        }
    }
}

/// A turn the platform gave: its id, and the kind of credential it runs
/// on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Given {
    pub turn: String,
    pub auth: Auth,
}

/// The platform, as a hosted box's agent turns ask it.
pub trait Gate: Send + Sync {
    /// May a turn of `agent` (`claude`, `codex`) start: its turn, or what
    /// to tell the person. `session` is the thread session (M254) whose
    /// agent it is, if it's one's: its turns are its asker's (aru-115).
    fn start(&self, agent: &'static str, session: Option<String>) -> BoxFuture<'_, Result<Given, String>>;
    /// The turn ended.
    fn end(&self, turn: String) -> BoxFuture<'_, ()>;
}

impl Gate for crate::control::Control {
    fn start(&self, agent: &'static str, session: Option<String>) -> BoxFuture<'_, Result<Given, String>> {
        Box::pin(async move {
            let mut body = serde_json::json!({ "agent": agent });
            if let Some(s) = session {
                body["session"] = s.into();
            }
            let path = arugula_control_wire::hosted::HOSTED_TURNS;
            match self.cloud_now(reqwest::Method::POST, path, Some(&body)).await {
                Ok(v) => {
                    let turn = v["turn"].as_str().ok_or("control's answer had no turn")?;
                    let auth = Auth::parse(v["auth"].as_str());
                    if agent == "claude" {
                        SUBSCRIPTION.store(auth == Auth::Subscription, Ordering::Relaxed);
                    }
                    Ok(Given { turn: turn.to_owned(), auth })
                }
                Err(crate::control::CloudRefused::Said { said, .. }) => Err(said),
                Err(e) => Err(format!("couldn't ask whose key this turn runs on: {e}")),
            }
        })
    }

    fn end(&self, turn: String) -> BoxFuture<'_, ()> {
        Box::pin(async move {
            let path = format!("{}/{turn}/end", arugula_control_wire::hosted::HOSTED_TURNS);
            if let Err(e) = self.cloud_now(reqwest::Method::POST, &path, None).await {
                tracing::warn!(error = %e, "couldn't say a hosted agent turn ended");
            }
        })
    }
}

static GATE: OnceLock<Arc<dyn Gate>> = OnceLock::new();

/// Whether Claude's last turn here ran on a subscription: what Claude
/// Code agents started after it hold.
static SUBSCRIPTION: AtomicBool = AtomicBool::new(false);

/// The kind of credential Claude's turns here last ran on.
pub fn claude_auth() -> Auth {
    if SUBSCRIPTION.load(Ordering::Relaxed) { Auth::Subscription } else { Auth::Key }
}

/// Whether `ARUGULA_HOSTED` says this daemon is a hosted box: set, and
/// not a "no" (as clap reads a flag's env).
pub fn hosted_env(v: Option<&str>) -> bool {
    v.is_some_and(|v| !matches!(v.trim().to_ascii_lowercase().as_str(), "" | "0" | "n" | "no" | "f" | "false" | "off"))
}

/// Turn it on for agents started after this, on a hosted box: one started
/// with `--hosted` (which the template's start command passes, and clap
/// doesn't put back in the environment), or with `ARUGULA_HOSTED` set.
pub fn init(gate: Arc<dyn Gate>, hosted: bool) {
    if hosted || hosted_env(std::env::var("ARUGULA_HOSTED").ok().as_deref()) {
        let _ = GATE.set(gate);
    }
}

/// The platform's gate, on a hosted box.
pub fn gate() -> Option<Arc<dyn Gate>> {
    GATE.get().cloned()
}

/// The agent a block's turns ask for: Claude Code's and Codex's. Any
/// other ACP command brings its own key.
pub fn agent_of(kind: Kind) -> Option<&'static str> {
    match kind {
        Kind::Claude => Some("claude"),
        Kind::Codex => Some("codex"),
        _ => None,
    }
}

/// What an agent's environment gets on a hosted box: the placeholder for
/// its provider's key (Claude Code's as `auth` says), and the proxy's CA
/// for Node when the box has it.
pub fn env(kind: Kind, auth: Auth, ca: bool) -> Vec<(String, String)> {
    let mut env = match (kind, auth) {
        (Kind::Claude, Auth::Key) => vec![("ANTHROPIC_API_KEY".to_owned(), PLACEHOLDER.to_owned())],
        (Kind::Claude, Auth::Subscription) => vec![("CLAUDE_CODE_OAUTH_TOKEN".to_owned(), PLACEHOLDER.to_owned())],
        (Kind::Codex, _) => vec![("OPENAI_API_KEY".to_owned(), PLACEHOLDER.to_owned())],
        _ => return vec![],
    };
    if ca {
        env.push(("NODE_EXTRA_CA_CERTS".to_owned(), PROXY_CA.to_owned()));
    }
    env
}

/// What an agent's environment loses on a hosted box: on a subscription,
/// any API key Claude Code would send before its OAuth token.
pub fn unset(kind: Kind, auth: Auth) -> &'static [&'static str] {
    match (kind, auth) {
        (Kind::Claude, Auth::Subscription) => &["ANTHROPIC_API_KEY", "ANTHROPIC_AUTH_TOKEN"],
        _ => &[],
    }
}

/// [`env`], looking for the proxy's CA on this box.
pub fn env_here(kind: Kind, auth: Auth) -> Vec<(String, String)> {
    env(kind, auth, std::path::Path::new(PROXY_CA).exists())
}

/// Give up the daemon's protection from the OOM killer in what `c` starts:
/// a hosted box's supervisor gives the daemon an `oom_score_adj` of -1000
/// (M258), which everything it starts inherits; a session's git (and what
/// a planted config runs) is as killable as any. It runs
/// [`arugula_mux::shim::unprotect_oom`], which the shim calls for agents
/// and panes, between fork and exec.
pub fn unprotect_oom(c: &mut tokio::process::Command) {
    #[cfg(target_os = "linux")]
    // SAFETY: unprotect_oom is async-signal-safe.
    unsafe {
        c.pre_exec(|| {
            arugula_mux::shim::unprotect_oom();
            Ok(())
        });
    }
    #[cfg(not(target_os = "linux"))]
    let _ = c;
}

/// The `PATH` a session user's processes get.
pub const SESSION_PATH: &str = "/usr/local/bin:/usr/bin:/bin";

/// One of a hosted box's session users (M255, S42 q1): a hosted session's
/// agent runs as it, and so does all of the daemon's git in the session's
/// checkout, never as the daemon's user or root (an agent's planted
/// `core.fsmonitor` would run as them). The template makes them (`ses1` to
/// `ses3`): no sudo, not in `user`'s group, homes 0700.
///
/// Through `sudo -n -u USER -- env -i …`: an empty environment but for
/// what's named, and nothing secret on that command line, which sudo logs
/// and other users can read in `/proc`.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct AsUser {
    pub user: String,
    pub home: String,
}

impl AsUser {
    /// `sudo`, or what a test stands in for it (`ARUGULA_HOSTED_SUDO`):
    /// the integration tests can't make Unix users.
    pub fn sudo() -> String {
        std::env::var("ARUGULA_HOSTED_SUDO").ok().filter(|s| !s.is_empty()).unwrap_or_else(|| "sudo".into())
    }

    /// Whether `user` can go on sudo's command line as a user's name.
    pub fn plain(user: &str) -> bool {
        !user.is_empty()
            && !user.starts_with('-')
            && user.bytes().all(|b| b.is_ascii_alphanumeric() || b"-_".contains(&b))
    }

    /// The command line that runs `argv` as this user, with `env` (nothing
    /// secret: [`env`]'s placeholder and the proxy's CA are fine) and the
    /// user's own `HOME`, `USER`, `LOGNAME` and [`SESSION_PATH`].
    pub fn wrap(&self, env: &[(String, String)], argv: &[String]) -> Vec<String> {
        let mut out = vec![Self::sudo(), "-n".into(), "-u".into(), self.user.clone(), "--".into(), "env".into()];
        out.push("-i".into());
        out.push(format!("HOME={}", self.home));
        out.push(format!("USER={}", self.user));
        out.push(format!("LOGNAME={}", self.user));
        out.push(format!("PATH={SESSION_PATH}"));
        out.push("LANG=C.UTF-8".into());
        for (k, v) in
            env.iter().filter(|(k, _)| !k.is_empty() && k.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_'))
        {
            out.push(format!("{k}={v}"));
        }
        out.extend(argv.iter().cloned());
        out
    }

    /// `argv` as this user ([`AsUser::wrap`]), without the daemon's
    /// protection from the OOM killer ([`unprotect_oom`]).
    pub fn command(&self, env: &[(String, String)], argv: &[String]) -> tokio::process::Command {
        let argv = self.wrap(env, argv);
        let mut c = tokio::process::Command::new(&argv[0]);
        c.args(&argv[1..]).current_dir("/");
        unprotect_oom(&mut c);
        c
    }

    /// The agent's own program as a session user can reach it: a path
    /// through a link (the adapters' place under the daemon's home, which
    /// is `/opt/arugula/agents` in the template) resolved, since the
    /// daemon's home is closed to session users.
    pub fn reachable(argv: &[String]) -> Vec<String> {
        let mut out = argv.to_vec();
        if let Some(first) = out.first_mut()
            && first.starts_with('/')
            && let Ok(real) = std::fs::canonicalize(&*first)
        {
            *first = real.display().to_string();
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_claude_and_codex_get_a_placeholder() {
        assert_eq!(
            env(Kind::Claude, Auth::Key, true),
            [
                ("ANTHROPIC_API_KEY".to_owned(), PLACEHOLDER.to_owned()),
                ("NODE_EXTRA_CA_CERTS".to_owned(), PROXY_CA.to_owned())
            ]
        );
        assert_eq!(env(Kind::Codex, Auth::Key, false), [("OPENAI_API_KEY".to_owned(), PLACEHOLDER.to_owned())]);
        assert!(env(Kind::Acp, Auth::Key, true).is_empty());
        assert_eq!(
            (agent_of(Kind::Claude), agent_of(Kind::Codex), agent_of(Kind::Acp)),
            (Some("claude"), Some("codex"), None)
        );
    }

    /// On a subscription, Claude Code holds the placeholder as its OAuth
    /// token and never as an API key, which it would send first.
    #[test]
    fn a_subscription_is_an_oauth_token() {
        assert_eq!(
            env(Kind::Claude, Auth::Subscription, false),
            [("CLAUDE_CODE_OAUTH_TOKEN".to_owned(), PLACEHOLDER.to_owned())]
        );
        assert_eq!(env(Kind::Codex, Auth::Subscription, false), env(Kind::Codex, Auth::Key, false));
        assert_eq!(unset(Kind::Claude, Auth::Subscription), ["ANTHROPIC_API_KEY", "ANTHROPIC_AUTH_TOKEN"]);
        assert!(unset(Kind::Claude, Auth::Key).is_empty() && unset(Kind::Codex, Auth::Subscription).is_empty());
        assert_eq!(Auth::parse(Some("subscription")), Auth::Subscription);
        for key in [None, Some("key"), Some("other")] {
            assert_eq!(Auth::parse(key), Auth::Key, "{key:?}");
        }
    }

    /// M255: a session's agent runs as its user with an empty environment
    /// but for what's named: the placeholder, never a key.
    #[test]
    fn a_session_user_runs_with_only_what_is_named() {
        let u = AsUser { user: "ses1".into(), home: "/home/ses1".into() };
        let env = [("ANTHROPIC_API_KEY".to_owned(), PLACEHOLDER.to_owned()), ("BAD KEY".to_owned(), "x".to_owned())];
        let argv = u.wrap(&env, &["/opt/arugula/agents/claude/acp".into()]);
        assert_eq!(
            argv[1..].join(" "),
            "-n -u ses1 -- env -i HOME=/home/ses1 USER=ses1 LOGNAME=ses1 PATH=/usr/local/bin:/usr/bin:/bin \
             LANG=C.UTF-8 ANTHROPIC_API_KEY=arugula-brokered /opt/arugula/agents/claude/acp"
        );
        for bad in ["", "-u", "ses 1", "ses1;id", "../x"] {
            assert!(!AsUser::plain(bad), "{bad}");
        }
        assert!(AsUser::plain("ses1"));
    }

    #[test]
    fn hosted_reads_as_a_flag() {
        for yes in ["1", "true", "yes", "on"] {
            assert!(hosted_env(Some(yes)), "{yes}");
        }
        for no in [None, Some(""), Some("0"), Some("false"), Some("off"), Some("No")] {
            assert!(!hosted_env(no), "{no:?}");
        }
    }
}
