//! Control's API: devices and approvals, daemons joining, the directory.
//!
//! Control checks what it's sent with the same rules daemons use
//! ([`arugula_e2e::Trust`]), so it never stores an approval that a daemon
//! would throw away. But it is not the authority: daemons and browsers
//! check again against the root they pinned.

use std::sync::Arc;

use arugula_control_wire as wire;
use arugula_e2e::{
    Cert, Kind, Refusal, Revocation, Trust,
    cert::{device_pin_ok, join_code, normalize_code},
    now_ms,
    team::{TeamPin, TeamRole},
};
use axum::{
    Json,
    extract::{Path, Query, State},
    http::StatusCode,
    response::{IntoResponse, Response},
};
use serde::Deserialize;
use serde_json::{Value, json};
use tracing::warn;

use crate::{
    ApiError, App,
    auth::{DaemonAuth, PageSession, Session, hash, token},
    err, refusal,
};

type R = Result<Json<Value>, ApiError>;

pub async fn me(State(app): State<Arc<App>>, s: Session) -> R {
    let a = app.db.account(&s.account)?.ok_or_else(|| err(StatusCode::UNAUTHORIZED, "no such account"))?;
    let passkeys = app.db.passkey_count(&a.id)?;
    Ok(Json(json!({ "account": a.id, "login": a.login, "name": a.name, "root": a.root, "passkeys": passkeys })))
}

/// A display name as people type it: trimmed, spaces folded, no control
/// characters, 1 to 64 characters.
pub fn display_name(s: &str) -> Result<String, ApiError> {
    let name = s.split_whitespace().collect::<Vec<_>>().join(" ");
    if name.is_empty() || name.chars().count() > 64 || name.chars().any(char::is_control) {
        return Err(err(StatusCode::BAD_REQUEST, "a name of 1 to 64 characters"));
    }
    Ok(name)
}

#[derive(Deserialize)]
pub struct Name {
    name: String,
}

/// Change what other people see (#102). Rosters keep the name an owner
/// signed until they sign the next one.
pub async fn set_name(State(app): State<Arc<App>>, s: Session, Json(b): Json<Name>) -> R {
    let name = display_name(&b.name)?;
    app.db.set_name(&s.account, &name)?;
    // Daemons that let this account in call it by name.
    let mut ds: Vec<String> = app.db.daemons(&s.account)?.into_iter().map(|d| d.id).collect();
    ds.extend(crate::teams::reachable(&app, &s.account)?);
    app.relay.nudge(&ds);
    app.analytics.capture(&s.account, "name changed", json!({}));
    Ok(Json(json!({ "name": name })))
}

/// What the account trusts now, by control's own reckoning.
fn trusted(app: &App, account: &str) -> anyhow::Result<(Option<Trust>, Vec<Cert>, Vec<Revocation>)> {
    let root = app.db.account(account)?.and_then(|a| a.root);
    let (certs, _) = app.db.devices(account)?;
    let revs = app.db.revocations(account)?;
    Ok((root.map(|root| Trust { account: account.to_owned(), root }), certs, revs))
}

/// The account's daemons should look at its certificates again now, and
/// so should the hosted machines of its projects (M180).
fn nudge(app: &App, account: &str) {
    let mut ids: Vec<String> =
        app.db.daemons(account).map(|ds| ds.into_iter().map(|d| d.id).collect()).unwrap_or_default();
    ids.extend(crate::hosted::of_account(app, account));
    app.relay.nudge(&ids);
}

/// A refused enrolment or approval, in the log (#94): which request, the
/// reason, and the ids. Never a signature or a body.
/// The ids are the request's own, so they're clipped and quoted.
fn refused(what: &str, account: &str, c: &Cert, why: &str) {
    let clip = |s: &str| s.chars().take(64).collect::<String>();
    warn!(what, account, device = ?clip(&c.device), kind = c.kind.as_str(), approver = ?clip(&c.approver), why, "refused");
}

/// How far a revocation's time may be from control's clock: one backdated
/// would take back what the revoked device approved since.
const REVOKE_SKEW_MS: u64 = 10 * 60 * 1000;

/// A removed key, approved again (#330's words).
const REVOKED: &str = "that key was removed from this account, so it can't be approved again: it needs a new key";

/// What the person who approved reads when control refuses (#327): what
/// failed and what to do. Pages show their own words by the reason code.
fn refused_say(r: Refusal) -> &'static str {
    match r {
        Refusal::Revoked => REVOKED,
        Refusal::ApproverUntrusted => {
            "the approving device isn't one this account trusts any more: approve from another of your devices, or enroll this one again (a recovery code approves it)"
        }
        Refusal::BadSignature => {
            "the approval isn't signed by the approving device's key (its key changed?): enroll that device again, then approve"
        }
        Refusal::CantApprove => "a machine can't approve devices: approve from a browser, phone or the CLI",
        Refusal::RecoveryForMachine => {
            "a recovery code approves browsers and phones, not machines: approve the machine from one of your devices"
        }
        Refusal::NeedsBrowser => {
            "only a browser or a phone approves a browser, a phone or recovery codes: the desktop app and the CLI approve machines, CLIs and apps"
        }
        Refusal::NoChain => {
            "the approval doesn't chain to this account's first device (its form, kind or time): reload and try again"
        }
    }
}

