//! Link previews (#108): a link in a message unfurls into a card under
//! it, drawn when it's read, for the reader.
//!
//! - **Our own objects.** A link to an issue, run, deploy or hosted
//!   workspace, in any path form the platform page has (`web/src/platform/
//!   paths.ts`), on one of control's own hosts ([`own_host`]), becomes a
//!   card of title, state, who and when, read from the platform's tables
//!   *as the reader*: only a member of the object's project gets one (the
//!   rule object threads use, `objects::member`), and anyone else gets
//!   nothing, the plain link, as for a link to something that isn't there.
//!   A GitHub PR or issue link is the same when its repo is linked to a
//!   project the reader is in (and, for a private repo, may read on GitHub:
//!   M165); the card is the mirror's. A card isn't stored, so it's the
//!   object's state now: the page asks again while it's open, and a closed
//!   issue's card shows closed.
//! - **Web links.** Anything else is read for Open Graph by
//!   [`link_fetch`] (SSRF-guarded), kept [`TTL`] (a failure [`FAILED_TTL`]),
//!   and shown as a card whose image comes through
//!   `GET /api/chat/previews/image` so the reader's browser never meets the
//!   site. A team can turn web previews off (an owner's setting; its
//!   objects' cards stay: they fetch nothing), and a message's author can
//!   take its previews off.
//! - **One request per page of messages**: `GET /api/chat/rooms/{id}/
//!   previews?seqs=…` takes message numbers, not links, so what's fetched
//!   is what a member of the room said, and answers for all of them.
//!
//! Docs and panes live on a machine, not in the platform's tables; their
//! links stay plain until the multiplexer block (#123) can answer for them.

use std::{
    collections::{BTreeMap, HashMap, HashSet},
    sync::Arc,
};

use arugula_control_common::{ApiError, err};
use arugula_e2e::{now_ms, team::TeamRole};
use axum::{
    Json,
    extract::{Path, Query, State},
    http::{HeaderValue, StatusCode, header},
    response::{IntoResponse, Response},
};
use serde::{Deserialize, Serialize};
use url::Url;

use super::{J, gate, link_fetch, objects, room_for};
use crate::{
    Platform,
    auth::Person,
    db::{
        issues::{CloseReason, State as IssueState},
        link_previews::WebPreview,
    },
};

/// How long a read of a page is kept, and a failed one.
pub const TTL: u64 = 24 * 3_600_000;
pub const FAILED_TTL: u64 = 3_600_000;
/// At most this many links are previewed in a message, messages in a
/// request, and pages fetched (not cached) in one.
const PER_MESSAGE: usize = 3;
const MAX_SEQS: usize = 50;
const FETCHES: usize = 6;

/// How a card's state reads, for its colour: `open` (live or active),
/// `done` (merged, succeeded, closed as done), `bad`, `wait` (needs
/// someone) or `off` (closed without being done).
#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Tone {
    Open,
    Done,
    Bad,
    Wait,
    Off,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Preview {
    Object {
        url: String,
        /// `issue`, `pull request`, `run`, `deploy`, `workspace`.
        what: String,
        title: String,
        /// In words: `open`, `merged`, `waiting for approval`.
        state: String,
        tone: Tone,
        /// A display name, when the object has a person.
        who: Option<String>,
        at: u64,
    },
    Web {
        url: String,
        site: String,
        title: String,
        description: String,
        /// Our proxy's path for its image.
        image: Option<String>,
    },
}

#[derive(Debug, Serialize)]
pub struct Previews {
    /// By message number; a message with none isn't there.
    pub previews: BTreeMap<i64, Vec<Preview>>,
    /// Whether this room's team shows web previews.
    pub web: bool,
}

#[derive(Deserialize)]
pub struct PreviewsQuery {
    /// Comma-separated message numbers.
    seqs: String,
}

/// What a link in a message names.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
enum Link {
    Issue(String),
    Run(String),
    Deploy(String),
    Workspace(String),
    /// `OWNER/NAME`, number.
    Github(String, u64),
    Web,
}

