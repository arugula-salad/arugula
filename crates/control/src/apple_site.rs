//! The macOS app's passkeys (D2): `GET /.well-known/apple-app-site-association`.
//!
//! A WKWebView in an app other than a browser gets WebAuthn only for a
//! relying party the app is associated with: the app claims
//! `webcredentials:<host>` (its associated-domains entitlement,
//! crates/desktop/macos), and the host names the app back in this file.
//! macOS fetches it through Apple's CDN (macOS 11 and later), which wants
//! it over https, as JSON, with no redirect. So this answers on every site
//! of control's, before the page's fallback, and the moved-site redirect
//! (`moved`) never touches `/.well-known/`.
//!
//! The app is `<team>.io.arugula.desktop`: the Developer ID team that signs
//! it (`--apple-team-id`, `ARUGULA_CONTROL_APPLE_TEAM_ID`) and its bundle
//! identifier (crates/desktop/tauri.conf.json). Unset, there's no file and
//! the app's window offers no passkey (its init script hides the button
//! unless it carries the entitlement, `crates/desktop/src/cloud.rs`).

use std::sync::Arc;

use axum::{
    Json,
    extract::State,
    http::{HeaderValue, StatusCode, header},
    response::{IntoResponse, Response},
};

use crate::App;

pub const PATH: &str = "/.well-known/apple-app-site-association";

/// The desktop app's bundle identifier (crates/desktop/tauri.conf.json).
pub const BUNDLE_ID: &str = "io.arugula.desktop";

/// A Team ID as Apple gives it: ten upper-case letters and digits. Empty
/// is none.
pub fn team_id(s: &str) -> anyhow::Result<Option<String>> {
    let s = s.trim();
    if s.is_empty() {
        return Ok(None);
    }
    anyhow::ensure!(
        s.len() == 10 && s.bytes().all(|b| b.is_ascii_uppercase() || b.is_ascii_digit()),
        "ARUGULA_CONTROL_APPLE_TEAM_ID: an Apple Team ID is ten upper-case letters and digits, not {s:?}"
    );
    Ok(Some(s.to_owned()))
}

/// `GET /.well-known/apple-app-site-association`.
pub async fn association(State(app): State<Arc<App>>) -> Response {
    let Some(team) = &app.cfg.apple_team_id else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let mut r = Json(serde_json::json!({
        "webcredentials": { "apps": [format!("{team}.{BUNDLE_ID}")] },
    }))
    .into_response();
    // Apple's CDN fetches it within a day and devices about weekly: an hour
    // here costs nothing.
    r.headers_mut().insert(header::CACHE_CONTROL, HeaderValue::from_static("public, max-age=3600"));
    r
}

#[cfg(test)]
mod tests {
    use std::net::SocketAddr;

    use super::*;

    const CONTROL: &str = "http://control.test";

    async fn serve(team: Option<&str>, also: &[&str]) -> String {
        let mut app = App::for_tests(CONTROL);
        app.cfg.apple_team_id = team.map(Into::into);
        for a in also {
            app.cfg.sites.push(crate::Site::new(a).unwrap());
        }
        app.cfg.also_redirect = !also.is_empty();
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

    /// The file names the app by its Team ID and bundle identifier, as JSON,
    /// on control's own site and on a site it's moving from (whose pages
    /// redirect): Apple's CDN takes no redirect.
    #[tokio::test]
    async fn names_the_app_as_json_with_no_redirect() {
        let base = serve(Some("HNDPMQPF22"), &["http://old.test"]).await;
        for host in ["control.test", "old.test"] {
            let r = http().get(format!("{base}{PATH}")).header(header::HOST, host).send().await.unwrap();
            assert_eq!(r.status(), 200, "{host}");
            assert_eq!(r.headers()[header::CONTENT_TYPE], "application/json", "{host}");
            let body: serde_json::Value = r.json().await.unwrap();
            assert_eq!(body, serde_json::json!({ "webcredentials": { "apps": ["HNDPMQPF22.io.arugula.desktop"] } }));
        }
        // A page on the old site does redirect: the file is the exception.
        let r = http().get(format!("{base}/")).header(header::HOST, "old.test").send().await.unwrap();
        assert_eq!(r.status(), 308);
    }

    /// Without a Team ID there's no file (and not the page's fallback).
    #[tokio::test]
    async fn without_a_team_id_there_is_none() {
        let base = serve(None, &[]).await;
        let r = http().get(format!("{base}{PATH}")).send().await.unwrap();
        assert_eq!(r.status(), 404);
    }

    #[test]
    fn a_team_id_is_apples_shape() {
        assert_eq!(team_id("").unwrap(), None);
        assert_eq!(team_id(" HNDPMQPF22 ").unwrap().as_deref(), Some("HNDPMQPF22"));
        for bad in ["hndpmqpf22", "HNDPMQPF2", "HNDPMQPF22X", "HNDPMQPF2.", "HNDPMQPF22.io.arugula.desktop"] {
            assert!(team_id(bad).is_err(), "{bad}");
        }
    }
}
