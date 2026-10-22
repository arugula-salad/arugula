//! The inbox (M171, #1217): the PRs and issues on linked repos that want
//! something of a member, worked out by the platform from the mirror's
//! rows and pushed through the hub (aru-081), with every machine asleep.
//!
//! **The block's rules.** An item's reasons are what a forge block's
//! attention finds (`model::attention` and `model::issue_attention`, run
//! on the item as the mirror has it, against the member's GitHub login),
//! in its words, plus a draft waiting for them to send (M170):
//! `review_requested`, `checks_failed`, `changes_requested`, `mentioned`,
//! `assigned`, `done` (merged, or approved and green; an assigned issue
//! closed) and `draft`. Mentions newer than "you last looked" and a done
//! you've seen are the block's view of attention; here every reason that
//! holds is an entry, and seeing it or calling it done is the entry's.
//! What the mirror can't say:
//! - **A team's review request.** GitHub tells a person their teams only
//!   with their own token, which the platform never holds: only a request
//!   to you by login counts.
//! - **Mentions** need the text: only where the project mirrors content
//!   (M166). Elsewhere there are none.
//! - **Who assigned an issue.** The block's `assigned` is given to you by
//!   someone else; the mirror keeps the assignees, not who added them. An
//!   issue you opened counts as one you took yourself, and any other one
//!   given to you as someone else's doing.
//! - **Whether a PR can merge:** only GitHub's `mergeable_state` (`dirty`
//!   is a conflict, `blocked` held by protection), not its `mergeable`,
//!   and only once GitHub has worked it out: lists never carry it,
//!   webhooks often say `unknown`, and a base branch that moves changes it
//!   with no event at all. So a green PR is done ("checks green") only
//!   with a known state that's neither ([`can_merge`]); none, or
//!   `unknown`, holds it back like `blocked`. The poller reads each open
//!   PR whose state it doesn't know, and rechecks green ones, stalest
//!   first ([`super::poller`]). Merged is done either way.
//!
//! **Who.** A project's members whom the viewer check ([`viewer::readers`])
//! lets read the repo, by the GitHub login they signed in with (the hub's,
//! `HubPort::github_user`). A member without one gets only their drafts.
//! Anyone else's rows for the item go: a private item is never in the inbox
//! of someone GitHub doesn't let read it, and reads check again
//! ([`Db::inbox`](crate::db::Db::inbox) takes the check's `Readable`).
//!
//! **When.** Ingest marks what it wrote ([`written`]): a webhook's items,
//! and the items with a head whose checks changed. Each poller turn marks
//! the whole repo ([`polled`]), which also catches a member who joined or
//! lost access; a backfill's are worked out quietly. A draft made or
//! discarded marks its item. Each machine works out what it marked, every
//! little while ([`start`]).
//!
//! **Pushes.** A reason newly holding makes a row; only the machine whose
//! insert made it pushes ([`Db::set_inbox`](crate::db::Db::set_inbox)), so
//! one item and reason pushes once while it holds. One push per item per
//! person a minute ([`PUSH_GAP_MS`]), at most [`INBOX_PUSHES`] an hour, to
//! the person's approved devices, and only for what changed in the last
//! hour ([`FRESH_MS`]): an old failure found by a sweep, or a member who
//! just joined, lands in the inbox without a push. Control has no quiet
//! hours or push preferences yet; when it does, they apply here.
//!
//! **Mentions and assignments on the platform's own objects** (M202) are
//! entries here too, made when they happen: [`crate::inbox`].
//!
//! **Shadow mode** ([`shadow`]): machines opted in report their forge
//! blocks' attention (items and reasons, no titles), and the platform logs
//! how often its inbox agrees (docs/control-ops.md).

use std::{
    collections::{BTreeSet, HashMap},
    sync::{Arc, Mutex},
    time::Duration,
};

use arugula_control_common::{ApiError, err};
use arugula_control_wire::projects as wire;
use arugula_e2e::now_ms;
use arugula_forge_github::{
    model::{
        self, Check, CheckState, Event, EventKind, Item, ItemKind, ItemState, Me, Pr, Review, ReviewState, Reviewer,
    },
    parse,
};
use axum::{
    Json,
    extract::{Path, Query, State},
    http::StatusCode,
};
use serde::Deserialize;
use serde_json::json;
use tracing::{info, warn};

