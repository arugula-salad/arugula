//! Shared Docs (#1064): control keeps a Doc's snapshot as the daemon sent
//! it, encrypted in parts, and serves it to anyone with the link. The key
//! is in the link's fragment, which a browser never sends, so control
//! can't read what it holds and never parses a part.
//!
//! - A machine pushes with its signature (`PUT` a part, `POST` the commit,
//!   `DELETE` the link, `GET` its quota). A link belongs to the machine that
//!   made it: anyone else's call about it is a 404, which doesn't say the id
//!   exists.
//! - The account's people list and unshare its links with a session.
//! - The viewer ([`serve`]) answers anonymous `GET`s of a page, a link's
//!   `meta` and its parts, on `docs.<block domain>` (`blocks::Host::Docs`)
//!   or, with no block domain, under `/d/` on control's own origin. It
//!   answers nothing else, under one strict CSP, and sets no cookie.
//!
//! Every limit is checked before a part is written (`db/doc_links.rs`), and
//! the disk guard first of all.

use std::{
    collections::HashMap,
    net::SocketAddr,
    path::PathBuf,
    sync::{Arc, Mutex},
    time::Duration,
};

use arugula_e2e::now_ms;
use axum::{
    Json,
    body::{Body, Bytes},
    extract::{FromRequestParts, Path, State},
    http::{HeaderMap, HeaderValue, Method, StatusCode, header, request::Parts},
    response::{IntoResponse, Response},
};
use serde::Deserialize;
use serde_json::{Value, json};
use tracing::warn;

use crate::{
    ApiError, App,
    auth::{DaemonAuth, MAX_SIGNED_BODY, Session, auth_header},
    db::doc_links::{DocErr, MAX_ACCOUNT, MAX_LINK, MAX_LINKS, MAX_PARTS, Maker},
    err, refusal,
};

type R = Result<Json<Value>, ApiError>;

/// Control refuses new parts while its disk is fuller than this, in percent.
const DISK_FULL: u64 = 80;
/// A version nobody committed is swept after this.
pub const SWEEP_AFTER_MS: u64 = 24 * 3600 * 1000;
/// Opens counted in memory reach the database this often.
pub const FLUSH_EVERY: Duration = Duration::from_secs(60);

/// What the viewer's pages are served under (#1064, decision 5), with the
/// Doc's own fonts from the build (DECISIONS.md "A shared Doc's page loads
/// its fonts from its own origin").
const CSP: &str = "default-src 'none'; script-src 'self'; style-src 'self' 'unsafe-inline'; img-src data:; \
                   font-src 'self'; connect-src 'self'; worker-src 'none'; base-uri 'none'; form-action 'none'; \
                   frame-ancestors 'none'";

/// Reads of the viewer's data, per client address and hour. A 48-part Doc
/// is about 50 requests, so this is a couple of hundred opens an hour from
/// one address (an office behind one NAT, say); the limiter keeps a
/// timestamp for each, so it can't be much higher.
pub const VIEWS: (&str, usize) = ("doc-view", 12_000);

/// Control's side of shared Docs that isn't in the database.
#[derive(Default)]
pub struct Docs {
    /// A directory on the database's filesystem, for the disk guard.
    pub dir: Option<PathBuf>,
    /// Tests: what the disk looks like, as `(used, total)`.
    #[cfg(test)]
    pub fake_disk: Mutex<Option<(u64, u64)>>,
    /// Page opens since the last flush, by link. Rough on purpose: a
    /// restart forgets up to a minute, and preview bots count.
    opens: Mutex<HashMap<String, u64>>,
}

impl Docs {
    pub fn at(dir: PathBuf) -> Self {
        Docs { dir: Some(dir), ..Default::default() }
    }

    /// `(used, total)` bytes of the filesystem the database is on.
    pub fn disk(&self) -> Option<(u64, u64)> {
        #[cfg(test)]
        if let Some(d) = *self.fake_disk.lock().unwrap() {
            return Some(d);
        }
        use std::os::unix::ffi::OsStrExt as _;
        let dir = std::ffi::CString::new(self.dir.as_ref()?.as_os_str().as_bytes()).ok()?;
        // SAFETY: a NUL-terminated path and a pointer to a local, which
        // `statvfs` fills in.
        let s = unsafe {
            let mut s: libc::statvfs = std::mem::zeroed();
            if libc::statvfs(dir.as_ptr(), &mut s) != 0 {
                return None;
            }
            s
        };
        // The types are 32 bits on macOS and 64 on Linux.
        #[allow(clippy::unnecessary_cast)]
        let (frsize, blocks, free) = (s.f_frsize as u64, s.f_blocks as u64, s.f_bfree as u64);
        Some((blocks.saturating_sub(free) * frsize, blocks * frsize))
    }