/// Whether `u` is on a host control answers at.
fn own_host(app: &Platform, u: &Url) -> bool {
    let Some(host) = u.host_str().map(str::to_ascii_lowercase) else { return false };
    let authority = match u.port() {
        Some(p) => format!("{host}:{p}"),
        None => host.clone(),
    };
    let known = |s: &String| {
        Url::parse(s).ok().is_some_and(|k| {
            let h = k.host_str().unwrap_or_default().to_ascii_lowercase();
            k.port().map_or(h.clone(), |p| format!("{h}:{p}")) == authority
        })
    };
    matches!(host.as_str(), "app.arugula.io" | "control.arugula.io")
        || app.cfg.sites.iter().any(known)
        || known(&app.cfg.public_url)
}

/// The id at the front of a path segment, before its slug: the shape
/// `paths.ts` reads.
fn id_of(segment: &str, prefix: &str) -> Option<String> {
    let id = segment.split('-').next()?;
    let hex = id.strip_prefix(prefix)?;
    (hex.len() >= 4 && hex.len() <= 64 && hex.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)))
        .then(|| id.to_owned())
}

/// What a link names: our own object in one of the page's path forms, a
/// GitHub PR or issue, or a web link.
fn classify(app: &Platform, u: &Url) -> Link {
    let segs: Vec<&str> = u.path_segments().map(|s| s.filter(|x| !x.is_empty()).collect()).unwrap_or_default();
    if own_host(app, u) {
        // `/p/:project/…` and the id-only forms name the same things.
        let rest: &[&str] = match segs.as_slice() {
            ["p", _, rest @ ..] => rest,
            all => all,
        };
        let found = match rest {
            ["issues", s] => id_of(s, "iss_").map(Link::Issue),
            ["runs", s] => id_of(s, "run_").map(Link::Run),
            ["workspaces", s] => id_of(s, "hw_").map(Link::Workspace),
            ["deploys", s] => s.starts_with("dep_").then(|| Link::Deploy((*s).to_owned())),
            _ => None,
        };
        // A path of ours that isn't an object is still not for the web
        // fetcher: it would only find a sign-in page.
        return found.unwrap_or(Link::Web);
    }
    if matches!(u.host_str(), Some("github.com" | "www.github.com"))
        && let [owner, repo, kind @ ("pull" | "issues"), n, ..] = segs.as_slice()
        && let Ok(n) = n.parse::<u64>()
    {
        let _ = kind;
        return Link::Github(format!("{owner}/{repo}"), n);
    }
    Link::Web
}

/// The links in a message's Markdown, in order, without code (a URL in a
/// code span is text about a URL), each once: at most [`PER_MESSAGE`].
pub fn links(text: &str) -> Vec<Url> {
    let mut plain = String::with_capacity(text.len());
    let mut fenced = false;
    for line in text.lines() {
        if line.trim_start().starts_with("```") {
            fenced = !fenced;
            continue;
        }
        if fenced {
            continue;
        }
        // Inline code: what's between backticks.
        for (i, part) in line.split('`').enumerate() {
            if i % 2 == 0 {
                plain.push_str(part);
            }
            plain.push(' ');
        }
        plain.push('\n');
    }
    let mut out: Vec<Url> = Vec::new();
    let mut rest = plain.as_str();
    while let Some(i) = rest.find("http") {
        let tail = &rest[i..];
        if !(tail.starts_with("http://") || tail.starts_with("https://")) {
            rest = &rest[i + 4..];
            continue;
        }
        let end =
            tail.find(|c: char| c.is_whitespace() || matches!(c, '<' | '>' | '"' | ')' | '`')).unwrap_or(tail.len());
        let raw = tail[..end].trim_end_matches(['.', ',', ';', ':', '!', '?', '\'', '*', '_']);
        rest = &tail[end..];
        if raw.len() > 2048 {
            continue;
        }
        if let Ok(mut u) = Url::parse(raw)
            && u.host_str().is_some()
        {
            u.set_fragment(None);
            if !out.contains(&u) {
                out.push(u);
            }
        }
        if out.len() == PER_MESSAGE {
            break;
        }
    }
    out
}

fn issue_state(s: IssueState) -> (String, Tone) {
    match s {
        IssueState::Open => ("open".into(), Tone::Open),
        IssueState::Closed(CloseReason::Completed) => ("closed".into(), Tone::Done),
        IssueState::Closed(CloseReason::NotPlanned) => ("closed, not planned".into(), Tone::Off),
        IssueState::Closed(CloseReason::Duplicate) => ("closed, duplicate".into(), Tone::Off),
    }
}

