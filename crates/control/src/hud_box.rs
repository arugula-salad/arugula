//! hud boxes as clients (M237, platform step 8, "hud is an in-app client:
//! platform assertions, a project roster"). A `hud` client an operator
//! bound to a platform project (`issuer add-client --kind hud --project`)
//! and gave a secret (`issuer client-secret`) calls the first two from the
//! box's server, never a browser, as itself: HTTP Basic with its client id
//! (the box's `HUD_FORGE_SIGNIN_AUDIENCE`) and that secret.
//!
//! - `GET /oauth/hud/members`: its roster, the project's members now
//!   (`arugula_platform::boxes::members`), each with their numeric GitHub id,
//!   which hud matches people on, and their login for display only.
//! - `POST /oauth/hud/posts`: `{ target, author, text }`, a comment-mode
//!   review the box asks its author to post into the thread on an object in
//!   that project. Nothing is posted yet: the box gets back a link to the
//!   hub's own page (`confirm_url`, lasting [`POST_MS`]), which the person
//!   opens.
//! - `GET /oauth/hud/post?id=`: that page, for the review's author only,
//!   signed in to the hub: which box, which object, and the text, word for
//!   word, with *Post* and *Cancel*. It runs no script and nothing may frame
//!   it, so nothing on the box (an app the agent wrote included) can press
//!   *Post* for them.
//! - `POST /oauth/hud/post`: the answer, from that page only (the consent
//!   screen's same-origin check, `issuer::own_page_form`), once. *Post*
//!   posts it (`arugula_platform::boxes::post`): kept as posted via the box,
//!   and only if the author may post in that thread now, as when they post
//!   themselves.
//!
//! That's all a box's secret does: it can read its project's members and
//! ask its people to post, and it posts nothing itself. It reads no
//! threads, names no other project, and never links accounts (an `oidc`
//! client's secret does that, `links.rs`). Every call counts against the box
//! ([`limit::HUD_BOX_CALLS`]), and its reviews against their own limit and
//! alert line. An unbound box gets 409 `not_bound` and keeps its own roster.

use std::{collections::HashMap, net::SocketAddr, sync::Arc, sync::Mutex};

use arugula_e2e::now_ms;
use arugula_platform::{
    PlatformPort as _,
    boxes::{BoxPost, BoxPosted},
};
use axum::{
    Form, Json,
    body::Bytes,
    extract::{ConnectInfo, Query, State},
    http::{HeaderMap, HeaderValue, StatusCode, Uri, header},
    response::{IntoResponse, Response},
};
use base64::Engine as _;
use serde::Deserialize;

use crate::{
    App,
    auth::{hash, token},
    db::issuer::{Client, ClientKind},
    issuer::{escape, no_store, off, own_page_form},
    limit,
};

/// The longest object id or account a box names.
const ID_MAX: usize = 100;
/// The longest review a box posts: a chat message's.
const TEXT_MAX: usize = 8 * 1024;
/// How long a review waits for its author to post it.
pub const POST_MS: u64 = 10 * 60_000;
/// Reviews waiting at once, per box.
pub const WAITING_PER_BOX: usize = 50;

/// A review waiting for its author (M237).
#[derive(Debug, Clone)]
pub struct WaitingPost {
    client: String,
    box_name: String,
    project: String,
    target: String,
    author: String,
    text: String,
    until: u64,
}

/// The reviews waiting, by their id's hash: in memory, like the issuer's
/// codes. A hub that restarts drops them; the box asks again.
#[derive(Default)]
pub struct Waiting(Mutex<HashMap<String, WaitingPost>>);

impl Waiting {
    fn put(&self, id_hash: String, w: WaitingPost, now: u64) -> bool {
        let mut m = self.0.lock().unwrap();
        m.retain(|_, w| w.until > now);
        if m.values().filter(|o| o.client == w.client).count() >= WAITING_PER_BOX {
            return false;
        }
        m.insert(id_hash, w);
        true
    }

    fn peek(&self, id_hash: &str, now: u64) -> Option<WaitingPost> {
        self.0.lock().unwrap().get(id_hash).filter(|w| w.until > now).cloned()
    }

    /// Single use, whatever the answer.
    fn take(&self, id_hash: &str, now: u64) -> Option<WaitingPost> {
        self.0.lock().unwrap().remove(id_hash).filter(|w| w.until > now)
    }

