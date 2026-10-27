//! An account's email address in Account settings (#1609): what control
//! keeps (`GET /api/me/email`), a typed one sent a link to verify it
//! (`POST`), the link (`GET /auth/email/verify`), and taking it off
//! (`DELETE`). For security notices only (mail.rs). A GitHub sign-in keeps
//! GitHub's primary verified address when the account has none (auth.rs).

use std::sync::Arc;

use arugula_e2e::now_ms;
use axum::{
    Json,
    extract::{Query, State},
    http::StatusCode,
    response::{IntoResponse, Redirect, Response},
};
use serde::Deserialize;
use serde_json::{Value, json};
use tracing::info;

use crate::{
    ApiError, App,
    auth::{PageSession, hash, token},
    err, mail,
};

type R = Result<Json<Value>, ApiError>;

/// How long a verify link works.
pub const LINK_MS: u64 = 24 * 3600 * 1000;

/// What control keeps for the account, and whether it sends email at all.
pub async fn get(State(app): State<Arc<App>>, s: PageSession) -> R {
    let e = app.db.email(&s.account)?;
    Ok(Json(json!({
        "email": e.as_ref().map(|e| &e.email),
        "source": e.as_ref().map(|e| &e.source),
        "verified": e.as_ref().is_some_and(|e| e.verified_at.is_some()),
        "pending": app.db.pending_email(&s.account, now_ms())?,
        "sends": app.mail.on(),
    })))
}

#[derive(Deserialize)]
pub struct Set {
    email: String,
}

/// A typed address: a link to verify it goes there, and it's the
/// account's once the link is opened. Until then the old one stands.
pub async fn set(State(app): State<Arc<App>>, s: PageSession, Json(b): Json<Set>) -> R {
    s.not_app(&app)?;
    let email = b.email.trim().to_owned();
    if !mail::plausible(&email) {
        return Err(err(StatusCode::BAD_REQUEST, "that doesn't look like an email address"));
    }
    if !app.mail.on() {
        return Err(err(StatusCode::SERVICE_UNAVAILABLE, "this control doesn't send email"));
    }
    let t = token();
    app.db.add_email_token(&s.account, &email, &hash(&t), now_ms() + LINK_MS)?;
    let link = format!("{}/auth/email/verify?t={t}", app.cfg.public_url);
    let body = format!(
        "Someone signed in to Arugula asked to use this address for your account's security notices.\n\n\
         If that was you, open this link within a day:\n\n{link}\n\n\
         If it wasn't, ignore this email: nothing changes."
    );
    if !mail::send(&app, &s.account, &email, "verify", "Verify your email for Arugula".into(), body) {
        return Err(err(StatusCode::TOO_MANY_REQUESTS, "too many emails today: try again tomorrow"));
    }
    info!(account = s.account, "sent a link to verify an address");
    Ok(Json(json!({ "sent": true })))
}

/// The account's address taken off: no more notices by email.
pub async fn remove(State(app): State<Arc<App>>, s: PageSession) -> R {
    s.not_app(&app)?;
    Ok(Json(json!({ "removed": app.db.remove_email(&s.account)? })))
}

#[derive(Deserialize)]
pub struct Verify {
    t: String,
}

