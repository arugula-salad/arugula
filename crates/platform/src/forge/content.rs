//! Bodies on read, and content mirroring opt-in per project (M166, #1212).
//! The mirror's rows are metadata (M164). An item's body, comments,
//! reviews' text and review comments are:
//!
//! - **Fetched on read** (`GET /api/forge/items/{id}/content`, [`get`]),
//!   after M165's viewer check, from GitHub, with a read token for the one
//!   repo minted for the reader ([`Purpose::Read`], logged). They're kept
//!   [`KEEP`] in memory on the machine that read them ([`Cache`]), never in
//!   the database. A webhook for the item drops them: on this machine at
//!   once, and on every machine through the item's `content_rev`, which
//!   the cache checks on each read. A reader the check refuses never gets
//!   as far as the cache. Reads on demand are what the poller's rate
//!   reserve is for (M164): only an installation with none of its limit
//!   left is refused, and each account fetches at most [`FETCHES`] an hour
//!   (the cache's answers are free).
//! - **Mirrored**, for a repo whose project turns `mirror_content` on
//!   ([`settings`]: a member who may change links and, to turn it on,
//!   maintains the repo on GitHub, as linking takes). Webhooks
//!   ([`changes`], in M164's ingest) and the poller write `forge_content`
//!   (`db::content`), reads come from it, and the project's search finds
//!   their text. An item the mirror doesn't have whole yet is fetched on
//!   read and kept. Turning it off deletes the repo's rows in the same
//!   transaction; unlinking or uninstalling purges them within a day
//!   (M163).
//!
//! **Never logged.** Nothing here logs a body, a comment or a title: only
//! repo ids, numbers, counts and accounts. [`ContentRow`]'s `Debug` leaves
//! the text out, and GitHub's errors are its own message, not a body.

use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

use arugula_control_common::{ApiError, err, refusal};
use arugula_control_wire::projects::{self as wire, LinkedRepo as WireRepo, RepoSettings};
use arugula_e2e::now_ms;
use arugula_forge_github::{Github, TokenSource};
use axum::{
    Json,
    extract::{Path, State},
    http::StatusCode,
};
use serde_json::Value;
use tracing::{info, warn};

use super::{
    Install, NO_APP, Purpose, ingest,
    poller::RepoTokens,
    viewer::{self, Viewer},
};
use crate::{
    Platform,
    auth::Person,
    cloud::CloudDriver,
    db::{
        content::{self as rows, Change, ContentRow},
        forge::LinkedRepo,
    },
};

/// How long content read from GitHub is kept in memory.
pub const KEEP: Duration = Duration::from_secs(180);

/// Most items kept at once on a machine.
const MOST_KEPT: usize = 500;

/// GitHub fetches on read per account per hour (the cache's answers
/// don't count).
pub const FETCHES: (&str, usize) = ("forge-content", 300);

/// Most pages of one list an item's fetch reads.
const PAGES: usize = 10;

struct Kept {
    at: Instant,
    rev: u64,
    rows: Arc<Vec<ContentRow>>,
}

/// Content read from GitHub, per item (repo, number), for [`KEEP`], while
/// its `content_rev` is the one it was read at.
#[derive(Default)]
pub struct Cache(Mutex<HashMap<(u64, u64), Kept>>);

impl Cache {
    fn get(&self, repo: u64, number: u64, rev: u64) -> Option<Arc<Vec<ContentRow>>> {
        let mut m = self.0.lock().unwrap();
        match m.get(&(repo, number)) {
            Some(k) if k.rev == rev && k.at.elapsed() < KEEP => Some(k.rows.clone()),
            Some(_) => {
                m.remove(&(repo, number));
                None
            }
            None => None,
        }
    }

    fn put(&self, repo: u64, number: u64, rev: u64, rows: Arc<Vec<ContentRow>>) {
        let mut m = self.0.lock().unwrap();
        m.retain(|_, k| k.at.elapsed() < KEEP);
        if m.len() >= MOST_KEPT
            && let Some(oldest) = m.iter().min_by_key(|(_, k)| k.at).map(|(key, _)| *key)
        {
            m.remove(&oldest);
        }
        m.insert((repo, number), Kept { at: Instant::now(), rev, rows });
    }

    /// Drop what's kept of an item: a webhook said it changed.
    pub fn forget(&self, repo: u64, number: u64) {
        self.0.lock().unwrap().remove(&(repo, number));
    }

    /// Whether anything of an item is kept, for tests.
    #[cfg(test)]
    pub fn has(&self, repo: u64, number: u64) -> bool {
        self.0.lock().unwrap().contains_key(&(repo, number))
    }
}

