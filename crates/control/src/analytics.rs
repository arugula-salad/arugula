//! Product analytics: what people and their machines do with control,
//! sent to PostHog. Off unless `POSTHOG_API_KEY` is set, so a control
//! someone else runs never reports to anyone.
//!
//! Events are about control's own metadata: sign-ins, devices, joins,
//! teams, invites, billing, hosted sandboxes, cloud runs, relay sockets
//! and the requests people make. A person is their account id (no name,
//! login or address), a team is a `team` group, and nothing carries an IP
//! (`$geoip_disable`). Control never sees terminal bytes, so no event can
//! hold any.
//!
//! The client, its queue and its batches are [`Analytics`]
//! (`arugula_control_common::analytics`), which the platform uses too;
//! here is what control notes about each request.

use std::sync::Arc;

use axum::{
    extract::{MatchedPath, Request, State},
    middleware::Next,
    response::Response,
};
use serde_json::json;

pub use arugula_control_common::analytics::Analytics;

use crate::App;

/// Who asked, as the request's handler found out (a session's account):
/// put in the request's extensions for [`requests`] to read afterwards.
#[derive(Clone, Default)]
pub struct Asker(Arc<std::sync::OnceLock<String>>);

impl Asker {
    pub fn set(&self, account: &str) {
        let _ = self.0.set(account.to_owned());
    }
}

/// Every request a person makes that changes something, and every one
/// that fails: `control request`, by its route (not its path, which can
/// hold codes), method, status and time taken. Reads that succeed are
/// left out (pages poll), and so are daemons' own requests, which have
/// events of their own where they matter.
pub async fn requests(State(app): State<Arc<App>>, mut req: Request, next: Next) -> Response {
    if !app.analytics.on() {
        return next.run(req).await;
    }
    let asker = Asker::default();
    req.extensions_mut().insert(asker.clone());
    let route = req.extensions().get::<MatchedPath>().map(|p| p.as_str().to_owned());
    let method = req.method().clone();
    let started = std::time::Instant::now();
    let res = next.run(req).await;
    let status = res.status().as_u16();
    let (Some(route), Some(account)) = (route, asker.0.get()) else { return res };
    if method == axum::http::Method::GET && status < 400 {
        return res;
    }
    // The page's own usage reports and the answer about them (M104): the
    // answer decides what's sent about the page, so neither is a request
    // to report (a report sends `page day` itself).
    if route == arugula_control_wire::telemetry::PAGE || route == arugula_control_wire::telemetry::ME_TELEMETRY {
        return res;
    }
    app.analytics.capture(
        account,
        "control request",
        json!({
            "route": route, "method": method.as_str(), "status": status,
            "ok": status < 400, "ms": started.elapsed().as_millis() as u64,
        }),
    );
    res
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use serde_json::Value;
    use tokio::sync::mpsc;

    /// A fake PostHog: where it listens, and the batches it gets.
    pub(crate) async fn fake_posthog() -> (String, mpsc::UnboundedReceiver<Value>) {
        use axum::{Json, Router, routing::post};
        let (got_tx, got) = mpsc::unbounded_channel::<Value>();
        let fake = Router::new().route(
            "/batch/",
            post(move |Json(b): Json<Value>| {
                let got_tx = got_tx.clone();
                async move {
                    let _ = got_tx.send(b);
                    "{}"
                }
            }),
        );
        let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let at = l.local_addr().unwrap();
        tokio::spawn(async move { axum::serve(l, fake).await });
        (format!("http://{at}/"), got)
    }

    /// A person's change goes as `control request` by its route, with the
    /// handler's own event; their reads that succeed don't.
    #[tokio::test]
    async fn requests_by_route() {
        let (host, mut got) = fake_posthog().await;
        let mut app = App::for_tests("http://control.test");
        app.analytics = Analytics::start("phc_test".into(), &host, reqwest::Client::new());
        let now = arugula_e2e::now_ms();
        let account = app.db.account_for("github", "1", "someone", "acct1", now).unwrap();
        app.db.add_session(&crate::auth::hash("tok"), &account, now, now + 60_000).unwrap();
        let app = app.start();
        let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let at = l.local_addr().unwrap();
        let r = crate::router(app.clone()).into_make_service_with_connect_info::<std::net::SocketAddr>();
        tokio::spawn(async move { axum::serve(l, r).await });

        let http = reqwest::Client::new();
        let cookie = format!("{}=tok", crate::auth::SESSION_COOKIE);
        let me = http.get(format!("http://{at}/api/me")).header("cookie", &cookie).send().await.unwrap();
        assert!(me.status().is_success());
        let named = http
            .post(format!("http://{at}/api/me/name"))
            .header("cookie", &cookie)
            .header("origin", "http://control.test")
            .json(&json!({ "name": "Someone" }))
            .send()
            .await
            .unwrap();
        assert!(named.status().is_success());
        app.analytics.flush().await;
        let b = got.recv().await.unwrap();
        let events: Vec<&str> = b["batch"].as_array().unwrap().iter().map(|e| e["event"].as_str().unwrap()).collect();
        assert_eq!(events, ["name changed", "control request"]);
        let req = &b["batch"][1];
        assert_eq!(req["distinct_id"], "acct1");
        assert_eq!(req["properties"]["route"], "/api/me/name");
        assert_eq!(req["properties"]["method"], "POST");
        assert_eq!(req["properties"]["status"], 200);
    }
}