/// The link in the email: the address is the account's, verified. Back to
/// control's page, which says what happened (`#email=verified` or
/// `#email=expired`). No session needed: the token is the proof, used once.
pub async fn verify(State(app): State<Arc<App>>, Query(q): Query<Verify>) -> Response {
    match app.db.verify_email(&hash(&q.t), now_ms()) {
        Ok(Some((account, _))) => {
            info!(account, "verified an address");
            Redirect::to("/#email=verified").into_response()
        }
        Ok(None) => Redirect::to("/#email=expired").into_response(),
        Err(e) => ApiError::from(e).into_response(),
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        mail::Mailer,
        routing_wire::{control, person, session},
    };

    /// Typed, sent a link, verified by it once; a used or wrong link does
    /// nothing; taking it off stops notices.
    #[tokio::test]
    async fn a_typed_address_is_verified_by_its_link() {
        let sent = std::sync::Arc::default();
        let fake = Mailer::Fake(std::sync::Arc::clone(&sent));
        let c = control(move |app| app.mail = fake).await;
        person(&c.app, "lex00", "a1");
        let cookie = session(&c.app, "a1");

        let (st, v) = c.as_person(&cookie, "POST", "/api/me/email", Some(serde_json::json!({ "email": "nope" }))).await;
        assert_eq!(st, 400, "{v}");
        let (st, v) = c
            .as_person(&cookie, "POST", "/api/me/email", Some(serde_json::json!({ "email": "lex@example.com" })))
            .await;
        assert_eq!(st, 200, "{v}");
        let (_, v) = c.as_person(&cookie, "GET", "/api/me/email", None).await;
        assert_eq!(
            (v["email"].clone(), v["pending"].clone()),
            (serde_json::Value::Null, serde_json::json!("lex@example.com")),
            "{v}"
        );

        // The email went (in the background), with the link.
        let link = tokio::time::timeout(std::time::Duration::from_secs(5), async {
            loop {
                if let Some(m) = sent.lock().unwrap().first().cloned() {
                    break m;
                }
                tokio::time::sleep(std::time::Duration::from_millis(20)).await;
            }
        })
        .await
        .unwrap();
        assert_eq!(link.to, "lex@example.com");
        let url = link.body.lines().find(|l| l.contains("/auth/email/verify?t=")).unwrap().trim().to_owned();
        let http = reqwest::Client::builder().redirect(reqwest::redirect::Policy::none()).build().unwrap();
        let go = |u: String| {
            let http = http.clone();
            async move { http.get(u).send().await.unwrap().headers()["location"].to_str().unwrap().to_owned() }
        };
        assert_eq!(go(url.clone()).await, "/#email=verified");
        assert_eq!(go(url).await, "/#email=expired", "used once");
        assert_eq!(go(format!("{}/auth/email/verify?t=nope", c.base)).await, "/#email=expired");
        assert_eq!(c.app.db.verified_email("a1").unwrap().as_deref(), Some("lex@example.com"));

        let (st, _) = c.as_person(&cookie, "DELETE", "/api/me/email", None).await;
        assert_eq!(st, 200);
        assert_eq!(c.app.db.verified_email("a1").unwrap(), None);
    }

    /// With no mailer, nothing is sent, and setting one says so.
    #[tokio::test]
    async fn no_mailer_no_email() {
        let c = control(|_| {}).await;
        person(&c.app, "lex00", "a1");
        let cookie = session(&c.app, "a1");
        let (_, v) = c.as_person(&cookie, "GET", "/api/me/email", None).await;
        assert_eq!(v["sends"], false);
        let (st, _) = c
            .as_person(&cookie, "POST", "/api/me/email", Some(serde_json::json!({ "email": "lex@example.com" })))
            .await;
        assert_eq!(st, 503);
    }

    /// GitHub's address fills an empty one, follows GitHub, and never
    /// replaces a typed one.
    #[test]
    fn githubs_address_never_replaces_a_typed_one() {
        let db = crate::db::Db::memory();
        assert!(db.email_from_github("a1", "gh@example.com", 1).unwrap());
        assert!(db.email_from_github("a1", "gh2@example.com", 2).unwrap());
        assert_eq!(db.verified_email("a1").unwrap().as_deref(), Some("gh2@example.com"));
        db.add_email_token("a1", "typed@example.com", "h", 100).unwrap();
        assert!(db.verify_email("h", 3).unwrap().is_some());
        assert!(!db.email_from_github("a1", "gh3@example.com", 4).unwrap());
        assert_eq!(db.verified_email("a1").unwrap().as_deref(), Some("typed@example.com"));
    }

    /// At most five a day, then refused.
    #[tokio::test]
    async fn five_a_day() {
        let fake = Mailer::Fake(std::sync::Arc::default());
        let c = control(move |app| app.mail = fake).await;
        person(&c.app, "lex00", "a1");
        let cookie = session(&c.app, "a1");
        for _ in 0..crate::mail::DAILY_LIMIT {
            let (st, v) = c
                .as_person(&cookie, "POST", "/api/me/email", Some(serde_json::json!({ "email": "lex@example.com" })))
                .await;
            assert_eq!(st, 200, "{v}");
        }
        let (st, _) = c
            .as_person(&cookie, "POST", "/api/me/email", Some(serde_json::json!({ "email": "lex@example.com" })))
            .await;
        assert_eq!(st, 429);
    }
}
