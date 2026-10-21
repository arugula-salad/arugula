//! `POST /github/webhook`: the App's events (M40), the platform's since
//! M161. Each is checked against `X-Hub-Signature-256` (HMAC-SHA256 of the
//! body with the App's webhook secret, compared in constant time) and
//! deduplicated by `X-GitHub-Delivery` in `github_webhook_deliveries`, kept
//! 7 days, so a redelivery is known after a deploy. What's kept of it is a
//! *poke*: `{provider: github, host: github.com, repo, number?, event,
//! delivery}`, one per pull request or issue it names (check runs, suites
//! and workflow runs through their `pull_requests`; a status, or a run on a
//! fork's PR, is repo-wide: no number). The hub relays the pokes to the
//! daemons watching the repository (`HubPort::forge_pokes`) until M174.
//! Nothing else of the payload goes anywhere, and only the event, delivery,
//! repository and count are logged.

use std::sync::Arc;

use arugula_control_common::err;
use arugula_control_wire::forge::Poke;
use arugula_e2e::now_ms;
use axum::{
    Json,
    body::Bytes,
    extract::State,
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
};
use serde_json::{Value, json};
use sha2::Sha256;
use tracing::{info, warn};

use crate::Platform;

type HmacSha256 = hmac::Hmac<Sha256>;

/// `X-Hub-Signature-256: sha256=<hex>` over the body, in constant time.
pub fn signed(secret: &str, header: Option<&str>, body: &[u8]) -> bool {
    use hmac::{KeyInit, Mac};
    let Some(hex_sig) = header.and_then(|h| h.trim().strip_prefix("sha256=")) else { return false };
    let Ok(sig) = hex::decode(hex_sig) else { return false };
    if secret.is_empty() {
        return false;
    }
    let mut mac = HmacSha256::new_from_slice(secret.as_bytes()).expect("hmac");
    mac.update(body);
    mac.verify_slice(&sig).is_ok()
}

/// The pokes an event makes: one per pull request or issue it names, or
/// one for the whole repository.
pub fn pokes(event: &str, delivery: &str, v: &Value) -> Vec<Poke> {
    let Some(repo) = v["repository"]["full_name"].as_str().filter(|r| r.contains('/')) else { return vec![] };
    let prs = |list: &Value| -> Vec<u64> {
        list.as_array().into_iter().flatten().filter_map(|p| p["number"].as_u64()).collect()
    };
    let mut numbers: Vec<u64> = match event {
        "pull_request" | "pull_request_review" | "pull_request_review_comment" | "pull_request_review_thread" => {
            v["pull_request"]["number"].as_u64().into_iter().collect()
        }
        "issues" | "issue_comment" => v["issue"]["number"].as_u64().into_iter().collect(),
        "check_run" => prs(&v["check_run"]["pull_requests"]),
        "check_suite" => prs(&v["check_suite"]["pull_requests"]),
        "workflow_run" => prs(&v["workflow_run"]["pull_requests"]),
        _ => vec![],
    };
    numbers.sort_unstable();
    numbers.dedup();
    let one = |number| Poke {
        provider: "github".to_owned(),
        host: "github.com".to_owned(),
        repo: repo.to_owned(),
        number,
        event: event.to_owned(),
        delivery: delivery.to_owned(),
    };
    if numbers.is_empty() { vec![one(None)] } else { numbers.into_iter().map(|n| one(Some(n))).collect() }
}

