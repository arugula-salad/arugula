//! Resetting your own devices (aru-119, #1582,
//! docs/design/self-service-reset.md): for a person none of whose devices
//! can approve and who has no recovery code left.
//!
//! - `POST /api/me/reset` starts one: a GitHub sign-in from the last ten
//!   minutes ([`passkey::fresh_github`]), and not while a device of the
//!   account was used in the last [`QUIET_MS`] (then something can
//!   approve), a reset is pending, one was done this week
//!   ([`account_reset::AGAIN_AFTER_MS`]), or three were started today.
//!   Every device is told (a notice on control's page, and a push), and so
//!   is the account's verified email (mail.rs).
//! - `GET /api/me/reset` says whether one's pending and when it's due, and
//!   finishes it once it's due: no timer, the next read does it, as does a
//!   browser asking to join (`api::enroll`).
//! - `POST /api/me/reset/cancel` stops it: the sign-in that started it, or
//!   a device or recovery code the account trusts, signing
//!   [`cancel_body`]. A signed-in session alone can't, so a stolen one can't
//!   start a reset and keep the real person from stopping it.
//!
//! When it's done, the account's devices are reset as support's are
//! (`Db::reset_account_devices`): the next browser enrolls as the first.
//! Its passkeys are removed too, `by reset` in its passkey log
//! (`Db::drop_passkeys_by_reset`, DK10 #393): the one way to remove a
//! passkey without another's say-so, and what lets a GitHub sign-in add a
//! first one again to an account that had one.

use std::sync::Arc;

use arugula_e2e::{cert::verify_hex, now_ms};
use axum::{Json, extract::State, http::StatusCode};
use serde::Deserialize;
use serde_json::{Value, json};
use tracing::info;

use crate::{ApiError, App, account_reset, auth::PageSession, err, mail, passkey, push, refusal};

type R = Result<Json<Value>, ApiError>;

/// How long a reset waits before it's done.
pub const WAIT_MS: u64 = 48 * 3600 * 1000;
/// A device used this recently can approve: no reset then.
pub const QUIET_MS: u64 = 48 * 3600 * 1000;
/// Resets an account may start a day.
pub const DAILY_STARTS: u64 = 3;
const DAY_MS: u64 = 24 * 3600 * 1000;

/// What a device or recovery code signs to cancel the reset started at
/// `started_at`.
pub fn cancel_body(account: &str, started_at: u64) -> String {
    format!("arugula reset cancel v1\n{account}\n{started_at}")
}

/// Every device and the account's email hear about it.
fn tell(app: &Arc<App>, account: &str, kind: &'static str, title: &str, body: String) {
    let now = now_ms();
    if let Err(e) = app.db.add_notice(account, title, &body, now) {
        tracing::warn!(account, "no notice for the reset: {e}");
    }
    push::notify(app, vec![account.to_owned()], "reset", title.to_owned(), body.clone());
    mail::notice(app, account, kind, format!("Arugula: {title}"), body);
}

fn when(ms: u64) -> String {
    let hours = ms.saturating_sub(now_ms()).div_ceil(3600 * 1000);
    format!("in about {hours} hours")
}