use super::{
    drafts::Drafter,
    items::names,
    viewer::{self, Viewer},
};
use crate::{
    Platform,
    auth::{Machine, Person},
    db::{
        forge::ForgeCheck,
        inbox::{InboxFilter, Sources, THREAD_MENTION},
    },
    hub::PushOptions,
};

/// Pushes per person per hour.
pub const INBOX_PUSHES: (&str, usize) = ("inbox-push", 60);
/// At most one push per item per person in this long.
pub const PUSH_GAP_MS: u64 = 60_000;
/// Only what changed this recently pushes.
pub const FRESH_MS: u64 = 3600 * 1000;
/// Shadow reports per account per hour.
const SHADOW_REPORTS: (&str, usize) = ("inbox-shadow", 60);
/// The most blocks one report compares.
const SHADOW_BLOCKS: usize = 200;
/// How often each machine works out what it marked, and how long after a
/// mark it waits for more.
const EVERY: Duration = Duration::from_secs(30);
const SETTLE: Duration = Duration::from_millis(500);
/// How many entries a list holds at most, and when it doesn't say.
const MOST: u32 = 500;
const DEFAULT: u32 = 200;

/// What a repo's ingest marked: its whole inbox, or some items, by number
/// or by a head whose checks changed. `quiet`: a backfill's, which pushes
/// nothing.
#[derive(Debug, Default)]
struct Dirty {
    all: bool,
    quiet: bool,
    numbers: BTreeSet<u64>,
    shas: BTreeSet<String>,
}

/// What this machine marked and hasn't worked out yet, and whom it pushed
/// lately, by person and item.
#[derive(Default)]
pub struct Queue {
    dirty: Mutex<HashMap<u64, Dirty>>,
    wake: Arc<tokio::sync::Notify>,
    pushed: Mutex<HashMap<(String, String), u64>>,
}

impl Queue {
    fn mark(&self, repo: u64, f: impl FnOnce(&mut Dirty)) {
        f(self.dirty.lock().unwrap().entry(repo).or_default());
        self.wake.notify_one();
    }

    /// Forget whom it pushed (tests: a minute on).
    #[cfg(test)]
    pub(crate) fn forget_pushes(&self) {
        self.pushed.lock().unwrap().clear();
    }

    /// Whether `account` may be pushed for `item` now (and note it).
    pub(crate) fn may_push(&self, account: &str, item: &str, now: u64) -> bool {
        let mut p = self.pushed.lock().unwrap();
        if p.len() > 50_000 {
            p.retain(|_, at| now.saturating_sub(*at) < PUSH_GAP_MS);
        }
        let key = (account.to_owned(), item.to_owned());
        if p.get(&key).is_some_and(|at| now.saturating_sub(*at) < PUSH_GAP_MS) {
            return false;
        }
        p.insert(key, now);
        true
    }
}

/// A webhook's rows were written ([`super::ingest`]): their items, and
/// those whose head is a check's.
pub fn written(p: &Platform, repo: u64, rows: &super::ingest::Rows) {
    p.inbox.mark(repo, |d| {
        d.numbers.extend(rows.items.iter().map(|i| i.number));
        d.numbers.extend(rows.events.iter().map(|e| e.number));
        d.shas.extend(rows.checks.iter().map(|c| c.head_sha.clone()));
    });
}

/// A poller turn finished: the whole repo, quietly after its backfill.
pub fn polled(p: &Platform, repo: u64, backfill: bool) {
    p.inbox.mark(repo, |d| {
        d.all = true;
        d.quiet |= backfill;
    });
}

/// A draft on an item was made or discarded.
pub fn drafted(p: &Platform, repo: u64, number: Option<u64>) {
    if let Some(n) = number {
        p.inbox.mark(repo, |d| {
            d.numbers.insert(n);
        });
    }
}

/// One reason an item is in a member's inbox, and why, in the block's
/// words.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Want {
    pub reason: &'static str,
    pub why: String,
}

/// `OWNER/NAME#N` on github.com.
pub fn url(full_name: &str, kind: &str, number: u64) -> String {
    let path = if kind == "issue" { "issues" } else { "pull" };
    format!("https://github.com/{full_name}/{path}/{number}")
}