    /// Every review waiting, `ms` older (tests).
    #[cfg(test)]
    pub fn age(&self, ms: u64) {
        for w in self.0.lock().unwrap().values_mut() {
            w.until = w.until.saturating_sub(ms);
        }
    }
}

fn error(status: StatusCode, error: &str, why: &str) -> Response {
    no_store((status, Json(serde_json::json!({ "error": error, "error_description": why }))).into_response())
}

fn invalid_client() -> Response {
    let mut r =
        error(StatusCode::UNAUTHORIZED, "invalid_client", "this needs the box's client id and secret (HTTP Basic)");
    r.headers_mut().insert(header::WWW_AUTHENTICATE, HeaderValue::from_static("Basic realm=\"arugula\""));
    r
}

/// The `hud` client a request comes from, by its HTTP Basic id and secret.
fn client(app: &App, headers: &HeaderMap) -> Option<Client> {
    let basic = headers.get(header::AUTHORIZATION)?.to_str().ok()?.strip_prefix("Basic ")?;
    let pair = base64::engine::general_purpose::STANDARD.decode(basic.trim()).ok()?;
    let (id, secret) = std::str::from_utf8(&pair).ok()?.split_once(':')?;
    if id.is_empty() || secret.is_empty() {
        return None;
    }
    app.db.issuer_client_by_secret(id, secret, ClientKind::Hud).ok()?
}

fn limited(app: &App, peer: SocketAddr, headers: &HeaderMap) -> Option<Response> {
    app.limits.check(limit::ISSUER, app.limits.client_ip(peer, headers)).err().map(IntoResponse::into_response)
}

/// The box and its project, or the answer that refuses it: the address's
/// limit, the box's credentials, then the box's own limit.
fn bound(app: &App, peer: SocketAddr, headers: &HeaderMap) -> Result<(Client, String), Box<Response>> {
    if app.issuer.is_none() {
        return Err(Box::new(off()));
    }
    if let Some(r) = limited(app, peer, headers) {
        return Err(Box::new(r));
    }
    let Some(c) = client(app, headers) else { return Err(Box::new(invalid_client())) };
    app.limits.check_client(limit::HUD_BOX_CALLS, &c.id).map_err(|e| Box::new(e.into_response()))?;
    let Some(project) = c.project.clone() else {
        return Err(Box::new(error(
            StatusCode::CONFLICT,
            "not_bound",
            "this box isn't bound to a project: it keeps its own roster",
        )));
    };
    Ok((c, project))
}

fn server_error(what: &str, e: &anyhow::Error) -> Response {
    tracing::warn!("a hud box's {what} failed: {e:#}");
    error(StatusCode::BAD_GATEWAY, "server_error", "try again")
}

/// `GET /oauth/hud/members`: the box's project's members now.
pub async fn members(
    State(app): State<Arc<App>>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
) -> Response {
    let (_, project) = match bound(&app, peer, &headers) {
        Ok(b) => b,
        Err(r) => return *r,
    };
    let members = match app.platform.box_members(&project).await {
        Ok(Some(m)) => m,
        Ok(None) => return error(StatusCode::NOT_FOUND, "no_such_project", "this box's project isn't there"),
        Err(e) => return server_error("roster", &e),
    };
    let mut out = Vec::with_capacity(members.len());
    for m in members {
        // Matched by the number only; the login is for showing.
        let gh = match crate::issuer::github_id(&app, &m.account) {
            Ok(g) => g,
            Err(e) => return server_error("roster", &e),
        };
        out.push(serde_json::json!({
            "account": m.account,
            "github_id": gh.as_ref().map(|(id, _)| *id),
            "login": gh.map(|(_, login)| login),
            "name": m.name,
            "role": m.role,
        }));
    }
    no_store(Json(serde_json::json!({ "project": project, "members": out })).into_response())
}

#[derive(Deserialize)]
struct PostBody {
    target: String,
    author: String,
    text: String,
}

fn id_ok(s: &str) -> bool {
    (1..=ID_MAX).contains(&s.len()) && s.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
}

