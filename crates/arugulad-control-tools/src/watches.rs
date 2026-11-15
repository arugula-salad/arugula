//! `watch` (Arugula #631): wake this session when something happens, after
//! its turn has ended. An agent makes a watch, ends its turn, and is woken by
//! a follow-up when it fires. Control evaluates it (`/api/daemon/watches`),
//! and the message comes back as an `agents.mention` for this Claude Code
//! session alone (`agent_mentions::watched`).
//!
//! - **Its session.** A watch is keyed to the calling pane's Claude Code
//!   session: a terminal's from its hooks, or an agent block's. A pane that
//!   can't take follow-ups (no hooks heard, or no agent) is refused, since
//!   nothing could ever wake it. So is a hosted box's session, for now.
//! - **Its targets.** A pull request on github.com in a repo linked through
//!   Arugula's GitHub App, until it merges or closes. Control refuses a repo
//!   without the App, and a PR the owner can't see answers as one that
//!   doesn't exist.
//! - **Once.** It fires once, or expires (24 hours unless told, at most 7
//!   days) with one last message saying it ended without firing.

use arugula_control_wire::watch::{
    AGENT_WATCHES, AGENT_WATCHES_CANCEL, EXPIRES_MAX_MS, PrUntil, WatchCancel, WatchCancelled, WatchCreate, WatchList,
    WatchMade, WatchState, WatchTarget,
};
use arugulad_base::forge_wait::PrRef;
use reqwest::Method;
use rmcp::handler::server::common::schema_for_type;
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::Value;

use crate::control::CloudRefused;
use crate::mcp::tools::{Call, Kind, Out, done, parse};

pub(crate) const TOOL: &str = "watch";

pub(crate) const DESCRIPTION: &str = "Wake this session later, when something happens, without polling or holding your turn open: create a watch, then end your turn. When it fires, you get a follow-up saying what happened (ids and states only), as a chat mention arrives. It fires once. It expires after 24 hours unless you say (expires, at most 7d), and then you get one message saying it ended without firing. Targets, any of which fires it: pr (OWNER/REPO#N or its link) until merged, closed, checks_done, checks_failed or ready (checks green and it can merge cleanly), in a repo linked to Arugula's GitHub App (else use wait with pr); session (a hosted agent session, ses_) until it settles; run (run_ or dep_) until it ends; room (a chat room's id, with parent for one thread and from for one person) until a reply you didn't write. If it has already happened, create says so and makes no watch. A watch belongs to this Claude Code session: a new session in this pane doesn't get it. Works from a terminal with Arugula's hooks installed, or an agent block. list shows this session's watches; cancel takes one by id, or all.";

/// `watch`'s kinds.
pub(crate) const KINDS: &[Kind] = &[
    Kind {
        name: "create",
        description: "Wake this session when a PR, a hosted session, a thread, a run or a deploy gets somewhere. End your turn after.",
        schema: schema_for_type::<CreateArgs>,
        flag: None,
    },
    Kind {
        name: "list",
        description: "This session's live watches (all: every session's on this machine).",
        schema: schema_for_type::<ListArgs>,
        flag: None,
    },
    Kind {
        name: "cancel",
        description: "Cancel one of this session's watches by id, or all of them.",
        schema: schema_for_type::<CancelArgs>,
        flag: None,
    },
];

#[derive(Deserialize, JsonSchema)]
pub struct CreateArgs {
    /// A pull request: OWNER/REPO#N, or its link on github.com, or several.
    #[serde(default)]
    pub pr: Option<PrList>,
    /// A hosted agent session (ses_…), or several: until it settles (done,
    /// waiting for input, failed or stopped).
    #[serde(default)]
    pub session: Option<PrList>,
    /// A run (run_…) or a deploy (dep_…), or several: until it ends.
    #[serde(default)]
    pub run: Option<PrList>,
    /// A chat room (its id, as read_thread gives it): until a reply in it
    /// that this session didn't write.
    #[serde(default)]
    pub room: Option<String>,
    /// With room: only a reply under this thread (its root message's
    /// number).
    #[serde(default)]
    pub parent: Option<i64>,
    /// With room: only a reply by this person (login or name).
    #[serde(default)]
    pub from: Option<String>,
    /// merged (the default), closed (merged or not), checks_done (none
    /// queued or running), checks_failed, or ready (checks green and it can
    /// merge cleanly).
    #[serde(default)]
    pub until: Option<String>,
    /// How long to wait before it ends without firing: 30m, 6h, 3d. Default
    /// 24h, at most 7d.
    #[serde(default)]
    pub expires: Option<String>,
}

/// One id, or several. Whichever target happens first fires the watch.
#[derive(Deserialize, JsonSchema)]
#[serde(untagged)]
pub enum PrList {
    One(String),
    Many(Vec<String>),
}

#[derive(Deserialize, JsonSchema)]
pub struct ListArgs {
    /// Every session's watches on this machine, not only this session's.
    #[serde(default)]
    pub all: bool,
}