/// Whether the mirror knows a PR can merge, as far as "checks green"
/// goes: GitHub worked its `mergeable_state` out, and it's neither a
/// conflict (`dirty`) nor held by protection (`blocked`). None, or
/// `unknown` (GitHub works it out lazily, and lists never carry it), isn't
/// evidence that it can: the model hears it as `blocked`, and a green PR
/// isn't done until the poller learns more ([`super::poller`]).
pub(crate) fn can_merge(merge_state: Option<&str>) -> bool {
    !matches!(merge_state, None | Some("unknown" | "dirty" | "blocked"))
}

/// The mirror's rows as the block's model has them: no body unless
/// content is mirrored, checks from their rows, reviews without text.
fn model_item(s: &Sources) -> Item {
    let i = &s.item;
    let state = match i.state.as_str() {
        "merged" => ItemState::Merged,
        "closed" => ItemState::Closed,
        _ => ItemState::Open,
    };
    let mut item = Item {
        kind: if i.kind == "issue" { ItemKind::Issue } else { ItemKind::Pr },
        number: i.number,
        url: url(&s.full_name, &i.kind, i.number),
        title: i.title.clone(),
        body: s.content.iter().find(|c| c.kind == "body").map(|c| c.body.clone()).unwrap_or_default(),
        author: i.author_login.clone().unwrap_or_default(),
        state,
        draft: i.draft,
        labels: names(&i.labels),
        assignees: names(&i.assignees),
        mergeable: (i.merge_state.as_deref() == Some("dirty")).then_some(false),
        blocked: !can_merge(i.merge_state.as_deref()),
        // A team is `org/slug`, which no login can be.
        requested: names(&i.reviewers)
            .into_iter()
            .map(|r| if r.contains('/') { Reviewer::Team(r) } else { Reviewer::User(r) })
            .collect(),
        updated_at: i.updated_at as i64,
        ..Default::default()
    };
    item.head.sha = i.head_sha.clone().unwrap_or_default();
    item.head.branch = i.head_ref.clone().unwrap_or_default();
    item.base.branch = i.base_ref.clone().unwrap_or_default();
    item
}

/// Comments, as events with their text (only where content is mirrored).
fn comments(s: &Sources) -> Vec<Event> {
    s.content
        .iter()
        .filter_map(|c| {
            let kind = match c.kind.as_str() {
                "comment" => EventKind::Commented,
                "review_comment" => EventKind::ReviewComment,
                _ => return None,
            };
            Some(Event {
                id: format!("gh-{}", c.github_id),
                at: c.updated_at as i64,
                actor: c.author_login.clone(),
                kind,
                what: None,
                target: None,
                body: Some(c.body.clone()),
                commits: None,
                force: false,
            })
        })
        .collect()
}

/// A head's check rows as the block's model has them.
fn model_checks(rows: &[ForgeCheck]) -> Vec<Check> {
    let runs: Vec<_> = rows
        .iter()
        .filter(|c| c.kind == "check_run")
        .map(|c| json!({ "name": c.name, "status": c.status, "conclusion": c.conclusion, "html_url": c.url }))
        .collect();
    let statuses: Vec<_> = rows
        .iter()
        .filter(|c| c.kind == "status")
        .map(|c| {
            let state = if c.status == "pending" { "pending" } else { c.conclusion.as_deref().unwrap_or("") };
            json!({ "context": c.name, "state": state, "target_url": c.url })
        })
        .collect();
    parse::checks(&json!({ "check_runs": runs }), &json!({ "statuses": statuses }))
}

/// Whether a head's checks roll up green, as the block counts them.
pub(crate) fn green(rows: &[ForgeCheck]) -> bool {
    model::rollup(&model_checks(rows)) == Some(CheckState::Success)
}

/// A PR as the block's model has it.
fn model_pr(s: &Sources) -> Pr {
    let checks = model_checks(&s.checks);
    let reviews = s
        .reviews
        .iter()
        .map(|e| Review {
            id: e.id.clone(),
            author: e.actor_login.clone(),
            state: match e.review_state.as_deref() {
                Some("approved") => ReviewState::Approved,
                Some("changes_requested") => ReviewState::ChangesRequested,
                Some("dismissed") => ReviewState::Dismissed,
                Some("pending") => ReviewState::Pending,
                _ => ReviewState::Commented,
            },
            commit: None,
            stale: false,
            at: Some(e.at as i64),
            body: None,
            url: None,
        })
        .collect();
    Pr { item: model_item(s), reviews, rollup: model::rollup(&checks), checks, events: comments(s) }
}