/// Start one.
pub async fn start(State(app): State<Arc<App>>, s: PageSession) -> R {
    s.not_app(&app)?;
    let now = now_ms();
    let info = app.db.session_info(&s.token_hash, now)?.ok_or_else(|| err(StatusCode::UNAUTHORIZED, "signed out"))?;
    if !passkey::fresh_github(&info) {
        return Err(refusal(
            StatusCode::FORBIDDEN,
            passkey::REAUTH,
            "to reset your devices, sign in with GitHub again first: it takes a sign-in from the last ten minutes",
        ));
    }
    let account = app.db.account(&s.account)?.ok_or_else(|| err(StatusCode::UNAUTHORIZED, "no such account"))?;
    if account.root.is_none() {
        return Err(refusal(
            StatusCode::CONFLICT,
            "no_devices",
            "this account has no devices: reload, and this browser becomes its first",
        ));
    }
    if app.db.devices_used(&s.account)?.iter().any(|(_, used)| now.saturating_sub(*used) < QUIET_MS) {
        return Err(refusal(
            StatusCode::CONFLICT,
            "device_active",
            "one of your devices was used in the last two days: approve this browser from it instead",
        ));
    }
    if let Some((at, _)) = app.db.last_reset(&s.account)?
        && now.saturating_sub(at) < account_reset::AGAIN_AFTER_MS
    {
        return Err(refusal(
            StatusCode::CONFLICT,
            "reset_recently",
            "this account's devices were reset less than a week ago",
        ));
    }
    if app.db.resets_started_since(&s.account, now.saturating_sub(DAY_MS))? >= DAILY_STARTS {
        return Err(refusal(
            StatusCode::TOO_MANY_REQUESTS,
            "too_many",
            "too many resets started today: try again tomorrow",
        ));
    }
    let due = now + WAIT_MS;
    if !app.db.start_reset(&s.account, &s.token_hash, now, due)? {
        return Err(refusal(StatusCode::CONFLICT, "pending", "a reset of your devices is already waiting"));
    }
    info!(account = s.account, "a reset of the account's devices started");
    tell(
        &app,
        &s.account,
        "reset-started",
        "A reset of your devices started",
        format!(
            "Someone signed in to your Arugula account with GitHub asked to reset its devices. It happens {}: \
             every device, passkey and recovery code stops working, and the next browser that signs in becomes your first device.\n\n\
             If this wasn't you, open Arugula control on any of your devices and cancel it under Devices and machines.",
            when(due)
        ),
    );
    Ok(Json(json!({ "pending": { "started_at": now, "due": due } })))
}

/// Whether one's pending, finishing it if it's due.
pub async fn status(State(app): State<Arc<App>>, s: PageSession) -> R {
    let now = now_ms();
    if app.db.finish_due_reset(&s.account, now)? {
        info!(account = s.account, "the account's devices were reset");
        tell(
            &app,
            &s.account,
            "reset-done",
            "Your devices were reset",
            "Your Arugula account's devices were reset, and its passkeys removed. Sign in with a browser to make it your first device, and keep the new recovery codes it shows; then add a passkey again.".into(),
        );
        return Ok(Json(json!({ "pending": null, "done": true })));
    }
    let p = app.db.pending_reset(&s.account)?;
    Ok(Json(json!({
        "pending": p.as_ref().map(|p| json!({ "started_at": p.started_at, "due": p.due, "mine": p.session_hash == s.token_hash })),
        "done": false,
    })))
}

#[derive(Deserialize, Default)]
pub struct Cancel {
    device: Option<String>,
    sig: Option<String>,
}

/// Stop it.
pub async fn cancel(State(app): State<Arc<App>>, s: PageSession, body: Option<Json<Cancel>>) -> R {
    let b = body.map(|Json(b)| b).unwrap_or_default();
    let p = app.db.pending_reset(&s.account)?.ok_or_else(|| err(StatusCode::NOT_FOUND, "no reset is waiting"))?;
    let by = if p.session_hash == s.token_hash {
        "session".to_owned()
    } else {
        let (Some(device), Some(sig)) = (b.device, b.sig) else {
            return Err(err(StatusCode::FORBIDDEN, "cancel it from one of your devices, or with a recovery code"));
        };
        let (trust, certs, revs) = crate::api::trusted(&app, &s.account)?;
        let trusted = trust.map(|t| t.evaluate(&certs, &revs)).unwrap_or_default();
        let ok = trusted
            .get(&device)
            .is_some_and(|c| verify_hex(&c.sign, cancel_body(&s.account, p.started_at).as_bytes(), &sig));
        if !ok {
            return Err(err(StatusCode::FORBIDDEN, "that isn't a device or recovery code this account trusts"));
        }
        device
    };
    app.db.cancel_reset(&s.account, &by, now_ms())?;
    info!(account = s.account, by, "the reset of the account's devices was cancelled");
    tell(
        &app,
        &s.account,
        "reset-cancelled",
        "The reset of your devices was cancelled",
        "The reset of your Arugula account's devices was cancelled: nothing changed.".into(),
    );
    Ok(Json(json!({ "cancelled": true })))
}