/// `cert` checks out against the account's trusted devices. `what` names
/// the request, for the log. Refused, the 403 says which check failed, as
/// a `reason` code and a sentence (#327).
fn approval_ok(app: &App, account: &str, cert: &Cert, what: &str) -> Result<(), ApiError> {
    let (trust, certs, revs) = trusted(app, account)?;
    let Some(trust) = trust else {
        refused(what, account, cert, "the account has no devices yet");
        return Err(refusal(StatusCode::CONFLICT, "no_devices", "this account has no devices yet"));
    };
    // A removed key never counts again (#330): say so, not that it
    // doesn't chain. `revoked` says so to pages (#327).
    if revs.iter().any(|r| r.device == cert.device) {
        refused(what, account, cert, "this device was removed from the account (revoked): it needs a new key");
        return Err(refusal(StatusCode::FORBIDDEN, Refusal::Revoked.code(), REVOKED));
    }
    if let Some(r) = trust.refusal(&certs, &revs, cert) {
        refused(what, account, cert, r.check());
        return Err(refusal(StatusCode::FORBIDDEN, r.code(), refused_say(r)));
    }
    // A browser or a recovery code (which makes browsers) comes only from
    // a browser or a phone whose every link back to the root, the root
    // included, is one too (`Cert::is_browser`): not the desktop app's
    // devices from before it had a kind of its own (`browser` ones it
    // named), nor anything they certified, from now on. What they approved
    // before stands, and those control stored before then count on the
    // way (`legacy_app`), never as the approver. A recovery code approves a
    // browser when the browser that made the code is one all the way back.
    if matches!(cert.kind, Kind::Browser | Kind::Recovery) {
        let legacy = crate::legacy_app::devices(&app.db, account)?;
        let by = trust.evaluate(&certs, &revs).get(&cert.approver).cloned();
        let from = match &by {
            Some(a) if a.kind == Kind::Recovery && cert.kind == Kind::Browser => Some(a.approver.clone()),
            Some(a) => Some(a.device.clone()),
            None => None,
        };
        if !from.is_some_and(|d| browser_all_the_way(&trust, &certs, &d, &legacy)) {
            let r = Refusal::NeedsBrowser;
            refused(what, account, cert, r.check());
            return Err(refusal(StatusCode::FORBIDDEN, r.code(), refused_say(r)));
        }
    }
    Ok(())
}

/// Whether `device`, and every device back to the account's root (the root
/// included), is a browser's or a phone's own (`Cert::is_browser`): not a
/// CLI, not the desktop app, not the app's devices from before it had a
/// kind. What may make a browser, a recovery code, a team's owner or
/// editor, or remove a browser, here. Recovery codes count on the way, and
/// so do the app's windows from before it had a kind that control stored
/// before then (`legacy`: `legacy_app::devices` of the account `trust`
/// names), never as `device` itself.
pub(crate) fn browser_all_the_way(trust: &Trust, certs: &[Cert], device: &str, legacy: &[String]) -> bool {
    trust.chained(certs, device, |c| arugula_e2e::cert::browser_or_legacy_link(c, device, legacy))
}

#[derive(Deserialize)]
pub struct Enroll {
    cert: Cert,
    /// The machine's join code this browser came to approve (#326): a
    /// browser that isn't one of the account's devices yet can't, so the
    /// device that approves it is shown the machine alongside.
    #[serde(default)]
    join: Option<String>,
    /// What a waiting device understands (#761): `device-secret`, it shows
    /// a code, and the device that approves it answers with a MAC.
    #[serde(default)]
    features: String,
    /// Approving: the approving device's MAC under the code the waiting
    /// device shows (#761), passed on to it: control never sees the code's
    /// secret.
    #[serde(default)]
    mac: Option<String>,
}

/// A browser (or the desktop app's window) asks to join the account, from
/// control's page: by its cookie, never a CLI's signature. The first device
/// is self-signed and trusted on first use; later ones wait for an
/// approval, unless they come already signed by a device the account
/// trusts. The app's window, signed in by a browser's hand-over, asks as an
/// `app` device only.
pub async fn enroll(State(app): State<Arc<App>>, s: PageSession, Json(b): Json<Enroll>) -> R {
    let c = b.cert;
    // The sign-in that asked is this device's (Sign-in and Devices say so).
    if c.account == s.account {
        app.db.set_session_device(&s.token_hash, &s.account, &c.device)?;
    }
    let join = b.join.as_deref().and_then(|j| normalize_code(j).ok());
    if let Err(e) = c.check_request() {
        refused("enroll", &s.account, &c, &e.to_string());
        return Err(err(StatusCode::BAD_REQUEST, &e.to_string()));
    }
    if c.account != s.account || !c.kind.connects() {
        refused("enroll", &s.account, &c, "not a browser, app or CLI certificate for this account");
        return Err(err(StatusCode::BAD_REQUEST, "a browser, app or CLI certificate for your account"));
    }
    let via = app.db.session_info(&s.token_hash, now_ms())?.and_then(|i| i.via);
    if via.as_deref() == Some(crate::auth::Via::App.as_str()) && c.kind != Kind::App {
        refused("enroll", &s.account, &c, "the desktop app's sign-in asked as another kind");
        return Err(err(StatusCode::BAD_REQUEST, "the desktop app's window joins as an app device"));
    }
    let account = app.db.account(&s.account)?.ok_or_else(|| err(StatusCode::UNAUTHORIZED, "no such account"))?;
    if let Some((have, true)) = app.db.device(&s.account, &c.device)? {
        return Ok(Json(json!({ "approved": true, "cert": have, "root": account.root })));
    }
    if b.features.len() > 200 || !b.features.bytes().all(|b| b.is_ascii_lowercase() || b == b'-' || b == b',') {
        return Err(err(StatusCode::BAD_REQUEST, "features are lowercase words, comma-separated"));
    }
    match account.root {
        None => {
            if !c.kind.roots() {
                refused("enroll", &s.account, &c, "the desktop app can't be the first device");
                return Err(refusal(
                    StatusCode::CONFLICT,
                    "first_in_browser",
                    "your account's first device is a browser: sign in to Arugula in your browser first, then here",
                ));
            }
            if let Err(e) = c.check_form() {
                refused("enroll", &s.account, &c, &e.to_string());
                return Err(err(StatusCode::BAD_REQUEST, &e.to_string()));
            }
            if c.approver != c.device || !c.signed_by(&c) {
                refused("enroll", &s.account, &c, "the first device's certificate isn't signed by itself");
                return Err(err(StatusCode::BAD_REQUEST, "the first device signs its own certificate"));
            }
            app.db.put_device(&c, true, now_ms())?;
            app.analytics.capture(
                &s.account,
                "device enrolled",
                json!({ "kind": c.kind.as_str(), "first": true, "presigned": false }),
            );
            Ok(Json(json!({ "approved": true, "cert": c, "root": c.device })))
        }
        Some(root) => {
            let new = app.db.device(&s.account, &c.device)?.is_none();
            if new {
                app.limits.check_account(crate::limit::ENROLLS, &s.account)?;
            }
            // Already signed by one of the account's devices (#758: the
            // browser that allowed the desktop app's sign-in signed the
            // app's certificate there): approved now, with no prompt. One
            // that doesn't check out waits for an approval like any other.
            if !c.approver.is_empty()
                && c.approver != c.device
                && c.check_form().is_ok()
                && approval_ok(&app, &s.account, &c, "enroll").is_ok()
            {
                app.db.put_device(&c, true, now_ms())?;
                nudge(&app, &s.account);
                app.analytics.capture(
                    &s.account,
                    "device enrolled",
                    json!({ "kind": c.kind.as_str(), "first": false, "presigned": true }),
                );
                return Ok(Json(json!({ "approved": true, "cert": c, "root": root })));
            }
            app.db.put_device(&Cert { approver: String::new(), sig: String::new(), ..c.clone() }, false, now_ms())?;
            app.db.set_device_join(&s.account, &c.device, join.as_deref())?;
            app.db.set_device_features(&s.account, &c.device, &b.features)?;
            // The account's other devices hear of it once (#104), not on
            // every reload of the waiting page.
            if new {
                app.analytics.capture(
                    &s.account,
                    "device requested",
                    json!({ "kind": c.kind.as_str(), "with_machine": join.is_some() }),
                );
                let (title, body) = match join {
                    Some(_) => (
                        "A new browser and a machine want into your account",
                        "Open Arugula to check them and approve both at once.",
                    ),
                    None => (
                        "A new browser wants into your account",
                        "Open Arugula to check its fingerprint and approve it.",
                    ),
                };
                crate::push::notify(&app, vec![s.account.clone()], "control-device", title.into(), body.into());
            }
            Ok(Json(json!({ "approved": false, "root": root })))
        }
    }
}

