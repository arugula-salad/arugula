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

use crate::{
    ApiError, App, account_reset, auth::PageSession, db::reset::Kind as Reset, err, mail, passkey, push, refusal,
};

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

/// Every device and the account's email hear about it. A reset's email is
/// outside the daily limit (DK10), so a session can't use it up first.
fn tell(app: &Arc<App>, account: &str, kind: &'static str, title: &str, body: String) {
    notify(app, account, title, &body);
    mail::security_notice(app, account, kind, format!("Arugula: {title}"), body);
}

fn notify(app: &Arc<App>, account: &str, title: &str, body: &str) {
    if let Err(e) = app.db.add_notice(account, title, body, now_ms()) {
        tracing::warn!(account, "no notice for the reset: {e}");
    }
    push::notify(app, vec![account.to_owned()], "reset", title.to_owned(), body.to_owned());
}

/// As [`tell`], waiting for the email to be taken: whether it was (DK10: a
/// reset that drops passkeys never starts unannounced).
async fn tell_now(app: &Arc<App>, account: &str, kind: &'static str, title: &str, body: String) -> bool {
    let emailed = mail::notice_now(app, account, kind, format!("Arugula: {title}"), body.clone()).await;
    notify(app, account, title, &body);
    emailed
}

/// How long after the account's address changes no passkey reset starts
/// (DK10): the old address was told, and has this long to act.
pub const EMAIL_SETTLE_MS: u64 = 48 * 3600 * 1000;
/// How long after a device, recovery code or passkey cancels a reset no
/// passkey reset starts (DK10): no endless restarts.
pub const AFTER_CANCEL_MS: u64 = 7 * 24 * 3600 * 1000;

/// Why a reset can't drop the account's passkeys now, if it can't (DK10):
/// no verified address for its warning, or one that changed in the last
/// [`EMAIL_SETTLE_MS`].
fn passkeys_unwarned(app: &App, account: &str, now: u64) -> Result<Option<ApiError>, ApiError> {
    if app.db.verified_email(account)?.is_none() || !app.mail.on() {
        return Ok(Some(refusal(
            StatusCode::CONFLICT,
            "no_email",
            "a passkey reset needs a verified email address for its warning: add one under Sign-in and account, or contact support",
        )));
    }
    if app.db.email_changed_at(account)?.is_some_and(|at| now.saturating_sub(at) < EMAIL_SETTLE_MS) {
        return Ok(Some(refusal(
            StatusCode::CONFLICT,
            "email_changed",
            "your account's email address changed in the last two days: a passkey reset can start 48 hours after that, or contact support",
        )));
    }
    Ok(None)
}

/// The `webauthn.get` options a passkey of the account signs to cancel a
/// reset: `challenge`, and the account's passkeys.
fn cancel_options(
    app: &App,
    headers: &axum::http::HeaderMap,
    account: &str,
    challenge: String,
) -> anyhow::Result<Value> {
    Ok(json!({
        "challenge": challenge,
        "rpId": app.cfg.site(headers).host(),
        "userVerification": "required",
        "timeout": 5 * 60 * 1000,
        "allowCredentials": app.db.passkey_ids(account)?.iter()
            .map(|id| json!({ "type": "public-key", "id": id })).collect::<Vec<_>>(),
    }))
}

/// SHA-256 of `body`, base64url: what a passkey signs to agree to it.
fn challenge_of(body: &str) -> String {
    use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD as B64};
    use sha2::{Digest as _, Sha256};
    B64.encode(Sha256::digest(body.as_bytes()))
}

/// A passkey's challenge to cancel the device reset started at `started_at`.
pub fn cancel_challenge(account: &str, started_at: u64) -> String {
    challenge_of(&cancel_body(account, started_at))
}

