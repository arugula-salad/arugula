//! `POST /github/webhook`: the App's events (M40), the platform's since
//! M161. Each is checked against `X-Hub-Signature-256` (HMAC-SHA256 of the
//! body with the App's webhook secret, compared in constant time) and
//! deduplicated by `X-GitHub-Delivery` in `github_webhook_deliveries`, kept
//! 7 days, so a redelivery is known after a deploy. A linked repo's payload
//! writes the forge mirror's rows (M164, [`super::ingest`]): its metadata,
//! never a body or a comment's text. Until M174 the hub also relayed each
//! event to the daemons watching the repository as pokes; that relay is
//! retired, and nothing else of the payload goes anywhere. Only the event,
//! delivery and repository are logged.

use std::sync::Arc;

use arugula_control_common::err;
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

/// `POST /github/webhook`: the App's events, kept as the mirror's rows.
pub async fn webhook(State(p): State<Arc<Platform>>, headers: HeaderMap, body: Bytes) -> Response {
    let came = std::time::Instant::now();
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
    // M165: a repo made private, and what changes who may read one.
    if let Err(e) = super::viewer::event(&p, &event, &v) {
        warn!(%event, %delivery, error = format!("{e:#}"), "dropping kept GitHub access answers");
        let _ = p.db.forget_delivery(&delivery);
        return err(StatusCode::INTERNAL_SERVER_ERROR, "couldn't drop kept access answers").into_response();
    }
    // M164: a linked repo's rows. M174: no pokes go to the hub.
    super::ingest::webhook(&p, &event, &delivery, &v, came);
    (StatusCode::ACCEPTED, Json(json!({ "taken": true }))).into_response()
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
}
