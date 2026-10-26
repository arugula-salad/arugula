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
//! - **Which box** ([`crate::sessions::pool`], Arugula #214): the
//!   project's "agents run on" setting (`Db::agents_run_on`), else its
//!   only workspace, else the pool: the session picks one of those with
//!   the repo, awake with a free slot first, then the least busy, then by
//!   name. With none, a line says to set one up. Its card says which, and
//!   Move to… sends it to another ([`crate::sessions::moves`]).
//! - **Which repo:** the boxes' specs' repos (GitHub clone URLs, as
//!   `owner/name`). The one the ask names (`owner/name`, or by its name
//!   when that's unambiguous) is it; else the only one; none, the session
//!   works in no repo, and its first line says so. A repo the project
//!   linked that no box has, named by `owner/name`, is said so. Several,
//!   and the ask named none: an ask ([`crate::sessions::asks`]) mentions
//!   the asker, in the thread and the channel, with a button per repo
//!   ([`ask_show`], [`ask_answer`]).
//! - **Answering it in words** (the fallback): the asker's next reply in
//!   the thread that names exactly one of the repos picks it (and one
//!   naming a box by its name runs on that box). Nothing is stored for
//!   it: the thread is read again from the ask on, so a reply that names
//!   none, or several, or anyone else's reply, is ordinary chat.
//!   The questions and the answers are for the platform: the session's
//!   first prompt leaves them out ([`answers`]).
//! - **Starting:** the session and its job in one transaction
//!   (`db::sessions::create_session_in`, `db::sessions::dispatch`), bound
//!   to the room and the thread, its branch's slug from the ask's words
//!   (on a native issue, from the issue: `i<number>-<title's words>`, as
//!   `arugula issue open` names its branches), its first prompt the
//!   hydrated thread ([`crate::sessions::hydrate`]).
//! - **Agent on this** ([`issue_agent`], M265) on a native issue writes
//!   [`AGENT_ON_THIS`] in the issue's thread as the person who pressed it,
//!   so it's exactly an `@arugula` there: the same checks, asks and
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
    sessions::{ChatAsk, ChatAskAnswer, IssueAgent},
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
        sessions::{NewPrompt, NewSession, Source, create_session_in, dispatch, issue_slug, slug},
    },
    sessions::{asks, hydrate, pool},
    triggers::{Cannot, may_start},
};

/// The reserved word: `@arugula`.
pub const MENTION: &str = "arugula";

/// Where `@arugula` works, said where it doesn't.
pub const NOT_HERE: &str =
    "@arugula works in a project's channels, and in the threads on its issues and pull requests.";
/// The platform's which-box question starts so.
pub const WHICH_BOX: &str = "Which cloud workspace should @arugula run on?";
/// The platform's which-repo question starts so.
pub const WHICH_REPO: &str = "Which repo should @arugula work in on ";

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
        return start(app, room, thread, &project, author, text).await;
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

fn list(items: &[String]) -> String {
    match items {
        [] => String::new(),
        [one] => one.clone(),
        [init @ .., last] => format!("{} or {last}", init.join(", ")),
    }
}

/// `@arugula` in a thread with no session going: the boxes it may run on
/// ([`pool::candidates`]), the repo ([`pool::settle`]), then the box
/// ([`pool::pick`]).
async fn start(app: &Platform, room: &Room, thread: u64, project: &str, asker: &str, text: &str) -> Result<()> {
    if let Some(why) = may_start(app, project, asker).await? {
        let who = super::name_of(app, asker).await?;
        say(app, room, thread, &refusal(if who.is_empty() { "the asker" } else { &who }, why))?;
        return Ok(());
    }
    let boxes = pool::candidates(app, project)?;
    if boxes.is_empty() {
        say(app, room, thread, NO_BOX)?;
        return Ok(());
    }
    settled(app, room, thread, project, asker, boxes, &[text]).await
}

/// The boxes are these: settle the repo from what the asker wrote, then
/// start, or say what's missing, or ask which repo.
async fn settled(
    app: &Platform,
    room: &Room,
    thread: u64,
    project: &str,
    asker: &str,
    boxes: Vec<HostedWorkspace>,
    said: &[&str],
) -> Result<()> {
    let all = pool::all_repos(&boxes);
    let linked = pool::linked(app, project)?;
    let mut how = pool::Settled::Ambiguous;
    for text in if said.is_empty() { &[""][..] } else { said } {
        how = pool::settle(text, &all, &linked);
        if how != pool::Settled::Ambiguous {
            break;
        }
    }
    match how {
        pool::Settled::Repo(repo) => go(app, room, thread, project, asker, boxes, repo.as_deref()).await,
        pool::Settled::Missing(repo) => {
            say(app, room, thread, &missing(&repo))?;
            Ok(())
        }
        pool::Settled::Ambiguous => {
            let on = match boxes.as_slice() {
                [one] => Some(one.name.as_str()),
                _ => None,
            };
            ask_repo(app, room, thread, asker, &all, on).await
        }
    }
}

