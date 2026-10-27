//! Linking GitHub to an account that's signed in (#208), and unlinking it.
//!
//! Sign-in's GitHub callback finds or makes the account holding a GitHub
//! id; a link adds that id to the account already signed in instead,
//! through the same callback (as a send does, `forge_send`):
//! 1. Sign-in and account's Link GitHub goes to `/auth/github?link=1`.
//!    [`start`] keeps who asked (the account and this sign-in's row in
//!    `sessions`, by the cookie alone) under a random state,
//!    `link.<nonce>`, for ten minutes, and goes to GitHub's account picker.
//! 2. GitHub comes back with a code and that state. [`finish`] takes the
//!    state once, and only for the same sign-in in the same browser: a
//!    code and state from someone else's browser attach nothing, either
//!    way round. The code is used up and its token revoked all the same.
//! 3. It reads the GitHub user, revokes the token, and links its numeric
//!    id (never the login) to the account: unless another account signs in
//!    with it, or this one has another GitHub account linked. Nothing is
//!    merged, and the session stays as it was: no new one, no other
//!    account.
//! 4. The browser goes back to the page with `#github-link=<outcome>`,
//!    which opens Sign-in and account to say what happened.
//!
//! The desktop app's window can't take a callback (its navigation would be
//! read as a sign-in), so it links through GitHub's device flow (#235):
//! [`device_start`] asks GitHub for a code and keeps its `device_code`
//! here, for the account and the session that asked; the window shows the
//! user code, and [`device_poll`] asks GitHub once per call, and on a token
//! links through the same [`attach`] as step 3.

use std::{collections::HashMap, sync::Arc, sync::Mutex};

use arugula_e2e::now_ms;
use axum::{
    Json,
    extract::{Path, State},
    http::{HeaderMap, StatusCode, header},
    response::{IntoResponse, Redirect, Response},
};
use serde::Deserialize;
use serde_json::{Value, json};
use tracing::{info, warn};

use crate::{
    ApiError, App, Github,
    auth::{Callback, GithubUser, PageSession, SESSION_COOKIE, cookie, hash, token},
    db::GithubLink,
    err,
};

/// What a link's state starts with, so the callback knows it from a
/// sign-in's.
pub const STATE_PREFIX: &str = "link.";

/// How long a link may take.
const START_MS: u64 = 10 * 60 * 1000;

/// The most under way at once.
const MOST: usize = 10_000;

/// A link under way: whose, from which sign-in, back to where.
struct Start {
    account: String,
    /// The hash of the session cookie's token that started it.
    session: String,
    next: String,
    redirect_uri: String,
    until: u64,
}

/// Links under way, by their state's nonce: in memory, as a hub restart
/// drops them (the person presses Link GitHub again).
#[derive(Default)]
pub struct Starts(Mutex<HashMap<String, Start>>);

impl Starts {
    fn put(&self, nonce: String, s: Start) -> Result<(), &'static str> {
        let mut m = self.0.lock().unwrap();
        let now = now_ms();
        m.retain(|_, s| s.until > now);
        if m.len() >= MOST {
            return Err("too many links under way: try again in a minute");
        }
        m.insert(nonce, s);
        Ok(())
    }

    /// Once: a state is gone after it's taken.
    fn take(&self, nonce: &str) -> Option<Start> {
        let s = self.0.lock().unwrap().remove(nonce)?;
        (s.until > now_ms()).then_some(s)
    }
}

/// What became of a link: the page's `#github-link=` says it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Outcome {
    Linked,
    Taken,
    HasOther,
    /// Back to another sign-in or browser than the one that started it.
    Elsewhere,
    /// Used already, or too old.
    Expired,
    Declined,
    Failed,
}

impl Outcome {
    fn as_str(self) -> &'static str {
        match self {
            Outcome::Linked => "linked",
            Outcome::Taken => "taken",
            Outcome::HasOther => "has-other",
            Outcome::Elsewhere => "elsewhere",
            Outcome::Expired => "expired",
            Outcome::Declined => "declined",
            Outcome::Failed => "failed",
        }
    }
}

/// The session cookie's row and its account, by the cookie alone (not a
/// CLI's signature).
fn cookie_session(app: &App, headers: &HeaderMap) -> Option<(String, String)> {
    let h = hash(cookie(headers, SESSION_COOKIE)?);
    let account = app.db.session(&h, now_ms()).ok().flatten()?;
    Some((h, account))
}

