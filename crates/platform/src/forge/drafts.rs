//! Drafts on linked repos (M170, #1216): a comment, a review or a new
//! issue that a person, or an agent on their machine, wrote and a person
//! sends (M168). "Agents draft, people send" (aru-041): nothing here sends.
//!
//! - **Who reads one:** the people [`viewer::check`] lets read its repo's
//!   items, which means they're in the repo's project (M165). Every read
//!   goes through the check's [`Readable`](viewer::Readable), as items' do.
//! - **Who makes one:** a person, for themselves, or a machine with a live
//!   `cloud.drive` grant ([`CloudDriver`]), for its owner: then the draft
//!   names the machine and the agent. Either way the author must be able
//!   to read the repo now.
//! - **Uploads:** a daemon brings the drafts it kept in `layout.json` for a
//!   linked repo with an `import_key` of its own. The platform makes it the
//!   machine's (`<machine>:<key>`), so one machine's keys never meet
//!   another's, and a second upload of the same key is answered with the
//!   first draft: a daemon that crashed before it dropped its copy uploads
//!   it again and gets the same row.
//! - **Who discards one:** its author, or anyone who may read it, but not
//!   while it's being sent.
//! - **Who sends one:** its person, as themselves ([`super::send`], M168).
//! - **Is a repo linked** ([`repo`]): what a daemon asks before it keeps a
//!   block's drafts here. A repo linked to a project the reader isn't in
//!   is no linked repo to them.

use std::sync::Arc;

use arugula_control_common::{ApiError, Principal, err};
use arugula_control_wire::projects as wire;
use arugula_e2e::now_ms;
use axum::{
    Json,
    extract::{FromRequestParts, Path, Query, State},
    http::{StatusCode, request::Parts},
};
use serde::Deserialize;

use super::viewer::{self, Viewer, Withheld};
use crate::{
    Platform,
    auth::{Caller, Person},
    cloud::CloudDriver,
    db::{
        Taken,
        forge::{Discarder, DraftFilter, DraftRow, NewDraftRow},
    },
};

/// Drafts made per account per hour: an agent in a loop stops here.
pub const DRAFTS_MADE: (&str, usize) = ("forge-draft", 600);

/// How many drafts a list holds at most, and when it doesn't say.
const MOST: u32 = 500;
const DEFAULT: u32 = 200;

/// The longest agent name and import key taken.
const MAX_NAME: usize = 100;
const MAX_KEY: usize = 200;

/// A person, or a machine for its owner ([`CloudDriver`]'s rule), and
/// which machine.
pub struct Drafter {
    pub account: String,
    pub machine: Option<String>,
}

impl FromRequestParts<Arc<Platform>> for Drafter {
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, app: &Arc<Platform>) -> Result<Self, ApiError> {
        let Caller(who) = Caller::from_request_parts(parts, app).await?;
        let CloudDriver(Person { account }) = CloudDriver::from_request_parts(parts, app).await?;
        let machine = match who {
            Principal::Daemon { daemon, .. } => Some(daemon),
            Principal::Person { .. } | Principal::Cli { .. } => None,
        };
        Ok(Self { account, machine })
    }
}

pub(super) fn no_draft() -> ApiError {
    err(StatusCode::NOT_FOUND, "no such draft")
}

fn not_linked(repo: &str) -> ApiError {
    err(StatusCode::NOT_FOUND, &format!("{repo} isn't linked to a project you can read it in"))
}

/// As the wire says it, with each machine's name where the hub still has
/// the machine.
pub(super) async fn wire_drafts(app: &Platform, rows: Vec<DraftRow>) -> Result<Vec<wire::ForgeDraft>, ApiError> {
    let mut ids: Vec<String> = rows.iter().filter_map(|d| d.agent_machine.clone()).collect();
    ids.sort();
    ids.dedup();
    let names = if ids.is_empty() { Default::default() } else { app.cache.daemons(&*app.hub, &ids).await? };
    let now = now_ms();
    Ok(rows
        .into_iter()
        .map(|d| {
            // A send holds it (M168): `sending`, with no `sent_at` yet.
            let sending = d.sending(now);
            let (state, sent_at) = if sending { ("sending".to_owned(), None) } else { (d.state, d.sent_at) };
            wire::ForgeDraft {
                machine_name: d.agent_machine.as_ref().and_then(|m| names.get(m)).map(|m| m.name.clone()),
                id: d.id,
                project: d.project_id,
                repo: d.full_name,
                repo_id: d.repo_id,
                number: d.number,
                item: d.item,
                item_title: d.item_title,
                kind: d.kind,
                event: d.event,
                title: d.title,
                body: d.body,
                state,
                author: d.author,
                agent: d.agent_name,
                machine: d.agent_machine,
                created_at: d.created_at,
                updated_at: d.updated_at,
                sent_at,
                sent_url: d.sent_url,
                error: d.error,
            }
        })
        .collect())
}

