//! Webhooks write rows (M164, #1210). The App's `pull_request`,
//! `pull_request_review`, `pull_request_review_comment`, `issues`,
//! `issue_comment`, `check_run` and `status` events for a linked repo
//! (not one unlinked) become `forge_items`, `forge_checks` and
//! `forge_events` rows, in the webhook's own request and before its pokes
//! go to the hub. GitHub's payloads are mapped with `arugula-forge-github`'s
//! parse code where it fits (an item); checks and events keep GitHub's own
//! words and times.
//!
//! - **Metadata only** in these rows. Never a body, a review's text or a
//!   comment's: a comment is an event that one was made. The same webhook
//!   bumps the item's `content_rev` (the read cache's stamp), and writes
//!   its content to `forge_content` only for a repo that mirrors it
//!   ([`super::content`], M166).
//! - **Out of order** (`db::mirror`): a row is only overwritten by a
//!   payload whose GitHub time is newer; one from the same second that
//!   disagrees is read again by the poller.
//! - `check_suite` and `workflow_run` write nothing of their own: their
//!   check runs come as `check_run` events. A commit status, or a fork's
//!   check run (which names no PR), is kept for any commit: one that comes
//!   before its PR's `synchronize` is there when the new head is. The
//!   poller prunes checks on a commit no item has as its head after an
//!   hour.
//! - **An issue deleted or transferred** goes, with its events, its
//!   head's checks and its inbox rows (`db::mirror::delete_item`). Its
//!   drafts stay, for M170: a send to it fails and says why.
//! - A row that fails to write is logged and left to the poller
//!   ([`super::poller`]), which finds it as drift; the pokes go as before.

use std::{collections::HashMap, sync::Mutex, time::Instant};

use arugula_forge_github::{EtagCache, model::ItemState, parse, util};
use serde_json::Value;
use tracing::{debug, warn};

use super::stats::Stats;
use crate::{
    Platform,
    db::mirror::{self, CheckRow, EventRow, ItemRow, Source, Wrote},
};

/// The poller's settings (`Args`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Settings {
    /// How often each linked repo is reconciled with GitHub, in ms.
    pub every_ms: u64,
    /// The share of an installation's rate limit, in percent, the poller
    /// leaves for reads on demand and sends.
    pub reserve: u8,
    /// The most list pages a turn reads; a turn that has more carries on
    /// from where it stopped at the next.
    pub pages: usize,
}

impl Default for Settings {
    fn default() -> Self {
        Self { every_ms: 5 * 60_000, reserve: 30, pages: 50 }
    }
}

/// The mirror's state on this machine: its settings, its numbers, and
/// each installation's ETags (one installation reads as one identity, so
/// its repos' polls share them).
#[derive(Default)]
pub struct Ingest {
    pub settings: Settings,
    pub stats: Stats,
    etags: Mutex<HashMap<u64, EtagCache>>,
}

impl Ingest {
    pub fn etags(&self, installation: u64) -> EtagCache {
        self.etags.lock().unwrap().entry(installation).or_default().clone()
    }
}

/// GitHub's time as ms since the epoch.
pub fn ms(v: &Value) -> Option<u64> {
    v.as_str().and_then(util::time).and_then(|t| u64::try_from(t).ok())
}

fn list(mut v: Vec<String>) -> String {
    v.sort();
    v.dedup();
    serde_json::to_string(&v).unwrap_or_else(|_| "[]".into())
}

fn some(s: String) -> Option<String> {
    (!s.is_empty()).then_some(s)
}

fn login(v: &Value) -> Option<String> {
    v["login"].as_str().map(str::to_owned)
}

