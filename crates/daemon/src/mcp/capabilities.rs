//! `capabilities` (#229): what this daemon offers the agent asking, so it
//! can plan its work around it without its person reading the docs first.
//! The blocks and tools are what `tools/list` shows it (the Labs flags
//! leave out the same ones); the forges are asked of the user's own CLIs
//! (`gh`, and with the `forges` flag `tea` and `glab`) as a block would;
//! hooks are live in a pane when a Claude Code hook has reached the daemon
//! from it ([`App::hooks_heard`](crate::server::App)), which, with the
//! hooks installed, includes the `PreToolUse` of this very call.

use arugula_proto::flags::On;

use super::{
    results::{Capabilities, CloudReach, ControlJoined, ForgeSignIn, HooksLive, ThreadsReach},
    tools::{self, Call, Out, done},
};
use crate::{review::Runner, store::now_ms};

/// A hook heard this recently counts as live: a session that has been
/// quiet longer may have ended.
const LIVE_MS: u64 = 24 * 3600 * 1000;

impl Call<'_> {
    pub(super) async fn capabilities(&self) -> Out {
        let app = self.app();
        let on = crate::labs::flags(app.control.state_dir());
        let forges = self.forges(on).await;
        let hooks = self.hooks().await;
        let (cloud, threads) = tokio::join!(self.cloud_reach(), self.threads_reach());
        let control = control(app, cloud, threads);
        let tools = tools::list(self.scope(), on).into_iter().map(|t| t.name.to_string()).collect();
        let signed: Vec<&str> = forges.iter().filter(|f| f.signed_in).map(|f| f.forge.as_str()).collect();
        let summary = format!(
            "Arugula {}: hooks {}; forges signed in: {}; control: {}; Cloud workspaces: {}; conversations: {}",
            env!("CARGO_PKG_VERSION"),
            match hooks.live {
                Some(true) => "live",
                Some(false) => "not live",
                None => "unknown",
            },
            if signed.is_empty() { "none".to_owned() } else { signed.join(", ") },
            control.state,
            if control.cloud.reachable { "yes" } else { "no" },
            if control.threads.live { "yes" } else { "no" },
        );
        done(
            summary,
            Capabilities {
                version: env!("CARGO_PKG_VERSION").into(),
                flags: on.names().map(str::to_owned).collect(),
                blocks: tools::block_offers(on),
                tools,
                forges,
                hooks,
                control,
            },
        )
    }

    /// GitHub through `gh`, and Labs' forges where the flag is on.
    async fn forges(&self, on: On) -> Vec<ForgeSignIn> {
        if !crate::forges::BUILT {
            return Vec::new();
        }
        let app = self.app();
        let runner = match crate::api::agent_env(app).await {
            Ok((home, env)) => Runner::Local { env, home },
            Err(e) => {
                return vec![ForgeSignIn {
                    forge: "github".into(),
                    host: "github.com".into(),
                    signed_in: false,
                    user: None,
                    note: e.1,
                }];
            }
        };
        let gh = crate::forges::gh_signed_in(&runner, "github.com").await;
        let joined = app.control.enrolled().is_some();
        let mut out = vec![ForgeSignIn {
            forge: "github".into(),
            host: "github.com".into(),
            signed_in: gh.is_ok(),
            user: None,
            note: match gh {
                Ok(()) => "gh".into(),
                Err(e) if joined => {
                    format!("{e}. Until then blocks read through Arugula control's GitHub App, and nothing is sent")
                }
                Err(e) => e,
            },
        }];
        if on.has(arugula_proto::flags::FORGES) {
            out.extend(crate::labs::forge_logins(&runner, app.control.state_dir()).await);
        }
        out
    }

    /// Claude Code's hooks in the caller's pane.
    async fn hooks(&self) -> HooksLive {
        const INSTALL: &str = "`arugula hooks install`, or Arugula's Claude Code plugin, adds them (ask your person first); a new Claude Code session picks them up";
        if let Some(id) = self.scope().block() {
            return HooksLive {
                pane: Some(id),
                live: None,
                heard_secs_ago: None,
                inbox: false,
                note: "You run in an agent block: your approvals and questions are its cards already, with no hooks"
                    .into(),
            };
        }
        let Some(pane) = self.own_pane() else {
            return HooksLive {
                pane: None,
                live: None,
                heard_secs_ago: None,
                inbox: false,
                note: "Arugula doesn't know your pane: run `arugula mcp` from inside an Arugula terminal pane".into(),
            };
        };
        let inbox = self.readable(pane).await.is_ok_and(|p| p.info.inbox);
        let heard = self.app().hooks_heard.lock().unwrap().get(&pane).copied();
        let ago = heard.map(|at| now_ms().saturating_sub(at));
        let live = inbox || ago.is_some_and(|a| a < LIVE_MS);
        let note = if live {
            format!(
                "Claude Code's hooks reach Arugula from %{pane}: your questions and permission prompts are cards on the phone and the swarm{}",
                if inbox { ", and a follow-up sent now goes straight in" } else { "" }
            )
        } else {
            format!(
                "No Claude Code hook has reached Arugula from %{pane}: your questions and permission prompts stay in the terminal. {INSTALL}"
            )
        };
        HooksLive { pane: Some(pane), live: Some(live), heard_secs_ago: ago.map(|a| a / 1000), inbox, note }
    }
}

/// This machine's standing with control, the teams it holds rosters of,
/// and whether its agents reach the owner's Cloud workspaces (#1094) and
/// conversations (M198).
fn control(app: &crate::server::App, cloud: CloudReach, threads: ThreadsReach) -> ControlJoined {
    let st = app.control.state();
    let teams = app
        .control
        .enrolled()
        .map(|e| {
            e.saved
                .roster
                .iter()
                .map(|r| r.name.clone())
                .chain(e.saved.shared_teams.values().map(|t| t.roster.name.clone()))
                .filter(|n| !n.is_empty())
                .collect()
        })
        .unwrap_or_default();
    ControlJoined { state: st.state, kind: st.kind, name: st.name, connected: st.connected, teams, cloud, threads }
}
