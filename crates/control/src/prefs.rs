//! An account's small preferences, kept for every page it signs in to
//! (DECISIONS.md "Hints come from facts, once per account"): which hints
//! it has seen (`seen:<hint>`), whether it wants tips at all (`tips`),
//! the last version What's new showed. Control knows nothing of the keys:
//! the page names them, and each is one short JSON value.
//!
//! `GET /api/me/prefs` answers them all, as an object; `POST` takes some
//! (`null` takes one back) and answers them all again. A key left out
//! stays as it was, so two tabs writing different keys don't undo each
//! other.

use std::sync::Arc;

use axum::{Json, extract::State, http::StatusCode};
use serde_json::{Map, Value};

use crate::{ApiError, App, auth::Session, err};

type R = Result<Json<Value>, ApiError>;

/// How many keys one account may keep.
pub const MAX_KEYS: usize = 256;
/// A key's longest.
const MAX_KEY: usize = 80;
/// A value's longest, as JSON.
const MAX_VALUE: usize = 512;

/// A key the page may use: lowercase, digits and `.:_-`, starting with a
/// letter or digit.
fn good_key(k: &str) -> bool {
    !k.is_empty()
        && k.len() <= MAX_KEY
        && k.starts_with(|c: char| c.is_ascii_lowercase() || c.is_ascii_digit())
        && k.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || ".:_-".contains(c))
}

/// `GET /api/me/prefs`.
pub async fn get(State(app): State<Arc<App>>, s: Session) -> R {
    all(&app, &s.account)
}

/// `POST /api/me/prefs`: `{ "seen:port-listening": 1730000000000, "tips": null }`.
pub async fn set(State(app): State<Arc<App>>, s: Session, Json(b): Json<Map<String, Value>>) -> R {
    let mut changes = Vec::with_capacity(b.len());
    for (k, v) in &b {
        if !good_key(k) {
            return Err(err(StatusCode::BAD_REQUEST, "a pref's name is lowercase letters, digits and .:_-"));
        }
        let v = (!v.is_null()).then(|| v.to_string());
        if v.as_ref().is_some_and(|v| v.len() > MAX_VALUE) {
            return Err(err(StatusCode::BAD_REQUEST, "that pref is too long"));
        }
        changes.push((k.as_str(), v));
    }
    if !app.db.set_prefs(&s.account, &changes, MAX_KEYS)? {
        return Err(err(StatusCode::BAD_REQUEST, "too many prefs"));
    }
    all(&app, &s.account)
}

fn all(app: &App, account: &str) -> R {
    let m: Map<String, Value> =
        app.db.prefs(account)?.into_iter().filter_map(|(k, v)| Some((k, serde_json::from_str(&v).ok()?))).collect();
    Ok(Json(Value::Object(m)))
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use crate::routing_wire::{control, person, session};

    const PREFS: &str = "/api/me/prefs";

    /// Kept per account, merged key by key, `null` takes one back; another
    /// account sees none of it, and a stranger can't ask.
    #[tokio::test]
    async fn an_accounts_prefs_are_its_own_and_merge() {
        let c = control(|_| {}).await;
        person(&c.app, "jake", "acct-j");
        person(&c.app, "ruth", "acct-r");
        let (j, r) = (session(&c.app, "acct-j"), session(&c.app, "acct-r"));

        assert_eq!(c.as_person(&j, "GET", PREFS, None).await, (200, json!({})));
        let (st, got) = c.as_person(&j, "POST", PREFS, Some(json!({ "seen:port-listening": 17, "tips": true }))).await;
        assert_eq!((st, got), (200, json!({ "seen:port-listening": 17, "tips": true })));
        // Another tab's write of another key leaves these be.
        let (_, got) = c.as_person(&j, "POST", PREFS, Some(json!({ "whats-new": "0.64.0", "tips": null }))).await;
        assert_eq!(got, json!({ "seen:port-listening": 17, "whats-new": "0.64.0" }));
        assert_eq!(c.as_person(&r, "GET", PREFS, None).await, (200, json!({})), "another account's");
        assert_eq!(c.as_person("", "GET", PREFS, None).await.0, 401);
    }

    /// Names are the page's own short words, values are short, and an
    /// account keeps a bounded number.
    #[tokio::test]
    async fn prefs_are_bounded() {
        let c = control(|_| {}).await;
        person(&c.app, "jake", "acct-j");
        let j = session(&c.app, "acct-j");
        for bad in ["", "Tips", "-x", "a b", "seen/x"] {
            assert_eq!(c.as_person(&j, "POST", PREFS, Some(json!({ bad: 1 }))).await.0, 400, "{bad:?}");
        }
        assert_eq!(c.as_person(&j, "POST", PREFS, Some(json!({ "k".repeat(81): 1 }))).await.0, 400);
        assert_eq!(c.as_person(&j, "POST", PREFS, Some(json!({ "big": "x".repeat(600) }))).await.0, 400);
        let many: serde_json::Map<_, _> = (0..super::MAX_KEYS).map(|i| (format!("seen:h{i}"), json!(1))).collect();
        assert_eq!(c.as_person(&j, "POST", PREFS, Some(many.into())).await.0, 200);
        assert_eq!(c.as_person(&j, "POST", PREFS, Some(json!({ "one-more": 1 }))).await.0, 400);
        // Changing one it has, or taking one back, is still fine.
        assert_eq!(c.as_person(&j, "POST", PREFS, Some(json!({ "seen:h0": 2, "seen:h1": null }))).await.0, 200);
        assert_eq!(c.as_person(&j, "POST", PREFS, Some(json!({ "one-more": 1 }))).await.0, 200);
        assert_eq!(c.as_person(&j, "GET", PREFS, None).await.1.as_object().unwrap().len(), super::MAX_KEYS);
    }
}