#[cfg(test)]
mod tests {
    use arugula_e2e::{Cert, DeviceKeys, Kind};
    use serde_json::json;

    use super::*;
    use crate::{
        auth::{SESSION_COOKIE, Via, hash},
        mail::Mailer,
        routing_wire::{Control, control, person, session},
    };

    /// A sign-in of `account` made `via`, `ago` ms ago.
    fn signed_in(c: &Control, account: &str, via: Via, ago: u64) -> String {
        let cookie = session(&c.app, account);
        let token = cookie.strip_prefix(&format!("{SESSION_COOKIE}=")).unwrap();
        c.app.db.set_session_via(&hash(token), via.as_str()).unwrap();
        c.app.db.conn().execute("UPDATE sessions SET created = created - ?1", [ago as i64]).unwrap();
        cookie
    }

    async fn with_mail() -> (Control, std::sync::Arc<std::sync::Mutex<Vec<mail::Sent>>>) {
        let sent = std::sync::Arc::default();
        let fake = Mailer::Fake(std::sync::Arc::clone(&sent));
        (control(move |app| app.mail = fake).await, sent)
    }

    #[tokio::test]
    async fn starting_takes_a_fresh_github_sign_in_and_quiet_devices() {
        let (c, _) = with_mail().await;
        person(&c.app, "lex00", "a1");
        let old = signed_in(&c, "a1", Via::Github, REAUTH_PLUS);
        let (st, v) = c.as_person(&old, "POST", "/api/me/reset", Some(json!({}))).await;
        assert_eq!((st, v["reason"].as_str()), (403, Some(passkey::REAUTH)), "{v}");
        let pk = signed_in(&c, "a1", Via::Passkey, 0);
        let (st, _) = c.as_person(&pk, "POST", "/api/me/reset", Some(json!({}))).await;
        assert_eq!(st, 403, "a passkey sign-in isn't GitHub's");

        // A device used in the last two days can approve instead.
        let root = c.app.db.account("a1").unwrap().unwrap().root.unwrap();
        c.app.db.touch_device("a1", &root, now_ms()).unwrap();
        let fresh = signed_in(&c, "a1", Via::Github, 0);
        let (st, v) = c.as_person(&fresh, "POST", "/api/me/reset", Some(json!({}))).await;
        assert_eq!((st, v["reason"].as_str()), (409, Some("device_active")), "{v}");
        c.app.db.touch_device("a1", &root, now_ms() - QUIET_MS - 1).unwrap();
        let (st, v) = c.as_person(&fresh, "POST", "/api/me/reset", Some(json!({}))).await;
        assert_eq!(st, 200, "{v}");
        let (st, v) = c.as_person(&fresh, "POST", "/api/me/reset", Some(json!({}))).await;
        assert_eq!((st, v["reason"].as_str()), (409, Some("pending")), "{v}");
    }

    /// Ten minutes and a second: too old.
    const REAUTH_PLUS: u64 = passkey::REAUTH_MS + 1000;