#[derive(Deserialize)]
pub struct RecoveryCerts {
    certs: Vec<Cert>,
    /// New codes replace the old (#106): a revocation for each one still
    /// good, signed by the device making the new ones.
    #[serde(default)]
    revocations: Vec<Revocation>,
}

/// Recovery codes: certificates a device signed for keys only the person
/// holds (on paper). Control never sees the keys. The first device makes
/// them; any device can make new ones, which retire the old.
pub async fn add_recovery(State(app): State<Arc<App>>, s: PageSession, Json(b): Json<RecoveryCerts>) -> R {
    if b.certs.is_empty() || b.certs.len() > 4 {
        return Err(err(StatusCode::BAD_REQUEST, "one to 4 recovery codes"));
    }
    for c in &b.certs {
        if c.kind != Kind::Recovery || c.account != s.account {
            return Err(err(StatusCode::BAD_REQUEST, "recovery certificates for your account"));
        }
        approval_ok(&app, &s.account, c, "recovery")?;
    }
    let (trust, certs, revs) = trusted(&app, &s.account)?;
    // New codes are new keys: one the account has (or had) isn't.
    for c in &b.certs {
        if app.db.device(&s.account, &c.device)?.is_some() || revs.iter().any(|r| r.device == c.device) {
            return Err(err(StatusCode::CONFLICT, "that recovery code is one this account has already: make new ones"));
        }
    }
    let now = trust.as_ref().map(|t| t.evaluate(&certs, &revs)).unwrap_or_default();
    let legacy = crate::legacy_app::devices(&app.db, &s.account)?;
    for r in &b.revocations {
        // The old codes go only by a browser or a phone all the way back,
        // and now: one backdated would take out the browsers they let in.
        let ok = now.get(&r.device).is_some_and(|c| c.kind == Kind::Recovery)
            && now.get(&r.by).is_some_and(|c| c.kind.connects() && r.account == s.account && r.signed_by(c))
            && trust.as_ref().is_some_and(|t| browser_all_the_way(t, &certs, &r.by, &legacy))
            && r.at.abs_diff(now_ms()) <= REVOKE_SKEW_MS;
        if !ok {
            return Err(err(StatusCode::FORBIDDEN, "that revocation doesn't check out"));
        }
    }
    let old = now.devices.values().filter(|c| c.kind == Kind::Recovery).count();
    let retired = now
        .devices
        .values()
        .filter(|c| c.kind == Kind::Recovery && b.revocations.iter().any(|r| r.device == c.device))
        .count();
    if retired != old {
        return Err(err(StatusCode::CONFLICT, "new recovery codes replace the old ones: revoke every one still good"));
    }
    for c in &b.certs {
        app.db.put_device(c, true, now_ms())?;
    }
    for r in &b.revocations {
        app.db.add_revocation(r)?;
    }
    if !b.revocations.is_empty() {
        nudge(&app, &s.account);
    }
    app.analytics.capture(
        &s.account,
        "recovery codes made",
        json!({ "codes": b.certs.len(), "replaced": b.revocations.len() }),
    );
    Ok(Json(json!({})))
}

pub async fn devices(State(app): State<Arc<App>>, s: Session) -> R {
    let (trust, certs, revs) = trusted(&app, &s.account)?;
    let (_, pending) = app.db.devices(&s.account)?;
    // #326: the machine a waiting browser came to approve, by its code,
    // while that join is open.
    let joins: serde_json::Map<String, Value> =
        app.db.device_joins(&s.account, now_ms())?.into_iter().map(|(d, code)| (d, Value::String(code))).collect();
    // When each device last loaded the page, where control has seen it.
    let used: serde_json::Map<String, Value> =
        app.db.devices_used(&s.account)?.into_iter().map(|(d, at)| (d, Value::from(at))).collect();
    // #761: which waiting devices show a code, for the page to ask for it.
    let features: serde_json::Map<String, Value> =
        app.db.device_features(&s.account)?.into_iter().map(|(d, f)| (d, Value::String(f))).collect();
    // #764: devices whose keys come from a passkey (they pinned their root),
    // for the page to say removing one signs out every browser with it.
    let passkeys = app.db.pinned_devices(&s.account)?;
    // The app's windows control grandfathered (`legacy_app`), for the page
    // to count them on the way back as control does.
    let legacy_app = crate::legacy_app::devices(&app.db, &s.account)?;
    Ok(Json(json!({
        "trust": trust, "certs": certs, "revocations": revs, "pending": pending, "joins": joins, "used": used,
        "features": features, "passkeys": passkeys, "legacy_app": legacy_app,
    })))
}

