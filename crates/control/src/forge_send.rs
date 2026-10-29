//! A person's send through GitHub's authorization of the App (M168, #1214):
//! the hub's half of the one-use user token ("Sending as a person uses a
//! one-use user token, falling back to their machine"; the platform's half
//! is `arugula_platform::forge::send`).
//!
//! The hub owns the App's OAuth client ("The hub stays the identity
//! provider"), and the App's one callback URL is the hub's
//! `/auth/github/callback`, which sign-in uses too. So a send needs no
//! other URL registered with GitHub:
//! 1. The person presses Send on the platform page, which asks the
//!    platform (`POST /api/forge/drafts/{id}/send`, `via: "browser"`). The
//!    platform checks the draft is theirs to send and asks here
//!    ([`authorize`], `HubPort::send_authorization`): this keeps what the
//!    send is for (the account, the draft, the page's path to come back
//!    to) under a random state, `send.<nonce>`, for ten minutes, and
//!    answers GitHub's authorization page.
//! 2. GitHub comes back (silently, once the person has authorized the App,
//!    which signing in with GitHub did) to the callback with a code and
//!    that state. [`finish`] takes the state once: a replayed or old one is
//!    refused. It must be the same account's session on control's site.
//! 3. It swaps the code for a user token, hands it to the platform over the
//!    signed internal API for the one call (`PlatformPort::send_draft`),
//!    then revokes it at GitHub (`DELETE /applications/{client_id}/token`)
//!    and drops it. The token is never stored, logged or put in a URL.
//! 4. If the person declined or the swap failed, the platform tries one of
//!    their machines instead (`send_draft` with no token).
//! 5. The browser goes back to the platform page's path with
//!    `draft_send=<draft id>`, and the page shows what became of it from
//!    the draft itself (sent and its URL, or why not).

use std::{collections::HashMap, sync::Mutex};

use arugula_control_wire::internal::UserToken;
use arugula_e2e::now_ms;
use arugula_platform_api::PlatformPort as _;
use axum::{
    http::{HeaderMap, StatusCode, header},
    response::{IntoResponse, Redirect, Response},
};
use tracing::{info, warn};

use crate::{
    App, Github,
    auth::{Callback, SESSION_COOKIE, cookie, hash, token},
};

/// What a send's state starts with, so the callback knows it from a
/// sign-in's.
pub const STATE_PREFIX: &str = "send.";

/// How long a send's authorization may take.
const START_MS: u64 = 10 * 60 * 1000;

/// The most under way at once.
const MOST: usize = 10_000;

/// A send under way: what it's for, until when.
struct Start {
    account: String,
    draft: String,
    back: String,
    redirect_uri: String,
    until: u64,
}

/// Sends whose authorization is under way, by their state's nonce: in
/// memory, as a hub restart drops them (the person presses Send again).
#[derive(Default)]
pub struct Starts(Mutex<HashMap<String, Start>>);