/// Past the alert line for a box's reviews: a warn, once an hour, and the
/// ops counter (`link_alerts`, as `posts`). Never refuses.
fn watch_posts(app: &App, client: &str) {
    if app.limits.check_client(limit::HUD_BOX_POSTS_ALERT, client).is_ok() {
        return;
    }
    if let Err(e) = app.db.bump_link_alert(client, "posts", now_ms()) {
        tracing::warn!("the box post alert counter wasn't kept: {e:#}");
    }
    if app.limits.check_client(limit::LINK_ALERT_LOG, &format!("{client}/posts")).is_ok() {
        tracing::warn!(
            ops = "hud-box-posts-bulk",
            client,
            line = limit::HUD_BOX_POSTS_ALERT.1,
            "a hud box is asking for many review posts: over {} an hour (arugula-control issuer link-alerts)",
            limit::HUD_BOX_POSTS_ALERT.1
        );
    }
}

/// `POST /oauth/hud/posts`: a review waiting for its author, and the link
/// to the hub's page where they post it.
pub async fn posts(
    State(app): State<Arc<App>>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    let (c, project) = match bound(&app, peer, &headers) {
        Ok(b) => b,
        Err(r) => return *r,
    };
    let Ok(b) = serde_json::from_slice::<PostBody>(&body) else {
        return error(StatusCode::BAD_REQUEST, "invalid_request", "a JSON body: target, author, text");
    };
    if !id_ok(&b.target) || !id_ok(&b.author) || b.text.trim().is_empty() || b.text.len() > TEXT_MAX {
        return error(
            StatusCode::BAD_REQUEST,
            "invalid_request",
            "target is an object's id, author an account, and text 1 byte to 8 KB",
        );
    }
    if let Err(e) = app.limits.check_client(limit::HUD_BOX_POSTS, &c.id) {
        return e.into_response();
    }
    watch_posts(&app, &c.id);
    let Some(issuer) = &app.issuer else { return off() };
    let id = token();
    let now = now_ms();
    let w = WaitingPost {
        client: c.id.clone(),
        box_name: c.name.clone(),
        project,
        target: b.target,
        author: b.author,
        text: b.text,
        until: now + POST_MS,
    };
    if !issuer.box_posts.put(hash(&id), w, now) {
        return error(
            StatusCode::TOO_MANY_REQUESTS,
            "too_many_waiting",
            "this box has too many reviews waiting to be posted",
        );
    }
    let url = format!("{}/oauth/hud/post?id={id}", app.cfg.public_url.trim_end_matches('/'));
    no_store(Json(serde_json::json!({ "confirm_url": url, "expires_in": POST_MS / 1000 })).into_response())
}