pub async fn device(State(app): State<Arc<App>>, s: Session, Path(id): Path<String>) -> Result<Response, ApiError> {
    let Some((cert, approved)) = app.db.device(&s.account, &id)? else {
        // Turned down (#105): say by which device, for the one waiting.
        let by = app.db.turned_down(&s.account, &id)?;
        let body = match by {
            Some(by) => json!({ "error": "this device's request was turned down", "turned_down": true, "by": by }),
            None => json!({ "error": "no such device" }),
        };
        return Ok((StatusCode::NOT_FOUND, Json(body)).into_response());
    };
    // #761: the approving device's answer to the code it showed, said
    // only when there is one.
    let mut body = json!({ "approved": approved, "cert": cert });
    if approved && let Some(mac) = app.db.device_mac(&s.account, &id)? {
        body["mac"] = Value::String(mac);
    }
    // #764: the device's own signature over the account's root, for a
    // browser that derives its keys from the same passkey to check.
    if approved && let Some(pin) = app.db.device_pin(&s.account, &id)? {
        body["pin"] = Value::String(pin);
    }
    Ok(Json(body).into_response())
}

#[derive(Deserialize)]
pub struct Pin {
    pin: String,
}

/// `POST /api/devices/{id}/pin` (#764): a browser whose keys come from a
/// passkey's PRF signs the account's root with them once it's approved.
/// Another browser with that passkey derives the same keys and checks this
/// before it takes the root, so control can't hand it an account of its
/// own. Kept only if it's that device's signature over this account's root.
pub async fn pin(State(app): State<Arc<App>>, s: PageSession, Path(id): Path<String>, Json(b): Json<Pin>) -> R {
    let (trust, certs, revs) = trusted(&app, &s.account)?;
    let trust = trust.ok_or_else(|| err(StatusCode::CONFLICT, "this account has no devices yet"))?;
    let now = trust.evaluate(&certs, &revs);
    let cert =
        now.get(&id).filter(|c| c.kind == Kind::Browser).ok_or_else(|| err(StatusCode::NOT_FOUND, "no such device"))?;
    if !device_pin_ok(&trust, cert, &b.pin) {
        return Err(err(StatusCode::BAD_REQUEST, "that isn't this device's signature over the account's root"));
    }
    app.db.set_device_pin(&s.account, &id, &b.pin)?;
    Ok(Json(json!({})))
}

pub async fn approve(State(app): State<Arc<App>>, s: PageSession, Path(id): Path<String>, Json(b): Json<Enroll>) -> R {
    let (pending, approved) =
        app.db.device(&s.account, &id)?.ok_or_else(|| err(StatusCode::NOT_FOUND, "no such device"))?;
    if approved {
        return Ok(Json(json!({ "approved": true })));
    }
    if b.cert.device != id || b.cert.account != s.account || !b.cert.same_request(&pending) {
        refused("approve", &s.account, &b.cert, "not the certificate that asked");
        return Err(err(StatusCode::BAD_REQUEST, "that's not the certificate that asked"));
    }
    mac_ok(&b.mac)?;
    approval_ok(&app, &s.account, &b.cert, "approve")?;
    app.db.put_device(&b.cert, true, now_ms())?;
    app.db.set_device_mac(&s.account, &id, b.mac.as_deref())?;
    nudge(&app, &s.account);
    app.analytics.capture(
        &s.account,
        "device approved",
        json!({ "kind": b.cert.kind.as_str(), "with_code": b.mac.is_some() }),
    );
    Ok(Json(json!({ "approved": true })))
}

#[derive(Deserialize, Default)]
pub struct Reject {
    /// The device turning it down, to name to the one waiting.
    #[serde(default)]
    by: Option<String>,
}

pub async fn reject(State(app): State<Arc<App>>, s: Session, Path(id): Path<String>, b: Option<Json<Reject>>) -> R {
    let by = b.unwrap_or_default().0.by;
    let name = match by {
        Some(by) => app.db.device(&s.account, &by)?.filter(|(_, ok)| *ok).map(|(c, _)| c.name).unwrap_or_default(),
        None => String::new(),
    };
    app.db.turn_down(&s.account, &id, &name, now_ms())?;
    app.analytics.capture(&s.account, "device turned down", json!({}));
    Ok(Json(json!({})))
}

#[derive(Deserialize)]
pub struct Revoke {
    revocation: Revocation,
}

pub async fn revoke(State(app): State<Arc<App>>, s: Session, Json(b): Json<Revoke>) -> R {
    let r = b.revocation;
    let (trust, certs, revs) = trusted(&app, &s.account)?;
    let trust = trust.ok_or_else(|| err(StatusCode::CONFLICT, "no devices"))?;
    let now = trust.evaluate(&certs, &revs);
    let signer = now.get(&r.by).filter(|c| c.kind.approves() && r.account == s.account && r.signed_by(c));
    if signer.is_none() {
        return Err(err(StatusCode::FORBIDDEN, "that revocation doesn't check out"));
    }
    // A browser or a phone is removed only from a browser or a phone all
    // the way back to the root (daemons and pages refuse the rest too).
    if r.at.abs_diff(now_ms()) > REVOKE_SKEW_MS {
        return Err(err(StatusCode::BAD_REQUEST, "that revocation's time is too far from control's clock"));
    }
    // A recovery code counts as a browser here: one revoked backdated
    // would take out the browser it let in.
    let of_browser = certs.iter().any(|c| c.device == r.device && matches!(c.kind, Kind::Browser | Kind::Recovery));
    let legacy = crate::legacy_app::devices(&app.db, &s.account)?;
    if of_browser && !browser_all_the_way(&trust, &certs, &r.by, &legacy) {
        return Err(refusal(
            StatusCode::FORBIDDEN,
            Refusal::NeedsBrowser.code(),
            "only a browser or a phone removes a browser or a phone: the desktop app and the CLI remove machines, CLIs and apps",
        ));
    }
    // Only its own: a revoked machine is refused everywhere (#330), so
    // another account's isn't for this one to remove.
    if !app.db.is_accounts(&s.account, &r.device)? {
        return Err(err(StatusCode::FORBIDDEN, "that device isn't this account's"));
    }
    // A revoked daemon leaves the directory and the relay.
    let machine = app.db.daemon_account(&r.device)?.as_deref() == Some(s.account.as_str());
    app.analytics.capture(&s.account, "device removed", json!({ "machine": machine }));
    if machine {
        app.db.add_machine_revocation(&r)?;
        nudge(&app, &s.account);
        app.db.drop_daemon(&r.device)?;
        app.relay.drop_daemon(&r.device);
    } else {
        app.db.add_revocation(&r)?;
        nudge(&app, &s.account);
    }
    Ok(Json(json!({})))
}

