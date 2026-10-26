//! CI and review feedback resume the same session (M260, thread =
//! session, Arugula #129): what GitHub says of a session's branch goes
//! back to that session as its next prompt, not to the project's rules.
//!
//! - **Whose** ([`route`], before any rule, `triggers::events::webhook`):
//!   a failed check (`check_failed`) on a head that's a session's branch,
//!   or a review or a review's inline comment on a PR from it. The branch
//!   (`arugula/<8 hex>-<slug>`) is one per project, so the webhook's head
//!   branch and repo find the session, even for a PR someone opened by
//!   hand from it. An event on any other branch goes to the rules as
//!   before.
//! - **From insiders only.** A review or comment by someone GitHub doesn't
//!   call the repo's `OWNER`, `MEMBER` or `COLLABORATOR`, or on a fork's
//!   PR, is ignored ([`Routed::Outsider`]), as an event rule refuses it.
//! - **Gathered, then one prompt** (`db::session_feedback`). A CI round is
//!   every failed check of one head commit, gathered for up to
//!   [`CI_GATHER_MS`] or until its check suite completes with nothing of
//!   it still running ([`suite_done`]). A review is its text and its
//!   inline comments, gathered for [`REVIEW_GATHER_MS`] (GitHub sends each
//!   comment, and the review, as webhooks of their own). The session's job
//!   (`sessions::job`) sends what's due ([`flush`]): one prompt listing
//!   every failed check, with its name, link and the end of its output,
//!   or the review's text and each comment with its file and line. It's
//!   GitHub's text, so it's `Source::Github`, by `github:<login>` (a
//!   check's by `github:GitHub`) with its `author_association`, and the
//!   agent gets it fenced as quoted data (`SessionPrompt::for_agent`).
//! - **By the session's state.** `done`: the prompt starts a new turn, as
//!   a thread reply does. `queued`, `working` or `needs_input`: it's
//!   queued for the next. `stopped` or `failed`: it's dropped, and the
//!   thread says so once, with the mention to copy ([`ended_line`]).
//! - **The cap:** at most [`MAX_CI_ROUNDS`] CI rounds a session. A round
//!   past that isn't sent: the thread says so, and the card says the
//!   session needs a person ([`needs_person`]) until someone in the
//!   thread prompts it. Reviews still go through.
//! - **The thread** gets one system line for each batch sent ("CI failed:
//!   2 checks; sent to the session (round 1 of 2)", "Review from Ana: 3
//!   comments; sent to the session"), naming the reviewer as the project
//!   knows them when their GitHub account is a member's, else by login.
//!
//! **Untrusted text.** A check's name and output, and a review's text, are
//! whoever caused them's. They're kept only to send, fenced, and lines in
//! the thread quote at most a check's name, cut short.

use anyhow::Result;
use serde_json::Value;
use tracing::{info, warn};

use crate::{
    Platform,
    db::{
        session_feedback::{
            Feedback, FeedbackKind, FeedbackState, Gathered, Item, MAX_CI_ROUNDS, NewFeedback, Prompt, Settled,
        },
        sessions::{MAX_PROMPT, Session, Source},
        triggers::RuleEvent,
    },
    forge::ingest::ms,
    triggers::events::Seen,
};

/// How long a CI round gathers its head commit's failed checks.
pub const CI_GATHER_MS: u64 = 2 * 60_000;

/// How long a review gathers its inline comments.
pub const REVIEW_GATHER_MS: u64 = 30_000;

/// The most of a check's output a round keeps: its end.
pub const TAIL: usize = 3000;

/// The most checks a round's prompt lists in full.
const MOST_CHECKS: usize = 10;

/// Who a check's prompt is by.
pub const CHECKS_AUTHOR: &str = "github:GitHub";

/// What came of an event on a session's branch.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Routed {
    /// Not a session's: the rules'.
    Elsewhere,
    /// A session's, from outside the repo: nothing comes of it.
    Outsider { session: String },
    /// Gathered into a batch; `new: false` when it was there already (a
    /// redelivery).
    Gathered { session: String, new: bool },
    /// Its batch was sent already: kept out.
    Late { session: String },
    /// Its session ended: dropped, and the thread told (once).
    Ended { session: String },
}