/// What a new draft says, checked: its kind and what that kind needs.
fn checked(b: &wire::NewDraft) -> Result<(), ApiError> {
    let bad = |why: &str| Err(err(StatusCode::BAD_REQUEST, why));
    if !super::good_repo(b.repo.trim()) {
        return bad("repo: OWNER/NAME");
    }
    if b.body.len() > wire::MAX_DRAFT_BODY || b.title.as_ref().is_some_and(|t| t.len() > 1024) {
        return bad("too long for GitHub");
    }
    match b.kind.as_str() {
        "comment" if b.body.trim().is_empty() => bad("a comment needs a body"),
        "comment" | "review" if b.number.is_none_or(|n| n == 0) => {
            bad("a comment or review needs the PR's or issue's number")
        }
        "review" if !matches!(b.event.as_deref(), Some("COMMENT" | "APPROVE" | "REQUEST_CHANGES")) => {
            bad("a review's event: COMMENT, APPROVE or REQUEST_CHANGES")
        }
        "review" if b.event.as_deref() != Some("APPROVE") && b.body.trim().is_empty() => {
            bad("a review that doesn't approve needs a body")
        }
        "issue" if b.number.is_some() => bad("a new issue has no number"),
        "issue" if b.title.as_deref().is_none_or(|t| t.trim().is_empty()) => bad("a new issue needs a title"),
        "comment" | "review" | "issue" => Ok(()),
        _ => bad("kind: comment, review or issue"),
    }
}

/// `POST /api/forge/drafts` ([`wire::NewDraft`]): keep a draft (module
/// docs).
pub async fn create(
    State(app): State<Arc<Platform>>,
    d: Drafter,
    Json(b): Json<wire::NewDraft>,
) -> Result<Json<wire::DraftCreated>, ApiError> {
    checked(&b)?;
    let agent = b.agent.as_deref().map(str::trim).filter(|a| !a.is_empty());
    let agent = match (&d.machine, agent) {
        (_, Some(a)) if a.len() > MAX_NAME => return Err(err(StatusCode::BAD_REQUEST, "agent: a short name")),
        (Some(_), None) => Some("an agent"),
        (_, a) => a,
    };
    let import_key = match (&d.machine, b.import_key.as_deref()) {
        (_, None) => None,
        (None, Some(_)) => return Err(err(StatusCode::BAD_REQUEST, "only a machine uploads a draft it kept")),
        (Some(_), Some(k)) if k.is_empty() || k.len() > MAX_KEY => {
            return Err(err(StatusCode::BAD_REQUEST, "import_key: a short id"));
        }
        (Some(m), Some(k)) => Some(format!("{m}:{k}")),
    };
    let repo_name = b.repo.trim();
    let v = Viewer::of(&app, &d.account).await?;
    let repo = app.db.linked_repo_named(repo_name)?.ok_or_else(|| not_linked(repo_name))?;
    let checked = viewer::check(&app, &v, std::slice::from_ref(&repo)).await?;
    if !checked.readable.has(repo.repo_id) {
        return Err(not_linked(repo_name));
    }
    app.limits.check_account(DRAFTS_MADE, &d.account)?;
    let new = NewDraftRow {
        project_id: &repo.project_id,
        author: &d.account,
        agent_name: agent,
        agent_machine: d.machine.as_deref(),
        repo_id: repo.repo_id,
        number: b.number,
        kind: &b.kind,
        event: b.event.as_deref(),
        title: b.title.as_deref().map(str::trim),
        body: &b.body,
        import_key: import_key.as_deref(),
        now: now_ms(),
    };
    let (row, created) = match app.db.add_draft(&checked.readable, &new) {
        Ok(Some(r)) => r,
        Ok(None) => return Err(not_linked(repo_name)),
        Err(e) if e.is::<Taken>() => return Err(err(StatusCode::CONFLICT, "that import_key is another draft's")),
        Err(e) => return Err(e.into()),
    };
    super::inbox::drafted(&app, row.repo_id, row.number);
    tracing::info!(
        draft = row.id,
        account = d.account,
        machine = d.machine,
        repo = row.repo_id,
        created,
        "a draft on a linked repo"
    );
    let draft = wire_drafts(&app, vec![row]).await?.remove(0);
    Ok(Json(wire::DraftCreated { draft, created }))
}