fn run_state(s: arugula_e2e::run::State) -> (String, Tone) {
    use arugula_e2e::run::State as S;
    match s {
        S::Pending => ("waiting for approval".into(), Tone::Wait),
        S::Approved => ("approved".into(), Tone::Open),
        S::Running => ("running".into(), Tone::Open),
        S::Succeeded => ("succeeded".into(), Tone::Done),
        S::Failed => ("failed".into(), Tone::Bad),
        S::Denied => ("denied".into(), Tone::Bad),
        S::Expired => ("expired".into(), Tone::Off),
        S::Canceled => ("canceled".into(), Tone::Off),
    }
}

fn deploy_state(s: crate::db::deploys::State) -> (String, Tone) {
    use crate::db::deploys::State as S;
    match s {
        S::Planned => ("planned".into(), Tone::Off),
        S::Pending => ("waiting for approval".into(), Tone::Wait),
        S::Approved => ("approved".into(), Tone::Open),
        S::Running => ("deploying".into(), Tone::Open),
        S::Succeeded => ("deployed".into(), Tone::Done),
        S::Failed => ("failed".into(), Tone::Bad),
        S::Denied => ("denied".into(), Tone::Bad),
        S::Expired => ("expired".into(), Tone::Off),
        S::Canceled => ("canceled".into(), Tone::Off),
        S::Stale => ("stale".into(), Tone::Off),
    }
}

fn workspace_state(s: &str) -> (String, Tone) {
    match s {
        "ready" | "running" => ("running".into(), Tone::Open),
        "paused" | "asleep" => ("paused".into(), Tone::Off),
        "failed" | "error" => ("failed".into(), Tone::Bad),
        other => (other.replace('_', " "), Tone::Wait),
    }
}

fn mirror_state(kind: &str, state: &str, draft: bool) -> (String, Tone) {
    match (kind, state) {
        ("pr", "open") if draft => ("draft".into(), Tone::Off),
        (_, "open") => ("open".into(), Tone::Open),
        ("pr", "merged") => ("merged".into(), Tone::Done),
        (_, "closed") => ("closed".into(), if kind == "pr" { Tone::Bad } else { Tone::Done }),
        (_, s) => (s.to_owned(), Tone::Off),
    }
}

/// The project ids among `projects` that `account` is a member of now.
async fn member_projects(app: &Platform, account: &str, projects: &[String]) -> anyhow::Result<HashSet<String>> {
    let holders = app.db.project_holders(projects)?;
    let teams: Vec<String> = holders.values().filter_map(|h| h.team.clone()).collect();
    let teams = app.cache.teams(&*app.hub, &teams).await?;
    Ok(holders
        .into_iter()
        .filter(|(_, h)| match (&h.account, &h.team) {
            (Some(a), _) => a == account,
            (None, Some(t)) => teams.get(t).is_some_and(|t| t.role(account).is_some()),
            (None, None) => false,
        })
        .map(|(p, _)| p)
        .collect())
}