/// Said when the ask names a repo the project linked and none of its
/// workspaces has.
pub fn missing(repo: &str) -> String {
    format!("None of this project's cloud workspaces has {repo}: add it to one's repos (its settings), then ask again.")
}

/// The repo is settled: the best of `boxes` with it, then start.
async fn go(
    app: &Platform,
    room: &Room,
    thread: u64,
    project: &str,
    asker: &str,
    boxes: Vec<HostedWorkspace>,
    repo: Option<&str>,
) -> Result<()> {
    let Some(w) = pool::pick(app, project, boxes, repo).await? else {
        say(app, room, thread, &missing(repo.unwrap_or("that repo")))?;
        return Ok(());
    };
    launch(app, room, thread, project, asker, &w, repo).await
}

/// The which-repo question, as an ask (Arugula #214): a form on its card,
/// a mention of its asker until it's answered, and its text the plain
/// fallback ("Reply with one").
pub fn repo_question(all: &[String], on: Option<&str>) -> String {
    let on = on.map(|n| format!(" on {n}")).unwrap_or_default();
    format!("Which repo should @arugula work in{on}? Reply with one: {}.", list(all))
}

async fn ask_repo(
    app: &Platform,
    room: &Room,
    thread: u64,
    asker: &str,
    all: &[String],
    on: Option<&str>,
) -> Result<()> {
    // A newer ask in the thread takes an older one's place.
    asks::close(app, room, thread, asks::REPO)?;
    asks::post(app, room, thread, asks::REPO, asker, &repo_question(all, on)).await?;
    Ok(())
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
        ms[i + 1..].iter().filter(|m| m.author == ask.author && m.deleted.is_none() && own(m)).cloned().collect();
    Ok(Some((ask, after)))
}

/// Whether `m` is its author's own words: not their agent's, nor a
/// product's for them (a session's answer is its asker's, `via` it).
fn own(m: &Message) -> bool {
    m.agent.is_none() && m.via.is_none()
}

/// Of a thread's messages `ms`, oldest first, the numbers of `asker`'s
/// replies that answered the platform's which-box or which-repo question
/// (as [`answer`] reads them: after the question, before the session's
/// card, naming exactly one of the project's boxes or their repos). The
/// first prompt leaves them out ([`hydrate::first_prompt`]): they were
/// for the platform, not the agent.
pub(crate) fn answers(app: &Platform, project: &str, asker: &str, ms: &[Message]) -> Result<Vec<i64>> {
    let ws: Vec<HostedWorkspace> = app
        .db
        .hosted_workspaces(project)?
        .into_iter()
        .filter(|w| !matches!(w.state.as_str(), "deleting" | "deleted"))
        .collect();
    let boxes: Vec<(String, Vec<String>)> = ws.iter().map(|w| (w.id.clone(), vec![w.name.clone()])).collect();
    let mut all: Vec<String> = Vec::new();
    for r in ws.iter().flat_map(pool::repos) {
        if !all.contains(&r) {
            all.push(r);
        }
    }
    let in_repos = pool::repo_options(&all);
    let (mut asked, mut out) = (false, Vec::new());
    for m in ms.iter().filter(|m| m.deleted.is_none()) {
        if m.author.is_empty() {
            let line = m.via.as_deref() == Some(SYSTEM_VIA);
            // The which-repo ask (Arugula #214), or the older lines.
            let repo_ask = m.via.as_deref() == Some(asks::via(asks::REPO).as_str());
            if repo_ask || line && (m.text.starts_with(WHICH_BOX) || m.text.starts_with(WHICH_REPO)) {
                asked = true;
            } else if m.via.as_deref().and_then(arugula_control_wire::sessions::card_of).is_some() {
                // The session started: replies from here on are prompts.
                asked = false;
            }
        } else if asked
            && m.author == asker
            && own(m)
            && (pool::named(&m.text, &boxes).is_some() || pool::named(&m.text, &in_repos).is_some())
        {
            out.push(m.seq);
        }
    }
    Ok(out)
}

/// A reply in a thread with no session going: the answer to its asker's
/// question, when it is one (module docs). The asker may also name one of
/// the boxes, to run on that one.
async fn answer(app: &Platform, room: &Room, thread: u64, project: &str, author: &str, seq: i64) -> Result<()> {
    let Some((ask, said)) = waiting_ask(app, room, thread, seq).await? else { return Ok(()) };
    if ask.author != author || !said.iter().any(|m| m.seq == seq) {
        return Ok(());
    }
    if may_start(app, project, author).await?.is_some() {
        return Ok(());
    }
    let mut boxes = pool::candidates(app, project)?;
    if boxes.is_empty() {
        return Ok(());
    }
    // A box named by name: that one.
    let options: Vec<(String, Vec<String>)> = boxes.iter().map(|w| (w.id.clone(), vec![w.name.clone()])).collect();
    let chose = (boxes.len() > 1)
        .then(|| said.iter().find_map(|m| pool::named(&m.text, &options).map(|id| (m.seq, id.to_owned()))))
        .flatten();
    if let Some((_, id)) = &chose {
        boxes.retain(|w| w.id == *id);
    }
    let all = pool::all_repos(&boxes);
    let from = chose.as_ref().map(|(s, _)| *s);
    let options = pool::repo_options(&all);
    if let Some(r) =
        said.iter().filter(|m| from.is_none_or(|f| m.seq >= f)).find_map(|m| pool::named(&m.text, &options))
    {
        // Named in this reply, or this reply chose the box after a repo
        // was named: either way it's now.
        let r = r.to_owned();
        return go(app, room, thread, project, author, boxes, Some(&r)).await;
    }
    if from == Some(seq) {
        // This reply chose the box: its only repo, or which of its repos.
        return settled(app, room, thread, project, author, boxes, &[]).await;
    }
    Ok(())
}