/// A list's query (`wire::FORGE_DRAFTS`).
#[derive(Debug, Default, Deserialize)]
pub struct DraftsQuery {
    pub repo: Option<String>,
    pub number: Option<u64>,
    /// Only the reader's own.
    #[serde(default)]
    pub mine: bool,
    /// `waiting` (the default), `sent`, `discarded`, `failed` or `all`.
    pub state: Option<String>,
    pub limit: Option<u32>,
}

impl DraftsQuery {
    fn filter<'a>(&'a self, account: &'a str) -> Result<DraftFilter<'a>, ApiError> {
        let state = match self.state.as_deref().unwrap_or("waiting") {
            "all" => None,
            s @ ("waiting" | "sent" | "discarded" | "failed") => Some(s),
            _ => return Err(err(StatusCode::BAD_REQUEST, "state: waiting, sent, discarded, failed or all")),
        };
        Ok(DraftFilter {
            project: None,
            repo_id: None,
            number: self.number,
            author: self.mine.then_some(account),
            state,
            limit: self.limit.unwrap_or(DEFAULT).clamp(1, MOST),
        })
    }
}

/// `GET /api/projects/{project}/drafts`: the project's drafts the reader
/// may read.
pub async fn project(
    State(app): State<Arc<Platform>>,
    v: Viewer,
    Path(project): Path<String>,
    Query(q): Query<DraftsQuery>,
) -> Result<Json<wire::ForgeDrafts>, ApiError> {
    let (project, _) = super::links::membership(&app, &v.account, &project, false).await?;
    let mut repos = app.db.linked_repos(&project)?;
    if let Some(name) = &q.repo {
        repos.retain(|r| r.full_name.eq_ignore_ascii_case(name.trim()));
        if repos.is_empty() {
            return Err(err(StatusCode::NOT_FOUND, &format!("{name} isn't linked to this project")));
        }
    }
    let checked = viewer::check(&app, &v, &repos).await?;
    let mut f = q.filter(&v.account)?;
    f.project = Some(&project);
    let rows = app.db.drafts(&checked.readable, &f)?;
    Ok(Json(wire::ForgeDrafts { drafts: wire_drafts(&app, rows).await? }))
}

/// `GET /api/forge/items/{id}/drafts`: an item's.
pub async fn item(
    State(app): State<Arc<Platform>>,
    v: Viewer,
    Path(id): Path<String>,
    Query(q): Query<DraftsQuery>,
) -> Result<Json<wire::ForgeDrafts>, ApiError> {
    let (readable, repo) =
        viewer::item(&app, &v, &id).await?.ok_or_else(|| err(StatusCode::NOT_FOUND, "no such item"))?;
    let i = app.db.forge_item(&readable, &id)?.ok_or_else(|| err(StatusCode::NOT_FOUND, "no such item"))?;
    let mut f = q.filter(&v.account)?;
    f.repo_id = Some(repo.repo_id);
    f.number = Some(i.number);
    let rows = app.db.drafts(&readable, &f)?;
    Ok(Json(wire::ForgeDrafts { drafts: wire_drafts(&app, rows).await? }))
}