fn login(v: &Value) -> Option<String> {
    v["login"].as_str().map(str::to_owned)
}

/// A PR's or an issue's body (`GET pulls/N`, `issues/N`, a list's, or a
/// webhook's item): an empty one is a body too.
pub fn body_row(repo_id: u64, number: u64, v: &Value) -> Option<ContentRow> {
    Some(ContentRow {
        repo_id,
        number,
        kind: "body".into(),
        github_id: v["id"].as_u64()?,
        author_login: login(&v["user"]),
        author_id: v["user"]["id"].as_u64(),
        body: v["body"].as_str().unwrap_or_default().to_owned(),
        path: None,
        line: None,
        reply_to: None,
        created_at: ingest::ms(&v["created_at"]).unwrap_or(0),
        updated_at: ingest::ms(&v["updated_at"]).unwrap_or(0),
    })
}

/// A comment (`kind` `comment`) or a review comment (`review_comment`,
/// with its file, line and the comment it answers).
pub fn comment_row(repo_id: u64, number: u64, kind: &str, v: &Value) -> Option<ContentRow> {
    let created_at = ingest::ms(&v["created_at"]).unwrap_or(0);
    Some(ContentRow {
        repo_id,
        number,
        kind: kind.into(),
        github_id: v["id"].as_u64()?,
        author_login: login(&v["user"]),
        author_id: v["user"]["id"].as_u64(),
        body: v["body"].as_str().unwrap_or_default().to_owned(),
        path: v["path"].as_str().map(str::to_owned),
        line: v["line"].as_u64().or(v["original_line"].as_u64()),
        reply_to: v["in_reply_to_id"].as_u64(),
        created_at,
        updated_at: ingest::ms(&v["updated_at"]).unwrap_or(created_at),
    })
}

/// A review's text: none for a review with none (an approval alone) or
/// one still pending.
pub fn review_row(repo_id: u64, number: u64, v: &Value) -> Option<ContentRow> {
    let body = v["body"].as_str().filter(|b| !b.trim().is_empty())?;
    if v["state"].as_str().is_some_and(|s| s.eq_ignore_ascii_case("pending")) {
        return None;
    }
    let at = ingest::ms(&v["submitted_at"]).unwrap_or(0);
    Some(ContentRow {
        repo_id,
        number,
        kind: "review".into(),
        github_id: v["id"].as_u64()?,
        author_login: login(&v["user"]),
        author_id: v["user"]["id"].as_u64(),
        body: body.to_owned(),
        path: None,
        line: None,
        reply_to: None,
        created_at: at,
        updated_at: at,
    })
}

/// What a webhook says of one item's content.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Changes {
    pub repo: u64,
    pub number: u64,
    pub changes: Vec<Change>,
}

/// The content an event changes: a PR or an issue opened or edited, a
/// comment or review comment made, edited or deleted, a review submitted
/// or edited. `None` for any other.
pub fn changes(event: &str, v: &Value) -> Option<Changes> {
    let repo = v["repository"]["id"].as_u64()?;
    let action = v["action"].as_str().unwrap_or_default();
    let (number, change) = match (event, action) {
        ("pull_request" | "issues", "opened" | "edited") => {
            let item = if event == "issues" { &v["issue"] } else { &v["pull_request"] };
            let number = item["number"].as_u64()?;
            let row = body_row(repo, number, item)?;
            (number, if action == "opened" { Change::Opened(row) } else { Change::Body(row) })
        }
        ("issue_comment" | "pull_request_review_comment", _) => {
            let (number, kind) = match event {
                "issue_comment" => (v["issue"]["number"].as_u64()?, "comment"),
                _ => (v["pull_request"]["number"].as_u64()?, "review_comment"),
            };
            let change = match action {
                "created" | "edited" => Change::Upsert(comment_row(repo, number, kind, &v["comment"])?),
                "deleted" => Change::Deleted(kind.into(), v["comment"]["id"].as_u64()?),
                _ => return None,
            };
            (number, change)
        }
        ("pull_request_review", "submitted" | "edited") => {
            let number = v["pull_request"]["number"].as_u64()?;
            let change = match review_row(repo, number, &v["review"]) {
                Some(r) => Change::Upsert(r),
                // Its text taken out: none to keep.
                None => Change::Deleted("review".into(), v["review"]["id"].as_u64()?),
            };
            (number, change)
        }
        _ => return None,
    };
    Some(Changes { repo, number, changes: vec![change] })
}