/// The cards for these object links, as `account` sees them: a link to
/// something they can't see, or that isn't there, has none.
async fn object_cards(
    app: &Platform,
    account: &str,
    links: &[(String, Link)],
) -> anyhow::Result<HashMap<String, Preview>> {
    // Read each object, then decide who may see it with one membership pass.
    struct Seen {
        url: String,
        project: String,
        what: &'static str,
        title: String,
        state: (String, Tone),
        who: Option<String>,
        at: u64,
        /// A private repo's mirror item: also needs the reader's own GitHub access.
        private: Option<(String, u64)>,
    }
    let mut seen: Vec<Seen> = Vec::new();
    for (url, link) in links {
        match link {
            Link::Issue(id) => {
                if let Some(i) = app.db.issue(id)? {
                    seen.push(Seen {
                        url: url.clone(),
                        project: i.project.clone(),
                        what: "issue",
                        title: format!("#{} {}", i.number, i.title),
                        state: issue_state(i.state),
                        who: Some(i.author.clone()).filter(|a| !a.is_empty()),
                        at: i.closed_at.unwrap_or(i.created_at),
                        private: None,
                    });
                }
            }
            Link::Run(id) => {
                if let Some(r) = app.db.run(id)? {
                    let short: String = id.trim_start_matches("run_").chars().take(8).collect();
                    seen.push(Seen {
                        url: url.clone(),
                        project: r.project.clone(),
                        what: "run",
                        title: format!("run {short} ({})", r.trigger.as_str()),
                        state: run_state(r.state),
                        who: Some(r.started_by.clone()).filter(|a| r.trigger.as_str() == "manual" && !a.is_empty()),
                        at: r.ended_at.or(r.started_at).unwrap_or(r.created_at),
                        private: None,
                    });
                }
            }
            Link::Deploy(id) => {
                if let Some(d) = app.db.deploy(id)?
                    && let Some(env) = app.db.environment(&d.environment)?
                {
                    seen.push(Seen {
                        url: url.clone(),
                        project: env.project.clone(),
                        what: "deploy",
                        title: format!("deploy to {}", env.name),
                        state: deploy_state(d.state),
                        who: Some(d.planned_by.clone()).filter(|a| !a.is_empty()),
                        at: d.updated_at,
                        private: None,
                    });
                }
            }
            Link::Workspace(id) => {
                if let Some((_, project, _)) = app.db.hosted_places(&[id.as_str()])?.into_iter().next()
                    && let Some(w) = app.db.hosted_any(&project, id)?
                {
                    seen.push(Seen {
                        url: url.clone(),
                        project,
                        what: "workspace",
                        title: w.name.clone(),
                        state: workspace_state(&w.state),
                        who: w.created_by.clone(),
                        at: w.created_at,
                        private: None,
                    });
                }
            }
            Link::Github(repo, n) => {
                // One card: the first linking project the reader is in is
                // settled below, so keep them all.
                for m in app.db.mirror_cards(repo, *n)? {
                    seen.push(Seen {
                        url: url.clone(),
                        project: m.project_id.clone(),
                        what: if m.kind == "pr" { "pull request" } else { "issue" },
                        title: format!("{}#{} {}", m.full_name, m.number, m.title),
                        state: mirror_state(&m.kind, &m.state, m.draft),
                        who: m.author.clone(),
                        at: m.updated_at,
                        private: m.private.then(|| (m.id.clone(), m.repo_id)),
                    });
                }
            }
            Link::Web => {}
        }
    }
    let mut projects: Vec<String> = seen.iter().map(|s| s.project.clone()).collect();
    projects.sort();
    projects.dedup();
    let mine = member_projects(app, account, &projects).await?;
    let mut shown: Vec<Seen> = Vec::new();
    for s in seen {
        if !mine.contains(&s.project) || shown.iter().any(|x| x.url == s.url) {
            continue;
        }
        // GitHub not answering is the same as not being allowed: the
        // other cards must not wait on it.
        if let Some((id, repo_id)) = &s.private
            && !objects::may_read_private(app, account, id, *repo_id).await.unwrap_or(false)
        {
            continue;
        }
        shown.push(s);
    }
    // People are shown by name; a GitHub login is already one, and an
    // account id (`acc_…`-shaped, from our own rows) is looked up.
    let ids: Vec<String> =
        shown.iter().filter(|s| s.what != "pull request" && s.who.is_some()).filter_map(|s| s.who.clone()).collect();
    let names = app.cache.accounts(&*app.hub, &ids).await?;
    Ok(shown
        .into_iter()
        .map(|s| {
            let who = match (s.what, s.who) {
                ("pull request", w) => w,
                (_, Some(id)) => names
                    .get(&id)
                    .map(|a| a.name.clone())
                    .filter(|n| !n.is_empty())
                    .or(Some(id).filter(|i| !i.contains('_'))),
                (_, None) => None,
            };
            (
                s.url.clone(),
                Preview::Object {
                    url: s.url,
                    what: s.what.to_owned(),
                    title: s.title,
                    state: s.state.0,
                    tone: s.state.1,
                    who,
                    at: s.at,
                },
            )
        })
        .collect())
}