/// `/auth/github?link=1`: GitHub's account picker, for a link to the
/// session's account (module docs, step 1).
pub fn start(app: &App, headers: &HeaderMap, gh: &Github, next: String, redirect_uri: String) -> Response {
    let Some((session, account)) = cookie_session(app, headers) else {
        return (StatusCode::UNAUTHORIZED, "Sign in to Arugula control first, then link GitHub.").into_response();
    };
    let nonce = token();
    let s = Start {
        account: account.clone(),
        session,
        next,
        redirect_uri: redirect_uri.clone(),
        until: now_ms() + START_MS,
    };
    if let Err(why) = app.github_links.put(nonce.clone(), s) {
        return (StatusCode::SERVICE_UNAVAILABLE, why).into_response();
    }
    let state = format!("{STATE_PREFIX}{nonce}");
    let url = url::Url::parse_with_params(
        &format!("{}/login/oauth/authorize", gh.url),
        &[
            ("client_id", gh.client_id.as_str()),
            ("redirect_uri", &redirect_uri),
            ("state", &state),
            ("prompt", "select_account"),
        ],
    )
    .unwrap();
    info!(%account, "linking GitHub started");
    Redirect::to(url.as_str()).into_response()
}

/// Back to the page, saying what happened.
fn back(next: &str, o: Outcome) -> Response {
    let path = next.split('#').next().unwrap_or("/");
    Redirect::to(&format!("{path}#github-link={}", o.as_str())).into_response()
}

/// The callback, for a link's state: module docs, steps 2 to 4.
pub async fn finish(app: &App, headers: &HeaderMap, q: Callback) -> Response {
    let Some(gh) = &app.cfg.github else {
        return (StatusCode::NOT_FOUND, "GitHub sign-in isn't configured here").into_response();
    };
    let nonce = q.state.as_deref().and_then(|s| s.strip_prefix(STATE_PREFIX)).unwrap_or("");
    let taken = app.github_links.take(nonce);
    let mine = taken.as_ref().filter(|s| {
        cookie_session(app, headers).is_some_and(|(session, account)| session == s.session && account == s.account)
    });
    let Some(s) = mine else {
        // Not this sign-in's link (someone sent the link, or it's old):
        // the code is used up all the same, and its token goes at once.
        if let Some(code) = &q.code {
            let uri =
                taken.as_ref().map_or_else(|| crate::auth::callback_url(app, headers), |s| s.redirect_uri.clone());
            if let Ok(t) = crate::auth::exchange(app, gh, code, &uri).await {
                crate::forge_send::revoke(app, gh, &t).await;
            }
        }
        let o = if taken.is_some() { Outcome::Elsewhere } else { Outcome::Expired };
        warn!(outcome = o.as_str(), "a GitHub link came back to no link of this sign-in's");
        return back(taken.as_ref().map_or("/", |s| s.next.as_str()), o);
    };
    let code = match (&q.error, &q.code) {
        (None, Some(code)) => code,
        _ => return back(&s.next, Outcome::Declined),
    };
    let user = match crate::auth::exchange(app, gh, code, &s.redirect_uri).await {
        Ok(t) => {
            let u = crate::auth::github_user(app, gh, &t).await;
            crate::forge_send::revoke(app, gh, &t).await;
            u
        }
        Err(e) => Err(e),
    };
    let o = attach(app, &s.account, user);
    back(&s.next, o)
}

/// Link the GitHub user (or why it couldn't be read) to `account`: what
/// the callback and the device flow share (#235).
fn attach(app: &App, account: &str, user: Result<GithubUser, String>) -> Outcome {
    let user = match user {
        Ok(u) => u,
        Err(why) => {
            warn!(account, %why, "linking GitHub failed");
            return Outcome::Failed;
        }
    };
    let o = match app.db.link_github(account, user.id, &user.login) {
        Ok(GithubLink::Linked | GithubLink::Already) => Outcome::Linked,
        Ok(GithubLink::Taken) => Outcome::Taken,
        Ok(GithubLink::HasOther) => Outcome::HasOther,
        Err(e) => {
            warn!(account, error = %e, "linking GitHub failed");
            Outcome::Failed
        }
    };
    info!(account, outcome = o.as_str(), "linking GitHub came back");
    if o == Outcome::Linked {
        app.analytics.capture(account, "github linked", json!({}));
    }
    o
}

