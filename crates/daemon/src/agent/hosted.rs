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

use std::sync::{Arc, OnceLock};

use futures_util::future::BoxFuture;

use super::defs::Kind;

/// What a box's agents hold for a key (the platform's
/// `hosted::inference::PLACEHOLDER`).
pub const PLACEHOLDER: &str = "arugula-brokered";

/// E2B's proxy CA, as its boxes have it.
const PROXY_CA: &str = "/usr/local/share/ca-certificates/e2b-ca.crt";

/// The platform, as a hosted box's agent turns ask it.
pub trait Gate: Send + Sync {
    /// May a turn of `agent` (`claude`, `codex`) start: its turn id, or
    /// what to tell the person.
    fn start(&self, agent: &'static str) -> BoxFuture<'_, Result<String, String>>;
    /// The turn ended.
    fn end(&self, turn: String) -> BoxFuture<'_, ()>;
}

impl Gate for crate::control::Control {
    fn start(&self, agent: &'static str) -> BoxFuture<'_, Result<String, String>> {
        Box::pin(async move {
            let body = serde_json::json!({ "agent": agent });
            let path = arugula_control_wire::hosted::HOSTED_TURNS;
            match self.cloud_now(reqwest::Method::POST, path, Some(&body)).await {
                Ok(v) => v["turn"].as_str().map(str::to_owned).ok_or_else(|| "control's answer had no turn".into()),
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
/// its provider's key, and the proxy's CA for Node when the box has it.
pub fn env(kind: Kind, ca: bool) -> Vec<(String, String)> {
    let mut env = match kind {
        Kind::Claude => vec![("ANTHROPIC_API_KEY".to_owned(), PLACEHOLDER.to_owned())],
        Kind::Codex => vec![("OPENAI_API_KEY".to_owned(), PLACEHOLDER.to_owned())],
        _ => return vec![],
    };
    if ca {
        env.push(("NODE_EXTRA_CA_CERTS".to_owned(), PROXY_CA.to_owned()));
    }
    env
}

/// [`env`], looking for the proxy's CA on this box.
pub fn env_here(kind: Kind) -> Vec<(String, String)> {
    env(kind, std::path::Path::new(PROXY_CA).exists())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_claude_and_codex_get_a_placeholder() {
        assert_eq!(
            env(Kind::Claude, true),
            [
                ("ANTHROPIC_API_KEY".to_owned(), PLACEHOLDER.to_owned()),
                ("NODE_EXTRA_CA_CERTS".to_owned(), PROXY_CA.to_owned())
            ]
        );
        assert_eq!(env(Kind::Codex, false), [("OPENAI_API_KEY".to_owned(), PLACEHOLDER.to_owned())]);
        assert!(env(Kind::Acp, true).is_empty());
        assert_eq!(
            (agent_of(Kind::Claude), agent_of(Kind::Codex), agent_of(Kind::Acp)),
            (Some("claude"), Some("codex"), None)
        );
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