    /// Whether the disk is too full for new parts. A disk we can't read
    /// isn't.
    fn full(&self) -> bool {
        self.disk().is_some_and(|(used, total)| used.saturating_mul(100) > total.saturating_mul(DISK_FULL))
    }

    fn bump(&self, id: &str) {
        *self.opens.lock().unwrap().entry(id.to_owned()).or_default() += 1;
    }

    fn waiting(&self, id: &str) -> u64 {
        self.opens.lock().unwrap().get(id).copied().unwrap_or(0)
    }

    /// Write the counts to the database. A failed write keeps them for the
    /// next try.
    pub fn flush(&self, db: &crate::db::Db) {
        let taken = std::mem::take(&mut *self.opens.lock().unwrap());
        if taken.is_empty() {
            return;
        }
        if let Err(e) = db.add_doc_opens(&taken) {
            warn!(error = %e, "flushing Doc link opens");
            let mut m = self.opens.lock().unwrap();
            for (id, n) in taken {
                *m.entry(id).or_default() += n;
            }
        }
    }
}

/// A link id: 128 random bits, as 32 lower-case hex digits.
fn valid_id(id: &str) -> bool {
    id.len() == 32 && id.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
}

fn id_of(id: &str) -> Result<(), ApiError> {
    if valid_id(id) { Ok(()) } else { Err(err(StatusCode::BAD_REQUEST, "a link id is 32 lower-case hex digits")) }
}

impl From<DocErr> for ApiError {
    fn from(e: DocErr) -> Self {
        let mb = |n: u64| n >> 20;
        match e {
            // Nothing says whether the id exists.
            DocErr::Gone => err(StatusCode::NOT_FOUND, "no such link"),
            DocErr::Stale => refusal(StatusCode::CONFLICT, "stale", "a newer version is already shared"),
            DocErr::TooBig => refusal(
                StatusCode::PAYLOAD_TOO_LARGE,
                "too_big",
                &format!("a shared Doc is at most {} MB once encrypted", mb(MAX_LINK)),
            ),
            DocErr::Quota => refusal(
                StatusCode::PAYLOAD_TOO_LARGE,
                "quota",
                &format!("an account may share {} MB of Docs in all: unshare one first", mb(MAX_ACCOUNT)),
            ),
            DocErr::Links => refusal(
                StatusCode::PAYLOAD_TOO_LARGE,
                "links",
                &format!("an account may have {MAX_LINKS} shared Docs: unshare one first"),
            ),
            DocErr::Incomplete(why) => refusal(StatusCode::CONFLICT, "incomplete", &why),
            DocErr::Db(e) => e.into(),
        }
    }
}

// ---------------------------------------------------------------- machines

/// `GET /api/doc-links/quota`: what the account holds and may hold, for a
/// machine to check before it uploads anything.
pub async fn quota(State(app): State<Arc<App>>, d: DaemonAuth) -> R {
    let u = app.db.doc_use(&d.cert.account)?;
    Ok(Json(json!({
        "used": u.used, "limit": MAX_ACCOUNT, "links": u.links, "max_links": MAX_LINKS, "max_link": MAX_LINK,
    })))
}

/// `PUT /api/doc-links/{id}/v/{ver}/parts/{n}`: one part, raw bytes.
pub async fn put_part(
    State(app): State<Arc<App>>,
    d: DaemonAuth,
    Path((id, ver, n)): Path<(String, u64, u32)>,
    body: Bytes,
) -> R {
    id_of(&id)?;
    if ver == 0 || n >= MAX_PARTS {
        return Err(err(
            StatusCode::BAD_REQUEST,
            &format!("a version is 1 or more, and a part 0 to {}", MAX_PARTS - 1),
        ));
    }
    // Before anything is stored: the disk, then this part's own size, then
    // the link's and the account's totals (in the database's transaction).
    if app.docs.full() {
        return Err(refusal(
            StatusCode::INSUFFICIENT_STORAGE,
            "disk",
            &format!("control's disk is over {DISK_FULL}% full: it takes no new shared Docs until there is room"),
        ));
    }
    if body.len() > MAX_SIGNED_BODY {
        return Err(refusal(StatusCode::PAYLOAD_TOO_LARGE, "part", "a part is at most 1 MB"));
    }
    if body.is_empty() {
        return Err(err(StatusCode::BAD_REQUEST, "a part isn't empty"));
    }
    app.db.put_doc_part(&id, Maker { account: &d.cert.account, daemon: &d.cert.device }, ver, n, &body, now_ms())?;
    Ok(Json(json!({ "stored": body.len() })))
}