/// `POST /github/webhook`: the App's events, handed to the hub as pokes.
pub async fn webhook(State(p): State<Arc<Platform>>, headers: HeaderMap, body: Bytes) -> Response {
    let Some(gh) = &p.github else {
        // Never dropped quietly: GitHub records the failure, to redeliver
        // once the App's secrets are set, and the hub's log has it too.
        tracing::error!(
            "a GitHub webhook came and this platform has no GitHub App: set GITHUB_APP_ID, \
             GITHUB_APP_WEBHOOK_SECRET and GITHUB_APP_PRIVATE_KEY on it (docs/control-ops.md)"
        );
        let why = format!("{}: the platform has no GitHub App key", super::NO_APP);
        return err(StatusCode::SERVICE_UNAVAILABLE, &why).into_response();
    };
    let h = |k: &str| headers.get(k).and_then(|v| v.to_str().ok());
    let delivery = h("x-github-delivery").unwrap_or_default().chars().take(64).collect::<String>();
    let event = h("x-github-event").unwrap_or_default().chars().take(64).collect::<String>();
    if !signed(&gh.webhook_secret, h("x-hub-signature-256"), &body) {
        warn!(%event, %delivery, "a GitHub webhook with a bad or missing signature");
        return err(StatusCode::UNAUTHORIZED, "bad signature").into_response();
    }
    if !delivery.is_empty() {
        match p.db.first_delivery(&delivery, &event, now_ms()) {
            Ok(true) => {}
            Ok(false) => return Json(json!({ "duplicate": true })).into_response(),
            Err(e) => {
                // Not taken: GitHub may deliver it again.
                warn!(%event, %delivery, error = %e, "recording a GitHub webhook's delivery");
                return err(StatusCode::INTERNAL_SERVER_ERROR, "couldn't record the delivery").into_response();
            }
        }
    }
    if event == "ping" {
        info!(%delivery, "GitHub pinged the webhook");
        return Json(json!({ "pong": true })).into_response();
    }
    let v: Value = serde_json::from_slice(&body).unwrap_or_default();
    if event.starts_with("installation") {
        gh.forget();
        // M163: kept, and what it takes away unlinked.
        if let Err(e) = super::links::installation_event(&p, &event, &v, now_ms()) {
            warn!(%event, %delivery, error = format!("{e:#}"), "keeping a GitHub installation's change");
            let _ = p.db.forget_delivery(&delivery);
            return err(StatusCode::INTERNAL_SERVER_ERROR, "couldn't keep the installation's change").into_response();
        }
    }
    // M165: what changes who may read a repo drops the answers kept.
    if let Err(e) = super::viewer::event(&p, &event, &v, now_ms()) {
        warn!(%event, %delivery, error = format!("{e:#}"), "dropping kept GitHub access answers");
        let _ = p.db.forget_delivery(&delivery);
        return err(StatusCode::INTERNAL_SERVER_ERROR, "couldn't drop kept access answers").into_response();
    }
    let pokes = pokes(&event, &delivery, &v);
    if pokes.is_empty() {
        return (StatusCode::ACCEPTED, Json(json!({ "relayed": 0 }))).into_response();
    }
    match p.hub.forge_pokes(&pokes).await {
        Ok(relayed) => {
            info!(%event, %delivery, pokes = pokes.len(), relayed, "webhook handed to the hub");
            (StatusCode::ACCEPTED, Json(json!({ "relayed": relayed }))).into_response()
        }
        Err(e) => {
            // Forgotten, so GitHub's redelivery is taken.
            warn!(%event, %delivery, error = format!("{e:#}"), "the hub didn't take a webhook's pokes");
            let _ = p.db.forget_delivery(&delivery);
            err(StatusCode::BAD_GATEWAY, "couldn't reach control's hub").into_response()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn signatures() {
        use hmac::{KeyInit, Mac};
        let body = br#"{"zen":"Keep it logically awesome."}"#;
        let mut mac = HmacSha256::new_from_slice(b"whsec").unwrap();
        mac.update(body);
        let good = format!("sha256={}", hex::encode(mac.finalize().into_bytes()));
        assert!(signed("whsec", Some(&good), body));
        assert!(!signed("other", Some(&good), body));
        assert!(!signed("whsec", Some(&good), b"{}"));
        assert!(!signed("whsec", None, body));
        assert!(!signed("whsec", Some("sha1=abc"), body));
        assert!(!signed("whsec", Some("sha256=zz"), body));
        assert!(!signed("", Some(&good), body));
    }

    #[test]
    fn what_a_poke_keeps() {
        let pr = json!({ "action": "review_requested", "repository": { "full_name": "jhgaylor/hud" },
            "pull_request": { "number": 7, "title": "secret title", "body": "secret body" } });
        let p = pokes("pull_request", "d1", &pr);
        assert_eq!(
            p,
            vec![Poke {
                provider: "github".into(),
                host: "github.com".into(),
                repo: "jhgaylor/hud".into(),
                number: Some(7),
                event: "pull_request".into(),
                delivery: "d1".into()
            }]
        );
        let wire = serde_json::to_string(&p[0]).unwrap();
        assert!(!wire.contains("secret"), "{wire}");
        let c = json!({ "repository": { "full_name": "o/r" }, "issue": { "number": 3, "pull_request": {} } });
        assert_eq!(pokes("issue_comment", "d", &c)[0].number, Some(3));
        let run = json!({ "repository": { "full_name": "o/r" },
            "check_run": { "head_sha": "abc", "pull_requests": [{ "number": 2 }, { "number": 9 }, { "number": 2 }] } });
        assert_eq!(pokes("check_run", "d", &run).iter().map(|p| p.number).collect::<Vec<_>>(), vec![Some(2), Some(9)]);
        // A fork's PR (no pull_requests) and a status: the whole repository.
        let fork = json!({ "repository": { "full_name": "o/r" }, "check_suite": { "pull_requests": [] } });
        assert_eq!(pokes("check_suite", "d", &fork)[0].number, None);
        let st = json!({ "repository": { "full_name": "o/r" }, "sha": "abc", "state": "failure" });
        assert_eq!(pokes("status", "d", &st)[0].number, None);
        let wr =
            json!({ "repository": { "full_name": "o/r" }, "workflow_run": { "pull_requests": [{ "number": 4 }] } });
        assert_eq!(pokes("workflow_run", "d", &wr)[0].number, Some(4));
        assert!(pokes("push", "d", &json!({})).is_empty());
    }
}
