//! `@arugula` in a thread (M256, thread = session, Arugula #129, "A
//! thread's agent session runs on the project's hosted box", aru-115):
//! the one mention that starts work. Chat calls [`posted`] after it keeps
//! each person's message ([`super::post_full`]); everything else is here.
//!
//! - **The word is reserved.** `@arugula` never names a person or a
//!   machine ([`super::mentions`] leaves it out), so nobody is pushed for it
//!   and it's never "unreached".
//! - **Where.** A project's channel (a message there is its own thread's
//!   first), a reply thread in one, or the thread on a native issue
//!   (`iss_`) or a mirrored PR or issue (`fi_`), whose thread is the
//!   object's room itself. Anywhere else a line says where it works.
//! - **Who** ([`crate::triggers::may_start`], the rule schedules and event
//!   rules fire by): an owner or editor of the project, on the hosted
//!   beta's list. Anyone else gets a line saying why, and no session.
//! - **Which box:** the project's "agents run on" setting
//!   (`Db::agents_run_on`): the one chosen, else its only workspace. With
//!   several and none chosen, a line asks which; with none, a line says to
//!   set one up.
//! - **Which repo:** the box's spec's repos (GitHub clone URLs, as
//!   `owner/name`). One is it. Several: a line lists them and asks. None:
//!   the session works in no repo, and its first line says so.
//! - **Answering a question** (which box, which repo): the asker's next
//!   reply in the thread that names exactly one of those listed (a box by
//!   its name; a repo as `owner/name`, or by its name when that's
//!   unambiguous) picks it. Nothing is stored for it: the thread is read
//!   again from the ask on, so a reply that names none, or several, or
//!   anyone else's reply, is ordinary chat. A box's answer may name the
//!   repo too.
//! - **Starting:** the session and its job in one transaction
//!   (`db::sessions::create_session_in`, `db::sessions::dispatch`), bound
//!   to the room and the thread, its branch's slug from the ask's words
//!   (on a native issue, from the issue: `i<number>-<title's words>`, as
//!   `arugula issue open` names its branches), its first prompt the
//!   hydrated thread ([`crate::sessions::hydrate`]).
//! - **Agent on this** ([`issue_agent`], M265) on a native issue writes
//!   [`AGENT_ON_THIS`] in the issue's thread as the person who pressed it,
//!   so it's exactly an `@arugula` there: the same checks, questions and
//!   session.
//!   Its status card (M257, [`super::session_card::post_card`]) says
//!   where it runs.
//! - **`@codex review`** (M264, [`crate::sessions::review`]): in a thread
//!   with a session going, a read-only review of what it pushed, posted in
//!   the thread. It's never a prompt to the session.
//! - **Replies.** In a thread with a session going, a reply from its asker
//!   or any owner or editor is a prompt, credited to its author
//!   (`session_prompts.author`), queued while a turn runs; the session's
//!   job is nudged so a paused box wakes. Billing stays the asker's. A
//!   viewer's reply stays ordinary chat. A message an agent or a product
//!   posted never starts or prompts a session, and neither does an edit.
//!
//! Lines are the platform's: no author, `via` [`SYSTEM_VIA`], in the
//! thread. A failure here is logged: the message was posted either way.

use anyhow::Result;
use arugula_control_common::{ApiError, err};
use arugula_control_wire::{
    chat::{ChatLive, SYSTEM_VIA},
    sessions::IssueAgent,
};
use arugula_e2e::{now_ms, team::TeamRole};
use axum::http::StatusCode;
use tracing::{info, warn};

use super::at_words;
use crate::{
    Platform,
    auth::Person,
    db::{
        Taken,
        chat::{Message, NewMessage, Room},
        hosted::HostedWorkspace,
        sessions::{AgentsOn, NewPrompt, NewSession, Source, create_session_in, dispatch, issue_slug, slug},
    },
    sessions::hydrate,
    triggers::{Cannot, may_start},
};

/// The reserved word: `@arugula`.
pub const MENTION: &str = "arugula";

/// Where `@arugula` works, said where it doesn't.
pub const NOT_HERE: &str =
    "@arugula works in a project's channels, and in the threads on its issues and pull requests.";
/// Said when the project has no hosted workspace.
pub const NO_BOX: &str =
    "@arugula runs on this project's cloud workspace, and it has none: set one up first, then ask again.";