/// A device flow under way (#235): whose, from which sign-in.
struct Device {
    account: String,
    /// The hash of the session cookie's token that started it.
    session: String,
    device_code: String,
    until: u64,
}

/// Device flows under way, by a random id the window polls with: in
/// memory, like [`Starts`]. GitHub's `device_code` never leaves the hub.
#[derive(Default)]
pub struct Devices(Mutex<HashMap<String, Device>>);

impl Devices {
    fn put(&self, id: String, d: Device) -> Result<(), &'static str> {
        let mut m = self.0.lock().unwrap();
        let now = now_ms();
        m.retain(|_, d| d.until > now);
        if m.len() >= MOST {
            return Err("too many links under way: try again in a minute");
        }
        m.insert(id, d);
        Ok(())
    }

    /// The code of `id`'s flow, only for the sign-in that started it: to
    /// anyone else it isn't there.
    fn code_for(&self, id: &str, s: &PageSession) -> Option<String> {
        let m = self.0.lock().unwrap();
        let d = m.get(id).filter(|d| d.until > now_ms())?;
        (d.session == s.token_hash && d.account == s.account).then(|| d.device_code.clone())
    }

    /// Once: a flow is gone after it's ended.
    fn end(&self, id: &str) {
        self.0.lock().unwrap().remove(id);
    }
}

#[derive(Deserialize)]
struct DeviceCode {
    device_code: Option<String>,
    user_code: Option<String>,
    verification_uri: Option<String>,
    expires_in: Option<u64>,
    interval: Option<u64>,
    error: Option<String>,
}

#[derive(Deserialize)]
struct DeviceToken {
    access_token: Option<String>,
    error: Option<String>,
    interval: Option<u64>,
}

/// GitHub's answer to a form POST, as JSON.
async fn github_form<T: serde::de::DeserializeOwned>(
    app: &App,
    url: String,
    pairs: &[(&str, &str)],
) -> Result<T, ApiError> {
    let form: String = url::form_urlencoded::Serializer::new(String::new()).extend_pairs(pairs).finish();
    let said = |e: reqwest::Error| {
        warn!(error = %e.without_url(), "GitHub's device flow");
        err(StatusCode::BAD_GATEWAY, "GitHub didn't answer: try again")
    };
    app.http
        .post(url)
        .header(header::ACCEPT, "application/json")
        .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
        .body(form)
        .send()
        .await
        .map_err(said)?
        .json()
        .await
        .map_err(said)
}

/// `POST /api/me/github/device`: GitHub's device code, for a link to the
/// session's account. The window shows `user_code` and polls with `id`.
pub async fn device_start(State(app): State<Arc<App>>, s: PageSession) -> Result<Json<Value>, ApiError> {
    let Some(gh) = &app.cfg.github else {
        return Err(err(StatusCode::NOT_FOUND, "GitHub sign-in isn't configured here"));
    };
    let c: DeviceCode =
        github_form(&app, format!("{}/login/device/code", gh.url), &[("client_id", &gh.client_id)]).await?;
    if c.error.as_deref() == Some("device_flow_disabled") {
        return Ok(Json(json!({ "state": "disabled" })));
    }
    let (Some(device_code), Some(user_code), Some(verification_uri)) = (c.device_code, c.user_code, c.verification_uri)
    else {
        warn!(error = c.error, "GitHub's device code came without a code");
        return Err(err(StatusCode::BAD_GATEWAY, "GitHub didn't answer: try again"));
    };
    let expires_in = c.expires_in.unwrap_or(900);
    let interval = c.interval.unwrap_or(5);
    let id = token();
    let d =
        Device { account: s.account.clone(), session: s.token_hash, device_code, until: now_ms() + expires_in * 1000 };
    app.github_devices.put(id.clone(), d).map_err(|why| err(StatusCode::SERVICE_UNAVAILABLE, why))?;
    info!(account = s.account, "linking GitHub by device code started");
    Ok(Json(json!({
        "id": id,
        "user_code": user_code,
        "verification_uri": verification_uri,
        "interval": interval,
        "expires_in": expires_in,
    })))
}