/// Write a webhook's content changes in `Db::mirror`'s transaction: the
/// item's `content_rev` goes up, and a repo that mirrors content keeps
/// them.
pub fn write(tx: &impl crate::db::sql::Sql, c: &Changes) -> anyhow::Result<()> {
    rows::bump(tx, c.repo, c.number)?;
    if rows::mirrors(tx, c.repo)? {
        for ch in &c.changes {
            rows::apply(tx, c.repo, ch)?;
        }
    }
    Ok(())
}

/// Every page of a list, up to [`PAGES`].
async fn list<T: TokenSource>(gh: &Github<T>, name: &str, path: &str) -> anyhow::Result<Vec<Value>> {
    let mut out = Vec::new();
    let mut next = Some(path.to_owned());
    for _ in 0..PAGES {
        let Some(p) = next.take() else { break };
        let (page, n) =
            gh.page(name, &p).await.map_err(|e| anyhow::anyhow!("{}: {e}", path.split('?').next().unwrap_or("")))?;
        out.extend(page.as_array().into_iter().flatten().cloned());
        next = n;
    }
    Ok(out)
}

/// An item's content from GitHub, whole: its body, comments and, for a
/// PR, reviews' text and review comments. `name` is the repo's
/// `OWNER/NAME`, for the client's own keeping.
pub async fn fetch<T: TokenSource>(
    gh: &Github<T>,
    name: &str,
    repo_id: u64,
    number: u64,
    pr: bool,
) -> anyhow::Result<Vec<ContentRow>> {
    let base = format!("repositories/{repo_id}");
    let path = format!("{base}/{}/{number}", if pr { "pulls" } else { "issues" });
    let item = gh.read(name, &path).await.map_err(|e| anyhow::anyhow!("{path}: {e}"))?;
    let mut out: Vec<ContentRow> = body_row(repo_id, number, &item).into_iter().collect();
    let comments = list(gh, name, &format!("{base}/issues/{number}/comments?per_page=100")).await?;
    out.extend(comments.iter().filter_map(|v| comment_row(repo_id, number, "comment", v)));
    if pr {
        let reviews = list(gh, name, &format!("{base}/pulls/{number}/reviews?per_page=100")).await?;
        out.extend(reviews.iter().filter_map(|v| review_row(repo_id, number, v)));
        let threads = list(gh, name, &format!("{base}/pulls/{number}/comments?per_page=100")).await?;
        out.extend(threads.iter().filter_map(|v| comment_row(repo_id, number, "review_comment", v)));
    }
    if !out.iter().any(|w| w.kind == "body") {
        anyhow::bail!("{path}: GitHub's answer had no id");
    }
    out.sort_by_key(|w| (w.kind != "body", w.created_at, w.github_id));
    Ok(out)
}

fn text(w: &ContentRow) -> wire::ForgeText {
    wire::ForgeText {
        kind: w.kind.clone(),
        id: w.github_id,
        author: w.author_login.clone(),
        body: w.body.clone(),
        path: w.path.clone(),
        line: w.line,
        reply_to: w.reply_to,
        created_at: w.created_at,
        updated_at: w.updated_at,
    }
}

fn answer(item: &str, from: &str, rows: &[ContentRow]) -> wire::ForgeContent {
    wire::ForgeContent {
        item: item.to_owned(),
        from: from.to_owned(),
        body: rows.iter().find(|w| w.kind == "body").map(text),
        comments: rows.iter().filter(|w| w.kind != "body").map(text).collect(),
    }
}

fn no_item() -> ApiError {
    err(StatusCode::NOT_FOUND, "no such item")
}