/// A PR (`GET pulls/N`, a list's, or a webhook's) as a row, through
/// `parse::item`. Never its body.
pub fn pr_row(repo_id: u64, full_name: &str, v: &Value) -> Option<ItemRow> {
    v["number"].as_u64()?;
    let it = parse::item(v, full_name);
    Some(ItemRow {
        repo_id,
        number: it.number,
        kind: "pr".into(),
        state: state(it.state).into(),
        title: it.title,
        labels: list(it.labels),
        author_login: some(it.author),
        author_id: v["user"]["id"].as_u64(),
        assignees: list(it.assignees),
        // Logins, and teams as `org/slug` (which no login can be).
        reviewers: list(it.requested.iter().map(|r| r.name().to_owned()).collect()),
        draft: it.draft,
        merge_state: v["mergeable_state"].as_str().map(str::to_owned),
        head_ref: some(it.head.branch),
        head_sha: some(it.head.sha),
        base_ref: some(it.base.branch),
        created_at: ms(&v["created_at"]).unwrap_or(0),
        updated_at: u64::try_from(it.updated_at).unwrap_or(0),
    })
}

/// An issue (not a PR's) as a row, through `parse::issue_item`.
pub fn issue_row(repo_id: u64, v: &Value) -> Option<ItemRow> {
    if v["pull_request"].is_object() {
        return None;
    }
    v["number"].as_u64()?;
    let it = parse::issue_item(v);
    Some(ItemRow {
        repo_id,
        number: it.number,
        kind: "issue".into(),
        state: state(it.state).into(),
        title: it.title,
        labels: list(it.labels),
        author_login: some(it.author),
        author_id: v["user"]["id"].as_u64(),
        assignees: list(it.assignees),
        reviewers: "[]".into(),
        draft: false,
        merge_state: None,
        head_ref: None,
        head_sha: None,
        base_ref: None,
        created_at: ms(&v["created_at"]).unwrap_or(0),
        updated_at: u64::try_from(it.updated_at).unwrap_or(0),
    })
}

fn state(s: ItemState) -> &'static str {
    match s {
        ItemState::Open => "open",
        ItemState::Closed => "closed",
        ItemState::Merged => "merged",
    }
}

/// A check run (a `check_run` webhook's, or `GET commits/SHA/check-runs`'),
/// timed by when it completed, else started.
pub fn run_row(repo_id: u64, v: &Value) -> Option<CheckRow> {
    Some(CheckRow {
        repo_id,
        head_sha: v["head_sha"].as_str().filter(|s| !s.is_empty())?.to_owned(),
        kind: "check_run".into(),
        name: v["name"].as_str()?.to_owned(),
        status: v["status"].as_str().unwrap_or("queued").to_owned(),
        conclusion: v["conclusion"].as_str().map(str::to_owned),
        url: v["html_url"].as_str().or(v["details_url"].as_str()).filter(|u| !u.is_empty()).map(str::to_owned),
        updated_at: ms(&v["completed_at"]).or(ms(&v["started_at"])).unwrap_or(0),
    })
}

/// A commit status on `sha` (a `status` webhook, or one of
/// `GET commits/SHA/status`'s).
pub fn status_row(repo_id: u64, sha: &str, v: &Value) -> Option<CheckRow> {
    let state = v["state"].as_str()?;
    let (status, conclusion) = match state {
        "pending" => ("pending", None),
        s => ("completed", Some(s.to_owned())),
    };
    Some(CheckRow {
        repo_id,
        head_sha: sha.to_owned(),
        kind: "status".into(),
        name: v["context"].as_str()?.to_owned(),
        status: status.into(),
        conclusion,
        url: v["target_url"].as_str().filter(|u| !u.is_empty()).map(str::to_owned),
        updated_at: ms(&v["updated_at"]).or(ms(&v["created_at"])).unwrap_or(0),
    })
}

/// A review (a webhook's, or `GET pulls/N/reviews`'), as an event: its
/// state, never its text. A pending one (the reviewer's own draft) isn't.
pub fn review_event(repo_id: u64, number: u64, v: &Value) -> Option<EventRow> {
    let state = v["state"].as_str()?.to_ascii_lowercase();
    if state == "pending" {
        return None;
    }
    Some(EventRow {
        repo_id,
        number,
        kind: "review".into(),
        github_id: Some(v["id"].as_u64()?),
        actor_login: login(&v["user"]),
        actor_id: v["user"]["id"].as_u64(),
        review_state: Some(state),
        at: ms(&v["submitted_at"]).unwrap_or(0),
    })
}