#[derive(Deserialize)]
pub struct Commit {
    parts: u32,
    size: u64,
}

/// `POST /api/doc-links/{id}/v/{ver}/commit`: serve `ver`.
pub async fn commit(
    State(app): State<Arc<App>>,
    d: DaemonAuth,
    Path((id, ver)): Path<(String, u64)>,
    Json(c): Json<Commit>,
) -> R {
    id_of(&id)?;
    if ver == 0 || c.parts == 0 || c.parts > MAX_PARTS {
        return Err(err(StatusCode::BAD_REQUEST, &format!("a version is 1 or more, and 1 to {MAX_PARTS} parts")));
    }
    app.db.commit_doc_link(
        &id,
        Maker { account: &d.cert.account, daemon: &d.cert.device },
        ver,
        c.parts,
        c.size,
        now_ms(),
    )?;
    Ok(Json(json!({ "ver": ver, "parts": c.parts, "size": c.size })))
}

// ------------------------------------------------- machines and sessions

/// Who is asking: a machine by its signature, or a person.
pub enum Caller {
    Daemon { account: String, daemon: String },
    Person { account: String },
}

impl FromRequestParts<Arc<App>> for Caller {
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, app: &Arc<App>) -> Result<Self, ApiError> {
        if auth_header(&parts.headers).is_some()
            && let Ok(d) = DaemonAuth::from_request_parts(parts, app).await
        {
            return Ok(Caller::Daemon { account: d.cert.account, daemon: d.cert.device });
        }
        // Not a machine: a person, whose cookie (or CLI signature) it is,
        // or the refusal for a signature that's bad.
        Session::from_request_parts(parts, app).await.map(|s| Caller::Person { account: s.account })
    }
}

/// `DELETE /api/doc-links/{id}`: unshare. A machine may unshare what it
/// made; a person, any link of their account.
pub async fn remove(State(app): State<Arc<App>>, who: Caller, Path(id): Path<String>) -> R {
    id_of(&id)?;
    match &who {
        Caller::Daemon { account, daemon } => app.db.delete_doc_link(&id, account, Some(daemon))?,
        Caller::Person { account } => app.db.delete_doc_link(&id, account, None)?,
    }
    Ok(Json(json!({})))
}

// ---------------------------------------------------------------- people

/// `GET /api/doc-links`: the account's shared Docs.
pub async fn list(State(app): State<Arc<App>>, s: Session) -> R {
    let mut links = app.db.doc_links(&s.account)?;
    for l in &mut links {
        l.opens += app.docs.waiting(&l.id);
    }
    crate::reply(&links)
}

// ---------------------------------------------------------------- viewer

/// Where the viewer is, for `/control.json`: `docs.<block domain>` on
/// control's scheme and port, or `/d` under control's URL.
pub fn viewer_url(public_url: &str, block_domain: Option<&str>) -> String {
    let Some(d) = block_domain else { return format!("{public_url}/d") };
    match url::Url::parse(public_url) {
        Ok(u) => match u.port() {
            Some(p) => format!("{}://docs.{d}:{p}", u.scheme()),
            None => format!("{}://docs.{d}", u.scheme()),
        },
        Err(_) => format!("{public_url}/d"),
    }
}

/// Control's origin, with a block domain: `/d/…` isn't the viewer there.
pub async fn fallback(
    State(app): State<Arc<App>>,
    axum::extract::ConnectInfo(peer): axum::extract::ConnectInfo<SocketAddr>,
    method: Method,
    headers: HeaderMap,
    uri: axum::http::Uri,
    Path(rest): Path<String>,
) -> Response {
    if app.cfg.block_domain.is_some() {
        return text(StatusCode::NOT_FOUND, "not found");
    }
    serve(&app, &method, &headers, Some(peer), &format!("/{rest}"), uri.query())
}

fn text(status: StatusCode, msg: &'static str) -> Response {
    let mut r = (status, msg).into_response();
    secure(r.headers_mut());
    r
}