#[derive(Deserialize, JsonSchema)]
pub struct CancelArgs {
    /// The watch's id (wch_…).
    #[serde(default)]
    pub id: Option<String>,
    /// All of this session's watches.
    #[serde(default)]
    pub all: bool,
}

/// What control's answer says when it fails.
fn said(e: CloudRefused) -> String {
    match e {
        CloudRefused::NotJoined => "this machine hasn't joined Arugula cloud, where watches are kept: ask your person to join it (Getting started on the web page, or `arugulad join`)".into(),
        CloudRefused::OldControl | CloudRefused::Said { status: 404 | 405, .. } => {
            "Arugula cloud doesn't keep watches yet: use wait with pr to block until it happens".into()
        }
        e => e.to_string(),
    }
}

/// `watch`.
pub(crate) async fn watch(c: &Call<'_>, kind: &str, args: Value) -> Out {
    c.owners_own(TOOL).await?;
    match kind {
        "create" => create(c, parse(args)?).await,
        "list" => list(c, parse(args)?).await,
        "cancel" => cancel(c, parse(args)?).await,
        k => Err(format!("{TOOL} kind {k} isn't wired up")),
    }
}

/// The caller's Claude Code session and its name, or why it has none a
/// watch could wake.
async fn session(c: &Call<'_>) -> Result<(String, Option<String>), String> {
    if c.app().control.enrolled().is_some_and(|e| e.saved.is_hosted()) {
        return Err("watch doesn't wake a session on a hosted box yet: reply in the session's thread instead".into());
    }
    let pane = c.own_pane().ok_or("watch wakes the session it's called from: call it from a pane in Arugula")?;
    let Some(id) = arugulad_base::agent_mentions::conversation(c.app(), pane).await else {
        return Err(format!(
            "%{pane} can't be woken: a watch wakes a Claude Code session through Arugula's hooks (`arugula hooks install`), or an agent block, and this pane has neither a hooked Claude Code session nor a block's"
        ));
    };
    let name = arugulad_base::agent_mentions::session_name(c.app(), pane).await;
    Ok((id, name))
}

/// `until`'s word.
fn until(word: Option<&str>) -> Result<PrUntil, String> {
    match word.map(str::trim).unwrap_or("merged") {
        "" | "merged" => Ok(PrUntil::Merged),
        "closed" => Ok(PrUntil::Closed),
        "checks_done" => Ok(PrUntil::ChecksDone),
        "checks_failed" => Ok(PrUntil::ChecksFailed),
        "ready" => Ok(PrUntil::Ready),
        w => Err(format!("until {w}: a watch on a PR waits until merged, closed, checks_done, checks_failed or ready")),
    }
}

/// `expires`' words, in ms: `30m`, `6h`, `3d`.
fn expires(word: &str) -> Result<u64, String> {
    let w = word.trim();
    let (n, unit) = w.split_at(w.find(|c: char| !c.is_ascii_digit()).unwrap_or(w.len()));
    let n: u64 = n.parse().map_err(|_| format!("expires {w}: a number and m, h or d, like 6h"))?;
    let ms = match unit.trim() {
        "m" => n * 60_000,
        "h" => n * 3_600_000,
        "d" => n * 86_400_000,
        _ => return Err(format!("expires {w}: a number and m, h or d, like 6h")),
    };
    match ms {
        0 => Err("expires: more than nothing".into()),
        ms if ms > EXPIRES_MAX_MS => Err(format!("expires {w}: at most 7d")),
        ms => Ok(ms),
    }
}

/// When, as people read it: UTC to the minute, `2026-11-14T10:42Z`.
fn when(ms: i64) -> String {
    let t = arugula_e2e::deploy::chant::timestamp(u64::try_from(ms).unwrap_or(0));
    format!("{}Z", t.get(..16).unwrap_or(&t))
}