/// Why a removed key can't come back (#330), when `device` was revoked:
/// the words, and when and by which device (that by name only with
/// `detail`, for the key's holder).
pub fn removed(app: &App, device: &str, kind: Kind, detail: bool) -> anyhow::Result<Option<(String, wire::RemovedAt)>> {
    let Some(r) = app.db.revoked(device)? else { return Ok(None) };
    let day = crate::day(r.at);
    if !detail {
        let msg = format!("this key was removed from its account on {day}: it needs a new key, then join again");
        return Ok(Some((msg, wire::RemovedAt { at: r.at, by: None })));
    }
    let what = if kind == Kind::Daemon { "this machine" } else { "this device" };
    let by = app.db.device(&r.account, &r.by)?.map(|(c, _)| c.name).filter(|n| !n.is_empty());
    let by_words = by.as_deref().map(|n| format!(" by {n}")).unwrap_or_default();
    let msg = format!(
        "{what} was removed from its account on {day}{by_words}, so its key can't join again: it needs a new key"
    );
    Ok(Some((msg, wire::RemovedAt { at: r.at, by })))
}

// ---------------------------------------------------------------- joining

/// How far a join proof's time may be from control's clock.
const PROOF_SKEW_MS: u64 = 5 * 60 * 1000;

/// What a daemon from before 0.17, which asks without its key's proof, is
/// told.
const JOIN_NEEDS_UPDATE: &str =
    "this machine's Arugula is too old for this control: update Arugula (0.17 or newer) on it, then run join again";

/// Whether the join request comes from the key's holder: an error if it
/// says so and doesn't, `false` if it doesn't say (a daemon from before
/// 0.17, which [`join`] turns away).
fn join_proven(app: &App, b: &wire::JoinRequest) -> Result<bool, ApiError> {
    let Some(p) = &b.proof else { return Ok(false) };
    let body = arugula_e2e::cert::join_proof_body(&b.cert, p.ms);
    if !arugula_e2e::cert::verify_hex(&b.cert.sign, body.as_bytes(), &p.sig) {
        return Err(err(StatusCode::UNAUTHORIZED, "the join request isn't signed by the key it asks with"));
    }
    if now_ms().abs_diff(p.ms) > PROOF_SKEW_MS {
        return Err(err(StatusCode::UNAUTHORIZED, "clock skew: check this machine's time"));
    }
    if !app.daemon_sigs.first(&format!("join {}", p.sig), p.ms + PROOF_SKEW_MS + 60_000) {
        return Err(err(StatusCode::UNAUTHORIZED, "that join request was used already; run join again"));
    }
    Ok(true)
}

/// A daemon that can't check this team's rosters (one was written with a
/// presigned invite) doesn't go into it.
fn can_follow(app: &App, team: &str, features: &str, name: &str) -> Result<(), ApiError> {
    if !crate::teams::takes_presigned(features) && crate::teams::has_presigned(app, team)? {
        return Err(err(
            StatusCode::CONFLICT,
            &format!(
                "{name} needs an update before it can join this team (its Arugula is older than the team's invites)"
            ),
        ));
    }
    Ok(())
}

fn check_urls(urls: &[String]) -> Result<(), ApiError> {
    if urls.len() > 8 || urls.iter().any(|u| u.len() > 256 || !(u.starts_with("https://") || u.starts_with("http://")))
    {
        return Err(err(StatusCode::BAD_REQUEST, "at most 8 http(s) URLs"));
    }
    Ok(())
}

/// A daemon asks to join; no account yet. It gets the code to show, and
/// a token to poll with.
pub async fn join(
    State(app): State<Arc<App>>,
    axum::extract::ConnectInfo(peer): axum::extract::ConnectInfo<std::net::SocketAddr>,
    headers: axum::http::HeaderMap,
    Json(b): Json<wire::JoinRequest>,
) -> Result<Response, ApiError> {
    app.limits.check(crate::limit::JOINS, app.limits.client_ip(peer, &headers))?;
    b.cert.check_request().map_err(|e| err(StatusCode::BAD_REQUEST, &e.to_string()))?;
    // A daemon, or the CLI (M49), which joins the same way: a code shown
    // where it runs, approved on a signed-in device.
    match b.cert.kind {
        Kind::Daemon => {}
        Kind::Cli if b.urls.is_empty() && b.team.is_none() && b.proof.is_some() => {}
        Kind::Cli => {
            return Err(err(StatusCode::BAD_REQUEST, "a CLI joins with its key's proof and nothing else"));
        }
        _ => return Err(err(StatusCode::BAD_REQUEST, "a daemon certificate")),
    }
    check_urls(&b.urls)?;
    // A daemon joins only with its key's proof: anyone may have a known
    // daemon's certificate. One from before 0.17 doesn't send it.
    let proven = join_proven(&app, &b)?;
    if !proven {
        return Err(err(StatusCode::UPGRADE_REQUIRED, JOIN_NEEDS_UPDATE));
    }
    // A removed key never joins again (#330), and gets no code: it says
    // when it was removed (and by which device, to the key's holder), and
    // that it needs a new key. `removed` says so to the program asking.
    if let Some((msg, removed)) = removed(&app, &b.cert.device, b.cert.kind, proven)? {
        let said = wire::RemovedAnswer { error: Some(msg), removed: Some(removed) };
        return Ok((StatusCode::GONE, crate::reply(&said)?).into_response());
    }
    let code = join_code(&b.cert);
    let poll = token();
    let team_name = match &b.team {
        Some(t) => {
            let name = app.db.team(t)?.ok_or_else(|| err(StatusCode::NOT_FOUND, "no such team"))?.name;
            can_follow(&app, t, &b.features, "this machine")?;
            Some(name)
        }
        None => None,
    };
    let asked =
        app.db.add_join(&code, &b.cert, &hash(&poll), &b.urls, b.team.as_deref(), &b.features, proven, now_ms())?;
    // Another join from this machine is waiting (#329), and this request
    // didn't prove it holds the key: it doesn't take that one over.
    if asked == crate::db::Asked::Taken {
        return Err(err(
            StatusCode::CONFLICT,
            &format!(
                "this machine has a join waiting already (code {code}): finish it there, or wait for it to expire"
            ),
        ));
    }
    app.analytics.capture_anon(
        &format!("device:{}", b.cert.device),
        "machine join started",
        json!({
            "kind": b.cert.kind.as_str(), "team": b.team.is_some(),
            "proven": proven, "urls": b.urls.len(),
        }),
    );
    let started = wire::JoinStarted { code, poll, expires_in_secs: crate::db::JOIN_TTL_MS / 1000, team_name };
    Ok(crate::reply(&started)?.into_response())
}