/// What every answer of the viewer carries, whatever it is.
fn secure(h: &mut HeaderMap) {
    h.insert(header::CONTENT_SECURITY_POLICY, HeaderValue::from_static(CSP));
    h.insert(header::REFERRER_POLICY, HeaderValue::from_static("no-referrer"));
    h.insert(header::X_CONTENT_TYPE_OPTIONS, HeaderValue::from_static("nosniff"));
    h.insert(header::X_FRAME_OPTIONS, HeaderValue::from_static("DENY"));
}

/// The viewer's whole surface, for a path relative to its root:
///
/// - `/<id>`: the page (it loads for any id, shared or not: the page says
///   so itself);
/// - `/<id>/data/meta`: `{ver, parts, size}` of the served version;
/// - `/<id>/data/<n>`: one ciphertext part of it;
/// - `/assets/…`: the page's built files.
///
/// Everything else is a 404. No answer sets a cookie (nothing here does,
/// and `blocks::split` strips one from any answer on the docs host).
pub fn serve(
    app: &App,
    method: &Method,
    headers: &HeaderMap,
    peer: Option<SocketAddr>,
    path: &str,
    query: Option<&str>,
) -> Response {
    let mut res = answer(app, method, headers, peer, path, query);
    secure(res.headers_mut());
    res
}

fn answer(
    app: &App,
    method: &Method,
    headers: &HeaderMap,
    peer: Option<SocketAddr>,
    path: &str,
    query: Option<&str>,
) -> Response {
    let not_found = || text(StatusCode::NOT_FOUND, "not found");
    if !matches!(*method, Method::GET | Method::HEAD) {
        return not_found();
    }
    let segs: Vec<&str> = path.trim_start_matches('/').split('/').collect();
    match segs.as_slice() {
        [id] if valid_id(id) => page(app, id),
        [id, "data", what] if valid_id(id) => {
            if let Some(peer) = peer
                && let Err(e) = app.limits.check(VIEWS, app.limits.client_ip(peer, headers))
            {
                return e.into_response();
            }
            // A part says which version it expects (`?v=`), so a page that
            // began on one version never mixes in another's.
            let want =
                query.and_then(|q| q.split('&').find_map(|kv| kv.strip_prefix("v="))).map(|v| v.parse::<u64>().ok());
            data(app, headers, id, what, want).unwrap_or_else(|e| {
                warn!(error = %e, "serving a Doc link");
                text(StatusCode::INTERNAL_SERVER_ERROR, "something went wrong")
            })
        }
        ["assets", rest @ ..] if !rest.is_empty() && rest.iter().all(|s| !s.is_empty() && !s.starts_with('.')) => {
            match crate::static_file(app, &format!("assets/{}", rest.join("/"))) {
                Some((body, mime)) => Response::builder()
                    .header(header::CONTENT_TYPE, mime)
                    .header(header::CACHE_CONTROL, "public, max-age=31536000, immutable")
                    .body(Body::from(body))
                    .unwrap(),
                None => not_found(),
            }
        }
        _ => not_found(),
    }
}

/// `doc.html` of the web build when there is one (#1066 adds it as a Vite
/// input beside `share.html`), else a page that says it isn't ready, with
/// the link preview's tags ([`preview`]) in its head.
fn page(app: &App, id: &str) -> Response {
    let html = match crate::static_file(app, "doc.html") {
        Some((b, _)) => String::from_utf8_lossy(&b).into_owned(),
        None => PLACEHOLDER.to_owned(),
    };
    let viewer = viewer_url(&app.cfg.public_url, app.cfg.block_domain.as_deref());
    let tags = preview(&viewer, id);
    let body = match html.find("</head>") {
        Some(at) => format!("{}{tags}{}", &html[..at], &html[at..]),
        None => format!("{tags}{html}"),
    };
    Response::builder()
        .header(header::CONTENT_TYPE, "text/html; charset=utf-8")
        .header(header::CACHE_CONTROL, "no-cache")
        .body(Body::from(body))
        .unwrap()
}

const PLACEHOLDER: &str = "<!doctype html><html lang=\"en\"><head><meta charset=\"utf-8\">\
<meta name=\"viewport\" content=\"width=device-width, initial-scale=1\"><title>Arugula</title>\
<style>body{font:16px system-ui,sans-serif;max-width:32rem;margin:20vh auto;padding:0 1rem;color:#222}\
@media(prefers-color-scheme:dark){body{background:#111;color:#ddd}}</style></head>\
<body><h1>A shared Doc</h1><p>This page can't show Docs yet.</p></body></html>";