/// The batch an event's item goes in.
struct Batch {
    kind: FeedbackKind,
    key: String,
    number: Option<u64>,
    login: Option<String>,
    github_id: Option<u64>,
    association: Option<String>,
    review_state: Option<String>,
    gather: u64,
}

fn text(v: &Value) -> Option<String> {
    v.as_str().filter(|s| !s.trim().is_empty()).map(str::to_owned)
}

/// The end of `s`, at most `max` bytes, on a character's boundary.
fn tail(s: &str, max: usize) -> String {
    if s.len() <= max {
        return s.to_owned();
    }
    let mut at = s.len() - max;
    while !s.is_char_boundary(at) {
        at += 1;
    }
    format!("…{}", &s[at..])
}

/// What a check's output says: its title, summary and text, the end of
/// them.
fn output(o: &Value) -> String {
    let parts: Vec<String> = ["title", "summary", "text"].iter().filter_map(|k| text(&o[*k])).collect();
    tail(&parts.join("\n\n"), TAIL)
}

/// The batch and item of a session's event (`seen`, from webhook `v`).
fn read(seen: &Seen, v: &Value, now: u64) -> Option<(Batch, Item)> {
    let ci = |sha: Option<String>, item: Item| {
        let b = Batch {
            kind: FeedbackKind::Ci,
            key: sha?,
            number: None,
            login: None,
            github_id: None,
            association: None,
            review_state: None,
            gather: CI_GATHER_MS,
        };
        Some((b, item))
    };
    if let Some(run) = v.get("check_run").filter(|_| seen.event == RuleEvent::CheckFailed) {
        let item = Item {
            item: format!("check_run:{}", run["id"].as_u64()?),
            name: text(&run["name"]).unwrap_or_default(),
            line: None,
            conclusion: text(&run["conclusion"]),
            url: text(&run["html_url"]).or(text(&run["details_url"])),
            text: output(&run["output"]),
            at: ms(&run["completed_at"]).unwrap_or(now),
        };
        return ci(text(&run["head_sha"]), item);
    }
    if seen.event == RuleEvent::CheckFailed {
        // A commit status.
        let item = Item {
            item: format!("status:{}", v["id"].as_u64()?),
            name: text(&v["context"]).unwrap_or_default(),
            line: None,
            conclusion: text(&v["state"]),
            url: text(&v["target_url"]),
            text: tail(&text(&v["description"]).unwrap_or_default(), TAIL),
            at: ms(&v["updated_at"]).unwrap_or(now),
        };
        return ci(text(&v["sha"]), item);
    }
    if seen.event != RuleEvent::Review {
        return None;
    }
    let number = v["pull_request"]["number"].as_u64();
    if let Some(review) = v.get("review") {
        let id = review["id"].as_u64()?;
        let b = Batch {
            kind: FeedbackKind::Review,
            key: id.to_string(),
            number,
            login: text(&review["user"]["login"]),
            github_id: review["user"]["id"].as_u64(),
            association: text(&review["author_association"]),
            review_state: text(&review["state"]).map(|s| s.to_ascii_lowercase()),
            gather: REVIEW_GATHER_MS,
        };
        let item = Item {
            item: format!("review:{id}"),
            name: String::new(),
            line: None,
            conclusion: None,
            url: text(&review["html_url"]),
            text: text(&review["body"]).unwrap_or_default(),
            at: ms(&review["submitted_at"]).unwrap_or(now),
        };
        return Some((b, item));
    }
    let comment = &v["comment"];
    let id = comment["id"].as_u64()?;
    let b = Batch {
        kind: FeedbackKind::Review,
        // GitHub makes a review of every comment; one that doesn't say is
        // its own.
        key: comment["pull_request_review_id"].as_u64().map_or(format!("comment:{id}"), |r| r.to_string()),
        number,
        login: text(&comment["user"]["login"]),
        github_id: comment["user"]["id"].as_u64(),
        association: text(&comment["author_association"]),
        review_state: None,
        gather: REVIEW_GATHER_MS,
    };
    let item = Item {
        item: format!("comment:{id}"),
        name: text(&comment["path"]).unwrap_or_default(),
        line: comment["line"].as_u64().or(comment["original_line"].as_u64()),
        conclusion: None,
        url: text(&comment["html_url"]),
        text: text(&comment["body"]).unwrap_or_default(),
        at: ms(&comment["created_at"]).unwrap_or(now),
    };
    Some((b, item))
}