/// The daemon waits for someone to approve its code.
pub async fn join_poll(State(app): State<Arc<App>>, Path(code): Path<String>, Query(q): Query<wire::PollQuery>) -> R {
    let code = normalize_code(&code).map_err(|e| err(StatusCode::BAD_REQUEST, &e.to_string()))?;
    let j =
        app.db.join(&code, now_ms())?.ok_or_else(|| err(StatusCode::NOT_FOUND, "that code expired; run join again"))?;
    if j.poll_hash != hash(&q.poll) {
        // A later request from this machine took it over (#329).
        if j.replaced.split(' ').any(|h| h == hash(&q.poll)) {
            return Err(err(
                StatusCode::CONFLICT,
                "replaced by another join from this machine (Getting started, or `arugulad join`): finish it there",
            ));
        }
        return Err(err(StatusCode::FORBIDDEN, "not your join"));
    }
    if let Some(on) = j.rejected {
        app.db.drop_join(&code)?;
        return crate::reply(&wire::JoinPoll::turned_down(on));
    }
    let Some(account) = j.account else { return crate::reply(&wire::JoinPoll::waiting()) };
    let (trust, certs, revs) = trusted(&app, &account)?;
    app.db.drop_join(&code)?;
    // A team daemon pins the team's founder too, if the approver signed it in.
    let team = match &j.team {
        Some(t) => app.db.team(t)?.map(|t| wire::JoinTeam {
            team: t.id,
            founder: t.founder,
            founder_root: t.founder_root,
            name: t.name,
            sig: j.team_sig,
        }),
        None => None,
    };
    crate::reply(&wire::JoinPoll {
        approved: true,
        rejected: None,
        cert: Some(j.cert),
        trust,
        team,
        certs,
        revocations: revs,
        mac: j.mac,
    })
}

/// What a signed-in person sees before approving a code.
pub async fn join_show(State(app): State<Arc<App>>, _s: Session, Path(code): Path<String>) -> R {
    let code = normalize_code(&code).map_err(|e| err(StatusCode::BAD_REQUEST, &e.to_string()))?;
    let j = app
        .db
        .join(&code, now_ms())?
        .filter(|j| j.account.is_none() && j.rejected.is_none())
        .ok_or_else(|| err(StatusCode::NOT_FOUND, "no such code (expired?)"))?;
    // The team it asked for (`--team`): the approver may pick another.
    let team = match &j.team {
        Some(t) => app.db.team(t)?.map(|t| json!({ "team": t.id, "name": t.name })),
        None => None,
    };
    // `features` says whether it made a join secret (#760): then the page
    // answers with a MAC, from the whole code, and nothing is compared.
    Ok(Json(json!({
        "code": code, "cert": j.cert, "urls": j.urls, "created": j.created, "team": team, "features": j.features,
    })))
}

#[derive(Deserialize)]
pub struct JoinApprove {
    cert: Cert,
    /// The team the approver puts it in (one they own), and their signature
    /// over [`TeamPin::join_body`]; none for their own account (#100).
    #[serde(default)]
    team: Option<String>,
    #[serde(default)]
    team_sig: Option<String>,
    /// The approving device's MAC under the machine's join secret (#760),
    /// passed on to the machine: control never sees the secret.
    #[serde(default)]
    mac: Option<String>,
}

/// A join secret's MAC as the page sends it: HMAC-SHA256, hex.
fn mac_ok(mac: &Option<String>) -> Result<(), ApiError> {
    match mac {
        Some(m) if m.len() != 64 || !m.bytes().all(|b| b.is_ascii_hexdigit()) => {
            Err(err(StatusCode::BAD_REQUEST, "the approval's MAC is 64 hex digits"))
        }
        _ => Ok(()),
    }
}

