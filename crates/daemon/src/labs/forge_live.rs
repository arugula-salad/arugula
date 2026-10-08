//! The webhooks Forgejo and GitLab send to this daemon (M40): the two
//! routes, their signature checks, and the paths a new hook is pointed at.
//! The record of the hooks, and the poke a delivery makes, are
//! `forge/live.rs`'s; GitHub's live updates come through control and need
//! none of this.
//!
//! The routes sit outside the API's authorization (a delivery is trusted by
//! its signature or token, [`is_hook`] tells the guard so). Where Labs is off
//! on the machine they answer 404, as an unknown route does.

use std::sync::Arc;

use axum::{
    Json, Router,
    body::Bytes,
    extract::{Query, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::post,
};
use serde::Deserialize;
use serde_json::{Value, json};
use tracing::warn;

use crate::{
    forge::{
        live::{hear, key, poke, rec_by},
        model::Provider,
    },
    server::App,
};

pub const FORGEJO_PATH: &str = "/api/forge/hooks/forgejo";
pub const GITLAB_PATH: &str = "/api/forge/hooks/gitlab";

/// The route a hook for `provider` is made to deliver to.
pub fn path(provider: Provider) -> &'static str {
    if provider == Provider::Gitlab { GITLAB_PATH } else { FORGEJO_PATH }
}

/// Whether `path` is one of the two routes.
pub fn is_hook(path: &str) -> bool {
    [FORGEJO_PATH, GITLAB_PATH].contains(&path)
}

/// Adds the two routes.
pub fn routes(r: Router<Arc<App>>) -> Router<Arc<App>> {
    r.route(FORGEJO_PATH, post(forgejo_hook)).route(GITLAB_PATH, post(gitlab_hook))
}

#[derive(Deserialize)]
pub struct HookQuery {
    #[serde(default)]
    k: String,
}

fn eq_ct(a: &[u8], b: &[u8]) -> bool {
    a.len() == b.len() && a.iter().zip(b).fold(0u8, |x, (p, q)| x | (p ^ q)) == 0
}

/// Forgejo's signature: hex HMAC-SHA256 of the body.
pub fn forgejo_signed(secret: &str, sig: Option<&str>, body: &[u8]) -> bool {
    use hmac::{KeyInit, Mac};
    let Some(sig) = sig.and_then(|s| hex::decode(s.trim().trim_start_matches("sha256=")).ok()) else { return false };
    let Ok(mut mac) = hmac::Hmac::<sha2::Sha256>::new_from_slice(secret.as_bytes()) else { return false };
    mac.update(body);
    mac.verify_slice(&sig).is_ok()
}

fn refused() -> Response {
    (StatusCode::UNAUTHORIZED, Json(json!({ "error": "bad signature" }))).into_response()
}

/// What a delivery gets where the machine's Labs is off.
fn off() -> Response {
    StatusCode::NOT_FOUND.into_response()
}

/// `POST /api/forge/hooks/forgejo?k=…`: a Forgejo webhook, signed.
pub async fn forgejo_hook(
    State(app): State<Arc<App>>,
    Query(q): Query<HookQuery>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    if !crate::labs::on(app.control.state_dir(), arugula_proto::flags::FORGES) {
        return off();
    }
    let h = |k: &str| headers.get(k).and_then(|v| v.to_str().ok());
    let Some(rec) = rec_by(&q.k, Provider::Forgejo) else { return refused() };
    let sig = h("x-forgejo-signature").or(h("x-gitea-signature"));
    if !forgejo_signed(&rec.secret, sig, &body) {
        warn!(repo = rec.repo, "a Forgejo webhook with a bad or missing signature");
        return refused();
    }
    let event = h("x-forgejo-event").or(h("x-gitea-event")).unwrap_or_default().to_owned();
    let v: Value = serde_json::from_slice(&body).unwrap_or_default();
    if !v["repository"]["full_name"].as_str().is_some_and(|r| r.eq_ignore_ascii_case(&rec.repo)) {
        return (StatusCode::BAD_REQUEST, Json(json!({ "error": "not this hook's repository" }))).into_response();
    }
    let number = v["pull_request"]["number"].as_u64().or(v["issue"]["number"].as_u64());
    poke(Provider::Forgejo, &rec.host, &rec.repo, number, &event);
    Json(json!({ "ok": true })).into_response()
}

/// `POST /api/forge/hooks/gitlab?k=…`: a GitLab webhook, with its token.
pub async fn gitlab_hook(
    State(app): State<Arc<App>>,
    Query(q): Query<HookQuery>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    if !crate::labs::on(app.control.state_dir(), arugula_proto::flags::FORGES) {
        return off();
    }
    let Some(rec) = rec_by(&q.k, Provider::Gitlab) else { return refused() };
    let token = headers.get("x-gitlab-token").and_then(|v| v.to_str().ok()).unwrap_or_default();
    if !eq_ct(token.as_bytes(), rec.secret.as_bytes()) {
        warn!(repo = rec.repo, "a GitLab webhook with a bad or missing token");
        return refused();
    }
    let v: Value = serde_json::from_slice(&body).unwrap_or_default();
    if !v["project"]["path_with_namespace"].as_str().is_some_and(|r| r.eq_ignore_ascii_case(&rec.repo)) {
        return (StatusCode::BAD_REQUEST, Json(json!({ "error": "not this hook's project" }))).into_response();
    }
    let kind = v["object_kind"].as_str().unwrap_or_default();
    let number = match kind {
        "merge_request" => v["object_attributes"]["iid"].as_u64(),
        "note" | "pipeline" => v["merge_request"]["iid"].as_u64(),
        _ => None,
    };
    if kind == "issue" || (kind == "note" && number.is_none()) {
        // Issues number apart from merge requests on GitLab: heard, no poll.
        hear(&key(Provider::Gitlab, &rec.host, &rec.repo));
    } else {
        poke(Provider::Gitlab, &rec.host, &rec.repo, number, kind);
    }
    Json(json!({ "ok": true })).into_response()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn signatures() {
        use hmac::{KeyInit, Mac};
        let mut mac = hmac::Hmac::<sha2::Sha256>::new_from_slice(b"s3cret").unwrap();
        mac.update(b"{}");
        let sig = hex::encode(mac.finalize().into_bytes());
        assert!(forgejo_signed("s3cret", Some(&sig), b"{}"));
        assert!(!forgejo_signed("other", Some(&sig), b"{}"));
        assert!(!forgejo_signed("s3cret", Some(&sig), b"{ }"));
        assert!(!forgejo_signed("s3cret", None, b"{}"));
        assert!(!forgejo_signed("s3cret", Some("nothex"), b"{}"));
        assert!(eq_ct(b"abc", b"abc") && !eq_ct(b"abc", b"abd") && !eq_ct(b"abc", b"ab"));
    }
}