/// Who cancels: the sign-in that started it (`session`), one of the
/// account's passkeys over `challenge` (`passkey <id>`), or a device or
/// recovery code it trusts, signing `body`. A session alone doesn't.
fn canceller(
    app: &App,
    headers: &axum::http::HeaderMap,
    s: &PageSession,
    started_by: &str,
    b: Cancel,
    body: &str,
) -> Result<String, ApiError> {
    if started_by == s.token_hash {
        return Ok("session".to_owned());
    }
    if let Some(a) = &b.passkey {
        let id = passkey::agreed(app, headers, &s.account, a, &challenge_of(body))?;
        return Ok(format!("passkey {id}"));
    }
    let (Some(device), Some(sig)) = (b.device, b.sig) else {
        return Err(err(
            StatusCode::FORBIDDEN,
            "cancel it from one of your devices, with one of your passkeys, or with a recovery code",
        ));
    };
    let (trust, certs, revs) = crate::api::trusted(app, &s.account)?;
    let trusted = trust.map(|t| t.evaluate(&certs, &revs)).unwrap_or_default();
    if !trusted.get(&device).is_some_and(|c| verify_hex(&c.sign, body.as_bytes(), &sig)) {
        return Err(err(StatusCode::FORBIDDEN, "that isn't a device or recovery code this account trusts"));
    }
    Ok(device)
}

/// Whether a reset of `kind` finished in the last day: a cancel that finds
/// none waiting came too late, rather than for nothing.
fn finished_lately(app: &App, account: &str, kind: crate::db::reset::Kind) -> anyhow::Result<bool> {
    Ok(app.db.last_reset_done_of(account, kind)?.is_some_and(|at| now_ms().saturating_sub(at) < DAY_MS))
}

/// A cancel that came too late: the reset finished first.
fn already_done() -> ApiError {
    refusal(StatusCode::CONFLICT, "already_done", "too late to cancel: the reset already finished")
}

/// What a cancel's notice adds when something the account trusts cancelled.
fn after_trusted_cancel(by: &str) -> &'static str {
    if by == "session" { "" } else { " A new passkey reset can't start for 7 days." }
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
    // Its passkeys go too only when its warning reaches a verified address
    // that didn't just change (DK10); the devices' reset is as it was.
    let passkeys_too = passkeys_unwarned(&app, &s.account, now)?.is_none();
    let what = if passkeys_too { "every device, passkey and recovery code" } else { "every device and recovery code" };
    let emailed = tell_now(
        &app,
        &s.account,
        "reset-started",
        "A reset of your devices started",
        format!(
            "Someone signed in to your Arugula account with GitHub asked to reset its devices. It happens {}: \
             {what} stops working, and the next browser that signs in becomes your first device.\n\n\
             If this wasn't you, open Arugula control on any of your devices, or sign in with any of your passkeys, \
             and cancel it under Devices and machines.",
            when(due)
        ),
    )
    .await;
    if passkeys_too && emailed {
        app.db.reset_takes_passkeys(&s.account, now)?;
    }
    Ok(Json(json!({ "pending": { "started_at": now, "due": due, "passkeys": passkeys_too && emailed } })))
}

/// Whether one's pending, finishing it if it's due; while it is, what a
/// passkey signs to cancel it.
pub async fn status(State(app): State<Arc<App>>, headers: axum::http::HeaderMap, s: PageSession) -> R {
    let now = now_ms();
    if let Some(passkeys) = app.db.finish_due_reset(&s.account, now)? {
        info!(account = s.account, passkeys, "the account's devices were reset");
        let and = if passkeys { ", and its passkeys removed" } else { "" };
        let then = if passkeys { "; then add a passkey again" } else { "" };
        tell(
            &app,
            &s.account,
            "reset-done",
            "Your devices were reset",
            format!(
                "Your Arugula account's devices were reset{and}. Sign in with a browser to make it your first device, \
                 and keep the new recovery codes it shows{then}."
            ),
        );
        return Ok(Json(json!({ "pending": null, "done": true })));
    }
    let p = app.db.pending_reset(&s.account)?;
    let cancel = match &p {
        Some(p) => cancel_options(&app, &headers, &s.account, cancel_challenge(&s.account, p.started_at))?,
        None => Value::Null,
    };
    Ok(Json(json!({
        "pending": p.as_ref().map(|p| json!({ "started_at": p.started_at, "due": p.due, "mine": p.session_hash == s.token_hash })),
        "cancel": cancel,
        "done": false,
    })))
}

