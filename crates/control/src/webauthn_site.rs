//! Related origins for control's passkeys (S46, DK5): `GET
//! /.well-known/webauthn`.
//!
//! A passkey's relying party is the host of the site it was made on
//! (`passkey::rp_id`, aru-078), never the platform page's. A page on another
//! origin may still ask for one of those passkeys (WebAuthn's Related Origin
//! Requests): the browser fetches `https://<rp>/.well-known/webauthn` itself,
//! with no cookie and no Origin, and goes on only if the file lists the
//! page's origin. So this is public JSON on every site of control's, before
//! the page's fallback, and the moved-site redirect (`moved`) never touches
//! `/.well-known/`. On the platform page's own host it's a 404, as all of
//! the hub's are there (`platform_host::split`): the file belongs to the
//! relying party.
//!
//! It lists the platform page's origin (`--platform-host`, the same origin
//! `/control.json` gives as `platform`). Control has one platform host for
//! all its sites, so every site lists that one origin. Without one there's
//! no file. It does nothing until a page on that origin asks for a passkey,
//! and the hub still checks each assertion's origin itself (`passkey.rs`).

use std::sync::Arc;

use axum::{
    Json,
    extract::State,
    http::{HeaderValue, StatusCode, header},
    response::{IntoResponse, Response},
};

use crate::App;

pub const PATH: &str = "/.well-known/webauthn";

/// `GET /.well-known/webauthn`.
pub async fn related_origins(State(app): State<Arc<App>>) -> Response {
    let Some(ph) = &app.cfg.platform_host else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let mut r = Json(serde_json::json!({ "origins": [ph.origin] })).into_response();
    // Browsers fetch it for each ceremony that needs it; five minutes is
    // short enough that a change of platform host shows soon.
    r.headers_mut().insert(header::CACHE_CONTROL, HeaderValue::from_static("public, max-age=300"));
    r
}

#[cfg(test)]
mod tests {
    use std::net::SocketAddr;

    use super::*;

    const CONTROL: &str = "http://control.test";

    async fn serve(platform: Option<&str>, also: &[&str]) -> String {
        let mut app = App::for_tests(CONTROL);
        for a in also {
            app.cfg.sites.push(crate::Site::new(a).unwrap());
        }
        app.cfg.also_redirect = !also.is_empty();
        app.cfg.platform_host = platform
            .map(|h| crate::platform_host::parse(h, CONTROL, &app.cfg.sites, None).unwrap().unwrap());
        let app = app.start();
        let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}", l.local_addr().unwrap());
        let svc = crate::router(app).into_make_service_with_connect_info::<SocketAddr>();
        tokio::spawn(async move { axum::serve(l, svc).await.unwrap() });
        base
    }

    fn http() -> reqwest::Client {
        reqwest::Client::builder().redirect(reqwest::redirect::Policy::none()).build().unwrap()
    }

    /// The file lists the platform page's origin, as JSON, to a request
    /// with no session, on control's own site and on one it's moving from
    /// (whose pages redirect): the browser follows no redirect for it.
    #[tokio::test]
    async fn lists_the_platform_origin_without_a_session() {
        let base = serve(Some("app.test"), &["http://old.test"]).await;
        for host in ["control.test", "old.test"] {
            let r = http().get(format!("{base}{PATH}")).header(header::HOST, host).send().await.unwrap();
            assert_eq!(r.status(), 200, "{host}");
            assert_eq!(r.headers()[header::CONTENT_TYPE], "application/json", "{host}");
            assert_eq!(r.headers()[header::CACHE_CONTROL], "public, max-age=300", "{host}");
            assert!(r.headers().get(header::SET_COOKIE).is_none(), "{host}");
            let body: serde_json::Value = r.json().await.unwrap();
            assert_eq!(body, serde_json::json!({ "origins": ["http://app.test"] }), "{host}");
        }
        // A page on the old site does redirect: the file is the exception.
        let r = http().get(format!("{base}/")).header(header::HOST, "old.test").send().await.unwrap();
        assert_eq!(r.status(), 308);
        // Not on the platform page's own host: the file is the RP's.
        let r = http().get(format!("{base}{PATH}")).header(header::HOST, "app.test").send().await.unwrap();
        assert_eq!(r.status(), 404);
    }

    /// Without a platform host there's no file (and not the page's fallback).
    #[tokio::test]
    async fn without_a_platform_host_there_is_none() {
        let base = serve(None, &[]).await;
        let r = http().get(format!("{base}{PATH}")).send().await.unwrap();
        assert_eq!(r.status(), 404);
    }
}