pub async fn join_approve(
    State(app): State<Arc<App>>,
    s: PageSession,
    Path(code): Path<String>,
    Json(b): Json<JoinApprove>,
) -> R {
    let code = normalize_code(&code).map_err(|e| err(StatusCode::BAD_REQUEST, &e.to_string()))?;
    let j = app
        .db
        .join(&code, now_ms())?
        .filter(|j| j.account.is_none() && j.rejected.is_none())
        .ok_or_else(|| {
            warn!(what = "join_approve", account = %s.account, why = "no such code (expired, used or turned down)", "refused");
            err(StatusCode::NOT_FOUND, "no such code (expired?)")
        })?;
    mac_ok(&b.mac)?;
    let c = b.cert;
    if c.account != s.account || c.kind != j.cert.kind || !c.same_request(&j.cert) {
        refused("join_approve", &s.account, &c, "not the daemon that asked");
        return Err(err(StatusCode::BAD_REQUEST, "that's not the daemon that asked"));
    }
    approval_ok(&app, &s.account, &c, "join_approve")?;
    // The CLI (M49): one of the account's devices from now on, like a
    // browser it approved. The account's machines learn of it now.
    if c.kind == Kind::Cli {
        if b.team.is_some() {
            return Err(err(StatusCode::BAD_REQUEST, "a CLI joins your account, not a team"));
        }
        app.db.put_device(&c, true, now_ms())?;
        app.db.approve_join(&code, &c, None, b.mac.as_deref())?;
        nudge(&app, &s.account);
        app.analytics.capture(&s.account, "cli joined", json!({ "with_code": b.mac.is_some() }));
        return Ok(Json(json!({ "approved": true, "device": c.device })));
    }
    let known = app.db.device_known(&c.device)?;
    if known && !j.proven {
        refused("join_approve", &s.account, &c, "a known daemon's join without its key");
        return Err(err(StatusCode::CONFLICT, JOIN_NEEDS_UPDATE));
    }
    // A team's machine: any member adds one of their own (#332), and the
    // approving device signs it in, for the daemon to check.
    let team = match &b.team {
        Some(id) => {
            let pin = in_team(&app, &s.account, id, false, "only the team's members add machines to it")?;
            can_follow(&app, id, &j.features, &c.name)?;
            let sig = b.team_sig.as_deref().unwrap_or_default();
            let (trust, certs, revs) = trusted(&app, &s.account)?;
            let approver = trust.and_then(|t| t.evaluate(&certs, &revs).get(&c.approver).cloned());
            if !approver.is_some_and(|a| pin.join_signed_by(&c.device, &a, sig)) {
                refused("join_approve", &s.account, &c, "the team choice isn't signed by the approver");
                return Err(err(StatusCode::BAD_REQUEST, "the team choice isn't signed by the approving device"));
            }
            Some((pin.team, sig.to_owned()))
        }
        None => None,
    };
    // Its holder joined it again: whatever it was before goes.
    if known {
        app.db.drop_daemon(&c.device)?;
        app.relay.drop_daemon(&c.device);
    }
    app.db.put_device(&c, true, now_ms())?;
    app.db.put_daemon(&s.account, &c.device, &c.name, &j.urls)?;
    app.db.set_daemon_features(&c.device, &j.features)?;
    if let Some((team, _)) = &team {
        app.db.set_daemon_team(&c.device, team)?;
    }
    app.db.approve_join(&code, &c, team.as_ref().map(|(t, sig)| (t.as_str(), sig.as_str())), b.mac.as_deref())?;
    // M184: its projects' hosted machines let it drive them: they look
    // again now, not at their next refresh.
    app.relay.nudge(&crate::hosted::of_account(&app, &s.account));
    let props = json!({
        "rejoin": known, "with_code": b.mac.is_some(),
        "asked_for_team": j.team.is_some(), "urls": j.urls.len(),
    });
    match &team {
        Some((t, _)) => app.analytics.capture_team(&s.account, t, "machine joined", props),
        None => app.analytics.capture(&s.account, "machine joined", props),
    }
    Ok(Json(json!({ "approved": true, "daemon": c.device })))
}

#[derive(Deserialize)]
pub struct JoinReject {
    /// The device turning it down, to name to the daemon.
    #[serde(default)]
    device: String,
}

/// Cancel on the approval page: the daemon stops waiting (#100).
pub async fn join_reject(
    State(app): State<Arc<App>>,
    s: Session,
    Path(code): Path<String>,
    Json(b): Json<JoinReject>,
) -> R {
    let code = normalize_code(&code).map_err(|e| err(StatusCode::BAD_REQUEST, &e.to_string()))?;
    app.db
        .join(&code, now_ms())?
        .filter(|j| j.account.is_none() && j.rejected.is_none())
        .ok_or_else(|| err(StatusCode::NOT_FOUND, "no such code (expired?)"))?;
    let on = app.db.device(&s.account, &b.device)?.map(|(c, _)| c.name).unwrap_or_else(|| "a device".into());
    app.db.reject_join(&code, &on)?;
    app.analytics.capture(&s.account, "machine join turned down", json!({}));
    Ok(Json(json!({})))
}

/// A team `account` is in, as a daemon pins it; `no` when it isn't. Any
/// member adds their own machines (#332), but only owners while it's
/// locked; with `owner`, only owners at all.
fn in_team(app: &App, account: &str, id: &str, owner: bool, no: &str) -> Result<TeamPin, ApiError> {
    let t = app.db.team(id)?.ok_or_else(|| err(StatusCode::NOT_FOUND, "no such team"))?;
    let r = app.db.latest_roster(id)?.ok_or_else(|| err(StatusCode::NOT_FOUND, "no such team"))?;
    let r: arugula_e2e::team::Roster = serde_json::from_str(&r).map_err(anyhow::Error::from)?;
    let Some(role) = r.member(account).map(|m| m.role) else { return Err(err(StatusCode::FORBIDDEN, no)) };
    if role != TeamRole::Owner && (owner || t.locked) {
        return Err(err(
            StatusCode::FORBIDDEN,
            if owner { no } else { "the team is locked: only its owners add machines until it's unlocked" },
        ));
    }
    Ok(TeamPin { team: t.id, founder: t.founder, founder_root: t.founder_root })
}

/// How far a move's time may be from control's clock.
const MOVE_SKEW_MS: u64 = 10 * 60 * 1000;