/// What an item wants of a viewer, most pressing first: the block's
/// attention against their GitHub login, and their drafts on it.
pub fn wants(s: &Sources, v: &Viewer) -> Vec<Want> {
    let mut out = Vec::new();
    if let Some(u) = &v.github {
        let me = Me { login: u.login.clone(), teams: vec![] };
        let found = if s.item.kind == "issue" {
            let issue = model::Issue { item: model_item(s), events: comments(s), linked: vec![] };
            let mut found = model::issue_attention(&issue, &me, 0);
            // The block's "assigned" is someone else's doing. The mirror
            // doesn't keep who gave it to whom: one you opened, you took.
            if issue.item.author.eq_ignore_ascii_case(&me.login) {
                found.retain(|w| !matches!(w, model::Want::Assigned { .. }));
            }
            found
        } else {
            model::attention(&model_pr(s), &me, 0)
        };
        out.extend(found.into_iter().map(|w| {
            let reason = match &w {
                model::Want::Review { .. } => "review_requested",
                model::Want::Failed { .. } => "checks_failed",
                model::Want::Changes { .. } => "changes_requested",
                model::Want::Mention { .. } => "mentioned",
                model::Want::Assigned { .. } => "assigned",
                model::Want::Done { .. } => "done",
            };
            Want { reason, why: w.why().to_owned() }
        }));
    }
    let drafts = s.drafts.iter().filter(|(author, _)| *author == v.account).count();
    if drafts > 0 {
        let why =
            if drafts == 1 { "a draft waiting for you".into() } else { format!("{drafts} drafts waiting for you") };
        out.push(Want { reason: "draft", why });
    }
    out
}

/// When the item last changed, by its own rows: what a push must be newer
/// than [`FRESH_MS`].
fn changed_at(s: &Sources) -> u64 {
    let checks = s.checks.iter().map(|c| c.updated_at);
    let reviews = s.reviews.iter().map(|e| e.at);
    let content = s.content.iter().map(|c| c.updated_at);
    checks.chain(reviews).chain(content).fold(s.item.updated_at, u64::max)
}

/// A project's members: its account, or its team's (by the latest
/// roster).
pub(crate) async fn members(p: &Platform, project: &str) -> anyhow::Result<Vec<String>> {
    let Some((owner, team)) = p.db.project_holder(project)? else { return Ok(vec![]) };
    if let Some(o) = owner {
        return Ok(vec![o]);
    }
    let Some(team) = team else { return Ok(vec![]) };
    let Some(t) = p.cache.team(&*p.hub, &team).await? else { return Ok(vec![]) };
    Ok(t.members.iter().filter(|m| t.role(&m.account).is_some()).map(|m| m.account.clone()).collect())
}

fn api(e: ApiError) -> anyhow::Error {
    anyhow::anyhow!("{} {}", e.0, e.1)
}

/// Work out these items' inboxes on a linked repo, and push what's new
/// (unless `quiet`): how many entries it made.
pub async fn recompute(p: &Platform, repo_id: u64, numbers: &[u64], quiet: bool) -> anyhow::Result<usize> {
    if numbers.is_empty() {
        return Ok(0);
    }
    let Some(repo) = p.db.linked_repo(repo_id)?.filter(|r| r.unlinked_at.is_none()) else { return Ok(0) };
    let members = members(p, &repo.project_id).await?;
    let readers = viewer::readers(p, &repo, &members).await.map_err(api)?;
    let mut made = 0;
    for n in numbers {
        let Some(s) = p.db.inbox_sources(repo_id, *n)? else { continue };
        let mut want = Vec::new();
        let mut why: HashMap<(String, &str), String> = HashMap::new();
        for v in &readers {
            for w in wants(&s, v) {
                want.push((v.account.clone(), w.reason.to_owned()));
                why.insert((v.account.clone(), w.reason), w.why);
            }
        }
        let now = now_ms();
        let new = p.db.set_inbox(&s.item.id, &want, now)?;
        made += new.len();
        if quiet || new.is_empty() {
            continue;
        }
        // Who gets what: the reasons new for each, in the block's order.
        let mut to: Vec<(String, Vec<String>)> = Vec::new();
        for (account, _) in &want {
            if to.iter().any(|(a, _)| a == account) {
                continue;
            }
            let whys: Vec<String> = want
                .iter()
                .filter(|(a, r)| a == account && new.contains(&(a.clone(), r.clone())))
                .filter(|(_, r)| r == "draft" || changed_at(&s) + FRESH_MS >= now)
                .filter_map(|(a, r)| why.get(&(a.clone(), r.as_str())).cloned())
                .collect();
            if !whys.is_empty() {
                to.push((account.clone(), whys));
            }
        }
        push(p, &s, to, now).await;
    }
    Ok(made)
}