/// A person posted `text` as message `seq` (under `parent`) in `room`: start
/// a session, answer one's question, or prompt one. An agent's or a
/// product's message does nothing here.
pub(crate) async fn posted(
    app: &Platform,
    room: &Room,
    author: &str,
    by_agent_or_product: bool,
    seq: i64,
    parent: Option<i64>,
    text: &str,
) {
    if by_agent_or_product || author.is_empty() {
        return;
    }
    if let Err(e) = handle(app, room, author, seq, parent, text).await {
        warn!(room = room.id, seq, error = %format!("{e:#}"), "@arugula in a thread");
    }
}

/// The thread a message is in: its parent's, or a channel's message's own,
/// or (0) an object's room.
fn thread_of(room: &Room, seq: i64, parent: Option<i64>) -> u64 {
    match (parent, room.kind.as_str()) {
        (Some(p), _) => p as u64,
        (None, "object") => 0,
        (None, _) => seq as u64,
    }
}

/// Whether `@arugula` works in this room: a channel, or the thread on a
/// native issue or a mirrored item.
pub(super) fn works_in(room: &Room) -> bool {
    match room.kind.as_str() {
        "channel" => true,
        "object" => room.target.as_deref().is_some_and(|t| t.starts_with("iss_") || t.starts_with("fi_")),
        _ => false,
    }
}

pub(crate) fn asks(text: &str) -> bool {
    at_words(text).iter().any(|w| w == MENTION)
}

async fn handle(app: &Platform, room: &Room, author: &str, seq: i64, parent: Option<i64>, text: &str) -> Result<()> {
    let thread = thread_of(room, seq, parent);
    // `@codex review` (M264): a review of the thread's session, never a
    // prompt to it.
    if crate::sessions::review::mentions(text) && works_in(room) {
        return crate::sessions::review::asked(app, room, thread, author, text).await;
    }
    if let Some(s) = app.db.session_on_thread(&room.id, thread)? {
        return reply(app, &s, author, text).await;
    }
    let asked = asks(text);
    if !works_in(room) {
        if asked {
            say(app, room, thread, NOT_HERE)?;
        }
        return Ok(());
    }
    let Some(project) = app.db.chat_room_project(&room.id)? else {
        if asked {
            say(app, room, thread, NOT_HERE)?;
        }
        return Ok(());
    };
    if asked {
        return start(app, room, thread, &project, author).await;
    }
    // Only a thread, not a channel's new message, has a question to answer.
    if parent.is_some() || thread == 0 {
        answer(app, room, thread, &project, author, seq).await?;
    }
    Ok(())
}

/// Post the platform's line in the thread.
pub(crate) fn say(app: &Platform, room: &Room, thread: u64, text: &str) -> Result<i64> {
    let parent = (thread > 0).then_some(thread as i64);
    let seq = app.db.add_chat_message(
        &room.id,
        &NewMessage {
            author: "",
            at: now_ms(),
            text,
            quote: None,
            ref_machine: None,
            ref_pane: None,
            mentions: &[],
            agent: None,
            via: Some(SYSTEM_VIA),
            parent,
            also: false,
        },
    )?;
    let ev = match parent {
        Some(parent) => ChatLive::Reply { room: room.id.clone(), parent, seq },
        None => ChatLive::Message { room: room.id.clone(), seq },
    };
    app.chat.publish(&app.db, ev, None);
    Ok(seq)
}

/// Why `who` can't start a session, in a line.
pub(crate) fn refusal(who: &str, why: Cannot) -> String {
    match why {
        Cannot::Viewer => format!("@arugula needs an owner or editor of this project to ask: {who} can only view it."),
        Cannot::Unlisted => {
            format!("@arugula runs on cloud workspaces, which are in a limited beta: {who} isn't on its list yet.")
        }
        Cannot::Outside => format!("@arugula needs a member of this project to ask: {who} isn't in it."),
    }
}

/// The box's repos as `owner/name` (or the clone URL, when it isn't
/// GitHub's), in its spec's order.
fn repos(w: &HostedWorkspace) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for r in &w.spec.repos {
        let name = crate::hosted::setup::github_repo(r).unwrap_or_else(|| r.clone());
        if !out.contains(&name) {
            out.push(name);
        }
    }
    out
}