/// That a comment was made (`kind`: `comment`, `review_comment`), by whom
/// and when: never what it says.
fn comment_event(repo_id: u64, number: u64, kind: &str, v: &Value) -> Option<EventRow> {
    Some(EventRow {
        repo_id,
        number,
        kind: kind.into(),
        github_id: Some(v["id"].as_u64()?),
        actor_login: login(&v["user"]),
        actor_id: v["user"]["id"].as_u64(),
        review_state: None,
        at: ms(&v["created_at"]).unwrap_or(0),
    })
}

/// A state change or review request, by the webhook's sender.
fn change_event(repo_id: u64, item: &ItemRow, kind: &str, sender: &Value) -> EventRow {
    EventRow {
        repo_id,
        number: item.number,
        kind: kind.into(),
        github_id: None,
        actor_login: login(sender),
        actor_id: sender["id"].as_u64(),
        review_state: None,
        at: item.updated_at,
    }
}

/// What a webhook writes.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Rows {
    pub repo: Option<u64>,
    pub items: Vec<ItemRow>,
    pub checks: Vec<CheckRow>,
    pub events: Vec<EventRow>,
    /// An issue deleted, or transferred away: its number.
    pub deleted: Option<u64>,
    /// GitHub's time for the change, for webhook to row.
    pub at: Option<u64>,
}

/// The rows an event's payload makes, and its repo: `None` for an event
/// that makes none.
pub fn rows(event: &str, v: &Value) -> Option<(u64, Rows)> {
    let repo = v["repository"]["id"].as_u64()?;
    let full_name = v["repository"]["full_name"].as_str().unwrap_or_default();
    let action = v["action"].as_str().unwrap_or_default();
    let mut r = Rows { repo: Some(repo), ..Rows::default() };
    match event {
        "pull_request" => {
            let it = pr_row(repo, full_name, &v["pull_request"])?;
            let kind = match action {
                "closed" if it.state == "merged" => Some("merged"),
                "opened"
                | "closed"
                | "reopened"
                | "ready_for_review"
                | "converted_to_draft"
                | "review_requested"
                | "review_request_removed" => Some(action),
                _ => None,
            };
            r.events.extend(kind.map(|k| change_event(repo, &it, k, &v["sender"])));
            r.at = Some(it.updated_at);
            r.items.push(it);
        }
        "pull_request_review" => {
            let it = pr_row(repo, full_name, &v["pull_request"])?;
            r.events.extend(review_event(repo, it.number, &v["review"]));
            r.at = Some(it.updated_at);
            r.items.push(it);
        }
        "pull_request_review_comment" if action == "created" => {
            let number = v["pull_request"]["number"].as_u64()?;
            r.events.extend(comment_event(repo, number, "review_comment", &v["comment"]));
            r.at = ms(&v["comment"]["created_at"]);
        }
        "issues" if matches!(action, "deleted" | "transferred") => {
            r.deleted = Some(v["issue"]["number"].as_u64()?);
        }
        "issues" => {
            let it = issue_row(repo, &v["issue"])?;
            if matches!(action, "opened" | "closed" | "reopened") {
                r.events.push(change_event(repo, &it, action, &v["sender"]));
            }
            r.at = Some(it.updated_at);
            r.items.push(it);
        }
        "issue_comment" if action == "created" => {
            let number = v["issue"]["number"].as_u64()?;
            r.events.extend(comment_event(repo, number, "comment", &v["comment"]));
            r.at = ms(&v["comment"]["created_at"]);
            // A PR's issue says nothing of its head: its own events do.
            r.items.extend(issue_row(repo, &v["issue"]));
        }
        "check_run" => {
            let row = run_row(repo, &v["check_run"])?;
            r.at = Some(row.updated_at).filter(|t| *t > 0);
            r.checks.push(row);
        }
        "status" => {
            let row = status_row(repo, v["sha"].as_str()?, v)?;
            r.at = Some(row.updated_at).filter(|t| *t > 0);
            r.checks.push(row);
        }
        _ => return None,
    }
    Some((repo, r))
}