/// Push an item's new reasons to each person: the block's headline, with
/// the title, which only readers' rows ever carry.
async fn push(p: &Platform, s: &Sources, to: Vec<(String, Vec<String>)>, now: u64) {
    let i = &s.item;
    for (account, whys) in to {
        if !p.inbox.may_push(&account, &i.id, now) || p.limits.check_account(INBOX_PUSHES, &account).is_err() {
            continue;
        }
        let msg = json!({
            "inbox": { "item": i.id },
            "title": format!("{}#{} {}", s.full_name, i.number, whys.join("; ")),
            "body": i.title,
            "tag": format!("inbox-{}", i.id),
            "url": url(&s.full_name, &i.kind, i.number),
            "control": true,
        });
        // The item and whom; never its title.
        info!(item = %i.id, %account, "inbox push");
        if let Err(e) = p.hub.push_with(&account, &msg, PushOptions::default()).await {
            warn!(error = %e, %account, "the inbox's push didn't go");
        }
    }
}

/// Work out everything this machine marked: how many entries it made.
pub async fn drain(p: &Platform) -> usize {
    let dirty = std::mem::take(&mut *p.inbox.dirty.lock().unwrap());
    let mut made = 0;
    for (repo, d) in dirty {
        let mut loud = d.numbers;
        let shas: Vec<String> = d.shas.into_iter().collect();
        match p.db.head_numbers(repo, &shas) {
            Ok(n) => loud.extend(n),
            Err(e) => warn!(repo, error = format!("{e:#}"), "inbox: finding checks' items"),
        }
        let loud: Vec<u64> = loud.into_iter().collect();
        let run = |numbers: Vec<u64>, quiet: bool| async move {
            match recompute(p, repo, &numbers, quiet).await {
                Ok(n) => n,
                Err(e) => {
                    warn!(repo, error = format!("{e:#}"), "inbox: working it out");
                    0
                }
            }
        };
        made += run(loud.clone(), false).await;
        if d.all {
            match p.db.inbox_numbers(repo) {
                Ok(all) => made += run(all.into_iter().filter(|n| !loud.contains(n)).collect(), d.quiet).await,
                Err(e) => warn!(repo, error = format!("{e:#}"), "inbox: a repo's items"),
            }
        }
    }
    made
}

/// The inbox's worker on this machine: what it marked, a moment after it
/// was marked, and every [`EVERY`] anyway.
pub fn start(p: &Arc<Platform>) {
    let app = Arc::downgrade(p);
    tokio::spawn(async move {
        loop {
            let Some(p) = app.upgrade() else { return };
            let wake = p.inbox.wake.clone();
            if !p.is_paused() && !p.is_stopping() {
                drain(&p).await;
            }
            drop(p);
            let _ = tokio::time::timeout(EVERY, wake.notified()).await;
            tokio::time::sleep(SETTLE).await;
        }
    });
}

/// `GET /api/inbox`'s query (`wire::INBOX`).
#[derive(Debug, Default, Deserialize)]
pub struct InboxQuery {
    #[serde(default)]
    pub done: bool,
    pub limit: Option<u32>,
}

/// The readable repos of an account's inbox.
async fn readable(app: &Platform, v: &Viewer) -> Result<viewer::Readable, ApiError> {
    let mut repos = Vec::new();
    for id in app.db.inbox_repos(&v.account)? {
        repos.extend(app.db.linked_repo(id)?);
    }
    Ok(viewer::check(app, v, &repos).await?.readable)
}