/// `GET /api/forge/items/{id}/content` (module docs).
pub async fn get(
    State(app): State<Arc<Platform>>,
    v: Viewer,
    Path(id): Path<String>,
) -> Result<Json<wire::ForgeContent>, ApiError> {
    // The check first: a reader it refuses never reaches the mirror, the
    // cache or GitHub.
    let (readable, repo) = viewer::item(&app, &v, &id).await?.ok_or_else(no_item)?;
    let i = app.db.forge_item(&readable, &id)?.ok_or_else(no_item)?;
    if repo.mirror_content
        && let Some(rows) = app.db.forge_content(&readable, repo.repo_id, i.number)?
    {
        return Ok(Json(answer(&i.id, "mirror", &rows)));
    }
    let Some(gh) = &app.github else { return Err(err(StatusCode::NOT_FOUND, NO_APP)) };
    let rev = app.db.content_rev(&readable, repo.repo_id, i.number)?.ok_or_else(no_item)?;
    if let Some(rows) = gh.bodies.get(repo.repo_id, i.number, rev) {
        return Ok(Json(answer(&i.id, "github", &rows)));
    }
    let stats = &gh.ingest.stats;
    if let Some(reset) = stats.reserved(repo.installation_id, 0, now_ms() / 1000) {
        let mins = reset.saturating_sub(now_ms() / 1000).div_ceil(60).max(1);
        let why = format!("the GitHub App's rate limit for {} is spent: try again in {mins} min", repo.full_name);
        return Err(refusal(StatusCode::SERVICE_UNAVAILABLE, "rate_limit", &why));
    }
    app.limits.check_account(FETCHES, &v.account)?;
    let tokens = RepoTokens {
        p: &app,
        caller: &v.account,
        purpose: Purpose::Read,
        installation: repo.installation_id,
        repo_id: repo.repo_id,
    };
    // A client of its own: no ETag cache keeps the bodies past this read.
    let client = Github::new(&gh.api, app.http.clone(), tokens);
    let fetched = fetch(&client, &repo.full_name, repo.repo_id, i.number, i.kind == "pr").await;
    let r = client.rate();
    stats.heard(repo.installation_id, r.limit, r.remaining, r.reset);
    let rows = match fetched {
        Ok(rows) => Arc::new(rows),
        Err(e) => {
            // GitHub's message, the repo's id and the number: never a body.
            warn!(repo = repo.repo_id, number = i.number, error = format!("{e:#}"), "reading an item's content");
            return Err(err(StatusCode::BAD_GATEWAY, "couldn't read it from GitHub"));
        }
    };
    if repo.mirror_content {
        // Mirrored from now on, unless it was turned off meanwhile.
        app.db.mirror(repo.repo_id, None, |tx| {
            if rows::mirrors(tx, repo.repo_id)? {
                rows::replace_item(tx, repo.repo_id, i.number, &rows)?;
            }
            Ok(())
        })?;
    } else {
        gh.bodies.put(repo.repo_id, i.number, rev, rows.clone());
    }
    Ok(Json(answer(&i.id, "github", &rows)))
}

/// `PATCH /api/projects/{project}/repos/{owner}/{name}` ([`RepoSettings`]):
/// turn content mirroring on or off (module docs).
pub async fn settings(
    State(app): State<Arc<Platform>>,
    CloudDriver(Person { account }): CloudDriver,
    Path((project, owner, name)): Path<(String, String, String)>,
    Json(b): Json<RepoSettings>,
) -> Result<Json<WireRepo>, ApiError> {
    let project = super::links::changer(&app, &account, &project).await?;
    let full_name = format!("{owner}/{name}");
    let not_linked = || err(StatusCode::NOT_FOUND, &format!("{full_name} isn't linked to this project"));
    let link = app
        .db
        .linked_repos(&project)?
        .into_iter()
        .find(|r| r.unlinked_at.is_none() && r.full_name.eq_ignore_ascii_case(&full_name))
        .ok_or_else(not_linked)?;
    if b.mirror_content && !link.mirror_content {
        maintains(&app, &account, &link).await?;
    }
    let (repo, gone) = app.db.set_mirror_content(&project, &full_name, b.mirror_content)?.ok_or_else(not_linked)?;
    if !b.mirror_content
        && let Some(gh) = &app.github
    {
        gh.bodies.0.lock().unwrap().retain(|(r, _), _| *r != repo.repo_id);
    }
    info!(%account, %project, repo = repo.repo_id, on = b.mirror_content, deleted = gone, "a linked repo's content mirroring");
    Ok(Json(super::links::wire(&repo)))
}

/// Turning mirroring on takes what linking takes: a maintainer of the repo
/// on GitHub, by their numeric id.
async fn maintains(app: &Platform, account: &str, link: &LinkedRepo) -> Result<(), ApiError> {
    let Some(gh) = &app.github else { return Err(err(StatusCode::NOT_FOUND, NO_APP)) };
    let Some(user) = app.hub.github_user(account).await? else {
        return Err(err(StatusCode::FORBIDDEN, "sign in to control with GitHub to mirror a GitHub repo's content"));
    };
    app.limits.check_account(super::FORGE_LOOKUPS, account)?;
    let install =
        Install { id: link.installation_id, account: String::new(), account_id: 0, account_type: String::new() };
    let role = gh
        .repo_role(&super::Caller::of(app, account), &install, &user, &link.full_name)
        .await
        .map_err(|why| err(StatusCode::FORBIDDEN, &why))?;
    if role.id != link.repo_id {
        return Err(err(StatusCode::CONFLICT, &format!("{} is another repo on GitHub now", link.full_name)));
    }
    if !role.may_link() {
        let why = format!(
            "mirroring {}'s content takes admin or maintain on GitHub; {} has {}",
            role.full_name, user.login, role.role
        );
        return Err(refusal(StatusCode::FORBIDDEN, "github_role", &why));
    }
    Ok(())
}
