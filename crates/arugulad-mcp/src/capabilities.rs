//! `capabilities` (#229): what this daemon offers the agent asking, so it
//! can plan its work around it without its person reading the docs first.
//! The blocks and tools are what `tools/list` shows it (the Labs flags
//! leave out the same ones); the forges are asked of the user's own CLIs
//! (`gh`, and with the `additional-forges` flag `tea` and `glab`) as a block would;
//! hooks are live in a pane when a Claude Code hook has reached the daemon
//! from it ([`App::hooks_heard`](crate::server::App)), which, with the
//! hooks installed, includes the `PreToolUse` of this very call.

use arugula_proto::{api::AgentCan, flags::On};

use super::tools::{self, ToolCall};
use crate::mcp::{
    results::{BoardsReach, CloudReach, ForgeSignIn, RunsReach, ThreadsReach},
    tools::{Needs, Out, done},
};
use crate::results::{Capabilities, ControlJoined, HooksLive, ToolState, ToolStatus};
use crate::{review::Runner, server::App, store::now_ms};

/// A hook heard this recently counts as live: a session that has been
/// quiet longer may have ended.
const LIVE_MS: u64 = 24 * 3600 * 1000;

impl ToolCall<'_, '_> {
    pub(crate) async fn capabilities(&self) -> Out {
        let app = self.app();
        let on = crate::lab_flags::flags(app.control.state_dir());
        let forges = forges(app, on).await;
        let hooks = self.hooks().await;
        let offered = tools::listed_names(self.scope(), on);
        // Boards' reach is asked only where boards are offered and there is
        // a control to ask: the one call `tool_status` needs, and none else.
        let boards = async {
            match app.control.enrolled().is_some() && offered.iter().any(|n| n.starts_with("board_")) {
                true => Some(crate::mcp::registry::boards_reach(app).await),
                false => None,
            }
        };
        let ((cloud, threads, runs), boards) = tokio::join!(crate::mcp::registry::reach(app), boards);
        let tool_status = self.tool_status(&offered, &threads, &runs, boards.as_ref());
        let control = control(app, cloud, threads, runs);
        let tools = tools::list(self.scope(), on).into_iter().map(|t| t.name.to_string()).collect();
        let signed: Vec<&str> = forges.iter().filter(|f| f.signed_in).map(|f| f.forge.as_str()).collect();
        let summary = format!(
            "Arugula {}: hooks {}; forges signed in: {}; control: {}; Cloud workspaces: {}; conversations: {}; runs: {}",
            arugula_core::version::get(),
            match hooks.live {
                Some(true) => "live",
                Some(false) => "not live",
                None => "unknown",
            },
            if signed.is_empty() { "none".to_owned() } else { signed.join(", ") },
            control.state,
            if control.cloud.reachable { "yes" } else { "no" },
            if control.threads.live { "yes" } else { "no" },
            if control.runs.live { "yes" } else { "no" },
        );
        done(
            summary,
            Capabilities {
                version: arugula_core::version::get().into(),
                flags: on.names().map(str::to_owned).collect(),
                blocks: tools::block_offers(on),
                tools,
                tool_status,
                forges,
                hooks,
                control,
            },
        )
    }