/// The words of `text` as names may be written: lowercase, split at
/// anything but letters, digits, `/`, `.`, `-` and `_`, trailing dots off.
fn words(text: &str) -> Vec<String> {
    text.split(|c: char| !(c.is_alphanumeric() || matches!(c, '/' | '.' | '-' | '_')))
        .map(|w| w.trim_end_matches('.').to_lowercase())
        .filter(|w| !w.is_empty())
        .collect()
}

/// The one of `options` that `text` names, when it names exactly one: by
/// any of each option's names.
fn named<'a>(text: &str, options: &'a [(String, Vec<String>)]) -> Option<&'a str> {
    let ws = words(text);
    let mut hit = options.iter().filter(|(_, names)| names.iter().any(|n| ws.contains(&n.to_lowercase())));
    let first = hit.next()?;
    hit.next().is_none().then_some(first.0.as_str())
}

/// A repo goes by `owner/name`, and by `name` when no other listed repo
/// has it.
fn repo_options(all: &[String]) -> Vec<(String, Vec<String>)> {
    let short = |r: &str| r.rsplit('/').next().unwrap_or(r).to_owned();
    all.iter()
        .map(|r| {
            let mut names = vec![r.clone()];
            let s = short(r);
            if all.iter().filter(|o| short(o) == s).count() == 1 && s != *r {
                names.push(s);
            }
            (r.clone(), names)
        })
        .collect()
}

fn list(items: &[String]) -> String {
    match items {
        [] => String::new(),
        [one] => one.clone(),
        [init @ .., last] => format!("{} or {last}", init.join(", ")),
    }
}

/// The project's live workspaces among `ids`, by id.
fn workspaces(app: &Platform, project: &str, ids: &[String]) -> Result<Vec<HostedWorkspace>> {
    let mut out = Vec::new();
    for id in ids {
        if let Some(w) = app.db.hosted_any(project, id)?.filter(|w| !matches!(w.state.as_str(), "deleting" | "deleted"))
        {
            out.push(w);
        }
    }
    Ok(out)
}

/// `@arugula` in a thread with no session going.
async fn start(app: &Platform, room: &Room, thread: u64, project: &str, asker: &str) -> Result<()> {
    if let Some(why) = may_start(app, project, asker).await? {
        let who = super::name_of(app, asker).await?;
        say(app, room, thread, &refusal(if who.is_empty() { "the asker" } else { &who }, why))?;
        return Ok(());
    }
    let w = match app.db.agents_run_on(project)? {
        AgentsOn::Nowhere => {
            say(app, room, thread, NO_BOX)?;
            return Ok(());
        }
        AgentsOn::Ask(ids) => {
            let names: Vec<String> = workspaces(app, project, &ids)?.into_iter().map(|w| w.name).collect();
            say(
                app,
                room,
                thread,
                &format!(
                    "Which cloud workspace should @arugula run on? Reply with its name: {}. (An owner or editor can \
                     choose one for the project in its settings: agents run on.)",
                    list(&names)
                ),
            )?;
            return Ok(());
        }
        AgentsOn::Chosen(id) | AgentsOn::Only(id) => {
            workspaces(app, project, &[id])?.pop().ok_or_else(|| anyhow::anyhow!("the agents' workspace is gone"))?
        }
    };
    with_box(app, room, thread, project, asker, &w).await
}

/// The box is settled: ask which repo when there are several, else start.
async fn with_box(
    app: &Platform,
    room: &Room,
    thread: u64,
    project: &str,
    asker: &str,
    w: &HostedWorkspace,
) -> Result<()> {
    let all = repos(w);
    if all.len() > 1 {
        say(
            app,
            room,
            thread,
            &format!("Which repo should @arugula work in on {}? Reply with one: {}.", w.name, list(&all)),
        )?;
        return Ok(());
    }
    launch(app, room, thread, project, asker, w, all.first().map(String::as_str)).await
}

/// The ask in this thread that's waiting on its asker: the newest message
/// that asks `@arugula`, by a person, after the thread's last session was
/// made.
async fn waiting_ask(app: &Platform, room: &Room, thread: u64, before: i64) -> Result<Option<(Message, Vec<Message>)>> {
    let ms = hydrate::messages(app, room, thread)?;
    let since = app.db.latest_thread_session(&room.id, thread)?.map(|s| s.created_at);
    let Some(i) = ms.iter().rposition(|m| {
        m.seq < before
            && m.deleted.is_none()
            && m.agent.is_none()
            && m.via.is_none()
            && !m.author.is_empty()
            && asks(&m.text)
            && since.is_none_or(|t| m.at > t)
    }) else {
        return Ok(None);
    };
    let ask = ms[i].clone();
    let after: Vec<Message> =
        ms[i + 1..].iter().filter(|m| m.author == ask.author && m.deleted.is_none()).cloned().collect();
    Ok(Some((ask, after)))
}