/// `GET /api/chat/rooms/{room}/asks/{seq}`: the which-repo ask's form
/// (Arugula #214), for whoever reads its room.
pub async fn ask_show(
    axum::extract::State(app): axum::extract::State<std::sync::Arc<Platform>>,
    Person { account }: Person,
    axum::extract::Path((room, seq)): axum::extract::Path<(String, i64)>,
) -> Result<axum::Json<ChatAsk>, ApiError> {
    let (room, _, _) = super::room_for(&app, &account, &room).await?;
    Ok(axum::Json(repo_ask(&app, &room, seq, &account).await?))
}

/// `POST /api/chat/rooms/{room}/asks/{seq}` [`ChatAskAnswer`]: its asker
/// answers it, and the session starts on the best box with that repo.
pub async fn ask_answer(
    axum::extract::State(app): axum::extract::State<std::sync::Arc<Platform>>,
    Person { account }: Person,
    axum::extract::Path((room, seq)): axum::extract::Path<(String, i64)>,
    axum::Json(b): axum::Json<ChatAskAnswer>,
) -> Result<axum::Json<ChatAsk>, ApiError> {
    let (room, _, _) = super::room_for(&app, &account, &room).await?;
    let a = repo_ask(&app, &room, seq, &account).await?;
    if !a.mine {
        let who = if a.asker_name.is_empty() { "its asker".to_owned() } else { a.asker_name.clone() };
        return Err(err(StatusCode::FORBIDDEN, &format!("this is {who}'s to answer")));
    }
    if !a.open {
        return Err(err(StatusCode::CONFLICT, "it's answered already"));
    }
    let Some(choice) = a.choices.iter().find(|c| **c == b.choice).cloned() else {
        return Err(err(StatusCode::BAD_REQUEST, "that isn't one of its choices"));
    };
    let project = app.db.chat_room_project(&room.id)?.ok_or_else(|| err(StatusCode::NOT_FOUND, "no such ask"))?;
    match may_start(&app, &project, &account).await? {
        None => {}
        Some(why) => return Err(err(StatusCode::FORBIDDEN, &refusal("you", why).replace("you isn't", "you aren't"))),
    }
    let thread = a.parent.map_or(0, |p| p as u64);
    let boxes = pool::candidates(&app, &project)?;
    go(&app, &room, thread, &project, &account, boxes, Some(&choice)).await?;
    Ok(axum::Json(repo_ask(&app, &room, seq, &account).await?))
}

/// The which-repo ask at `seq` in `room`, as `reader` sees it.
async fn repo_ask(app: &Platform, room: &Room, seq: i64, reader: &str) -> Result<ChatAsk, ApiError> {
    let none = || err(StatusCode::NOT_FOUND, "no such ask");
    let m = app.db.chat_message(&room.id, seq)?.ok_or_else(none)?;
    if !m.author.is_empty() || m.via.as_deref() != Some(asks::via(asks::REPO).as_str()) {
        return Err(none());
    }
    let thread = m.parent.map_or(0, |p| p as u64);
    // Its asker: who wrote the `@arugula` it asks about.
    let ms = hydrate::messages(app, room, thread)?;
    let asker = ms
        .iter()
        .rev()
        .find(|x| x.seq < m.seq && x.agent.is_none() && x.via.is_none() && !x.author.is_empty() && asks(&x.text))
        .map(|x| x.author.clone())
        .unwrap_or_default();
    let open = !m.mentions.is_empty();
    let session = match open {
        true => None,
        false => app.db.latest_thread_session(&room.id, thread)?.filter(|s| s.created_at >= m.at).map(|s| s.id),
    };
    let choices = match app.db.chat_room_project(&room.id)? {
        Some(p) => pool::all_repos(&pool::candidates(app, &p)?),
        None => vec![],
    };
    let asker_name = if asker.is_empty() { String::new() } else { super::name_of(app, &asker).await? };
    Ok(ChatAsk {
        room: room.id.clone(),
        seq,
        parent: m.parent,
        kind: asks::REPO.into(),
        mine: !asker.is_empty() && asker == reader,
        asker,
        asker_name,
        question: m.text,
        choices,
        open,
        session,
    })
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
    // The which-repo ask is answered (Arugula #214).
    asks::close(app, room, thread, asks::REPO)?;
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