/// A web link's card: from the cache, or by fetching. `None` when there's
/// nothing to show.
async fn web_card(app: &Platform, url: &Url, fetch: bool) -> Option<Preview> {
    let key = url.as_str().to_owned();
    let now = now_ms();
    let cached = app.db.web_preview(&key).ok()?;
    let fresh = |p: &WebPreview| now.saturating_sub(p.fetched_at) < if p.ok { TTL } else { FAILED_TTL };
    let p = match cached {
        Some(p) if fresh(&p) => p,
        _ if !fetch => return None,
        _ => {
            let p = read_page(&link_fetch::Net::real(), &key, now).await;
            app.db.put_web_preview(&p).ok()?;
            // Old reads (and their images) go as new ones come.
            app.db.prune_web_previews(now.saturating_sub(7 * TTL)).ok();
            p
        }
    };
    p.ok.then(|| Preview::Web {
        // The page's own address for now: `previews` turns it into the
        // proxy's path for the message the card is under ([`image_path`]).
        image: p.image_url.as_ref().map(|_| key.clone()),
        url: key,
        site: p.site,
        title: p.title,
        description: p.description,
    })
}

/// Read a page for a card: whatever can't be read is a failure, kept for
/// a short while so it isn't tried at every view.
async fn read_page(net: &link_fetch::Net, url: &str, now: u64) -> WebPreview {
    let nothing = WebPreview { url: url.to_owned(), fetched_at: now, ..Default::default() };
    let Ok(got) = link_fetch::get(net, url, link_fetch::Want::Page).await else { return nothing };
    let m = link_fetch::meta(&got.body, &got.url);
    let site = if m.site.is_empty() {
        got.url.host_str().unwrap_or_default().trim_start_matches("www.").to_owned()
    } else {
        m.site
    };
    if m.title.is_empty() && m.description.is_empty() {
        return nothing;
    }
    WebPreview {
        url: url.to_owned(),
        fetched_at: now,
        ok: true,
        site,
        title: m.title,
        description: m.description,
        image_url: m.image,
    }
}

/// The team whose setting governs a room's web previews: its team's, or
/// its project's team's. A personal room has none (previews on).
async fn team_of(app: &Platform, room: &crate::db::chat::Room) -> anyhow::Result<Option<String>> {
    Ok(match room.scope.split_once(':') {
        Some(("team", t)) => Some(t.to_owned()),
        Some(("project", p)) => app.db.project_holders(&[p.to_owned()])?.remove(p).and_then(|h| h.team),
        _ => None,
    })
}

/// `GET /api/chat/rooms/{id}/previews?seqs=1,2,3`: the cards under these
/// messages, for the reader.
pub async fn previews(
    State(app): State<Arc<Platform>>,
    s: Person,
    Path(id): Path<String>,
    Query(q): Query<PreviewsQuery>,
) -> J<Previews> {
    gate(&app, &s.account).await?;
    let (room, from, _) = room_for(&app, &s.account, &id).await?;
    let seqs: Vec<i64> = q.seqs.split(',').filter_map(|n| n.trim().parse().ok()).take(MAX_SEQS).collect();
    let hidden = app.db.hidden_previews(&room.id, &seqs)?;
    let web = match team_of(&app, &room).await? {
        Some(t) => app.db.web_previews_on(&t)?,
        None => true,
    };
    let mut found: Vec<(i64, Vec<(Url, Link)>)> = Vec::new();
    for seq in seqs.into_iter().filter(|s| !hidden.contains(s)) {
        let Some(m) = app.db.chat_message(&room.id, seq)? else { continue };
        if m.deleted.is_some() || m.at < from {
            continue;
        }
        let ls: Vec<(Url, Link)> =
            links(&m.text).into_iter().map(|u| (classify(&app, &u), u)).map(|(l, u)| (u, l)).collect();
        if !ls.is_empty() {
            found.push((seq, ls));
        }
    }
    let objs: Vec<(String, Link)> = found
        .iter()
        .flat_map(|(_, ls)| ls.iter().filter(|(_, l)| *l != Link::Web).map(|(u, l)| (u.as_str().to_owned(), l.clone())))
        .collect();
    let cards = object_cards(&app, &s.account, &objs).await?;
    // Web pages: the cached ones, then fetches for as many uncached ones as
    // a request may start. A GitHub link without a card is a web link; a
    // link of ours without one is nothing (the page would be a sign-in).
    let mut budget = FETCHES;
    let mut web_links: Vec<Url> = Vec::new();
    for (_, ls) in &found {
        for (u, l) in ls {
            let webbable = match l {
                Link::Web | Link::Github(..) => !cards.contains_key(u.as_str()),
                _ => false,
            };
            if web && webbable && !own_host(&app, u) && !web_links.contains(u) {
                web_links.push(u.clone());
            }
        }
    }
    let fetches: Vec<_> = web_links
        .iter()
        .map(|u| {
            let fetch = budget > 0
                && app
                    .db
                    .web_preview(u.as_str())
                    .ok()
                    .flatten()
                    .is_none_or(|p| now_ms().saturating_sub(p.fetched_at) >= TTL);
            if fetch {
                budget -= 1;
            }
            let app = &app;
            async move { (u.as_str().to_owned(), web_card(app, u, fetch).await) }
        })
        .collect();
    let web_cards: HashMap<String, Preview> =
        futures_util::future::join_all(fetches).await.into_iter().filter_map(|(u, c)| Some((u, c?))).collect();
    let mut out = BTreeMap::new();
    for (seq, ls) in found {
        let cs: Vec<Preview> = ls
            .iter()
            .filter_map(|(u, _)| cards.get(u.as_str()).or_else(|| web_cards.get(u.as_str())).cloned())
            .map(|mut c| {
                if let Preview::Web { image: Some(page), .. } = &mut c {
                    *page = image_path(&room.id, seq, page);
                }
                c
            })
            .collect();
        if !cs.is_empty() {
            out.insert(seq, cs);
        }
    }
    Ok(Json(Previews { previews: out, web }))
}