/// How many rows a write made or changed, and whether the newest was
/// already there.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct Written {
    pub fixed: usize,
    pub same: usize,
    pub older: usize,
    /// Same second, saying otherwise: read again by the poller.
    pub ties: usize,
}

impl Written {
    fn add(&mut self, w: &Wrote) {
        match w {
            Wrote::New | Wrote::Changed(_) => self.fixed += 1,
            Wrote::Same => self.same += 1,
            Wrote::Older => self.older += 1,
            Wrote::Tie => self.ties += 1,
        }
    }
}

/// Write `rows` in the transaction `db::Db::mirror` opened.
pub fn write(tx: &impl crate::db::sql::Sql, rows: &Rows, now: u64) -> anyhow::Result<Written> {
    let mut w = Written::default();
    if let Some(n) = rows.deleted
        && let Some(repo) = rows.repo
        && mirror::delete_item(tx, repo, n)?
    {
        w.fixed += 1;
    }
    for it in &rows.items {
        w.add(&mirror::upsert_item(tx, it, Source::Pushed, now)?);
    }
    for c in &rows.checks {
        w.add(&mirror::upsert_check(tx, c, Source::Pushed, now)?);
    }
    for e in &rows.events {
        w.add(&mirror::add_event(tx, e)?);
    }
    Ok(w)
}