/// A check's name as a line in a thread quotes it: one line, at most 80
/// characters, in backticks.
fn quoted(name: &str) -> String {
    let one: String = name.chars().filter(|c| !c.is_control() && *c != '`').take(80).collect();
    if one.trim().is_empty() { "a check".into() } else { format!("`{}`", one.trim()) }
}

/// What the thread is told when feedback comes for a session that ended.
pub fn ended_line(kind: FeedbackKind, what: &str) -> String {
    match kind {
        FeedbackKind::Ci => {
            format!("CI failed on {}, but the session has ended; @arugula again to start one", quoted(what))
        }
        FeedbackKind::Review => {
            format!("Review from {}, but the session has ended; @arugula again to start one", quoted(what))
        }
    }
}

/// Post the platform's line in session `s`'s thread.
fn say(app: &Platform, s: &Session, line: &str) -> Result<()> {
    let Some(room) = app.db.chat_room(&s.room)? else { return Ok(()) };
    crate::chat::sessions::say(app, &room, s.parent_seq, line)?;
    Ok(())
}

/// The session whose branch is one of `seen`'s heads, if any.
fn session_of(app: &Platform, seen: &Seen) -> Result<Option<Session>> {
    for head in &seen.heads {
        if let Some(s) = app.db.session_on_branch(seen.repo_id, &seen.repo, head)? {
            return Ok(Some(s));
        }
    }
    Ok(None)
}

/// An event a webhook brought (`seen`, from `v`): back to its session, if
/// it's on a session's branch (module docs). [`Routed::Elsewhere`] leaves
/// it to the rules.
pub fn route(app: &Platform, seen: &Seen, v: &Value, now: u64) -> Result<Routed> {
    if !matches!(seen.event, RuleEvent::CheckFailed | RuleEvent::Review) {
        return Ok(Routed::Elsewhere);
    }
    let Some(s) = session_of(app, seen)? else { return Ok(Routed::Elsewhere) };
    let session = s.id.clone();
    if seen.outside {
        info!(session, key = seen.key, "feedback from outside the repo on a session's branch: ignored");
        return Ok(Routed::Outsider { session });
    }
    let Some((b, item)) = read(seen, v, now) else { return Ok(Routed::Elsewhere) };
    let new = NewFeedback {
        session: &s.id,
        kind: b.kind,
        batch: &b.key,
        number: b.number,
        login: b.login.as_deref(),
        github_id: b.github_id,
        association: b.association.as_deref(),
        review_state: b.review_state.as_deref(),
        due_at: now + b.gather,
    };
    if s.state.terminal() {
        if app.db.drop_feedback(&new, now)? {
            let what = match b.kind {
                FeedbackKind::Ci => item.name.clone(),
                FeedbackKind::Review => b.login.clone().unwrap_or_default(),
            };
            say(app, &s, &ended_line(b.kind, &what))?;
        }
        return Ok(Routed::Ended { session });
    }
    Ok(match app.db.gather_feedback(&new, &item, now)? {
        Gathered::Added(_) => {
            app.db.nudge_session(&s.id, now)?;
            Routed::Gathered { session, new: true }
        }
        Gathered::Already(_) => Routed::Gathered { session, new: false },
        Gathered::Late(fb) => {
            info!(session, feedback = fb.id, item = item.item, "feedback for a batch that was settled: kept out");
            Routed::Late { session }
        }
    })
}