#[derive(Deserialize, Default)]
pub struct Cancel {
    device: Option<String>,
    sig: Option<String>,
    /// One of the account's passkeys, over SHA-256 of the cancel's text
    /// (DK10: the reset drops passkeys, so a passkey may stop it).
    passkey: Option<passkey::Assertion>,
}

/// Stop it: the sign-in that started it, a device or recovery code the
/// account trusts signing [`cancel_body`], or any of its passkeys that
/// still works, over [`cancel_challenge`]. 409 if it finished first.
pub async fn cancel(
    State(app): State<Arc<App>>,
    headers: axum::http::HeaderMap,
    s: PageSession,
    body: Option<Json<Cancel>>,
) -> R {
    let b = body.map(|Json(b)| b).unwrap_or_default();
    let p = match app.db.pending_reset(&s.account)? {
        Some(p) => p,
        None if finished_lately(&app, &s.account, Reset::Devices)? => return Err(already_done()),
        None => return Err(err(StatusCode::NOT_FOUND, "no reset is waiting")),
    };
    // Due: the finish wins, whichever request reads it first.
    if p.due <= now_ms() {
        return Err(already_done());
    }
    let by = canceller(&app, &headers, &s, &p.session_hash, b, &cancel_body(&s.account, p.started_at))?;
    if !app.db.cancel_reset(&s.account, &by, now_ms())? {
        return Err(already_done());
    }
    info!(account = s.account, by, "the reset of the account's devices was cancelled");
    tell(
        &app,
        &s.account,
        "reset-cancelled",
        "The reset of your devices was cancelled",
        format!(
            "The reset of your Arugula account's devices was cancelled: nothing changed.{}",
            after_trusted_cancel(&by)
        ),
    );
    Ok(Json(json!({ "cancelled": true })))
}

// ------------------------------------------------------- passkey reset

/// What a device or recovery code signs to cancel the passkey reset started
/// at `started_at` (DK10). A passkey signs SHA-256 of it, base64url, as its
/// challenge ([`passkey_cancel_challenge`]).
pub fn passkey_cancel_body(account: &str, started_at: u64) -> String {
    format!("arugula passkey reset cancel v1\n{account}\n{started_at}")
}

/// A passkey's challenge to cancel the passkey reset started at `started_at`.
pub fn passkey_cancel_challenge(account: &str, started_at: u64) -> String {
    challenge_of(&passkey_cancel_body(account, started_at))
}

/// A due passkey reset was done: every device and the email hear.
pub fn passkeys_were_reset(app: &Arc<App>, account: &str) {
    info!(account, "the account's passkeys were reset");
    tell(
        app,
        account,
        "passkey-reset-done",
        "Your passkeys were reset",
        "Your Arugula account's passkeys were removed, as the passkey reset asked. Add a passkey again from \
         Sign-in and account. Your devices, machines and recovery codes are as they were."
            .into(),
    );
}