/// A page of the hub's own: no script, nothing may frame it.
fn page(status: StatusCode, title: &str, body: &str) -> Response {
    let html = format!(
        r#"<!doctype html><html lang="en"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1">
<title>{title}</title>
<style>body{{font:16px/1.5 system-ui,sans-serif;max-width:40rem;margin:8vh auto;padding:0 16px;background:#0d1117;color:#e6edf3}}
pre{{white-space:pre-wrap;word-break:break-word;background:#161b22;border:1px solid #30363d;border-radius:6px;padding:12px;font:14px/1.5 ui-monospace,monospace}}
button{{font:inherit;padding:.5em 1.2em;margin-right:.6em;border-radius:6px;border:1px solid #30363d;background:#21262d;color:inherit;cursor:pointer}}
button[value=post]{{background:#238636;border-color:#238636}}p.dim{{color:#8b949e}}</style></head>
<body>{body}</body></html>"#,
        title = escape(title)
    );
    let mut r = no_store((status, [(header::CONTENT_TYPE, "text/html; charset=utf-8")], html).into_response());
    let h = r.headers_mut();
    h.insert(
        header::CONTENT_SECURITY_POLICY,
        HeaderValue::from_static("default-src 'none'; style-src 'unsafe-inline'; frame-ancestors 'none'; base-uri 'none'; form-action 'self'"),
    );
    h.insert(header::X_FRAME_OPTIONS, HeaderValue::from_static("DENY"));
    r
}

fn gone() -> Response {
    page(
        StatusCode::GONE,
        "Review not waiting",
        "<h1>This review isn't waiting any more</h1><p>It was posted, cancelled, or it ran out (they wait ten minutes). Send it again from the box.</p>",
    )
}

fn someone_elses() -> Response {
    page(
        StatusCode::FORBIDDEN,
        "Someone else's review",
        "<h1>This review is someone else's to post</h1><p>Only the person who wrote it on the box can post it. Sign in to Arugula as them, or send it again from the box.</p>",
    )
}

#[derive(Deserialize)]
pub struct PostQuery {
    id: Option<String>,
}

/// `GET /oauth/hud/post?id=`: the hub's page where the review's author
/// posts it, or doesn't.
pub async fn post_page(
    State(app): State<Arc<App>>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    uri: Uri,
    Query(q): Query<PostQuery>,
) -> Response {
    let Some(issuer) = &app.issuer else { return off() };
    if let Some(r) = limited(&app, peer, &headers) {
        return r;
    }
    let now = now_ms();
    let Some(id) = q.id.filter(|i| (1..=200).contains(&i.len())) else { return gone() };
    let Some(w) = issuer.box_posts.peek(&hash(&id), now) else { return gone() };
    let Some((_, account)) = crate::issuer::hub_session(&app, &headers, now) else {
        return crate::issuer::sign_in_first(&uri);
    };
    if account != w.author {
        return someone_elses();
    }
    let (b, t, x, id) = (escape(&w.box_name), escape(&w.target), escape(&w.text), escape(&id));
    page(
        StatusCode::OK,
        "Post your review?",
        &format!(
            r#"<h1>Post your review to Arugula?</h1>
<p>The box <b>{b}</b> asks to post this into the thread on <b>{t}</b>, as you, shown as posted via {b}:</p>
<pre data-box-post-text>{x}</pre>
<p class="dim">Check it's what you wrote. Nothing is posted unless you press Post.</p>
<form method="post" action="/oauth/hud/post"><input type="hidden" name="id" value="{id}">
<button type="submit" name="decision" value="post" data-box-post>Post</button><button type="submit" name="decision" value="cancel" data-box-cancel>Cancel</button></form>"#
        ),
    )
}

#[derive(Deserialize)]
pub struct PostAnswer {
    id: Option<String>,
    decision: Option<String>,
}

/// `POST /oauth/hud/post`: the author's answer, from the hub's own page.
pub async fn post_answer(
    State(app): State<Arc<App>>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Form(a): Form<PostAnswer>,
) -> Response {
    let Some(issuer) = &app.issuer else { return off() };
    if let Some(r) = limited(&app, peer, &headers) {
        return r;
    }
    if !own_page_form(&headers) {
        tracing::info!("a box review's answer from another page was refused");
        return page(
            StatusCode::FORBIDDEN,
            "Not from Arugula",
            "<h1>That answer didn't come from Arugula's own page</h1>",
        );
    }
    let now = now_ms();
    let Some(id) = a.id else { return gone() };
    let id_hash = hash(&id);
    let Some(w) = issuer.box_posts.peek(&id_hash, now) else { return gone() };
    // Only the author's own session answers it, and only then is it used.
    match crate::issuer::hub_session(&app, &headers, now) {
        Some((_, account)) if account == w.author => {}
        _ => return someone_elses(),
    }
    let Some(w) = issuer.box_posts.take(&id_hash, now) else { return gone() };
    if a.decision.as_deref() != Some("post") {
        return page(StatusCode::OK, "Not posted", "<h1>Not posted</h1><p>You can close this tab.</p>");
    }
    let post = BoxPost {
        project: w.project,
        via: w.box_name.clone(),
        target: w.target.clone(),
        author: w.author,
        text: w.text,
    };
    match app.platform.box_post(&post).await {
        Ok(BoxPosted::Posted { .. }) => {
            tracing::info!(client = w.client, "a person posted a hud box's review");
            page(
                StatusCode::OK,
                "Posted",
                &format!(
                    "<h1>Posted</h1><p>Your review is in the thread on <b>{}</b>, via {}. You can close this tab.</p>",
                    escape(&w.target),
                    escape(&w.box_name)
                ),
            )
        }
        Ok(BoxPosted::Refused { status, error: why }) => page(
            StatusCode::from_u16(status).unwrap_or(StatusCode::FORBIDDEN),
            "Not posted",
            &format!("<h1>Not posted</h1><p>{}</p>", escape(&why)),
        ),
        Err(e) => {
            tracing::warn!("a hud box's review wasn't posted: {e:#}");
            page(
                StatusCode::BAD_GATEWAY,
                "Not posted",
                "<h1>Not posted</h1><p>That didn't work: send it again from the box.</p>",
            )
        }
    }
}