/// A webhook's rows, for a linked repo, that came at `came`: written,
/// and the time it took counted when one was new or changed. Its content
/// changes (M166) in the same transaction, and what this machine's read
/// cache kept of the item goes.
pub fn webhook(p: &Platform, event: &str, delivery: &str, v: &Value, came: Instant) {
    let Some(gh) = &p.github else { return };
    let rows = rows(event, v);
    let content = super::content::changes(event, v);
    let Some(repo) = rows.as_ref().map(|(r, _)| *r).or(content.as_ref().map(|c| c.repo)) else { return };
    let now = arugula_e2e::now_ms();
    let wrote = p.db.mirror(repo, None, |tx| {
        let w = match &rows {
            Some((_, r)) => write(tx, r, now)?,
            None => Written::default(),
        };
        if let Some(c) = &content {
            super::content::write(tx, c)?;
        }
        Ok(w)
    });
    if let Some(c) = &content {
        gh.bodies.forget(c.repo, c.number);
    }
    match wrote {
        Ok(None) => {}
        Ok(Some(w)) => {
            if let (true, Some((_, rows))) = (w.fixed > 0, &rows) {
                let event_ms = rows.at.map(|at| arugula_e2e::now_ms().saturating_sub(at));
                gh.ingest.stats.landed(came.elapsed().as_millis() as u64, event_ms);
                super::inbox::written(p, repo, rows);
                if !rows.items.is_empty() {
                    crate::sessions::pr::noted(p, repo);
                    crate::sessions::merged::seen(p, repo, &rows.items.iter().collect::<Vec<_>>());
                }
                let numbers: Vec<u64> =
                    rows.items.iter().map(|i| i.number).chain(rows.events.iter().map(|e| e.number)).collect();
                let shas: Vec<String> = rows.checks.iter().map(|c| c.head_sha.clone()).collect();
                crate::boards::mirror_changed(p, repo, &numbers, &shas);
            }
            debug!(
                event,
                delivery,
                repo,
                fixed = w.fixed,
                same = w.same,
                older = w.older,
                ties = w.ties,
                "webhook rows"
            );
        }
        // The poller finds it as drift.
        Err(e) => warn!(event, delivery, repo, error = format!("{e:#}"), "writing a webhook's mirror rows"),
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    /// A PR's row: GitHub's fields, sorted lists, and never its body.
    #[test]
    fn a_pr_as_a_row() {
        let v = json!({ "action": "opened", "repository": { "id": 77, "full_name": "acme/app" },
            "sender": { "login": "bob", "id": 2 },
            "pull_request": { "number": 5, "title": "Fix", "body": "secret body", "state": "open", "draft": true,
                "user": { "login": "bob", "id": 2 }, "labels": [{ "name": "z" }, { "name": "a" }],
                "requested_reviewers": [{ "login": "dan" }], "requested_teams": [{ "slug": "core" }],
                "head": { "ref": "fix", "sha": "abc" }, "base": { "ref": "main", "sha": "def" },
                "mergeable_state": "clean",
                "created_at": "2026-10-01T00:00:00Z", "updated_at": "2026-10-02T00:00:00Z" } });
        let (repo, r) = rows("pull_request", &v).unwrap();
        assert_eq!(repo, 77);
        let it = &r.items[0];
        assert_eq!((it.number, it.kind.as_str(), it.state.as_str(), it.draft), (5, "pr", "open", true));
        assert_eq!(it.labels, r#"["a","z"]"#);
        assert_eq!(it.reviewers, r#"["acme/core","dan"]"#);
        assert_eq!((it.head_sha.as_deref(), it.base_ref.as_deref()), (Some("abc"), Some("main")));
        assert_eq!(it.updated_at - it.created_at, 86_400_000);
        assert_eq!(r.events[0].kind, "opened");
        assert!(!format!("{r:?}").contains("secret"), "{r:?}");
        // A merge is a `merged` event.
        let mut m = v.clone();
        m["action"] = json!("closed");
        m["pull_request"]["merged_at"] = json!("2026-10-02T00:00:00Z");
        assert_eq!(rows("pull_request", &m).unwrap().1.events[0].kind, "merged");
    }

    /// A review and a comment are events with no text; a status names no
    /// PR; a suite, a workflow run, an edit, a push make nothing.
    #[test]
    fn events_checks_and_nothing() {
        let repo = json!({ "id": 77, "full_name": "acme/app" });
        let review = json!({ "action": "submitted", "repository": repo, "pull_request": { "number": 5, "updated_at": "2026-10-02T00:00:00Z" },
            "review": { "id": 9, "state": "CHANGES_REQUESTED", "body": "secret", "user": { "login": "dan", "id": 4 }, "submitted_at": "2026-10-02T00:00:00Z" } });
        let r = rows("pull_request_review", &review).unwrap().1;
        assert_eq!(r.events[0].review_state.as_deref(), Some("changes_requested"));
        assert!(!format!("{r:?}").contains("secret"));
        let comment = json!({ "action": "created", "repository": repo, "issue": { "number": 5, "pull_request": {} },
            "comment": { "id": 3, "body": "secret", "user": { "login": "dan", "id": 4 }, "created_at": "2026-10-02T00:00:00Z" } });
        let r = rows("issue_comment", &comment).unwrap().1;
        assert_eq!((r.events[0].kind.as_str(), r.items.len()), ("comment", 0), "a PR's issue isn't an item");
        assert!(!format!("{r:?}").contains("secret"));
        let status = json!({ "repository": repo, "sha": "abc", "context": "ci", "state": "failure", "updated_at": "2026-10-02T00:00:00Z" });
        let r = rows("status", &status).unwrap().1;
        assert_eq!(r.checks[0].conclusion.as_deref(), Some("failure"));
        let gone = json!({ "action": "transferred", "repository": repo, "issue": { "number": 6 } });
        assert_eq!(rows("issues", &gone).unwrap().1.deleted, Some(6));
        for e in ["check_suite", "workflow_run", "push"] {
            assert!(rows(e, &json!({ "repository": repo })).is_none(), "{e}");
        }
        assert!(rows("issue_comment", &json!({ "action": "edited", "repository": repo })).is_none());
    }
}