/// `POST /api/me/passkeys/reset` (DK10, #393, aru-119 as amended): start
/// a reset of the account's passkeys only, for a person who lost them while
/// their devices are in use. A GitHub sign-in from the last ten minutes;
/// not while one is pending, when one was done this week, or after three
/// started today; and only for an account that has, or had, a passkey.
/// Devices may be in use. Every device and the email are told.
pub async fn start_passkeys(State(app): State<Arc<App>>, s: PageSession) -> R {
    s.not_app(&app)?;
    let now = now_ms();
    let info = app.db.session_info(&s.token_hash, now)?.ok_or_else(|| err(StatusCode::UNAUTHORIZED, "signed out"))?;
    if !passkey::fresh_github(&info) {
        return Err(refusal(
            StatusCode::FORBIDDEN,
            passkey::REAUTH,
            "to reset your passkeys, sign in with GitHub again first: it takes a sign-in from the last ten minutes",
        ));
    }
    if app.db.passkey_count(&s.account)? == 0 && app.db.passkey_last_seen(&s.account)?.is_none() {
        return Err(refusal(
            StatusCode::CONFLICT,
            "no_passkeys",
            "this account has never had a passkey: add one, no reset needed",
        ));
    }
    if app
        .db
        .last_reset_done_of(&s.account, Reset::Passkeys)?
        .is_some_and(|at| now.saturating_sub(at) < account_reset::AGAIN_AFTER_MS)
    {
        return Err(refusal(
            StatusCode::CONFLICT,
            "reset_recently",
            "this account's passkeys were reset less than a week ago",
        ));
    }
    if app.db.resets_started_since_of(&s.account, Reset::Passkeys, now.saturating_sub(DAY_MS))? >= DAILY_STARTS {
        return Err(refusal(
            StatusCode::TOO_MANY_REQUESTS,
            "too_many",
            "too many passkey resets started today: try again tomorrow",
        ));
    }
    // No endless restarts: a reset something the account trusts cancelled
    // holds the next off for a week (DK10).
    if app.db.last_trusted_cancel(&s.account)?.is_some_and(|at| now.saturating_sub(at) < AFTER_CANCEL_MS) {
        return Err(refusal(
            StatusCode::CONFLICT,
            "cancelled_recently",
            "a reset was cancelled from one of your devices or passkeys, or with a recovery code, in the last week: a new passkey reset can't start for 7 days after that",
        ));
    }
    // Its warning must reach a verified address that didn't just change.
    if let Some(no) = passkeys_unwarned(&app, &s.account, now)? {
        return Err(no);
    }
    let due = now + WAIT_MS;
    if !app.db.start_reset_of(&s.account, Reset::Passkeys, &s.token_hash, now, due)? {
        return Err(refusal(StatusCode::CONFLICT, "pending", "a reset of your passkeys is already waiting"));
    }
    let emailed = tell_now(
        &app,
        &s.account,
        "passkey-reset-started",
        "A reset of your passkeys started",
        format!(
            "Someone signed in to your Arugula account with GitHub asked to reset its passkeys. It happens {}: \
             every passkey is removed, and a new one can be added. Your devices, machines and recovery codes stay.\n\n\
             If this wasn't you, open Arugula control on any of your devices, or sign in with any of your passkeys, \
             and cancel it.",
            when(due)
        ),
    )
    .await;
    // Never silently: one whose warning didn't go out didn't start.
    if !emailed {
        app.db.forget_reset(&s.account, Reset::Passkeys, now)?;
        tracing::warn!(account = s.account, "a passkey reset not started: its email didn't go");
        notify(
            &app,
            &s.account,
            "A reset of your passkeys didn't start",
            "A reset of your passkeys was asked for, but its warning email couldn't be sent, so it didn't start.",
        );
        return Err(refusal(
            StatusCode::SERVICE_UNAVAILABLE,
            "email_failed",
            "the warning email couldn't be sent, so the passkey reset didn't start: try again later, or contact support",
        ));
    }
    info!(account = s.account, "a reset of the account's passkeys started");
    Ok(Json(json!({ "pending": { "started_at": now, "due": due } })))
}

/// `GET /api/me/passkeys/reset`: whether a passkey reset is pending,
/// finishing it once it's due; while it is, what a passkey signs to cancel
/// it (`webauthn.get` options over [`passkey_cancel_challenge`]).
pub async fn passkeys_status(State(app): State<Arc<App>>, headers: axum::http::HeaderMap, s: PageSession) -> R {
    let now = now_ms();
    if app.db.finish_due_passkey_reset(&s.account, now)? {
        passkeys_were_reset(&app, &s.account);
        return Ok(Json(json!({ "pending": null, "done": true })));
    }
    let p = app.db.pending_reset_of(&s.account, Reset::Passkeys)?;
    let cancel = match &p {
        Some(p) => cancel_options(&app, &headers, &s.account, passkey_cancel_challenge(&s.account, p.started_at))?,
        None => Value::Null,
    };
    Ok(Json(json!({
        "pending": p.as_ref().map(|p| json!({ "started_at": p.started_at, "due": p.due, "mine": p.session_hash == s.token_hash })),
        "cancel": cancel,
        "done": false,
    })))
}