/// `POST /api/me/github/device/{id}`: one poll of GitHub. A state the
/// window keeps polling on (`pending`, `slow_down`), or its last word:
/// `expired`, `declined`, `disabled`, or the link's [`Outcome`].
pub async fn device_poll(
    State(app): State<Arc<App>>,
    s: PageSession,
    Path(id): Path<String>,
) -> Result<Json<Value>, ApiError> {
    let Some(gh) = &app.cfg.github else {
        return Err(err(StatusCode::NOT_FOUND, "GitHub sign-in isn't configured here"));
    };
    let Some(device_code) = app.github_devices.code_for(&id, &s) else {
        return Err(err(StatusCode::NOT_FOUND, "no GitHub link is under way here: start it again"));
    };
    let t: DeviceToken = github_form(
        &app,
        format!("{}/login/oauth/access_token", gh.url),
        &[
            ("client_id", &gh.client_id),
            ("device_code", &device_code),
            ("grant_type", "urn:ietf:params:oauth:grant-type:device_code"),
        ],
    )
    .await?;
    if let Some(access) = t.access_token {
        // Once: whatever comes of the link, this flow is over.
        app.github_devices.end(&id);
        let user = crate::auth::github_user(&app, gh, &access).await;
        crate::forge_send::revoke(&app, gh, &access).await;
        let o = attach(&app, &s.account, user);
        return Ok(Json(json!({ "state": o.as_str() })));
    }
    let state = match t.error.as_deref() {
        Some("authorization_pending") => return Ok(Json(json!({ "state": "pending" }))),
        Some("slow_down") => {
            return Ok(Json(json!({ "state": "slow_down", "interval": t.interval.unwrap_or(10) })));
        }
        Some("expired_token") => "expired",
        Some("access_denied") => "declined",
        Some("device_flow_disabled") => "disabled",
        other => {
            warn!(error = other, "GitHub's device code poll");
            Outcome::Failed.as_str()
        }
    };
    app.github_devices.end(&id);
    Ok(Json(json!({ "state": state })))
}

/// `POST /api/me/github/unlink`: never the last way to sign in.
pub async fn unlink(State(app): State<Arc<App>>, s: PageSession) -> Result<Json<Value>, ApiError> {
    s.not_app(&app)?;
    match app.db.unlink_github(&s.account)? {
        Ok(()) => {
            info!(account = s.account, "GitHub unlinked");
            app.analytics.capture(&s.account, "github unlinked", json!({}));
            Ok(Json(json!({ "ways": app.db.sign_ins(&s.account)? })))
        }
        Err(why @ "no GitHub account is linked") => Err(err(StatusCode::NOT_FOUND, why)),
        Err(why) => Err(err(StatusCode::CONFLICT, why)),
    }
}

#[cfg(test)]
mod tests {
    use std::net::SocketAddr;

    use axum::{
        Router,
        http::header,
        routing::{delete, get, post},
    };

    use super::*;

    /// What the fake GitHub saw: codes swapped and tokens revoked.
    #[derive(Default)]
    struct Gh {
        swapped: Vec<String>,
        revoked: usize,
        /// GitHub's answer to a device poll, until the person approves:
        /// an `error`, or none for a token for user `device_user`.
        device_error: Option<&'static str>,
        device_user: u64,
    }

    type Shared = Arc<Mutex<Gh>>;