/// `GET /api/inbox`: `inbox.list`, the reader's inbox on repos they may
/// read now, and for a person their native issues' entries (M202,
/// [`crate::inbox::entries`]): newest first.
pub async fn list(
    State(app): State<Arc<Platform>>,
    d: Drafter,
    Query(q): Query<InboxQuery>,
) -> Result<Json<wire::Inbox>, ApiError> {
    let v = Viewer::of(&app, &d.account).await?;
    let r = readable(&app, &v).await?;
    let f = InboxFilter { done: q.done, limit: q.limit.unwrap_or(DEFAULT).clamp(1, MOST) };
    let mut entries = Vec::new();
    for x in app.db.inbox(&r, &v.account, &f)? {
        let room = if x.reason == THREAD_MENTION { app.db.thread_of(&x.item)? } else { None };
        entries.push(wire::InboxEntry {
            url: url(&x.full_name, &x.kind, x.number),
            item: x.item,
            reason: x.reason,
            repo: x.full_name,
            number: x.number,
            kind: x.kind,
            state: x.state,
            title: x.title,
            author: x.author_login,
            created_at: x.created_at,
            updated_at: x.updated_at,
            seen_at: x.seen_at,
            done_at: x.done_at,
            project: None,
            room,
        });
    }
    if d.machine.is_none() {
        entries.extend(crate::inbox::entries(&app, &v.account, &f).await?);
        entries.sort_by(|a, b| (b.created_at, &a.item, &a.reason).cmp(&(a.created_at, &b.item, &b.reason)));
        entries.truncate(f.limit as usize);
    }
    Ok(Json(wire::Inbox { entries }))
}

/// `POST /api/inbox/{item}` ([`wire::InboxMark`]): seen or done, or not.
/// An item the reader may not read is no item (404).
pub async fn mark(
    State(app): State<Arc<Platform>>,
    Person { account }: Person,
    Path(item): Path<String>,
    Json(b): Json<wire::InboxMark>,
) -> Result<Json<wire::InboxMarked>, ApiError> {
    Ok(Json(mark_for(&app, &account, item, &b).await?))
}

/// A person's marks on an item's entries (`inbox.mark`'s too).
pub async fn mark_for(
    app: &Platform,
    account: &str,
    item: String,
    b: &wire::InboxMark,
) -> Result<wire::InboxMarked, ApiError> {
    if b.reason.as_deref().is_some_and(|r| !wire::INBOX_REASONS.contains(&r)) {
        return Err(err(StatusCode::BAD_REQUEST, "reason: one of the inbox's reasons"));
    }
    if item.starts_with("iss_") || item.starts_with("run_") {
        let marked = crate::inbox::mark(app, account, &item, b).await?;
        return Ok(wire::InboxMarked { item, marked });
    }
    let v = Viewer::of(app, account).await?;
    let (r, _) = viewer::item(app, &v, &item).await?.ok_or_else(|| err(StatusCode::NOT_FOUND, "no such item"))?;
    let n = app.db.mark_inbox(&r, account, &item, b.reason.as_deref(), b.seen, b.done, now_ms())?;
    Ok(wire::InboxMarked { item, marked: n as u32 })
}

/// Whether a block's reasons and the platform's agree: the same set.
pub fn agree(block: &[String], platform: &[String]) -> bool {
    let a: BTreeSet<&str> = block.iter().map(String::as_str).collect();
    let b: BTreeSet<&str> = platform.iter().map(String::as_str).collect();
    a == b
}

/// A disagreement the platform can't help, by design: the block's mention
/// on a repo that doesn't mirror content, with the rest the same. It's
/// left out of the score (Jake's call, #1217).
fn explain(block: &[String], platform: &[String], mirrors: bool) -> Option<&'static str> {
    let rest: Vec<String> = block.iter().filter(|r| *r != "mentioned").cloned().collect();
    (!mirrors && rest.len() < block.len() && agree(&rest, platform)).then_some("content not mirrored")
}

/// `YYYY-MM-DD`, UTC.
fn day(ms: u64) -> String {
    let days = (ms / 86_400_000) as i64;
    // Civil from days (Howard Hinnant's algorithm).
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!("{y:04}-{m:02}-{d:02}")
}