    #[tokio::test]
    async fn every_device_and_the_email_hear_and_a_trusted_device_cancels() {
        let (c, sent) = with_mail().await;
        let root = person(&c.app, "lex00", "a1");
        c.app.db.email_from_github("a1", "lex@example.com", 1).unwrap();
        let starter = signed_in(&c, "a1", Via::Github, 0);
        let (st, v) = c.as_person(&starter, "POST", "/api/me/reset", Some(json!({}))).await;
        assert_eq!(st, 200, "{v}");
        let started_at = v["pending"]["started_at"].as_u64().unwrap();
        assert!(c.app.db.notices("a1").unwrap().iter().any(|(_, t, _)| t == "A reset of your devices started"));
        let mailed = tokio::time::timeout(std::time::Duration::from_secs(5), async {
            loop {
                if let Some(m) = sent.lock().unwrap().first().cloned() {
                    break m;
                }
                tokio::time::sleep(std::time::Duration::from_millis(20)).await;
            }
        })
        .await
        .unwrap();
        assert_eq!(
            (mailed.to.as_str(), mailed.subject.as_str()),
            ("lex@example.com", "Arugula: A reset of your devices started")
        );

        // Another sign-in: only with a trusted device's signature.
        let other = signed_in(&c, "a1", Via::Github, 0);
        let (_, v) = c.as_person(&other, "GET", "/api/me/reset", None).await;
        assert_eq!(v["pending"]["mine"], false, "{v}");
        let (st, _) = c.as_person(&other, "POST", "/api/me/reset/cancel", Some(json!({}))).await;
        assert_eq!(st, 403);
        let stranger = DeviceKeys::generate();
        let body = cancel_body("a1", started_at);
        let sig = |k: &DeviceKeys| hex::encode(k.signature(body.as_bytes()));
        let (st, _) = c
            .as_person(
                &other,
                "POST",
                "/api/me/reset/cancel",
                Some(json!({ "device": stranger.id(), "sig": sig(&stranger) })),
            )
            .await;
        assert_eq!(st, 403, "a key the account doesn't trust");
        let (st, v) = c
            .as_person(&other, "POST", "/api/me/reset/cancel", Some(json!({ "device": root.id(), "sig": sig(&root) })))
            .await;
        assert_eq!(st, 200, "{v}");
        assert_eq!(c.app.db.pending_reset("a1").unwrap(), None);
        assert!(c.app.db.account("a1").unwrap().unwrap().root.is_some(), "nothing reset");
    }

    #[tokio::test]
    async fn the_sign_in_that_started_it_cancels_without_a_signature() {
        let (c, _) = with_mail().await;
        person(&c.app, "lex00", "a1");
        let starter = signed_in(&c, "a1", Via::Github, 0);
        c.as_person(&starter, "POST", "/api/me/reset", Some(json!({}))).await;
        let (_, v) = c.as_person(&starter, "GET", "/api/me/reset", None).await;
        assert_eq!(v["pending"]["mine"], true, "{v}");
        let (st, _) = c.as_person(&starter, "POST", "/api/me/reset/cancel", Some(json!({}))).await;
        assert_eq!(st, 200);
    }

    /// Once due, the next read does it, and a browser that signs its own
    /// certificate is the first device. Its passkeys go too, `by reset`
    /// (DK10, #393): what lets one be added again.
    #[tokio::test]
    async fn a_due_reset_is_done_on_the_next_read() {
        let (c, _) = with_mail().await;
        person(&c.app, "lex00", "a1");
        c.app.db.add_passkey("pk1", "a1", -8, &[7; 32], 1).unwrap();
        c.app.db.add_passkey("pk2", "a1", -8, &[8; 32], 1).unwrap();
        let starter = signed_in(&c, "a1", Via::Github, 0);
        c.as_person(&starter, "POST", "/api/me/reset", Some(json!({}))).await;
        let (_, v) = c.as_person(&starter, "GET", "/api/me/reset", None).await;
        assert_eq!(v["done"], false, "not due yet: {v}");
        assert_eq!(c.app.db.passkey_count("a1").unwrap(), 2, "nor its passkeys");
        c.app.db.conn().execute("UPDATE reset_requests SET due = 1", []).unwrap();

        let k = DeviceKeys::generate();
        let mut first = Cert::new(&k, "a1", Kind::Browser, "Chrome on Mac");
        first.sign_with(&k);
        let (st, v) = c.as_person(&starter, "POST", "/api/devices", Some(json!({ "cert": first }))).await;
        assert_eq!((st, v["root"].as_str()), (200, Some(first.device.as_str())), "{v}");
        assert_eq!(c.app.db.last_reset("a1").unwrap().map(|r| r.1), Some("self-service".to_owned()));
        let (_, v) = c.as_person(&starter, "GET", "/api/me/reset", None).await;
        assert_eq!(v["pending"], serde_json::Value::Null, "{v}");
        assert_eq!(c.app.db.passkey_count("a1").unwrap(), 0);
        let log = c.app.db.passkey_log("a1").unwrap();
        let removed: Vec<_> = log.events.iter().filter(|e| e.by == "reset").map(|e| e.credential.as_str()).collect();
        assert_eq!(removed, ["pk1", "pk2"]);
        assert_eq!(arugula_e2e::passkey::check_chain(&log), Ok(()));
    }
}