/// `GET /api/forge/drafts`: the reader's own, on repos they may still
/// read.
pub async fn mine(
    State(app): State<Arc<Platform>>,
    v: Viewer,
    Query(q): Query<DraftsQuery>,
) -> Result<Json<wire::ForgeDrafts>, ApiError> {
    let mut repos = Vec::new();
    for id in app.db.draft_repos(&v.account)? {
        repos.extend(app.db.linked_repo(id)?);
    }
    let checked = viewer::check(&app, &v, &repos).await?;
    let mut f = q.filter(&v.account)?;
    f.author = Some(&v.account);
    let rows = app.db.drafts(&checked.readable, &f)?;
    Ok(Json(wire::ForgeDrafts { drafts: wire_drafts(&app, rows).await? }))
}

/// The readable repos of a draft's repo, if the viewer may read it.
pub async fn readable(app: &Platform, v: &Viewer, repo_id: u64) -> Result<Option<viewer::Readable>, ApiError> {
    let Some(repo) = app.db.linked_repo(repo_id)? else { return Ok(None) };
    let c = viewer::check(app, v, std::slice::from_ref(&repo)).await?;
    Ok(c.readable.has(repo_id).then_some(c.readable))
}

/// `GET /api/forge/drafts/{id}`: one the reader may read.
pub async fn get(
    State(app): State<Arc<Platform>>,
    v: Viewer,
    Path(id): Path<String>,
) -> Result<Json<wire::ForgeDraft>, ApiError> {
    let (repo_id, _) = app.db.draft_target(&id)?.ok_or_else(no_draft)?;
    let r = readable(&app, &v, repo_id).await?.ok_or_else(no_draft)?;
    let row = app.db.draft(&r, &id)?.ok_or_else(no_draft)?;
    Ok(Json(wire_drafts(&app, vec![row]).await?.remove(0)))
}

/// `POST /api/forge/drafts/{id}/discard`: by its author or a reader. Again
/// is a no-op; a sent one can't be (409).
pub async fn discard(
    State(app): State<Arc<Platform>>,
    v: Viewer,
    Path(id): Path<String>,
) -> Result<Json<wire::DraftDiscarded>, ApiError> {
    let (repo_id, author) = app.db.draft_target(&id)?.ok_or_else(no_draft)?;
    let r = readable(&app, &v, repo_id).await?;
    let who = match &r {
        Some(r) => Discarder::Reader(r),
        None if author == v.account => Discarder::Author(&v.account),
        None => return Err(no_draft()),
    };
    let state = app.db.discard_draft(who, &id, now_ms())?.ok_or_else(no_draft)?;
    match state.as_str() {
        "discarded" => {}
        "sending" => return Err(err(StatusCode::CONFLICT, "it's being sent")),
        _ => return Err(err(StatusCode::CONFLICT, &format!("it's {state}, not waiting"))),
    }
    if let Some((repo_id, number)) = app.db.draft_item(&id)? {
        super::inbox::drafted(&app, repo_id, number);
    }
    tracing::info!(draft = id, account = v.account, "draft discarded");
    Ok(Json(wire::DraftDiscarded { id, state }))
}

/// `GET /api/forge/repos/{owner}/{name}`: whether the reader may keep
/// drafts for it here (module docs).
pub async fn repo(
    State(app): State<Arc<Platform>>,
    v: Viewer,
    Path((owner, name)): Path<(String, String)>,
) -> Result<Json<wire::ForgeRepo>, ApiError> {
    let full = format!("{owner}/{name}");
    if !super::good_repo(&full) {
        return Err(err(StatusCode::BAD_REQUEST, "repo: OWNER/NAME"));
    }
    let mut out = wire::ForgeRepo { repo: full.clone(), ..Default::default() };
    let Some(link) = app.db.linked_repo_named(&full)? else { return Ok(Json(out)) };
    let c = viewer::check(&app, &v, std::slice::from_ref(&link)).await?;
    if c.readable.has(link.repo_id) {
        out.linked = true;
        out.project = Some(link.project_id);
        out.repo_id = Some(link.repo_id);
        out.login = v.github.as_ref().map(|u| u.login.clone());
    } else if let Some((_, why)) =
        c.withheld.iter().find(|(_, w)| !matches!(w, Withheld::NotMember | Withheld::Unlinked))
    {
        // Their own project's repo, which they may not read now.
        out.why = Some(why.as_str().to_owned());
    }
    Ok(Json(out))
}