async fn create(c: &Call<'_>, a: CreateArgs) -> Out {
    let until = until(a.until.as_deref())?;
    let list = |l: Option<PrList>| match l {
        None => vec![],
        Some(PrList::One(p)) => vec![p],
        Some(PrList::Many(ps)) => ps,
    };
    let prs = list(a.pr);
    let mut targets = vec![];
    for id in list(a.session) {
        let id = id.trim().to_owned();
        if !id.starts_with("ses_") {
            return Err(format!("session {id}: a hosted agent session's id, ses_…"));
        }
        targets.push(WatchTarget::Session { id });
    }
    for id in list(a.run) {
        let id = id.trim().to_owned();
        if !id.starts_with("run_") && !id.starts_with("dep_") {
            return Err(format!("run {id}: a run's id (run_…) or a deploy's (dep_…)"));
        }
        targets.push(WatchTarget::Run { id });
    }
    match a.room.map(|r| r.trim().to_owned()).filter(|r| !r.is_empty()) {
        Some(room) => targets.push(WatchTarget::Thread { room, parent: a.parent, from: a.from }),
        None if a.parent.is_some() || a.from.is_some() => return Err("parent and from go with a room".into()),
        None => {}
    }
    if prs.is_empty() && targets.is_empty() {
        return Err("create needs something to watch: pr, session, run, or room".into());
    }
    for p in &prs {
        let r = PrRef::parse(p).ok_or_else(|| format!("pr {p}: OWNER/REPO#N, or its link on github.com"))?;
        if r.host != "github.com" {
            return Err(format!("pr {p}: a watch is on github.com only; use wait with pr"));
        }
        targets.push(WatchTarget::Pr { repo: r.repo, number: r.number, until });
    }
    let expires_in_ms = a.expires.as_deref().map(expires).transpose()?;
    let (session, session_name) = session(c).await?;
    let body = WatchCreate { session, session_name, targets, expires_in_ms };
    let body = serde_json::to_value(&body).map_err(|e| e.to_string())?;
    let v = c.app().control.cloud(Method::POST, AGENT_WATCHES, Some(&body)).await.map_err(said)?;
    let made: WatchMade =
        serde_json::from_value(v).map_err(|e| format!("Arugula cloud sent an answer I can't read: {e}"))?;
    let text = match (made.state, &made.id) {
        (WatchState::AlreadyMet, _) => {
            format!("It has already happened: {}. No watch was made.", made.met.as_deref().unwrap_or("met"))
        }
        (_, Some(id)) => format!(
            "Watch {id} is live until {}. End your turn now: you'll get a follow-up when it fires, or when it ends without firing.",
            made.expires_ms.map(when).unwrap_or_else(|| "it expires".into())
        ),
        (_, None) => "Arugula cloud made no watch, and said nothing of why".into(),
    };
    done(text, made)
}

async fn list(c: &Call<'_>, a: ListArgs) -> Out {
    let path = match a.all {
        true => AGENT_WATCHES.to_owned(),
        false => format!("{AGENT_WATCHES}?session={}", session(c).await?.0),
    };
    let v = c.app().control.cloud(Method::GET, &path, None).await.map_err(said)?;
    let got: WatchList =
        serde_json::from_value(v).map_err(|e| format!("Arugula cloud sent an answer I can't read: {e}"))?;
    let lines: Vec<String> = got
        .watches
        .iter()
        .map(|w| {
            let whose =
                if a.all { format!(" ({})", w.session_name.as_deref().unwrap_or(&w.session)) } else { "".into() };
            format!("{}{whose}: {}, until {}", w.id, w.targets.join(" or "), when(w.expires_ms))
        })
        .collect();
    let text = match lines.len() {
        0 => "No live watches".to_owned(),
        n => format!("{n} live watch{}:\n{}", if n == 1 { "" } else { "es" }, lines.join("\n")),
    };
    done(text, got)
}

async fn cancel(c: &Call<'_>, a: CancelArgs) -> Out {
    let id = a.id.map(|i| i.trim().to_owned()).filter(|i| !i.is_empty());
    if id.is_none() && !a.all {
        return Err("cancel needs an id (wch_…), or all".into());
    }
    let (session, _) = session(c).await?;
    let body = serde_json::to_value(WatchCancel { session, id, all: a.all }).map_err(|e| e.to_string())?;
    let v = c.app().control.cloud(Method::POST, AGENT_WATCHES_CANCEL, Some(&body)).await.map_err(said)?;
    let got: WatchCancelled =
        serde_json::from_value(v).map_err(|e| format!("Arugula cloud sent an answer I can't read: {e}"))?;
    let text = match got.cancelled.as_slice() {
        [] => "No live watch of this session's was cancelled".to_owned(),
        ids => format!(
            "Cancelled {}. No message follows for {}.",
            ids.join(", "),
            if ids.len() == 1 { "it" } else { "them" }
        ),
    };
    done(text, got)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn expires_reads_minutes_hours_and_days_up_to_a_week() {
        assert_eq!(expires("30m"), Ok(30 * 60_000));
        assert_eq!(expires("6h"), Ok(6 * 3_600_000));
        assert_eq!(expires(" 7d "), Ok(EXPIRES_MAX_MS));
        assert!(expires("8d").is_err());
        assert!(expires("0h").is_err());
        assert!(expires("soon").is_err());
        assert!(expires("6").is_err());
    }

    #[test]
    fn until_is_merged_unless_said() {
        assert_eq!(until(None), Ok(PrUntil::Merged));
        assert_eq!(until(Some("closed")), Ok(PrUntil::Closed));
        assert_eq!(until(Some("ready")), Ok(PrUntil::Ready));
        assert_eq!(until(Some("checks_failed")), Ok(PrUntil::ChecksFailed));
        assert!(until(Some("green")).is_err());
    }

    #[test]
    fn when_is_utc_to_the_minute() {
        assert_eq!(when(0), "1970-01-01T00:00Z");
        assert_eq!(when(1_794_652_920_000), "2026-11-14T10:42Z");
    }
}
