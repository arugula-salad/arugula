//! Mentions and assignments in the inbox (M202, #1273): what happens on the
//! platform's own objects joins M171's inbox ([`crate::forge::inbox`])
//! beside the GitHub reasons, and pushes through the hub's app (aru-081).
//!
//! **What makes an entry:**
//! - an account added to a native issue's assignees: `assigned`, on the
//!   issue; taken off, the entry goes;
//! - an `@` naming someone in a native issue's body (when it's opened, or
//!   newly in an edit), or in its thread (its comments): `mentioned`;
//! - an `@` naming someone in the thread on a mirrored PR or issue (M196):
//!   `thread_mention`, on the item, kept apart from its GitHub reasons.
//!
//! Nobody gets one for what they did themselves (an agent's is its
//! owner's). A mention names only those who see the object: chat's
//! audience for a thread (its project's members, past M165's check on a
//! private repo's item), and a native issue's project's members with chat
//! on for its body. Mentions of a machine's agents (M198's `machine:<id>`)
//! are never entries or pushes: agents read them over MCP.
//!
//! **Entries are made when it happens,** not worked out from rows as the
//! GitHub reasons are, and stay until they're called done. A new mention or
//! assignment brings a done one back.
//!
//! **One push per event.** A mention in an object's thread is an entry
//! here, and this pushes for it: chat doesn't (`chat::push`). Each person
//! gets one push saying all of an event's reasons for them, with M171's
//! limits: one per item per person a minute, [`INBOX_PUSHES`] an hour. A
//! thread's mention doesn't push someone with the thread open or muted.
//! A tap opens the native issue (at the message, for a mention in its
//! thread), or a mirrored item's thread.
//!
//! **Read** by `GET /api/inbox` beside the GitHub reasons ([`entries`]):
//! a native issue's entries only while the reader may read its project
//! (`issues::place`), and only for a person, never a machine's agents,
//! which reach issues through their own grants (M197).

use std::collections::HashMap;

use arugula_control_common::ApiError;
use arugula_control_wire::{chat as chat_wire, projects as wire};
use arugula_e2e::now_ms;
use axum::http::StatusCode;
use serde_json::{Value, json};
use tracing::{info, warn};

use crate::{
    Platform,
    chat::{self, chat_on},
    db::{
        chat::Room,
        inbox::{InboxFilter, NativeRow},
        issues::{self as db, Actor},
    },
    forge::inbox::{INBOX_PUSHES, members},
    hub::PushOptions,
};

pub use crate::db::inbox::THREAD_MENTION;

/// A native issue's mention, in its body or its thread.
pub const MENTIONED: &str = "mentioned";
/// A native issue given to someone.
pub const ASSIGNED: &str = "assigned";

/// The reason a mention in the thread on `target` is, or `None` when it
/// isn't the inbox's (a workspace's thread, for now): chat pushes it.
pub fn thread_reason(target: &str) -> Option<&'static str> {
    if target.starts_with("iss_") {
        Some(MENTIONED)
    } else if target.starts_with("fi_") {
        Some(THREAD_MENTION)
    } else {
        None
    }
}

/// What a push for an item says, and where its tap goes.
struct Notice<'a> {
    item: &'a str,
    /// `t#12`, `PR acme/app#3`.
    what: String,
    body: String,
    /// The push's `inbox`: the item, and for a thread's mention its room
    /// and message.
    tap: Value,
}