#[derive(Deserialize)]
pub struct Hide {
    hidden: bool,
}

/// `POST /api/chat/rooms/{id}/messages/{seq}/previews`: its author takes
/// the message's previews off (`hidden: true`) or puts them back.
pub async fn hide(
    State(app): State<Arc<Platform>>,
    s: Person,
    Path((id, seq)): Path<(String, i64)>,
    Json(b): Json<Hide>,
) -> J<serde_json::Value> {
    gate(&app, &s.account).await?;
    let (room, _, _) = room_for(&app, &s.account, &id).await?;
    let m = app.db.chat_message(&room.id, seq)?.ok_or_else(|| err(StatusCode::NOT_FOUND, "no such message"))?;
    if m.author != s.account {
        return Err(err(StatusCode::FORBIDDEN, "only its author takes a message's previews off"));
    }
    app.db.set_previews_hidden(&room.id, seq, b.hidden, now_ms())?;
    app.chat.publish(&app.db, super::ChatLive::Changed { room: room.id.clone(), seq }, None);
    Ok(Json(serde_json::json!({ "hidden": b.hidden })))
}

#[derive(Deserialize, Serialize)]
pub struct TeamPreviews {
    pub web: bool,
}

/// `GET /api/chat/teams/{team}/previews`: whether the team shows web
/// previews, for its members.
pub async fn team_get(State(app): State<Arc<Platform>>, s: Person, Path(team): Path<String>) -> J<TeamPreviews> {
    gate(&app, &s.account).await?;
    team_role(&app, &s.account, &team).await?;
    Ok(Json(TeamPreviews { web: app.db.web_previews_on(&team)? }))
}

/// `PUT /api/chat/teams/{team}/previews`: an owner's switch for web
/// previews in the team's rooms.
pub async fn team_put(
    State(app): State<Arc<Platform>>,
    s: Person,
    Path(team): Path<String>,
    Json(b): Json<TeamPreviews>,
) -> J<TeamPreviews> {
    gate(&app, &s.account).await?;
    if team_role(&app, &s.account, &team).await? != TeamRole::Owner {
        return Err(err(StatusCode::FORBIDDEN, "only a team's owners change its chat settings"));
    }
    app.db.set_web_previews(&team, b.web)?;
    Ok(Json(b))
}

async fn team_role(app: &Platform, account: &str, team: &str) -> Result<TeamRole, ApiError> {
    let teams = app.cache.teams(&*app.hub, &[team.to_owned()]).await?;
    teams.get(team).and_then(|t| t.role(account)).ok_or_else(|| err(StatusCode::NOT_FOUND, "no such team"))
}

