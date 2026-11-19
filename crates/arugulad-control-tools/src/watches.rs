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
//! - **Events.** A type in the event bus's catalog (`events::CATALOG`),
//!   matched on its fields, with the agent's note handed back when it
//!   fires. Only the cloud's types for now: the daemon's own (a pane's, an
//!   agent block's) come with local watches. `types` lists the catalog.
//!   A type that matches on project, given none, gets the current project
//!   (`Call::current_project`, as `issue_write`'s) when it names an issue by
//!   `#N` or no subject at all; a subject named by its id is left anywhere.
//! - **Once.** It fires once, or expires (24 hours unless told, at most 7
//!   days) with one last message saying it ended without firing.

use arugula_control_wire::events;
use arugula_control_wire::watch::{
    AGENT_WATCHES, AGENT_WATCHES_CANCEL, EventTarget, WatchCancel, WatchCancelled, WatchCreate, WatchList, WatchMade,
    WatchState, WatchTarget, event_target, event_types as types, expires_ms as expires, until_word_parse as until,
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

pub(crate) const DESCRIPTION: &str = "Wake this session later, when something happens: create a watch, then end your turn (watch ends your turn; wait instead blocks it until something happens). When it fires, you get a follow-up saying what happened (ids and states only), as a chat mention arrives. It fires once. It expires after 24 hours unless you say (expires, at most 7d), and then you get one message saying it ended without firing. Targets, any of which fires it: pr (OWNER/REPO#N or its link) until merged, closed, checks_done, checks_failed or ready (checks green and it can merge cleanly), in a repo linked to Arugula's GitHub App (else use wait with pr); session (a hosted agent session, ses_) until it settles; run (run_ or dep_) until it ends; room (a chat room's id, with parent for one thread and from for one person) until a reply you didn't write; event (a type from kind types, like issue.closed) with match (some of the type's match fields, like {\"issue\": \"#12\"}) and note (why you watch, at most 200 characters, handed back when it fires). If it has already happened, create says so and makes no watch. A watch belongs to this Claude Code session: a new session in this pane doesn't get it. Works from a terminal with Arugula's hooks installed, or an agent block. list shows this session's watches; cancel takes one by id, or all; types lists the event types, their match fields, and which this machine can watch now.";

/// `watch`'s kinds.
pub(crate) const KINDS: &[Kind] = &[
    Kind {
        name: "create",
        description: "Wake this session when a PR, a hosted session, a chat room, a run or a deploy gets somewhere, or an event happens. End your turn after.",
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
    Kind {
        name: "types",
        description: "The event types create's event takes: what each means, its match fields, and whether this machine can watch it now.",
        schema: schema_for_type::<TypesArgs>,
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
    /// An event type (types lists them): fires when an event of it happens
    /// that has every match field given.
    #[serde(default)]
    pub event: Option<String>,
    /// With event: field and value pairs the event must have, from the
    /// type's match fields ({"issue": "#12"}). Without project, a type that
    /// matches on it gets the current project's, so #N is its issue.
    #[serde(default, rename = "match")]
    pub matches: Option<serde_json::Map<String, Value>>,
    /// With event: why you watch, at most 200 characters: it comes back
    /// with the event when it fires.
    #[serde(default)]
    pub note: Option<String>,
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
pub struct TypesArgs {}

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
        "types" => {
            let _: TypesArgs = parse(args)?;
            let text = types(c.app().control.enrolled().is_some());
            done(text.clone(), serde_json::json!({ "types": text }))
        }
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

/// When, as people read it: UTC to the minute, `2026-11-14T10:42Z`.
fn when(ms: i64) -> String {
    let t = arugula_e2e::deploy::chant::timestamp(u64::try_from(ms).unwrap_or(0));
    format!("{}Z", t.get(..16).unwrap_or(&t))
}

/// What `create` asks for: its targets, event and expiry, checked here,
/// before the session is known.
fn create_body(a: CreateArgs) -> Result<WatchCreate, String> {
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
    let EventTarget { event, matches, note } = event_target(a.event, a.matches, a.note)?;
    if prs.is_empty() && targets.is_empty() && event.is_none() {
        return Err("create needs something to watch: pr, session, run, room, or event".into());
    }
    for p in &prs {
        let r = PrRef::parse(p).ok_or_else(|| format!("pr {p}: OWNER/REPO#N, or its link on github.com"))?;
        if r.host != "github.com" {
            return Err(format!("pr {p}: a watch is on github.com only; use wait with pr"));
        }
        targets.push(WatchTarget::Pr { repo: r.repo, number: r.number, until });
    }
    let expires_in_ms = a.expires.as_deref().map(expires).transpose()?;
    Ok(WatchCreate { session: String::new(), session_name: None, targets, expires_in_ms, event, matches, note })
}

/// Whether `body`'s event target wants the current project in its match:
/// its type matches on project, and the match names none. That's how `#N`
/// in it means the current project's.
fn wants_project(body: &WatchCreate) -> bool {
    let Some(t) = body.event.as_deref().and_then(events::lookup) else { return false };
    if !t.matches.iter().any(|f| f.name == "project")
        || body.matches.as_ref().is_some_and(|m| m.contains_key("project"))
    {
        return false;
    }
    // A subject named by its id (`iss_…`, `run_…`) is anywhere the owner can
    // read: limiting it to this project would keep it from ever firing. An
    // issue's `#N` means this project's, and no subject at all means the
    // project's own.
    let named = |k: &str| body.matches.as_ref().and_then(|m| m.get(k)).and_then(Value::as_str);
    match named("issue") {
        Some(i) => i.starts_with('#'),
        None => ["run", "deploy", "session"].iter().all(|k| named(k).is_none()),
    }
}

async fn create(c: &Call<'_>, a: CreateArgs) -> Out {
    let mut body = create_body(a)?;
    (body.session, body.session_name) = session(c).await?;
    // Without one, the watch goes as asked: the cloud says if it needs one.
    if wants_project(&body)
        && let Ok(project) = c.current_project(None).await
    {
        body.matches.get_or_insert_default().insert("project".into(), Value::String(project));
    }
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
    use arugula_control_wire::watch::{EXPIRES_MAX_MS, NOTE_MAX, PrUntil};
    use serde_json::json;

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

    fn args(v: Value) -> CreateArgs {
        serde_json::from_value(v).unwrap()
    }

    #[test]
    fn an_event_target_goes_in_the_watch() {
        let body = create_body(args(json!({
            "event": "issue.closed",
            "match": { "issue": "#12", "by": "ada" },
            "note": " ship the fix once it's closed ",
            "run": "run_1",
        })))
        .unwrap();
        assert_eq!(body.event.as_deref(), Some("issue.closed"));
        assert_eq!(body.note.as_deref(), Some("ship the fix once it's closed"));
        assert_eq!(body.targets, vec![WatchTarget::Run { id: "run_1".into() }]);
        let wire = serde_json::to_value(&body).unwrap();
        assert_eq!(wire["event"], json!("issue.closed"));
        assert_eq!(wire["match"], json!({ "issue": "#12", "by": "ada" }));
        assert_eq!(wire["note"], json!("ship the fix once it's closed"));
        // Alone, it's enough to watch.
        let alone = create_body(args(json!({ "event": "run.ended" }))).unwrap();
        assert!(alone.targets.is_empty() && alone.matches.is_none() && alone.note.is_none());
        // Without one, the body is what it was.
        let old = serde_json::to_value(create_body(args(json!({ "session": "ses_1" }))).unwrap()).unwrap();
        let keys: Vec<&str> = old.as_object().unwrap().keys().map(String::as_str).collect();
        assert_eq!(keys, ["expires_in_ms", "session", "session_name", "targets"]);
    }

    #[test]
    fn an_event_target_is_checked_against_the_catalog() {
        let err = |v| create_body(args(v)).err().unwrap();
        assert_eq!(
            err(json!({ "event": "issue.renamed" })),
            "event issue.renamed: no such type; watch types lists them"
        );
        assert_eq!(
            err(json!({ "event": "issue.closed", "match": { "label": "bug" } })),
            "match label: issue.closed matches on issue, project, by"
        );
        assert_eq!(
            err(json!({ "event": "pane.exited", "match": { "pane": "3" } })),
            "event pane.exited: local watches come soon; use wait for a pane"
        );
        assert_eq!(
            err(json!({ "event": "doc.share.answered", "match": { "answer": "shared" } })),
            "event doc.share.answered: local watches come soon; use wait for a pane"
        );
        assert_eq!(
            err(json!({ "event": "github.push", "match": { "pr": "o/r#1" } })),
            "match pr: github.push matches on repo, branch, by, outside"
        );
        let merged = json!({ "event": "github.pr.merged", "match": { "repo": "o/r", "outside": true } });
        assert_eq!(create_body(args(merged)).unwrap().event.as_deref(), Some("github.pr.merged"));
        assert!(
            create_body(args(json!({ "event": "issue.labeled", "match": { "issue": "#3", "label": "bug" } }))).is_ok()
        );
        assert!(err(json!({ "event": "issue.closed", "note": "x".repeat(NOTE_MAX + 1) })).contains("at most 200"));
        assert!(create_body(args(json!({ "event": "issue.closed", "note": "é".repeat(NOTE_MAX) }))).is_ok());
        assert_eq!(err(json!({ "run": "run_1", "note": "why" })), "match and note go with an event");
        assert!(err(json!({})).contains("or event"));
    }

    #[test]
    fn a_type_that_matches_on_project_wants_the_current_one() {
        let wants = |v| wants_project(&create_body(args(v)).unwrap());
        assert!(wants(json!({ "event": "issue.closed", "match": { "issue": "#12" } })));
        assert!(wants(json!({ "event": "run.ended" })));
        assert!(!wants(json!({ "event": "issue.closed", "match": { "issue": "#12", "project": "prj_2" } })));
        assert!(!wants(json!({ "event": "issue.closed", "match": { "issue": "iss_9" } })), "an id is anywhere");
        assert!(!wants(json!({ "event": "run.ended", "match": { "run": "run_1" } })), "an id is anywhere");
        assert!(!wants(json!({ "event": "chat.reply", "match": { "room": "r1" } })), "chat.reply has no project");
        assert!(!wants(json!({ "run": "run_1" })));
    }

    #[test]
    fn types_lists_the_catalog_and_what_this_machine_can_watch() {
        let joined = types(true);
        assert_eq!(joined.lines().count(), 2 * events::CATALOG.len(), "{joined}");
        assert!(joined.contains("issue.closed: A native issue was closed. [watchable now]\n  match: issue (the issue, iss_… or its #N in the project); project (the project, prj_…); by (the login of who closed it)"), "{joined}");
        assert!(
            joined.contains(
                "pane.exited: A pane's process exited. [not yet: local watches come soon; use wait for a pane]"
            ),
            "{joined}"
        );
        for name in ["issue.labeled", "draft.answered", "machine.updated", "github.pr.checks_done", "github.push"] {
            assert!(joined.contains(&format!("\n{name}: ")), "{name}: {joined}");
        }
        assert!(
            joined.contains(
                "outside (true for a fork's or a non-member's event; a watch only matches those when it asks)"
            ),
            "{joined}"
        );
        assert!(joined.contains("doc.share.answered: A Doc share card on this machine was answered. [not yet: local watches come soon; use wait for a pane]"), "{joined}");
        let alone = types(false);
        assert!(
            alone.contains(
                "issue.closed: A native issue was closed. [not now: this machine hasn't joined Arugula cloud]"
            ),
            "{alone}"
        );
        assert!(!alone.contains("watchable now"));
    }

    #[test]
    fn when_is_utc_to_the_minute() {
        assert_eq!(when(0), "1970-01-01T00:00Z");
        assert_eq!(when(1_794_652_920_000), "2026-11-14T10:42Z");
    }
}