/// A reply in a thread with no session going: the answer to its asker's
/// question, when it is one (module docs).
async fn answer(app: &Platform, room: &Room, thread: u64, project: &str, author: &str, seq: i64) -> Result<()> {
    let Some((ask, said)) = waiting_ask(app, room, thread, seq).await? else { return Ok(()) };
    if ask.author != author || !said.iter().any(|m| m.seq == seq) {
        return Ok(());
    }
    if may_start(app, project, author).await?.is_some() {
        return Ok(());
    }
    let (w, from) = match app.db.agents_run_on(project)? {
        AgentsOn::Nowhere => return Ok(()),
        AgentsOn::Chosen(id) | AgentsOn::Only(id) => match workspaces(app, project, &[id])?.pop() {
            Some(w) => (w, None),
            None => return Ok(()),
        },
        AgentsOn::Ask(ids) => {
            let ws = workspaces(app, project, &ids)?;
            let options: Vec<(String, Vec<String>)> = ws.iter().map(|w| (w.id.clone(), vec![w.name.clone()])).collect();
            let Some((m, id)) = said.iter().find_map(|m| named(&m.text, &options).map(|id| (m, id))) else {
                return Ok(());
            };
            let w = ws.into_iter().find(|w| w.id == id).expect("named among them");
            (w, Some(m.seq))
        }
    };
    let all = repos(&w);
    if all.len() <= 1 {
        // Only the box was asked: this is its answer, or nothing.
        return match from {
            Some(s) if s == seq => {
                launch(app, room, thread, project, author, &w, all.first().map(String::as_str)).await
            }
            _ => Ok(()),
        };
    }
    let options = repo_options(&all);
    let repo_said = said.iter().filter(|m| from.is_none_or(|f| m.seq >= f));
    if let Some(r) = repo_said.clone().find_map(|m| named(&m.text, &options)) {
        // Named in this reply, or this reply settled the box after a repo
        // was named: either way it's now.
        return launch(app, room, thread, project, author, &w, Some(r)).await;
    }
    if from == Some(seq) {
        // This reply chose the box: now the repo.
        return with_box(app, room, thread, project, author, &w).await;
    }
    Ok(())
}

/// Make the session and its job, bound to the thread, and say so.
async fn launch(
    app: &Platform,
    room: &Room,
    thread: u64,
    project: &str,
    asker: &str,
    w: &HostedWorkspace,
    repo: Option<&str>,
) -> Result<()> {
    let ask = waiting_ask(app, room, thread, i64::MAX).await?.map(|(m, _)| m.text).unwrap_or_default();
    let words: Vec<&str> = ask.split_whitespace().filter(|w| !w.starts_with('@')).collect();
    // On a native issue, the branch is named for the issue (M265).
    let issue = match room.target.as_deref().filter(|t| room.kind == "object" && t.starts_with("iss_")) {
        Some(t) => app.db.issue(t)?,
        None => None,
    };
    let slug = match &issue {
        Some(i) => issue_slug(i.number, &i.title),
        None => slug(&words.join(" ")),
    };
    let prompt = hydrate::first_prompt(app, room, thread, project, asker, repo, &slug).await?;
    let new = NewSession {
        project,
        room: &room.id,
        parent_seq: thread,
        asker,
        workspace: &w.id,
        repo,
        slug: &slug,
        prompt: NewPrompt { author: asker, source: Source::Member, association: None, text: &prompt },
    };
    let now = now_ms();
    let made = {
        let mut c = app.db.c();
        let tx = c.immediate()?;
        match create_session_in(&tx, &new, now) {
            Ok(Some((s, _))) => {
                dispatch(&tx, &s.id, now)?;
                tx.commit()?;
                Some(s)
            }
            Ok(None) => None,
            // Another message started one on this thread just now.
            Err(e) if e.downcast_ref::<Taken>().is_some() => return Ok(()),
            Err(e) => return Err(e),
        }
    };
    let Some(s) = made else { anyhow::bail!("the session's project or workspace is gone") };
    info!(session = s.id, room = room.id, thread, workspace = w.id, "a thread asked @arugula: session made");
    // Its status card (M257) says it started, and where, from here on.
    super::session_card::post_card(app, &s)?;
    Ok(())
}