/// *Move to…* on a machine (#100): its own account moves it into a team
/// it's in, between them, or back to the account (#332: members too).
/// A device of the account signs it; the daemon checks. A team's owners
/// may also take a member's machine out, signed by one of their devices,
/// which the daemon checks against the roster.
pub async fn move_daemon(
    State(app): State<Arc<App>>,
    s: Session,
    Path(id): Path<String>,
    Json(m): Json<arugula_e2e::team::Move>,
) -> R {
    let (owner, d) = app.db.daemon_row(&id)?.ok_or_else(|| err(StatusCode::NOT_FOUND, "no such machine"))?;
    let now = app.db.daemon_team(&id)?;
    if owner != s.account {
        let Some(t) = now.as_ref().filter(|_| m.team.is_none()) else {
            return Err(err(StatusCode::FORBIDDEN, "only the machine's own account moves it"));
        };
        in_team(&app, &s.account, t, true, "only the machine's own account and the team's owners take it out")?;
        if !crate::teams::has_feature(&app.db.daemon_features(&id)?, crate::teams::OWNER_MOVES) {
            return Err(err(
                StatusCode::CONFLICT,
                &format!(
                    "{} runs an older arugula: its owner updates it, then the team's owners can take it out",
                    d.name
                ),
            ));
        }
    }
    let pin = match &m.team {
        Some(p) => Some(in_team(&app, &s.account, &p.team, false, "only the team's members add machines to it")?),
        None => None,
    };
    if pin != m.team {
        return Err(err(StatusCode::BAD_REQUEST, "that's not the team's founder"));
    }
    if let Some(p) = &m.team
        && now.as_ref() != Some(&p.team)
    {
        can_follow(&app, &p.team, &app.db.daemon_features(&id)?, &d.name)?;
    }
    let last = app
        .db
        .daemon_moved(&id)?
        .and_then(|j| serde_json::from_str::<arugula_e2e::team::Move>(&j).ok())
        .map_or(0, |l| l.at);
    if m.at <= last || m.at.abs_diff(now_ms()) > MOVE_SKEW_MS {
        return Err(err(StatusCode::BAD_REQUEST, "that move is out of date; check this device's clock"));
    }
    let (trust, certs, revs) = trusted(&app, &s.account)?;
    let by = trust.and_then(|t| t.evaluate(&certs, &revs).get(&m.by).cloned());
    if !by.is_some_and(|by| m.signed_for(&id, &by)) {
        return Err(err(StatusCode::BAD_REQUEST, "the move isn't signed by one of your devices"));
    }
    app.db.move_daemon(
        &id,
        m.team.as_ref().map(|p| p.team.as_str()),
        &serde_json::to_string(&m).map_err(anyhow::Error::from)?,
    )?;
    app.relay.nudge(&[id]);
    let props = json!({ "into_team": m.team.is_some(), "from_team": now.is_some(), "own": owner == s.account });
    match m.team.as_ref().map(|p| &p.team).or(now.as_ref()) {
        Some(t) => app.analytics.capture_team(&s.account, t, "machine moved", props),
        None => app.analytics.capture(&s.account, "machine moved", props),
    }
    Ok(Json(json!({})))
}

// ---------------------------------------------------------------- daemons

/// The daemon's account's certificates, to evaluate against its root.
pub async fn daemon_trust(
    State(app): State<Arc<App>>,
    d: DaemonAuth,
    Query(q): Query<wire::Features>,
    Query(m): Query<crate::telemetry::Machine>,
) -> R {
    app.db.set_daemon_features(&d.cert.device, &q.features)?;
    crate::telemetry::machine_seen(&app, &d, &m)?;
    let (trust, certs, revs) = trusted(&app, &d.cert.account)?;
    // Its last move (#100), for the daemon to check and take.
    let moved = app.db.daemon_moved(&d.cert.device)?.and_then(|j| {
        serde_json::from_str(&j)
            .inspect_err(|e| warn!(daemon = %d.cert.device, error = %e, "a stored move doesn't parse; not sending it"))
            .ok()
    });
    // Labs flags its account and team set (#347).
    let flags = Some(crate::labs_flags::for_daemon(&app, &d.cert.account, &d.cert.device)?);
    crate::reply(&wire::TrustAnswer { trust, certs, revocations: revs, moved, flags })
}

pub async fn daemon_leave(State(app): State<Arc<App>>, d: DaemonAuth) -> R {
    app.analytics.capture(&d.cert.account, "machine left", json!({}));
    app.db.drop_daemon(&d.cert.device)?;
    app.relay.drop_daemon(&d.cert.device);
    crate::reply(&wire::Ack {})
}

// ---------------------------------------------------------------- directory

/// Whether a machine said it brings its old threads into control's chat
/// (M124): one that didn't is from before, and keeps them to itself.
fn takes_chat_import(app: &App, daemon: &str) -> bool {
    app.db
        .daemon_features(daemon)
        .is_ok_and(|f| crate::teams::has_feature(&f, arugula_control_wire::chat::CHAT_IMPORT_FEATURE))
}

pub async fn directory(State(app): State<Arc<App>>, s: crate::auth::Reacher) -> R {
    // M124: with chat on, the owner's page says which of their machines are
    // too old to bring their threads into it. The hub's own flags say
    // (M140), so the directory never waits on the platform.
    let chat = !s.daemon && crate::labs_flags::chat_on(&app, &s.account)?;
    let mut daemons: Vec<Value> = app
        .db
        .daemons(&s.account)?
        .into_iter()
        .map(|d| {
            let online = app.relay.online(&d.id);
            // One of my machines may be a team's (I joined it for them, M30).
            let team = app.db.daemon_team(&d.id).ok().flatten();
            let mut v = json!({ "id": d.id, "name": d.name, "urls": d.urls, "last_seen": d.last_seen, "online": online, "team": team });
            if chat && !takes_chat_import(&app, &d.id) {
                v["chat_update"] = json!(true);
            }
            v
        })
        .collect();
    // Teams' machines and those shared with me (M19), with their owner
    // account's certificates to check them by.
    let reachable = if s.daemon {
        crate::teams::reachable_for_agents(&app, &s.account)?
    } else {
        crate::teams::reachable(&app, &s.account)?
    };
    for id in reachable {
        let Some((owner, d)) = app.db.daemon_row(&id)? else { continue };
        if owner == s.account {
            continue;
        }
        let online = app.relay.online(&d.id);
        let team = app.db.daemon_team(&d.id)?;
        let owner_name = app.db.account(&owner)?.map(|a| a.name).unwrap_or_default();
        daemons.push(json!({
            "id": d.id, "name": d.name, "urls": d.urls, "last_seen": d.last_seen, "online": online,
            "account": owner, "owner_name": owner_name, "team": team, "chain": crate::teams::chain_of(&app, &owner)?,
        }));
    }
    // Hosted machines of my projects (M178), with what to check them by.
    daemons.extend(crate::hosted::directory(&app, &s.account)?);
    // Sessions someone offers to share with me, for me to answer first
    // (a person's to answer: a daemon is told none).
    let mut offers = Vec::new();
    for id in if s.daemon { vec![] } else { app.db.offers_for(&s.account)? } {
        let Some((owner, d)) = app.db.daemon_row(&id)? else { continue };
        let Some(who) = app.db.account(&owner)? else { continue };
        offers.push(json!({ "daemon": d.id, "name": d.name, "account": owner, "owner_name": who.name, "owner_login": who.login }));
    }
    Ok(Json(json!({ "daemons": daemons, "offers": offers })))
}