impl Starts {
    fn put(&self, nonce: String, s: Start) -> anyhow::Result<()> {
        let mut m = self.0.lock().unwrap();
        let now = now_ms();
        m.retain(|_, s| s.until > now);
        if m.len() >= MOST {
            anyhow::bail!("too many sends under way: try again in a minute");
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

/// `HubPort::send_authorization`: GitHub's authorization page for a send
/// of `draft` by `account`, coming back to the platform page's `back`.
pub fn authorize(app: &App, account: &str, draft: &str, back: &str) -> anyhow::Result<String> {
    let Some(gh) = &app.cfg.github_app_client else {
        anyhow::bail!("this control has no GitHub App client (GITHUB_APP_CLIENT_ID)");
    };
    let nonce = token();
    // The App's callback on control's own site, which GitHub knows.
    let redirect_uri = format!("{}/auth/github/callback", app.cfg.public_url);
    let start = Start {
        account: account.to_owned(),
        draft: draft.to_owned(),
        back: back.to_owned(),
        redirect_uri: redirect_uri.clone(),
        until: now_ms() + START_MS,
    };
    app.sends.put(nonce.clone(), start)?;
    let state = format!("{STATE_PREFIX}{nonce}");
    let url = url::Url::parse_with_params(
        &format!("{}/login/oauth/authorize", gh.url),
        &[("client_id", gh.client_id.as_str()), ("redirect_uri", &redirect_uri), ("state", &state)],
    )?;
    info!(%account, %draft, "a send's GitHub authorization started");
    Ok(url.into())
}

/// The account of the session on control's site, if there is one.
fn session_account(app: &App, headers: &HeaderMap) -> Option<String> {
    let t = cookie(headers, SESSION_COOKIE)?;
    app.db.session(&hash(t), now_ms()).ok().flatten()
}

/// Where the browser goes back to: the platform page's path, with the
/// draft to show.
fn back_to(app: &App, s: &Start) -> String {
    let origin = crate::app_signin::platform_origin(app).unwrap_or_else(|| app.cfg.public_url.clone());
    let sep = if s.back.contains('?') { '&' } else { '?' };
    format!("{origin}{}{sep}draft_send={}", s.back, s.draft)
}

/// The callback, for a send's state: module docs, steps 2 to 5.
pub async fn finish(app: &App, headers: &HeaderMap, q: Callback) -> Response {
    let nonce = q.state.as_deref().and_then(|s| s.strip_prefix(STATE_PREFIX)).unwrap_or("");
    let Some(s) = app.sends.take(nonce) else {
        warn!("a send's GitHub authorization came back used or too old");
        return (StatusCode::BAD_REQUEST, "This send was used already or took too long: press Send again.")
            .into_response();
    };
    if session_account(app, headers).as_deref() != Some(s.account.as_str()) {
        warn!(account = s.account, draft = s.draft, "a send's GitHub authorization came back to another session");
        return (StatusCode::FORBIDDEN, "Sign in to Arugula control in this browser, then press Send again.")
            .into_response();
    }
    let Some(gh) = &app.cfg.github_app_client else {
        return (StatusCode::NOT_FOUND, "this control has no GitHub App client").into_response();
    };
    let token = match (&q.error, &q.code) {
        (None, Some(code)) => crate::auth::exchange(app, gh, code, &s.redirect_uri).await,
        (Some(e), _) => Err(e.clone()),
        (None, None) => Err("no code".into()),
    };
    let outcome = match token {
        Ok(t) => {
            let o = app.platform.send_draft(&s.account, &s.draft, Some(UserToken::new(t.clone()))).await;
            revoke(app, gh, &t).await;
            o
        }
        Err(why) => {
            info!(account = s.account, draft = s.draft, why, "no user token for a send: trying a machine");
            app.platform.send_draft(&s.account, &s.draft, None).await
        }
    };
    match outcome {
        Ok(o) => match (&o.sent, &o.refused) {
            (Some(sent), _) => {
                info!(account = s.account, draft = s.draft, via = sent.via, "a send came back from GitHub")
            }
            (None, why) => info!(account = s.account, draft = s.draft, why, "a send came back from GitHub, not sent"),
        },
        Err(e) => warn!(account = s.account, draft = s.draft, error = format!("{e:#}"), "the platform didn't send it"),
    }
    Redirect::to(&back_to(app, &s)).into_response()
}

/// Revoke a user token at GitHub, with the App's client: best effort (it
/// expires anyway), and never logged.
pub(crate) async fn revoke(app: &App, gh: &Github, t: &str) {
    let r = app
        .http
        .delete(format!("{}/applications/{}/token", gh.api, gh.client_id))
        .basic_auth(&gh.client_id, Some(&gh.client_secret))
        .header(header::ACCEPT, "application/vnd.github+json")
        .header(header::USER_AGENT, "arugula-control")
        .json(&serde_json::json!({ "access_token": t }))
        .send()
        .await;
    match r {
        Ok(r) if r.status().is_success() => {}
        Ok(r) => warn!(status = %r.status(), "revoking a send's user token"),
        Err(e) => warn!(error = %e.without_url(), "revoking a send's user token"),
    }
}

#[cfg(test)]
mod tests {
    use std::{
        net::SocketAddr,
        sync::{Arc, Mutex},
    };

    use arugula_e2e::now_ms;
    use arugula_platform::{
        db::forge::{Installation, LinkedRepo},
        forge::{GithubApp, parse_pem},
    };
    use axum::{
        Json, Router,
        extract::State,
        http::{HeaderMap, StatusCode},
        routing::{delete, get, post},
    };
    use serde_json::{Value, json};

    use super::*;

    const TOKEN: &str = "ghu_one_use_for_the_send";

    /// What the fake GitHub saw: codes swapped, comments (with their
    /// `Authorization`), and revocations (with theirs, and the body).
    #[derive(Default)]
    struct Gh {
        swapped: Vec<String>,
        comments: Vec<(String, Value)>,
        revoked: Vec<(String, Value)>,
    }

    type Shared = Arc<Mutex<Gh>>;

    fn auth(h: &HeaderMap) -> String {
        h.get("authorization").and_then(|v| v.to_str().ok()).unwrap_or_default().to_owned()
    }

    async fn fake_github(gh: Shared) -> String {
        let r = Router::new()
            .route(
                "/login/oauth/access_token",
                post(|State(g): State<Shared>, body: String| async move {
                    g.lock().unwrap().swapped.push(body);
                    Json(json!({ "access_token": TOKEN, "token_type": "bearer" }))
                }),
            )
            .route(
                "/user",
                get(|h: HeaderMap| async move {
                    let ok = auth(&h) == format!("Bearer {TOKEN}");
                    if ok { Ok(Json(json!({ "id": 1001, "login": "jake-gh" }))) } else { Err(StatusCode::UNAUTHORIZED) }
                }),
            )
            .route(
                "/repos/acme/app/issues/1/comments",
                post(|State(g): State<Shared>, h: HeaderMap, Json(b): Json<Value>| async move {
                    g.lock().unwrap().comments.push((auth(&h), b));
                    Json(json!({ "html_url": "https://github.com/acme/app/pull/1#issuecomment-9" }))
                }),
            )
            .route(
                "/applications/cid/token",
                delete(|State(g): State<Shared>, h: HeaderMap, Json(b): Json<Value>| async move {
                    g.lock().unwrap().revoked.push((auth(&h), b));
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
        draft: String,
        http: reqwest::Client,
    }

    /// Control with the App's client and the platform's App pointed at a
    /// fake GitHub; a1 signed in with GitHub (1001), and a draft of theirs
    /// on `acme/app` (public, linked to their own project).
    async fn rig() -> Rig {
        let gh: Shared = Arc::default();
        let api = fake_github(gh.clone()).await;
        let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}", l.local_addr().unwrap());
        let mut app = App::for_tests(&base);
        app.cfg.github_app_client = Some(Github {
            client_id: "cid".into(),
            client_secret: "csecret".into(),
            url: api.clone(),
            api: api.clone(),
        });
        let (pem, _) = crate::forge::tests::throwaway_key();
        app.platform.inner_mut().github =
            Some(GithubApp::new("42".into(), "arugula-test".into(), "s".into(), &api, parse_pem(&pem).unwrap()));
        let app = app.start();
        app.db.account_for("github", "1001", "jake-gh", "a1", now_ms()).unwrap();
        let p = app.platform.inner();
        let project = p.db.personal_project("a1", now_ms()).unwrap();
        let repo = LinkedRepo {
            repo_id: 77,
            full_name: "acme/app".into(),
            private: false,
            project_id: project,
            installation_id: 9,
            linked_by: Some("a1".into()),
            linked_at: 1,
            unlinked_at: None,
            purge_after: None,
            mirror_content: false,
        };
        let install = Installation {
            id: 9,
            account_id: 500,
            login: "acme".into(),
            account_type: "Organization".into(),
            permissions: "{}".into(),
        };
        p.db.link_repo(&repo, &install, "a1", now_ms()).unwrap();
        let svc = crate::router(app.clone()).into_make_service_with_connect_info::<SocketAddr>();
        tokio::spawn(async move { axum::serve(l, svc).await.unwrap() });
        let http = reqwest::Client::builder().redirect(reqwest::redirect::Policy::none()).build().unwrap();
        let mut r = Rig { app, base, gh, draft: String::new(), http };
        // a1's own draft, made as a person makes one.
        let v: Value = r
            .http
            .post(format!("{}/api/forge/drafts", r.base))
            .header(header::COOKIE, r.session("a1"))
            .header(header::ORIGIN, &r.base)
            .json(&json!({ "repo": "acme/app", "number": 1, "kind": "comment", "body": "ship it" }))
            .send()
            .await
            .unwrap()
            .json()
            .await
            .unwrap();
        r.draft = v["draft"]["id"].as_str().expect("a draft").to_owned();
        r
    }

    impl Rig {
        fn session(&self, account: &str) -> String {
            let t = token();
            self.app.db.add_session(&hash(&t), account, now_ms(), now_ms() + 3_600_000).unwrap();
            format!("{SESSION_COOKIE}={t}")
        }

        /// GitHub's authorization page for a send: its state.
        fn start(&self) -> String {
            let url = authorize(&self.app, "a1", &self.draft, "/p/mine?tab=drafts").unwrap();
            let u = url::Url::parse(&url).unwrap();
            let q: HashMap<String, String> = u.query_pairs().into_owned().collect();
            assert_eq!(q["client_id"], "cid");
            assert_eq!(q["redirect_uri"], format!("{}/auth/github/callback", self.base));
            assert!(q["state"].starts_with(STATE_PREFIX));
            q["state"].clone()
        }

        /// GitHub coming back to the callback with `query`.
        async fn back(&self, cookie: &str, query: &str) -> reqwest::Response {
            self.http
                .get(format!("{}/auth/github/callback?{query}", self.base))
                .header(header::COOKIE, cookie)
                .send()
                .await
                .unwrap()
        }

        /// The draft's state, as its person reads it.
        async fn state_of(&self) -> String {
            let v: Value = self
                .http
                .get(format!("{}/api/forge/drafts/{}", self.base, self.draft))
                .header(header::COOKIE, self.session("a1"))
                .send()
                .await
                .unwrap()
                .json()
                .await
                .unwrap();
            v["state"].as_str().unwrap_or_default().to_owned()
        }
    }

    /// Authorize, the code swapped for a user token, the one call as the
    /// person, the token revoked with the App's client, and back to the
    /// platform page with the draft to show. Its state is good once.
    #[tokio::test]
    async fn a_send_through_githubs_authorization() {
        let r = rig().await;
        let cookie = r.session("a1");
        let state = r.start();
        let res = r.back(&cookie, &format!("code=c0de&state={state}")).await;
        assert_eq!(res.status(), StatusCode::SEE_OTHER);
        assert_eq!(res.headers()[header::LOCATION], format!("{}/p/mine?tab=drafts&draft_send={}", r.base, r.draft));
        assert_eq!(r.state_of().await, "sent");
        {
            let g = r.gh.lock().unwrap();
            assert_eq!(g.swapped.len(), 1);
            assert!(g.swapped[0].contains("code=c0de") && g.swapped[0].contains("client_id=cid"), "{:?}", g.swapped);
            assert_eq!(g.comments, [(format!("Bearer {TOKEN}"), json!({ "body": "ship it" }))]);
            let basic =
                format!("Basic {}", base64::Engine::encode(&base64::engine::general_purpose::STANDARD, "cid:csecret"));
            assert_eq!(g.revoked, [(basic, json!({ "access_token": TOKEN }))]);
        }
        // The same state again is refused, and nothing more happens.
        let res = r.back(&cookie, &format!("code=c0de&state={state}")).await;
        assert_eq!(res.status(), StatusCode::BAD_REQUEST);
        assert_eq!(r.gh.lock().unwrap().swapped.len(), 1);
    }

    /// A state that's too old, one never made, or one that comes back to
    /// another person's session is refused before any code is swapped.
    #[tokio::test]
    async fn a_stale_or_strange_state_is_refused() {
        let r = rig().await;
        let cookie = r.session("a1");
        let state = r.start();
        let nonce = state.strip_prefix(STATE_PREFIX).unwrap();
        r.app.sends.0.lock().unwrap().get_mut(nonce).unwrap().until = now_ms() - 1;
        assert_eq!(r.back(&cookie, &format!("code=c&state={state}")).await.status(), StatusCode::BAD_REQUEST);
        assert_eq!(r.back(&cookie, "code=c&state=send.made-up").await.status(), StatusCode::BAD_REQUEST);
        let state = r.start();
        let other = r.session("someone-else");
        assert_eq!(r.back(&other, &format!("code=c&state={state}")).await.status(), StatusCode::FORBIDDEN);
        // Taken once even so: the right session can't use it after.
        assert_eq!(r.back(&cookie, &format!("code=c&state={state}")).await.status(), StatusCode::BAD_REQUEST);
        assert!(r.gh.lock().unwrap().swapped.is_empty());
        assert_eq!(r.state_of().await, "waiting");
    }

    /// The person declined (or GitHub failed): the platform tries their
    /// machines, finds none, and the draft waits; back to the page.
    #[tokio::test]
    async fn a_declined_authorization_falls_back_and_keeps_the_draft() {
        let r = rig().await;
        let cookie = r.session("a1");
        let state = r.start();
        let res = r.back(&cookie, &format!("error=access_denied&state={state}")).await;
        assert_eq!(res.status(), StatusCode::SEE_OTHER);
        assert!(res.headers()[header::LOCATION].to_str().unwrap().ends_with(&format!("draft_send={}", r.draft)));
        assert_eq!(r.state_of().await, "waiting");
        assert!(r.gh.lock().unwrap().comments.is_empty());
    }
}