/// What a link preview (Slack, iMessage, X) shows for any shared Doc.
/// Control can't read the Doc (the key is in the fragment, which no
/// request carries), so the title and description are the same for every
/// link; nothing here says whether the id is shared.
pub const PREVIEW_TITLE: &str = "A shared Doc";
pub const PREVIEW_TEXT: &str = "A Doc made with Arugula. It's encrypted: only its link opens it.";
/// The picture: the site's own card, one for every Doc. Only a preview bot
/// fetches it; the page never does, so the CSP doesn't name it.
pub const PREVIEW_CARD: &str = "https://arugula.io/img/og.jpg";
const PREVIEW_CARD_ALT: &str = "Your team and its agents, in one project: a map of one project holding chat, issues, \
                                GitHub, Cloud workspaces, runs and deploys, with its people and their agents.";

/// The page's Open Graph and Twitter tags, with absolute URLs.
fn preview(viewer: &str, id: &str) -> String {
    let attr = |s: &str| s.replace('&', "&amp;").replace('"', "&quot;").replace('<', "&lt;");
    let (title, text, url, card, alt) = (
        attr(PREVIEW_TITLE),
        attr(PREVIEW_TEXT),
        attr(&format!("{viewer}/{id}")),
        attr(PREVIEW_CARD),
        attr(PREVIEW_CARD_ALT),
    );
    format!(
        "<meta name=\"description\" content=\"{text}\">\
<meta property=\"og:type\" content=\"article\"><meta property=\"og:site_name\" content=\"Arugula\">\
<meta property=\"og:title\" content=\"{title}\"><meta property=\"og:description\" content=\"{text}\">\
<meta property=\"og:url\" content=\"{url}\"><meta property=\"og:image\" content=\"{card}\">\
<meta property=\"og:image:type\" content=\"image/jpeg\">\
<meta property=\"og:image:width\" content=\"1200\"><meta property=\"og:image:height\" content=\"630\">\
<meta property=\"og:image:alt\" content=\"{alt}\">\
<meta name=\"twitter:card\" content=\"summary_large_image\"><meta name=\"twitter:title\" content=\"{title}\">\
<meta name=\"twitter:description\" content=\"{text}\"><meta name=\"twitter:image\" content=\"{card}\">"
    )
}

/// A tag matches `If-None-Match`, weak or not, or it's `*`.
fn fresh(headers: &HeaderMap, tag: &str) -> bool {
    headers
        .get(header::IF_NONE_MATCH)
        .and_then(|v| v.to_str().ok())
        .is_some_and(|v| v.split(',').map(str::trim).any(|t| t == "*" || t.strip_prefix("W/").unwrap_or(t) == tag))
}

/// `want` is the `?v=` a part was asked with: `Some(None)` when it isn't a
/// number.
fn data(app: &App, headers: &HeaderMap, id: &str, what: &str, want: Option<Option<u64>>) -> anyhow::Result<Response> {
    let gone = || text(StatusCode::NOT_FOUND, "this isn't shared any more");
    let Some(s) = app.db.doc_served(id)? else { return Ok(gone()) };
    if what != "meta" && want.is_some_and(|v| v != Some(s.ver)) {
        return Ok(gone());
    }
    let meta = what == "meta";
    let n: u32 = if meta {
        0
    } else {
        match what.parse::<u32>() {
            Ok(n) if n < s.parts && what == n.to_string() => n,
            _ => return Ok(gone()),
        }
    };
    if meta {
        // One a page open, not one a part.
        app.docs.bump(id);
    }
    let tag = format!("\"{id}-{}-{}\"", s.ver, if meta { "meta" } else { what });
    let cache =
        |b: axum::http::response::Builder| b.header(header::ETAG, &tag).header(header::CACHE_CONTROL, "no-cache");
    if fresh(headers, &tag) {
        return Ok(cache(Response::builder().status(StatusCode::NOT_MODIFIED)).body(Body::empty())?);
    }
    let res = if meta {
        let body = serde_json::to_vec(&json!({ "ver": s.ver, "parts": s.parts, "size": s.size }))?;
        cache(Response::builder()).header(header::CONTENT_TYPE, "application/json").body(Body::from(body))?
    } else {
        let Some(part) = app.db.doc_part(id, s.ver, n)? else { return Ok(gone()) };
        cache(Response::builder()).header(header::CONTENT_TYPE, "application/octet-stream").body(Body::from(part))?
    };
    Ok(res)
}