    /// The offered tools (and kinds) this session can't use yet: the ones
    /// its client hasn't listed (a flag came on after it started), and the
    /// ones whose grant this machine's owner hasn't given.
    fn tool_status(
        &self,
        offered: &[String],
        threads: &ThreadsReach,
        runs: &RunsReach,
        boards: Option<&BoardsReach>,
    ) -> Vec<ToolStatus> {
        let listed = self.0.listed.as_ref();
        let mut held: Vec<&str> = threads.scopes.iter().flat_map(|s| &s.powers).map(String::as_str).collect();
        held.extend(runs.projects.iter().flat_map(|p| &p.powers).map(String::as_str));
        held.extend(boards.into_iter().flat_map(|b| &b.projects).flat_map(|p| &p.powers).map(String::as_str));
        let joined = self.app().control.enrolled().is_some();
        status_of(
            offered,
            listed.and_then(|l| l.names()).as_deref(),
            listed.is_some_and(|l| l.was_told()),
            joined,
            &held,
        )
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

/// What a client does to get a tool list it hasn't asked for again.
const RECONNECT: &str = "reconnect Arugula's MCP server (`/mcp` in Claude Code)";

/// What to do about a tool added after the session started.
fn reconnect(told: bool) -> String {
    match told {
        true => format!(
            "added after your session started; Arugula told your client the tool list changed, so it will pick it up (if it doesn't, {RECONNECT})"
        ),
        false => format!("added after your session started, and your client hasn't listed again: {RECONNECT}"),
    }
}

/// The power each control tool needs: any one of them, held over a project
/// (`arugula grants add`). Asked of control on each call; `capabilities`
/// says what is missing from the reach it already reads.
const GRANTS: [(&str, &[&str]); 8] = [
    ("read_thread", &["threads.read"]),
    ("search_threads", &["threads.read"]),
    ("post_thread", &["threads.post"]),
    ("issue_write", &["issues.write"]),
    ("start_run", &["runs.start", "runs.command"]),
    ("read_run", &["runs.read"]),
    ("board_read", &["boards.read", "boards.write"]),
    ("board_write", &["boards.write"]),
];

/// The offered tools a session can't use yet. `had`: what its client last
/// listed (`None`: it hasn't, so nothing is known to be missing); `told`:
/// its client was sent `list_changed` since; `joined`: there is a control to
/// hold grants; `held`: the powers this machine's grants give.
fn status_of(offered: &[String], had: Option<&[String]>, told: bool, joined: bool, held: &[&str]) -> Vec<ToolStatus> {
    let mut out = Vec::new();
    for name in offered {
        // A grant is asked of control on each call; without it a restart doesn't help.
        let ungranted = GRANTS
            .iter()
            .find(|(tool, _)| tool == name)
            .filter(|(_, powers)| joined && !powers.iter().any(|p| held.contains(p)));
        let missing = had.is_some_and(|h| !h.contains(name));
        let (status, note) = match (ungranted, missing) {
            (Some((_, powers)), _) => {
                let power = powers[0];
                let mut note = format!(
                    "needs {power} over a project: its owner gives it with `arugula grants add <machine> {power} --project <name>`"
                );
                if missing {
                    note += &format!("; then {RECONNECT}");
                }
                (ToolState::NotGranted, note)
            }
            (None, true) => (ToolState::Reconnect, reconnect(told)),
            (None, false) => continue,
        };
        out.push(ToolStatus { tool: shown(name), status, note });
    }
    out
}

/// A listed name as a call writes it: `show:board` is `show {kind: board}`.
fn shown(name: &str) -> String {
    match name.split_once(':') {
        Some((tool, kind)) => format!("{tool} {{kind: {kind}}}"),
        None => name.to_owned(),
    }
}

/// GitHub through `gh`, and Labs' forges where the flag is on.
async fn forges(app: &App, on: On) -> Vec<ForgeSignIn> {
    if !crate::forges::built() {
        return Vec::new();
    }
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
    if on.has(arugula_proto::flags::ADDITIONAL_FORGES) {
        out.extend(crate::lab_hooks::labs().forge_logins(&runner, app.control.state_dir()).await);
    }
    out
}

/// The card *What your agent can do here* (`agent_can.get`): a line per
/// tool this machine offers, as `capabilities` would say it to an agent
/// started now: its flags, a forge signed in to here, control and the
/// grants there.
pub(crate) async fn can_do(app: &App) -> Vec<AgentCan> {
    let on = crate::lab_flags::flags(app.control.state_dir());
    let (forges, (cloud, threads, runs)) = tokio::join!(forges(app, on), crate::mcp::registry::reach(app));
    let signed = forges.iter().any(|f| f.signed_in);
    let joined = app.control.enrolled().is_some();
    let issues = threads.scopes.iter().any(|s| s.powers.iter().any(|p| p == "issues.write"));
    tools::can_do(on, |needs| match needs {
        Needs::Nothing => true,
        Needs::Forge => signed,
        Needs::Control => joined,
        Needs::Cloud => cloud.reachable,
        Needs::Threads => threads.live,
        Needs::Issues => issues,
        Needs::Runs => runs.live,
    })
}

/// This machine's standing with control, the teams it holds rosters of,
/// and whether its agents reach the owner's Cloud workspaces (#1094),
/// conversations (M198) and runs (M216).
fn control(app: &App, cloud: CloudReach, threads: ThreadsReach, runs: RunsReach) -> ControlJoined {
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
    ControlJoined {
        state: st.state,
        kind: st.kind,
        name: st.name,
        connected: st.connected,
        teams,
        cloud,
        threads,
        runs,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names(n: &[&str]) -> Vec<String> {
        n.iter().map(|n| (*n).to_owned()).collect()
    }

    /// Only what a session lacks is said, and a tool its client has
    /// listed says nothing: the common case stays empty.
    #[test]
    fn a_loaded_tool_is_not_said() {
        let offered = names(&["run", "show", "show:board"]);
        assert!(status_of(&offered, Some(&offered), false, false, &[]).is_empty());
        assert!(status_of(&offered, None, false, false, &[]).is_empty(), "never listed: nothing is known");
    }

    /// A tool a flag added after the session listed says to reconnect, or
    /// that the client was told and will pick it up; a kind is named as a
    /// call writes it.
    #[test]
    fn a_tool_added_after_the_session_started_says_what_to_do() {
        let offered = names(&["run", "show", "show:board"]);
        let had = names(&["run", "show"]);
        let s = status_of(&offered, Some(&had), false, false, &[]);
        assert_eq!((s.len(), s[0].tool.as_str(), s[0].status), (1, "show {kind: board}", ToolState::Reconnect));
        assert!(s[0].note.contains("`/mcp`") && !s[0].note.contains("will pick it up"), "{}", s[0].note);
        let s = status_of(&offered, Some(&had), true, false, &[]);
        assert!(s[0].note.contains("will pick it up"), "{}", s[0].note);
    }

    /// A control tool without its grant says which power and the command
    /// that gives it, whether or not it was listed; a held power says
    /// nothing; and nothing is said before the machine has a control.
    #[test]
    fn a_tool_without_its_grant_says_which_power_to_give() {
        let offered = names(&["board_read", "board_write"]);
        let s = status_of(&offered, Some(&offered), false, true, &["boards.read"]);
        assert_eq!(s.len(), 1, "boards.read is held, boards.write isn't: {s:?}");
        assert_eq!((s[0].tool.as_str(), s[0].status), ("board_write", ToolState::NotGranted));
        assert!(s[0].note.contains("arugula grants add <machine> boards.write --project <name>"), "{}", s[0].note);
        assert!(status_of(&offered, Some(&offered), false, true, &["boards.write"]).is_empty());
        assert!(status_of(&offered, Some(&offered), false, false, &[]).is_empty(), "no control, no grants to ask");
        let s = status_of(&offered, Some(&[]), false, true, &[]);
        assert!(s[0].note.contains("then reconnect"), "not granted and not listed: {}", s[0].note);
    }
}