/// `POST /api/inbox/shadow` ([`wire::InboxShadow`]): a machine's forge
/// blocks' attention, compared with the platform's inbox for its owner
/// on the same items, and logged: a line per report with its day's
/// counts, and one per disagreement with the item and both reasons, never
/// a title (docs/control-ops.md). Blocks on repos the owner may not read
/// here, or items the mirror doesn't have, aren't compared; nor is an
/// explained difference ([`explain`]), which is counted on its own.
pub async fn shadow(
    State(app): State<Arc<Platform>>,
    m: Machine,
    Json(b): Json<wire::InboxShadow>,
) -> Result<Json<wire::ShadowCompared>, ApiError> {
    app.limits.check_account(SHADOW_REPORTS, &m.account)?;
    let v = Viewer::of(&app, &m.account).await?;
    // Each repo's id, if the owner may read it here, and whether it mirrors content.
    let mut readable: HashMap<String, Option<(u64, bool)>> = HashMap::new();
    let mut out = wire::ShadowCompared::default();
    for blk in b.blocks.iter().take(SHADOW_BLOCKS) {
        let name = blk.repo.to_ascii_lowercase();
        let repo_id = match readable.get(&name) {
            Some(r) => *r,
            None => {
                let r = match app.db.linked_repo_named(&blk.repo)? {
                    Some(l) => viewer::check(&app, &v, std::slice::from_ref(&l))
                        .await?
                        .readable
                        .has(l.repo_id)
                        .then_some((l.repo_id, l.mirror_content)),
                    None => None,
                };
                *readable.entry(name).or_insert(r)
            }
        };
        let Some((repo_id, mirrors)) = repo_id else { continue };
        let Some(item) = app.db.forge_item_id(repo_id, blk.number)? else { continue };
        let mut block: Vec<String> = blk
            .reasons
            .iter()
            .filter(|r| wire::INBOX_REASONS.contains(&r.as_str()) && *r != "draft")
            .cloned()
            .collect();
        let drafts: Vec<String> = blk.drafts.iter().take(50).cloned().collect();
        if !app.db.waiting_drafts_of(&m.account, &drafts)?.is_empty() {
            block.push("draft".into());
        }
        let platform = app.db.inbox_reasons(&m.account, &item)?;
        if agree(&block, &platform) {
            out.compared += 1;
            out.agree += 1;
            continue;
        }
        block.sort();
        let why = explain(&block, &platform, mirrors);
        let (block, platform) = (block.join(","), platform.join(","));
        // Explained: not compared, so it doesn't count either way.
        if let Some(why) = why {
            out.explained += 1;
            info!(item = %item, block, platform, why, "inbox shadow: explained");
        } else {
            out.compared += 1;
            out.disagree += 1;
            info!(item = %item, block, platform, "inbox shadow: disagree");
        }
    }
    info!(
        day = day(now_ms()),
        daemon = %m.daemon,
        compared = out.compared,
        agree = out.agree,
        disagree = out.disagree,
        explained = out.explained,
        "inbox shadow"
    );
    Ok(Json(out))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn days_and_sets() {
        assert_eq!(day(0), "1970-01-01");
        assert_eq!(day(1_792_540_800_000), "2026-10-21");
        assert_eq!(day(951_782_400_000), "2000-02-29");
        let s = |v: &[&str]| v.iter().map(|x| x.to_string()).collect::<Vec<_>>();
        assert!(agree(&s(&["done", "draft"]), &s(&["draft", "done"])));
        assert!(agree(&s(&[]), &s(&[])));
        assert!(!agree(&s(&["checks_failed"]), &s(&[])));
        assert!(!agree(&s(&["checks_failed"]), &s(&["checks_failed", "mentioned"])));
    }

    /// Only a mention where content isn't mirrored is explained.
    #[test]
    fn explained() {
        let s = |v: &[&str]| v.iter().map(|x| x.to_string()).collect::<Vec<_>>();
        assert_eq!(explain(&s(&["mentioned"]), &s(&[]), false), Some("content not mirrored"));
        assert_eq!(explain(&s(&["done", "mentioned"]), &s(&["done"]), false), Some("content not mirrored"));
        assert_eq!(explain(&s(&["mentioned"]), &s(&[]), true), None, "mirrored: it should have found it");
        assert_eq!(explain(&s(&["mentioned"]), &s(&["assigned"]), false), None);
        assert_eq!(explain(&s(&[]), &s(&["assigned"]), false), None);
    }
}