/// The proxy's path for a card's image, tied to the message it's under.
fn image_path(room: &str, seq: i64, page: &str) -> String {
    let enc = |s: &str| url::form_urlencoded::byte_serialize(s.as_bytes()).collect::<String>();
    format!("/api/chat/previews/image?room={}&seq={seq}&page={}", enc(room), enc(page))
}

#[derive(Deserialize)]
pub struct ImageQuery {
    room: String,
    seq: i64,
    page: String,
}

/// `GET /api/chat/previews/image?room=&seq=&page=<link>`: the image of a
/// page that was read for a preview, fetched (guarded) the first time and
/// kept. Only a page's cached read names an image address, so this is no
/// open proxy; a reader's browser meets us, never the site. The reader
/// must be able to see the message and it must carry the link (and its
/// previews be on), or the answer would tell anyone signed in which pages
/// have been read here.
pub async fn image(
    State(app): State<Arc<Platform>>,
    s: Person,
    Query(q): Query<ImageQuery>,
) -> Result<Response, ApiError> {
    gate(&app, &s.account).await?;
    let gone = || err(StatusCode::NOT_FOUND, "no such image");
    let (room, from, _) = room_for(&app, &s.account, &q.room).await.map_err(|_| gone())?;
    let m = app.db.chat_message(&room.id, q.seq)?.ok_or_else(gone)?;
    let web = match team_of(&app, &room).await? {
        Some(t) => app.db.web_previews_on(&t)?,
        None => true,
    };
    if m.deleted.is_some()
        || m.at < from
        || !web
        || app.db.hidden_previews(&room.id, &[q.seq])?.contains(&q.seq)
        || !links(&m.text).iter().any(|u| u.as_str() == q.page)
    {
        return Err(gone());
    }
    let (image_url, have) = app.db.preview_image(&q.page)?.ok_or_else(gone)?;
    let (kind, bytes) = match have {
        Some(x) => x,
        None => {
            let u = image_url.ok_or_else(gone)?;
            let got =
                link_fetch::get(&link_fetch::Net::real(), &u, link_fetch::Want::Image).await.map_err(|_| gone())?;
            let kind = got.content_type.split(';').next().unwrap_or("").trim().to_ascii_lowercase();
            app.db.put_preview_image(&q.page, &kind, &got.body)?;
            (kind, got.body)
        }
    };
    let mut r = bytes.into_response();
    let h = r.headers_mut();
    h.insert(header::CONTENT_TYPE, HeaderValue::from_str(&kind).map_err(|_| gone())?);
    h.insert(header::CACHE_CONTROL, HeaderValue::from_static("private, max-age=86400"));
    h.insert(header::X_CONTENT_TYPE_OPTIONS, HeaderValue::from_static("nosniff"));
    h.insert(header::CONTENT_SECURITY_POLICY, HeaderValue::from_static("default-src 'none'; sandbox"));
    Ok(r)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn links_are_found_outside_code() {
        let t = "see https://a.example/x, and (https://b.example/y).\n`https://code.example/`\n```\nhttps://fenced.example/\n```\n[t](https://c.example/z#frag) https://a.example/x";
        let got: Vec<String> = links(t).iter().map(|u| u.to_string()).collect();
        assert_eq!(got, ["https://a.example/x", "https://b.example/y", "https://c.example/z"]);
        assert!(links("no links, httpx://nope and http:// alone").is_empty());
        assert_eq!(links("a b c d").len(), 0);
        let many = "https://1.example https://2.example https://3.example https://4.example";
        assert_eq!(links(many).len(), PER_MESSAGE);
    }

    #[test]
    fn state_words_and_tones() {
        assert_eq!(mirror_state("pr", "merged", false), ("merged".to_owned(), Tone::Done));
        assert_eq!(mirror_state("pr", "open", true).1, Tone::Off);
        assert_eq!(run_state(arugula_e2e::run::State::Pending), ("waiting for approval".to_owned(), Tone::Wait));
        assert_eq!(issue_state(IssueState::Closed(CloseReason::Completed)).0, "closed");
    }
}