/// A `check_suite` that completed: the CI round of its head commit on a
/// session's branch is due now, unless the mirror has a check run of it
/// still going.
pub fn suite_done(app: &Platform, event: &str, v: &Value, now: u64) -> Result<()> {
    if event != "check_suite" || v["action"] != "completed" {
        return Ok(());
    }
    let (Some(repo_id), Some(repo)) = (v["repository"]["id"].as_u64(), v["repository"]["full_name"].as_str()) else {
        return Ok(());
    };
    let suite = &v["check_suite"];
    if suite["head_repository"]["id"].as_u64() != Some(repo_id) {
        return Ok(());
    }
    let (Some(branch), Some(sha)) = (text(&suite["head_branch"]), text(&suite["head_sha"])) else { return Ok(()) };
    let Some(s) = app.db.session_on_branch(repo_id, repo, &branch)? else { return Ok(()) };
    if app.db.checks_running(repo_id, &sha)? {
        return Ok(());
    }
    if app.db.feedback_due(&s.id, FeedbackKind::Ci, &sha, now)? {
        app.db.nudge_session(&s.id, now)?;
    }
    Ok(())
}

fn plural(n: usize, one: &str) -> String {
    format!("{n} {one}{}", if n == 1 { "" } else { "s" })
}

/// A CI round's prompt: every failed check of the commit.
fn ci_prompt(fb: &Feedback, items: &[Item], s: &Session, round: u32) -> Prompt {
    let short: String = fb.batch.chars().take(12).collect();
    let mut out = format!(
        "CI failed on {short}, the head of this session's branch {}: {} failed. This is CI round {round} of \
         {MAX_CI_ROUNDS}; after that a person takes over.\n",
        s.branch,
        plural(items.len(), "check")
    );
    let each = (MAX_PROMPT - 2048) / items.len().clamp(1, MOST_CHECKS);
    for i in items.iter().take(MOST_CHECKS) {
        let mut one = format!("\n## {} ({})\n", i.name, i.conclusion.as_deref().unwrap_or("failed"));
        if let Some(u) = &i.url {
            one.push_str(u);
            one.push('\n');
        }
        if !i.text.is_empty() {
            one.push('\n');
            one.push_str(&i.text);
            one.push('\n');
        }
        out.push_str(&tail(&one, each));
    }
    if items.len() > MOST_CHECKS {
        let rest: Vec<&str> = items[MOST_CHECKS..].iter().map(|i| i.name.as_str()).collect();
        out.push_str(&tail(&format!("\nAnd {} more: {}\n", rest.len(), rest.join(", ")), 1024));
    }
    Prompt { author: CHECKS_AUTHOR.into(), association: "NONE".into(), text: out }
}

/// The comments of a review batch (its own text left out).
fn comments(items: &[Item]) -> impl Iterator<Item = &Item> {
    items.iter().filter(|i| i.item.starts_with("comment:") && !i.text.trim().is_empty())
}

/// A review's prompt: its text, and each inline comment with its file and
/// line. `None` when it says nothing, or only approves.
fn review_prompt(fb: &Feedback, items: &[Item]) -> Option<Prompt> {
    let body = items.iter().find(|i| i.item.starts_with("review:")).map(|i| i.text.trim()).unwrap_or_default();
    let n = comments(items).count();
    let state = fb.review_state.as_deref().unwrap_or("commented");
    let asks = state == "changes_requested";
    if n == 0 && (body.is_empty() || state == "approved") && !asks {
        return None;
    }
    let login = fb.login.clone().unwrap_or_else(|| "someone".into());
    let on = fb.number.map(|n| format!(" on PR #{n}")).unwrap_or_default();
    let said = match state {
        "changes_requested" => "requested changes",
        "approved" => "approved",
        _ => "commented",
    };
    let mut out = format!("A review{on} by {login}: {said}.\n");
    if !body.is_empty() {
        out.push('\n');
        out.push_str(body);
        out.push('\n');
    }
    if n > 0 {
        out.push_str(&format!("\n{}:\n", plural(n, "comment")));
        for c in comments(items) {
            let at = match (c.name.as_str(), c.line) {
                ("", _) => "(no file)".to_owned(),
                (f, Some(l)) => format!("{f}, line {l}"),
                (f, None) => f.to_owned(),
            };
            out.push_str(&format!("\n- {at}:\n"));
            for l in c.text.trim().lines() {
                out.push_str("  ");
                out.push_str(l);
                out.push('\n');
            }
        }
    }
    Some(Prompt {
        author: format!("github:{login}"),
        association: fb.association.clone().unwrap_or_else(|| "NONE".into()),
        text: tail(&out, MAX_PROMPT - 1024),
    })
}