    /// A code `c<id>` is GitHub user `<id>`, whose login is `gh<id>`.
    async fn fake_github(gh: Shared) -> String {
        let r = Router::new()
            .route(
                "/login/oauth/access_token",
                post(|State(g): State<Shared>, body: String| async move {
                    if body.contains("device_code=") {
                        assert!(body.contains("grant_type=urn%3Aietf%3Aparams%3Aoauth%3Agrant-type%3Adevice_code"));
                        assert!(body.contains("device_code=dc_secret"));
                        let g = g.lock().unwrap();
                        return Json(match g.device_error {
                            Some(e) => json!({ "error": e, "interval": 10 }),
                            None => json!({ "access_token": format!("ghu_{}", g.device_user) }),
                        });
                    }
                    let id = body.split('&').find_map(|kv| kv.strip_prefix("code=c")).unwrap_or("0").to_owned();
                    g.lock().unwrap().swapped.push(body);
                    Json(json!({ "access_token": format!("ghu_{id}"), "token_type": "bearer" }))
                }),
            )
            .route(
                "/login/device/code",
                post(|body: String| async move {
                    assert_eq!(body, "client_id=cid");
                    Json(json!({
                        "device_code": "dc_secret",
                        "user_code": "WDJB-MJHT",
                        "verification_uri": "https://github.com/login/device",
                        "expires_in": 900,
                        "interval": 5,
                    }))
                }),
            )
            .route(
                "/user",
                get(|h: HeaderMap| async move {
                    let id: u64 = h[header::AUTHORIZATION]
                        .to_str()
                        .unwrap()
                        .strip_prefix("Bearer ghu_")
                        .unwrap()
                        .parse()
                        .unwrap();
                    Json(json!({ "id": id, "login": format!("gh{id}") }))
                }),
            )
            .route(
                "/applications/cid/token",
                delete(|State(g): State<Shared>| async move {
                    g.lock().unwrap().revoked += 1;
                    StatusCode::NO_CONTENT
                }),
            )
            .with_state(gh);
        let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}", l.local_addr().unwrap());
        tokio::spawn(async move { axum::serve(l, r).await.unwrap() });
        base
    }

    struct Rig {
        app: Arc<App>,
        base: String,
        gh: Shared,
        http: reqwest::Client,
    }

    /// Control signing in with a fake GitHub: `pk1`, an account with a
    /// passkey and no GitHub, and `gh1`, one that signs in with GitHub 8.
    async fn rig() -> Rig {
        let gh: Shared = Arc::default();
        let api = fake_github(gh.clone()).await;
        let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}", l.local_addr().unwrap());
        let mut app = App::for_tests(&base);
        app.cfg.github = Some(Github { client_id: "cid".into(), client_secret: "s".into(), url: api.clone(), api });
        let app = app.start();
        app.db.account_for("passkey", "cred1", "", "pk1", now_ms()).unwrap();
        app.db.add_passkey("cred1", "pk1", -7, b"key", now_ms()).unwrap();
        app.db.account_for("github", "8", "gh8", "gh1", now_ms()).unwrap();
        let svc = crate::router(app.clone()).into_make_service_with_connect_info::<SocketAddr>();
        tokio::spawn(async move { axum::serve(l, svc).await.unwrap() });
        let http = reqwest::Client::builder().redirect(reqwest::redirect::Policy::none()).build().unwrap();
        Rig { app, base, gh, http }
    }

    fn sets_session(r: &reqwest::Response) -> bool {
        r.headers()
            .get_all(header::SET_COOKIE)
            .iter()
            .any(|v| v.to_str().unwrap().starts_with(&format!("{SESSION_COOKIE}=")))
    }

    impl Rig {
        fn session(&self, account: &str) -> String {
            let t = token();
            self.app.db.add_session(&hash(&t), account, now_ms(), now_ms() + 3_600_000).unwrap();
            format!("{SESSION_COOKIE}={t}")
        }

        /// Link GitHub, from `cookie`'s sign-in: the state GitHub gets.
        async fn start(&self, cookie: &str) -> String {
            let r = self
                .http
                .get(format!("{}/auth/github?link=1&next=/here", self.base))
                .header(header::COOKIE, cookie)
                .send()
                .await
                .unwrap();
            assert_eq!(r.status(), StatusCode::SEE_OTHER);
            assert!(!sets_session(&r));
            let u = url::Url::parse(r.headers()[header::LOCATION].to_str().unwrap()).unwrap();
            let q: HashMap<String, String> = u.query_pairs().into_owned().collect();
            assert_eq!(q["prompt"], "select_account");
            assert!(q["state"].starts_with(STATE_PREFIX));
            q["state"].clone()
        }

        /// GitHub coming back with user `id`'s code: where the page goes.
        async fn back(&self, cookie: &str, id: u64, state: &str) -> String {
            let r = self
                .http
                .get(format!("{}/auth/github/callback?code=c{id}&state={state}", self.base))
                .header(header::COOKIE, cookie)
                .send()
                .await
                .unwrap();
            assert_eq!(r.status(), StatusCode::SEE_OTHER);
            assert!(!sets_session(&r), "a link never signs anyone in");
            r.headers()[header::LOCATION].to_str().unwrap().to_owned()
        }

        fn github_of(&self, account: &str) -> Option<String> {
            self.app.db.github_identity(account).unwrap().map(|(id, _)| id)
        }

        /// POST `path` from `cookie`'s sign-in: the status and the body.
        async fn post(&self, cookie: &str, path: &str) -> (StatusCode, Value) {
            let r = self
                .http
                .post(format!("{}{path}", self.base))
                .header(header::COOKIE, cookie)
                .header(header::ORIGIN, &self.base)
                .send()
                .await
                .unwrap();
            let status = r.status();
            (status, r.json().await.unwrap_or(Value::Null))
        }

        async fn unlink(&self, cookie: &str) -> StatusCode {
            self.http
                .post(format!("{}/api/me/github/unlink", self.base))
                .header(header::COOKIE, cookie)
                .header(header::ORIGIN, &self.base)
                .send()
                .await
                .unwrap()
                .status()
        }
    }

    /// A passkey account links GitHub 7 from its sign-in: the session
    /// stays its own, the token is revoked, and signing in with GitHub 7
    /// afterwards lands on that same account. Linking it again is a no-op;
    /// another GitHub account while one is linked is refused.
    #[tokio::test]
    async fn a_link_attaches_to_the_signed_in_account() {
        let r = rig().await;
        let cookie = r.session("pk1");
        let state = r.start(&cookie).await;
        assert_eq!(r.back(&cookie, 7, &state).await, "/here#github-link=linked");
        assert_eq!(r.github_of("pk1").as_deref(), Some("7"));
        assert_eq!(r.gh.lock().unwrap().revoked, 1);
        // The same state again is used up.
        assert_eq!(r.back(&cookie, 7, &state).await, "/#github-link=expired");

        let state = r.start(&cookie).await;
        assert_eq!(r.back(&cookie, 7, &state).await, "/here#github-link=linked");
        let state = r.start(&cookie).await;
        assert_eq!(r.back(&cookie, 9, &state).await, "/here#github-link=has-other");
        assert_eq!(r.github_of("pk1").as_deref(), Some("7"));
        assert_eq!(r.app.db.github_account(9).unwrap(), None);

        // A plain GitHub sign-in as 7, in a browser signed in to nothing.
        let s = r.http.get(format!("{}/auth/github", r.base)).send().await.unwrap();
        let kept = s.headers()[header::SET_COOKIE].to_str().unwrap().split(';').next().unwrap().to_owned();
        let st = kept.split_once('=').unwrap().1.split_once('.').unwrap().0.to_owned();
        let s = r
            .http
            .get(format!("{}/auth/github/callback?code=c7&state={st}", r.base))
            .header(header::COOKIE, &kept)
            .send()
            .await
            .unwrap();
        let t = s
            .headers()
            .get_all(header::SET_COOKIE)
            .iter()
            .find_map(|v| {
                v.to_str()
                    .unwrap()
                    .strip_prefix(&format!("{SESSION_COOKIE}="))
                    .map(|v| v.split(';').next().unwrap().to_owned())
            })
            .expect("signed in");
        assert_eq!(r.app.db.session(&hash(&t), now_ms()).unwrap().as_deref(), Some("pk1"));
    }

    /// GitHub 8 signs in to gh1: pk1 linking it is refused, and nothing
    /// changes. A link's code and state that come back to another sign-in
    /// (someone sent the link) attach nothing to either account, and the
    /// code is used up all the same.
    #[tokio::test]
    async fn a_link_elsewhere_or_of_anothers_github_changes_nothing() {
        let r = rig().await;
        let cookie = r.session("pk1");
        let state = r.start(&cookie).await;
        assert_eq!(r.back(&cookie, 8, &state).await, "/here#github-link=taken");
        assert_eq!(r.github_of("pk1"), None);
        assert_eq!(r.app.db.github_account(8).unwrap().as_deref(), Some("gh1"));

        // pk1's link, its code for GitHub 7 opened in gh1's browser, and
        // in another of pk1's own sign-ins.
        for other in [r.session("gh1"), r.session("pk1"), String::new()] {
            let state = r.start(&cookie).await;
            assert_eq!(r.back(&other, 7, &state).await, "/here#github-link=elsewhere");
            // Taken once even so: the right sign-in can't use it after.
            assert_eq!(r.back(&cookie, 7, &state).await, "/#github-link=expired");
        }
        assert_eq!(r.github_of("pk1"), None);
        assert_eq!(r.github_of("gh1").as_deref(), Some("8"));
        assert_eq!(r.app.db.github_account(7).unwrap(), None);
        let g = r.gh.lock().unwrap();
        assert_eq!(g.swapped.iter().filter(|b| b.contains("code=c7")).count(), 6, "every code used up");
        assert_eq!(g.revoked, 7);
    }

    /// Unlinking GitHub is refused when it's the account's only way in,
    /// and otherwise removes it.
    #[tokio::test]
    async fn unlink_is_refused_when_its_the_last_way_in() {
        let r = rig().await;
        assert_eq!(r.unlink(&r.session("gh1")).await, StatusCode::CONFLICT);
        assert_eq!(r.github_of("gh1").as_deref(), Some("8"));

        let cookie = r.session("pk1");
        assert_eq!(r.unlink(&cookie).await, StatusCode::NOT_FOUND);
        let state = r.start(&cookie).await;
        assert_eq!(r.back(&cookie, 7, &state).await, "/here#github-link=linked");
        assert_eq!(r.unlink(&cookie).await, StatusCode::OK);
        assert_eq!(r.github_of("pk1"), None);
        assert_eq!(r.app.db.github_account(7).unwrap(), None);
    }

    /// The desktop window links GitHub 7 by device code (#235): the user
    /// code is shown, a poll before approval is pending, then the link
    /// lands on the session's account with the token revoked. Another
    /// sign-in, even of the same account, can't poll it; neither can a
    /// finished flow be polled again.
    #[tokio::test]
    async fn a_device_link_attaches_to_the_session_that_started_it() {
        let r = rig().await;
        let cookie = r.session("pk1");
        let (st, d) = r.post(&cookie, "/api/me/github/device").await;
        assert_eq!(st, StatusCode::OK);
        assert_eq!(d["user_code"], "WDJB-MJHT");
        assert_eq!(d["verification_uri"], "https://github.com/login/device");
        assert_eq!(d["interval"], 5);
        assert!(!d.to_string().contains("dc_secret"), "GitHub's device code stays in the hub");
        let poll = format!("/api/me/github/device/{}", d["id"].as_str().unwrap());

        // Another sign-in of the same account, another account, and none.
        for other in [r.session("pk1"), r.session("gh1"), String::new()] {
            assert_ne!(r.post(&other, &poll).await.0, StatusCode::OK);
        }
        assert_eq!(r.post(&r.session("pk1"), &poll).await.0, StatusCode::NOT_FOUND);

        r.gh.lock().unwrap().device_error = Some("authorization_pending");
        assert_eq!(r.post(&cookie, &poll).await, (StatusCode::OK, json!({ "state": "pending" })));
        r.gh.lock().unwrap().device_error = Some("slow_down");
        assert_eq!(r.post(&cookie, &poll).await, (StatusCode::OK, json!({ "state": "slow_down", "interval": 10 })));
        assert_eq!(r.github_of("pk1"), None);

        {
            let mut g = r.gh.lock().unwrap();
            g.device_error = None;
            g.device_user = 7;
        }
        assert_eq!(r.post(&cookie, &poll).await, (StatusCode::OK, json!({ "state": "linked" })));
        assert_eq!(r.github_of("pk1").as_deref(), Some("7"));
        assert_eq!(r.gh.lock().unwrap().revoked, 1);
        assert_eq!(r.post(&cookie, &poll).await.0, StatusCode::NOT_FOUND, "a finished flow is gone");

        // Another GitHub account while one is linked: the callback's refusal.
        let (_, d) = r.post(&cookie, "/api/me/github/device").await;
        r.gh.lock().unwrap().device_user = 9;
        let poll = format!("/api/me/github/device/{}", d["id"].as_str().unwrap());
        assert_eq!(r.post(&cookie, &poll).await.1, json!({ "state": "has-other" }));
        assert_eq!(r.github_of("pk1").as_deref(), Some("7"));
    }

    /// GitHub's last words on a device code, each ending the flow.
    #[tokio::test]
    async fn a_device_flow_ends_on_declined_expired_or_disabled() {
        let r = rig().await;
        let cookie = r.session("pk1");
        for (gh, state) in
            [("access_denied", "declined"), ("expired_token", "expired"), ("device_flow_disabled", "disabled")]
        {
            let (_, d) = r.post(&cookie, "/api/me/github/device").await;
            let poll = format!("/api/me/github/device/{}", d["id"].as_str().unwrap());
            r.gh.lock().unwrap().device_error = Some(gh);
            assert_eq!(r.post(&cookie, &poll).await.1, json!({ "state": state }));
            assert_eq!(r.post(&cookie, &poll).await.0, StatusCode::NOT_FOUND);
        }
        assert_eq!(r.github_of("pk1"), None);
    }
}