/// `POST /api/me/passkeys/reset/cancel`: stop it. The sign-in that started
/// it; a device or recovery code the account trusts, signing
/// [`passkey_cancel_body`]; or any of the account's passkeys that still
/// works, over [`passkey_cancel_challenge`]. A session alone can't. 409 if
/// it finished first.
pub async fn cancel_passkeys(
    State(app): State<Arc<App>>,
    headers: axum::http::HeaderMap,
    s: PageSession,
    body: Option<Json<Cancel>>,
) -> R {
    let b = body.map(|Json(b)| b).unwrap_or_default();
    let p = match app.db.pending_reset_of(&s.account, Reset::Passkeys)? {
        Some(p) => p,
        None if finished_lately(&app, &s.account, Reset::Passkeys)? => return Err(already_done()),
        None => return Err(err(StatusCode::NOT_FOUND, "no passkey reset is waiting")),
    };
    // Due: the finish wins, whichever request reads it first.
    if p.due <= now_ms() {
        return Err(already_done());
    }
    let by = canceller(&app, &headers, &s, &p.session_hash, b, &passkey_cancel_body(&s.account, p.started_at))?;
    if !app.db.cancel_reset_of(&s.account, Reset::Passkeys, &by, now_ms())? {
        return Err(already_done());
    }
    info!(account = s.account, by, "the reset of the account's passkeys was cancelled");
    tell(
        &app,
        &s.account,
        "passkey-reset-cancelled",
        "The reset of your passkeys was cancelled",
        format!(
            "The reset of your Arugula account's passkeys was cancelled: nothing changed.{}",
            after_trusted_cancel(&by)
        ),
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
        // A verified address that's been the account's a while: its
        // warning reaches it, so the reset takes the passkeys too (DK10).
        c.app.db.email_from_github("a1", "lex@example.com", 1).unwrap();
        c.app.db.add_passkey("pk1", "a1", -8, &[7; 32], 1).unwrap();
        c.app.db.add_passkey("pk2", "a1", -8, &[8; 32], 1).unwrap();
        let starter = signed_in(&c, "a1", Via::Github, 0);
        let (_, v) = c.as_person(&starter, "POST", "/api/me/reset", Some(json!({}))).await;
        assert_eq!(v["pending"]["passkeys"], true, "{v}");
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

    /// A passkey `id` of `account`, made for control's own host (the test
    /// server's), its key from `seed`.
    fn site_passkey(c: &Control, account: &str, id: &str, seed: u8) -> ed25519_dalek::SigningKey {
        let k = ed25519_dalek::SigningKey::from_bytes(&[seed; 32]);
        let host = c.base.trim_start_matches("http://").split(':').next().unwrap().to_owned();
        let d = arugula_e2e::passkey::Draft {
            account: account.into(),
            event: arugula_e2e::passkey::Change::Added,
            credential: id.into(),
            rp: host,
            alg: arugula_e2e::passkey::EDDSA,
            key: k.verifying_key().to_bytes().to_vec(),
            aaguid: arugula_e2e::passkey::NO_AAGUID.into(),
            at: 1,
            by: "signup".into(),
        };
        c.app.db.add_passkey_logged(&d.key.clone(), d.after(None), false).unwrap().unwrap();
        k
    }

    /// Passkey `id` (key `k`)'s assertion over `challenge`, from the test
    /// server's page, with UP and UV.
    fn assert_with(c: &Control, id: &str, k: &ed25519_dalek::SigningKey, challenge: &str) -> Value {
        use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD as B64};
        use ed25519_dalek::Signer as _;
        use sha2::{Digest as _, Sha256};
        let host = c.base.trim_start_matches("http://").split(':').next().unwrap();
        let cd =
            serde_json::to_vec(&json!({ "type": "webauthn.get", "challenge": challenge, "origin": c.base })).unwrap();
        let mut data = Sha256::digest(host.as_bytes()).to_vec();
        data.extend_from_slice(&[0x05, 0, 0, 0, 0]);
        let mut msg = data.clone();
        msg.extend_from_slice(&Sha256::digest(&cd));
        json!({
            "id": id,
            "clientDataJSON": B64.encode(&cd),
            "authenticatorData": B64.encode(&data),
            "signature": B64.encode(k.sign(&msg).to_bytes()),
        })
    }

    /// DK10 (#393, aru-119 as amended): a reset of the passkeys alone
    /// starts from a fresh GitHub sign-in even while a device is in use,
    /// and every device and the email hear of it. Any working passkey, a
    /// device, or the sign-in that started it cancels it; a session alone
    /// doesn't. Once due, every passkey goes `by reset`, and the devices
    /// stay. At most one a week.
    #[tokio::test]
    async fn a_passkey_reset_replaces_only_the_passkeys() {
        let (c, sent) = with_mail().await;
        let root = person(&c.app, "lex00", "a1");
        c.app.db.email_from_github("a1", "lex@example.com", 1).unwrap();
        let rid = root.id();
        c.app.db.touch_device("a1", &rid, now_ms()).unwrap();
        let (st, v) = c.as_person(&signed_in(&c, "a1", Via::Github, 0), "POST", "/api/me/passkeys/reset", None).await;
        assert_eq!((st, v["reason"].as_str()), (409, Some("no_passkeys")), "never had one: {v}");
        let k1 = site_passkey(&c, "a1", "pk1", 1);
        site_passkey(&c, "a1", "pk2", 2);
        let old = signed_in(&c, "a1", Via::Github, REAUTH_PLUS);
        let (st, v) = c.as_person(&old, "POST", "/api/me/passkeys/reset", None).await;
        assert_eq!((st, v["reason"].as_str()), (403, Some(passkey::REAUTH)), "{v}");

        // With a device used just now: it starts all the same.
        let starter = signed_in(&c, "a1", Via::Github, 0);
        let (st, v) = c.as_person(&starter, "POST", "/api/me/passkeys/reset", None).await;
        assert_eq!(st, 200, "{v}");
        let started_at = v["pending"]["started_at"].as_u64().unwrap();
        let (st, v) = c.as_person(&starter, "POST", "/api/me/passkeys/reset", None).await;
        assert_eq!((st, v["reason"].as_str()), (409, Some("pending")), "{v}");
        assert!(c.app.db.notices("a1").unwrap().iter().any(|(_, t, _)| t == "A reset of your passkeys started"));
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
        assert_eq!(mailed.subject, "Arugula: A reset of your passkeys started");
        assert_eq!(c.app.db.pending_reset("a1").unwrap(), None, "no device reset");

        // Another sign-in: a session alone can't cancel; a passkey can.
        let other = signed_in(&c, "a1", Via::Github, 0);
        let (_, v) = c.as_person(&other, "GET", "/api/me/passkeys/reset", None).await;
        let ch = v["cancel"]["challenge"].as_str().unwrap().to_owned();
        assert_eq!(ch, passkey_cancel_challenge("a1", started_at));
        assert_eq!(v["cancel"]["allowCredentials"].as_array().unwrap().len(), 2, "{v}");
        let cancel = "/api/me/passkeys/reset/cancel";
        let (st, _) = c.as_person(&other, "POST", cancel, Some(json!({}))).await;
        assert_eq!(st, 403);
        let wrong = assert_with(&c, "pk1", &k1, "bm90IG91cnM");
        let (st, _) = c.as_person(&other, "POST", cancel, Some(json!({ "passkey": wrong }))).await;
        assert_eq!(st, 403, "over another challenge");
        let (st, v) =
            c.as_person(&other, "POST", cancel, Some(json!({ "passkey": assert_with(&c, "pk1", &k1, &ch) }))).await;
        assert_eq!(st, 200, "{v}");
        assert_eq!(c.app.db.passkey_count("a1").unwrap(), 2, "nothing reset");
        // A cancel by something the account trusts holds the next one off
        // for a week, and says so (DK10: no endless restarts).
        let said = c.app.db.notices("a1").unwrap();
        assert!(said.iter().any(|(_, t, b)| t == "The reset of your passkeys was cancelled" && b.contains("7 days")));
        let (st, v) = c.as_person(&starter, "POST", "/api/me/passkeys/reset", None).await;
        assert_eq!((st, v["reason"].as_str()), (409, Some("cancelled_recently")), "{v}");
        let week_ago = |c: &Control| {
            c.app
                .db
                .conn()
                .execute("UPDATE reset_requests SET cancelled_at = cancelled_at - 8 * 24 * 3600 * 1000", [])
                .unwrap();
        };
        week_ago(&c);

        // Again, cancelled by a device the account trusts.
        let (st, v) = c.as_person(&starter, "POST", "/api/me/passkeys/reset", None).await;
        assert_eq!(st, 200, "{v}");
        let at = v["pending"]["started_at"].as_u64().unwrap();
        let sig = hex::encode(root.signature(passkey_cancel_body("a1", at).as_bytes()));
        let (st, v) = c.as_person(&other, "POST", cancel, Some(json!({ "device": rid, "sig": sig }))).await;
        assert_eq!(st, 200, "{v}");
        week_ago(&c);

        // A third time, left to run: once due, a cancel is too late, and the
        // next read does it.
        let (st, _) = c.as_person(&starter, "POST", "/api/me/passkeys/reset", None).await;
        assert_eq!(st, 200);
        c.app.db.conn().execute("UPDATE reset_requests SET due = 1 WHERE kind = 'passkeys'", []).unwrap();
        let ch = c.as_person(&other, "GET", "/api/me/passkeys/reset", None).await;
        assert_eq!(ch.1["done"], true, "{}", ch.1);
        let (st, v) = c.as_person(&starter, "POST", cancel, Some(json!({}))).await;
        assert_eq!((st, v["reason"].as_str()), (409, Some("already_done")), "the finish won: {v}");
        let (_, v) = (ch.0, ch.1);
        assert_eq!(v["done"], true, "{v}");
        assert_eq!(c.app.db.passkey_count("a1").unwrap(), 0);
        let log = c.app.db.passkey_log("a1").unwrap();
        assert_eq!(log.events.iter().filter(|e| e.by == "reset").count(), 2);
        assert_eq!(c.app.db.account("a1").unwrap().unwrap().root.as_deref(), Some(rid.as_str()), "devices stay");
        assert_eq!(c.app.db.last_reset("a1").unwrap(), None, "no device reset");
        assert!(c.app.db.notices("a1").unwrap().iter().any(|(_, t, _)| t == "Your passkeys were reset"));
        // Once a week.
        let (st, v) = c.as_person(&signed_in(&c, "a1", Via::Github, 0), "POST", "/api/me/passkeys/reset", None).await;
        assert_eq!((st, v["reason"].as_str()), (409, Some("reset_recently")), "{v}");
    }

    /// The mail the fake mailer took, waiting a little for background sends.
    async fn mailed(sent: &std::sync::Arc<std::sync::Mutex<Vec<mail::Sent>>>, n: usize) -> Vec<mail::Sent> {
        for _ in 0..250 {
            if sent.lock().unwrap().len() >= n {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        }
        sent.lock().unwrap().clone()
    }

    /// DK10 security review, item 1: a passkey reset starts only when its
    /// warning reaches a verified address that didn't change in the last
    /// two days, and its email is neither counted against nor stopped by
    /// the daily limit. One whose email doesn't go isn't started.
    #[tokio::test]
    async fn a_passkey_reset_starts_only_when_its_warning_goes() {
        let (c, sent) = with_mail().await;
        person(&c.app, "lex00", "a1");
        site_passkey(&c, "a1", "pk1", 1);
        let starter = signed_in(&c, "a1", Via::Github, 0);
        let start = "/api/me/passkeys/reset";
        let (st, v) = c.as_person(&starter, "POST", start, None).await;
        assert_eq!((st, v["reason"].as_str()), (409, Some("no_email")), "{v}");
        assert!(v["error"].as_str().unwrap_or_default().contains("contact support"), "{v}");
        c.app.db.email_from_github("a1", "lex@example.com", now_ms()).unwrap();
        let (st, v) = c.as_person(&starter, "POST", start, None).await;
        assert_eq!((st, v["reason"].as_str()), (409, Some("email_changed")), "{v}");
        c.app.db.conn().execute("UPDATE account_emails SET updated_at = 1", []).unwrap();
        // The day's limit used up: the reset's email goes all the same, and
        // doesn't count.
        for _ in 0..mail::DAILY_LIMIT {
            c.app.db.record_send("a1", "other", now_ms(), true).unwrap();
        }
        let (st, v) = c.as_person(&starter, "POST", start, None).await;
        assert_eq!(st, 200, "{v}");
        let got = mailed(&sent, 1).await;
        assert_eq!(got.last().map(|m| m.subject.as_str()), Some("Arugula: A reset of your passkeys started"));
        assert_eq!(c.app.db.sends_since("a1", 0).unwrap(), mail::DAILY_LIMIT, "not counted");

        // A mail server that's down: not started, and the caller hears why.
        let c = control(|app| app.mail = Mailer::Failing).await;
        person(&c.app, "lex00", "a1");
        site_passkey(&c, "a1", "pk1", 1);
        c.app.db.email_from_github("a1", "lex@example.com", 1).unwrap();
        let starter = signed_in(&c, "a1", Via::Github, 0);
        let (st, v) = c.as_person(&starter, "POST", start, None).await;
        assert_eq!((st, v["reason"].as_str()), (503, Some("email_failed")), "{v}");
        assert_eq!(c.app.db.pending_reset_of("a1", Reset::Passkeys).unwrap(), None, "not started");
        // A device reset still starts then, but leaves the passkeys.
        let (st, v) = c.as_person(&starter, "POST", "/api/me/reset", Some(json!({}))).await;
        assert_eq!((st, v["pending"]["passkeys"].as_bool()), (200, Some(false)), "{v}");
    }

    /// DK10 security review, items 2 and 4: the device reset drops
    /// passkeys, so a working passkey cancels it too; a cancel once it's
    /// due is too late (409), and says so rather than "nothing changed".
    #[tokio::test]
    async fn a_passkey_cancels_a_device_reset_unless_it_is_due() {
        let (c, _) = with_mail().await;
        person(&c.app, "lex00", "a1");
        c.app.db.email_from_github("a1", "lex@example.com", 1).unwrap();
        let k1 = site_passkey(&c, "a1", "pk1", 1);
        let starter = signed_in(&c, "a1", Via::Github, 0);
        let (st, v) = c.as_person(&starter, "POST", "/api/me/reset", Some(json!({}))).await;
        assert_eq!(st, 200, "{v}");
        let started_at = v["pending"]["started_at"].as_u64().unwrap();
        let other = signed_in(&c, "a1", Via::Github, 0);
        let (_, v) = c.as_person(&other, "GET", "/api/me/reset", None).await;
        let ch = v["cancel"]["challenge"].as_str().unwrap().to_owned();
        assert_eq!(ch, cancel_challenge("a1", started_at));
        let wrong = assert_with(&c, "pk1", &k1, &passkey_cancel_challenge("a1", started_at));
        let (st, _) = c.as_person(&other, "POST", "/api/me/reset/cancel", Some(json!({ "passkey": wrong }))).await;
        assert_eq!(st, 403, "the passkey reset's challenge isn't this one's");
        let ok = assert_with(&c, "pk1", &k1, &ch);
        let (st, v) = c.as_person(&other, "POST", "/api/me/reset/cancel", Some(json!({ "passkey": ok }))).await;
        assert_eq!(st, 200, "{v}");
        assert_eq!(c.app.db.pending_reset("a1").unwrap(), None);
        assert!(c.app.db.account("a1").unwrap().unwrap().root.is_some(), "nothing reset");
        // That holds passkey resets off for a week (item 3).
        let (st, v) = c.as_person(&starter, "POST", "/api/me/passkeys/reset", None).await;
        assert_eq!((st, v["reason"].as_str()), (409, Some("cancelled_recently")), "{v}");

        // Started again, and due: a cancel is too late.
        let (st, _) = c.as_person(&starter, "POST", "/api/me/reset", Some(json!({}))).await;
        assert_eq!(st, 200);
        c.app.db.conn().execute("UPDATE reset_requests SET due = 1 WHERE cancelled_at IS NULL", []).unwrap();
        let (st, v) = c.as_person(&starter, "POST", "/api/me/reset/cancel", Some(json!({}))).await;
        assert_eq!((st, v["reason"].as_str()), (409, Some("already_done")), "{v}");
        let nothing = |b: &str| b.contains("devices was cancelled: nothing changed");
        let n = c.app.db.notices("a1").unwrap().iter().filter(|(_, _, b)| nothing(b)).count();
        assert_eq!(n, 1, "only the first cancel's; none for the late one");
    }
}