/// Make each person's entries on the item, and push each once, saying all
/// of their reasons, unless they're `quiet` or past M171's limits. Whom it
/// pushed. The pushes go in the background: a failure is only logged.
fn notify(
    app: &Platform,
    n: &Notice,
    to: Vec<(String, Vec<(&'static str, String)>)>,
    quiet: &[String],
) -> anyhow::Result<Vec<String>> {
    let now = now_ms();
    let mut sends = Vec::new();
    for (account, reasons) in to {
        for (reason, _) in &reasons {
            app.db.notice_inbox(&account, n.item, reason, now)?;
        }
        if quiet.contains(&account)
            || !app.inbox.may_push(&account, n.item, now)
            || app.limits.check_account(INBOX_PUSHES, &account).is_err()
        {
            continue;
        }
        let whys: Vec<&str> = reasons.iter().map(|(_, w)| w.as_str()).collect();
        let msg = json!({
            "inbox": n.tap,
            "title": format!("{} {}", n.what, whys.join("; ")),
            "body": n.body,
            "tag": format!("inbox-{}", n.item),
            "control": true,
        });
        // The item and whom; never its title.
        info!(item = %n.item, %account, "inbox push");
        sends.push((account, msg));
    }
    let pushed = sends.iter().map(|(a, _)| a.clone()).collect();
    let hub = app.hub.clone();
    tokio::spawn(async move {
        for (account, msg) in sends {
            if let Err(e) = hub.push_with(&account, &msg, PushOptions::default()).await {
                warn!(error = %e, %account, "the inbox's push didn't go");
            }
        }
    });
    Ok(pushed)
}

/// Who did it, as a push names them: a person, or "<owner>'s agent on
/// <machine>".
async fn actor_name(app: &Platform, by: Actor<'_>) -> anyhow::Result<String> {
    let me = chat::name_of(app, by.account).await?;
    Ok(match by.agent {
        Some(m) => {
            let machine = app.cache.daemons(&*app.hub, &[m.to_owned()]).await?.remove(m).map(|d| d.name);
            format!("{me}'s agent on {}", machine.unwrap_or_else(|| m.to_owned()))
        }
        None => me,
    })
}

/// `<project>#<n>`, as a push and the inbox name a native issue.
fn issue_what(app: &Platform, issue: &db::Issue) -> anyhow::Result<String> {
    let project = app.db.issue_project(&issue.project)?.map(|p| p.name).unwrap_or_default();
    Ok(format!("{project}#{}", issue.number))
}

/// Whom a native issue's text mentions: its project's members with chat on
/// (who may read it), named by an `@`, but not `by`.
async fn issue_mentions(app: &Platform, project: &str, text: &str, by: &str) -> anyhow::Result<Vec<String>> {
    if !text.contains('@') {
        return Ok(vec![]);
    }
    let mut audience = Vec::new();
    for a in members(app, project).await? {
        if chat_on(app, &a).await? {
            audience.push(a);
        }
    }
    Ok(chat::mentions(app, text, &audience, by).await?.0)
}

/// Each person's reasons, in the order they came.
fn add(to: &mut Vec<(String, Vec<(&'static str, String)>)>, account: &str, reason: &'static str, why: String) {
    match to.iter_mut().find(|(a, _)| a == account) {
        Some((_, rs)) => rs.push((reason, why)),
        None => to.push((account.to_owned(), vec![(reason, why)])),
    }
}

/// A native issue was opened (`issues::create`): its assignees and those
/// its body mentions. Never fails the write: what goes wrong is logged.
pub async fn issue_opened(app: &Platform, by: Actor<'_>, issue: &db::Issue) {
    if let Err(e) = issue_changed_or_opened(app, by, None, issue).await {
        warn!(issue = %issue.id, error = format!("{e:#}"), "inbox: an issue opened");
    }
}

/// A native issue changed (`issues::change`): those newly assigned (an
/// entry, and a push), those taken off (their entry goes), and those its
/// new body newly mentions.
pub async fn issue_changed(app: &Platform, by: Actor<'_>, old: &db::Issue, new: &db::Issue) {
    if let Err(e) = issue_changed_or_opened(app, by, Some(old), new).await {
        warn!(issue = %new.id, error = format!("{e:#}"), "inbox: an issue changed");
    }
}

async fn issue_changed_or_opened(
    app: &Platform,
    by: Actor<'_>,
    old: Option<&db::Issue>,
    issue: &db::Issue,
) -> anyhow::Result<()> {
    let was: &[String] = old.map_or(&[], |o| &o.assignees);
    for gone in was.iter().filter(|a| !issue.assignees.contains(a)) {
        app.db.drop_inbox(gone, &issue.id, ASSIGNED)?;
    }
    // Newly assigned, with chat (and so native issues) on, as mentions are.
    let mut assigned: Vec<&String> = Vec::new();
    for a in issue.assignees.iter().filter(|a| !was.contains(a) && *a != by.account) {
        if chat_on(app, a).await? {
            assigned.push(a);
        }
    }
    // Those the body names now and didn't before.
    let mentioned = match old {
        Some(o) if o.body == issue.body => vec![],
        _ => issue_mentions(app, &issue.project, &issue.body, by.account).await?,
    };
    let before = match old {
        Some(o) if !mentioned.is_empty() => issue_mentions(app, &o.project, &o.body, by.account).await?,
        _ => vec![],
    };
    let mentioned: Vec<&String> = mentioned.iter().filter(|a| !before.contains(a)).collect();
    if assigned.is_empty() && mentioned.is_empty() {
        return Ok(());
    }
    let me = actor_name(app, by).await?;
    let mut to = Vec::new();
    for a in assigned {
        add(&mut to, a, ASSIGNED, format!("assigned to you by {me}"));
    }
    for a in mentioned {
        add(&mut to, a, MENTIONED, format!("{me} mentioned you"));
    }
    let n = Notice {
        item: &issue.id,
        what: issue_what(app, issue)?,
        body: issue.title.clone(),
        tap: json!({ "item": issue.id }),
    };
    notify(app, &n, to, &[])?;
    Ok(())
}

/// A message in an object's thread mentions `named` (accounts that see
/// it: chat worked them out), by `me`: an entry for each, and a push for
/// those not `quiet`, instead of chat's. Whom it pushed.
pub fn thread_mentioned(
    app: &Platform,
    room: &Room,
    me: &str,
    (seq, text): (i64, &str),
    named: &[String],
    quiet: &[String],
) -> anyhow::Result<Vec<String>> {
    let Some(target) = room.target.as_deref() else { return Ok(vec![]) };
    let Some(reason) = thread_reason(target) else { return Ok(vec![]) };
    let what = match app.db.issue(target)? {
        Some(issue) if reason == MENTIONED => issue_what(app, &issue)?,
        _ => room.name.clone(),
    };
    let why =
        if reason == MENTIONED { format!("{me} mentioned you") } else { format!("{me} mentioned you in its thread") };
    let to = named.iter().map(|a| (a.clone(), vec![(reason, why.clone())])).collect();
    let n = Notice {
        item: target,
        what,
        body: text.chars().take(chat_wire::PUSH_CHARS).collect(),
        tap: json!({ "item": target, "room": room.id, "seq": seq }),
    };
    notify(app, &n, to, quiet)
}

/// The reader's entries on native issues, as the inbox lists them: those
/// in projects they may read now, newest first.
pub async fn entries(app: &Platform, account: &str, f: &InboxFilter) -> Result<Vec<wire::InboxEntry>, ApiError> {
    let rows = app.db.native_inbox(account, f)?;
    let mut projects: HashMap<String, Option<String>> = HashMap::new();
    for r in &rows {
        if projects.contains_key(&r.project) {
            continue;
        }
        let name = match crate::issues::place(app, Actor::person(account), &r.project, false).await {
            Ok(p) => Some(p.project.name),
            Err(ApiError(StatusCode::NOT_FOUND | StatusCode::FORBIDDEN, ..)) => None,
            // One project's trouble leaves out its entries, not the inbox.
            Err(ApiError(status, said, _)) => {
                warn!(project = %r.project, %status, said, "inbox: a project's entries left out");
                None
            }
        };
        projects.insert(r.project.clone(), name);
    }
    let authors: Vec<String> = rows.iter().map(|r| r.author.clone()).filter(|a| !a.is_empty()).collect();
    let names = app.cache.accounts(&*app.hub, &authors).await?;
    let entry = |r: NativeRow, project: &str| wire::InboxEntry {
        url: format!("/issues/{}", r.item),
        author: names.get(&r.author).map(|a| a.name.clone()),
        item: r.item,
        reason: r.reason,
        repo: project.to_owned(),
        number: r.number,
        kind: "issue".into(),
        state: r.state,
        title: r.title,
        created_at: r.created_at,
        updated_at: r.updated_at,
        seen_at: r.seen_at,
        done_at: r.done_at,
        project: Some(r.project),
        room: None,
    };
    Ok(rows
        .into_iter()
        .filter_map(|r| {
            let name = projects.get(&r.project).cloned().flatten()?;
            Some(entry(r, &name))
        })
        .collect())
}

/// A person's marks on a native issue's entries: 404 unless they may read
/// it.
pub async fn mark(app: &Platform, account: &str, item: &str, b: &wire::InboxMark) -> Result<u32, ApiError> {
    let nothing = || arugula_control_common::err(StatusCode::NOT_FOUND, "no such item");
    let issue = app.db.issue(item)?.ok_or_else(nothing)?;
    match crate::issues::place(app, Actor::person(account), &issue.project, false).await {
        Ok(_) => {}
        Err(ApiError(StatusCode::NOT_FOUND | StatusCode::FORBIDDEN, ..)) => return Err(nothing()),
        Err(e) => return Err(e),
    }
    let n = app.db.mark_native_inbox(account, item, b.reason.as_deref(), (b.seen, b.done), now_ms())?;
    Ok(n as u32)
}