/// How the thread names a reviewer: their name in the project when their
/// GitHub account signs in a member of it, else their login.
async fn reviewer(app: &Platform, s: &Session, fb: &Feedback) -> String {
    let login = fb.login.clone().unwrap_or_else(|| "someone".into());
    let Some(id) = fb.github_id else { return login };
    let account = match app.hub.github_account(id).await {
        Ok(Some(a)) => a,
        _ => return login,
    };
    if crate::forge::links::membership(app, &account, &s.project, false).await.is_err() {
        return login;
    }
    match app.cache.accounts(&*app.hub, std::slice::from_ref(&account)).await {
        Ok(names) => names.get(&account).map(|a| a.name.clone()).filter(|n| !n.is_empty()).unwrap_or(login),
        Err(_) => login,
    }
}

/// Send session `s`'s feedback that's due by `now` (module docs), and say
/// so in its thread: when its next batch is due, if it has one gathering.
/// Its job calls it each step, before it looks at the session's prompts;
/// once the session ended, everything it gathered is settled (dropped).
pub async fn flush(app: &Platform, s: &Session, now: u64) -> Result<Option<u64>> {
    // An ended session's job looks no more: all it gathered is settled now.
    let by = if s.state.terminal() { i64::MAX as u64 } else { now };
    for id in app.db.due_feedback(&s.id, by)? {
        let settled = app.db.settle_feedback(&id, by, now, |fb, items, s| match fb.kind {
            FeedbackKind::Ci => Some(ci_prompt(fb, items, s, s.ci_rounds + 1)),
            FeedbackKind::Review => review_prompt(fb, items),
        })?;
        let Some((fb, items, how)) = settled else { continue };
        let checks = plural(items.len(), "check");
        let line = match (&how, fb.kind) {
            (Settled::Sent { round, .. }, FeedbackKind::Ci) => Some(format!(
                "CI failed: {checks}; sent to the session (round {} of {MAX_CI_ROUNDS})",
                round.unwrap_or_default()
            )),
            (Settled::Sent { .. }, FeedbackKind::Review) => {
                let n = comments(&items).count();
                let what = if n == 0 { "its text".to_owned() } else { plural(n, "comment") };
                Some(format!("Review from {}: {what}; sent to the session", reviewer(app, s, &fb).await))
            }
            (Settled::Refused, _) => Some(format!(
                "CI failed again: {checks}; not sent. The session had its {MAX_CI_ROUNDS} CI rounds, and needs a person: \
                 reply here to take it on."
            )),
            (Settled::Ended, FeedbackKind::Ci) => {
                Some(ended_line(fb.kind, items.first().map(|i| i.name.as_str()).unwrap_or_default()))
            }
            (Settled::Ended, FeedbackKind::Review) => {
                Some(ended_line(fb.kind, fb.login.as_deref().unwrap_or_default()))
            }
            (Settled::Nothing, _) => None,
        };
        info!(session = s.id, feedback = fb.id, kind = fb.kind.as_str(), settled = ?how, "a session's feedback was settled");
        if let Some(line) = line
            && let Err(e) = say(app, s, &line)
        {
            warn!(session = s.id, feedback = fb.id, error = %format!("{e:#}"), "saying a session's feedback in its thread");
        }
        if let Some(after) = app.db.session(&s.id)? {
            app.chat.session(&app.db, &after);
        }
    }
    app.db.next_feedback_due(&s.id)
}

/// Whether session `s` needs a person: CI failed again after its last
/// round ([`MAX_CI_ROUNDS`]), and nobody has prompted it in its thread
/// since.
pub fn needs_person(app: &Platform, s: &Session) -> Result<bool> {
    let fb = app.db.session_feedback(&s.id)?;
    let Some(last) = fb.iter().rev().find(|f| f.kind == FeedbackKind::Ci && f.state != FeedbackState::Gathering) else {
        return Ok(false);
    };
    if last.state != FeedbackState::Refused {
        return Ok(false);
    }
    let since = last.done_at.unwrap_or(last.created_at);
    let prompted = app.db.session_prompts(&s.id)?.iter().any(|p| p.source == Source::Member && p.created_at >= since);
    Ok(!prompted)
}