/// A reply in a thread with a session going: a prompt from its asker or an
/// owner or editor; anyone else's is ordinary chat.
async fn reply(app: &Platform, s: &crate::db::sessions::Session, author: &str, text: &str) -> Result<()> {
    if text.trim().is_empty() {
        return Ok(());
    }
    if author != s.asker {
        match crate::forge::links::membership(app, author, &s.project, true).await {
            Ok((_, TeamRole::Owner | TeamRole::Editor)) => {}
            Ok(_) => return Ok(()),
            Err(e) if e.0 == axum::http::StatusCode::NOT_FOUND => return Ok(()),
            Err(e) => anyhow::bail!("asking the hub who's in the project: {}", e.1),
        }
    }
    let now = now_ms();
    let p = NewPrompt { author, source: Source::Member, association: None, text };
    match app.db.queue_prompt(&s.id, &p, now)? {
        Ok(p) => {
            info!(session = s.id, prompt = p.id, "a reply in a session's thread queued a prompt");
            app.db.nudge_session(&s.id, now)?;
        }
        Err(why) => info!(session = s.id, refused = %why, "a reply in a session's thread wasn't queued"),
    }
    Ok(())
}

/// What Agent on this writes in an issue's thread, as its presser.
pub const AGENT_ON_THIS: &str = "@arugula work on this issue";

/// `POST /api/issues/{issue}/agent`: Agent on this (module docs). Refused
/// up front for whoever `@arugula` would refuse, and while a session is
/// going on the issue (a reply in its thread prompts that one).
pub async fn issue_agent(
    axum::extract::State(app): axum::extract::State<std::sync::Arc<Platform>>,
    Person { account }: Person,
    axum::extract::Path(issue): axum::extract::Path<String>,
) -> Result<axum::Json<IssueAgent>, ApiError> {
    let none = || err(StatusCode::NOT_FOUND, "no such issue");
    if !issue.starts_with("iss_") {
        return Err(none());
    }
    let i = app.db.issue(&issue)?.ok_or_else(none)?;
    match may_start(&app, &i.project, &account).await? {
        None => {}
        Some(Cannot::Outside) => return Err(none()),
        Some(why) => return Err(err(StatusCode::FORBIDDEN, &refusal("you", why).replace("you isn't", "you aren't"))),
    }
    let axum::Json(room) = super::open_object(&app, &account, &issue).await?;
    if app.db.session_on_thread(&room.id, 0)?.is_some() {
        return Err(err(
            StatusCode::CONFLICT,
            "an agent session is going on this issue: reply in its thread to prompt it",
        ));
    }
    let post = arugula_control_wire::chat::ChatPost { text: AGENT_ON_THIS.into(), ..Default::default() };
    let posted = super::post_full(&app, &account, None, None, &room.id, post).await?;
    let session = app.db.session_on_thread(&room.id, 0)?.filter(|s| s.asker == account).map(|s| s.id);
    info!(issue, room = room.id, session = ?session, "agent on this issue");
    Ok(axum::Json(IssueAgent { room: room.id, seq: posted.seq, session }))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn opts(names: &[&str]) -> Vec<(String, Vec<String>)> {
        names.iter().map(|n| ((*n).to_owned(), vec![(*n).to_owned()])).collect()
    }

    /// A reply picks the one option it names, and nothing when it names
    /// none or several.
    #[test]
    fn a_reply_names_one_option() {
        let boxes = opts(&["dev", "staging"]);
        assert_eq!(named("dev please.", &boxes), Some("dev"));
        assert_eq!(named("Staging", &boxes), Some("staging"));
        assert_eq!(named("either dev or staging", &boxes), None);
        assert_eq!(named("devbox", &boxes), None);
        let repos = repo_options(&["o/app".into(), "o/web".into(), "p/web".into()]);
        assert_eq!(named("app", &repos), Some("o/app"));
        assert_eq!(named("web", &repos), None, "two repos are called web");
        assert_eq!(named("use p/web.", &repos), Some("p/web"));
    }
}
