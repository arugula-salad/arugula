//! The push relay (M21): one VAPID key for every device, so a phone
//! subscribes once and hears from every daemon it may.
//!
//! A device subscribes with control's VAPID key and signs the subscription
//! with its device key (`arugula_e2e::push`). Daemons fetch the
//! subscriptions of the people they serve, check the signatures, encrypt
//! each notification for each subscription themselves (RFC 8291), and hand
//! control only ciphertext: control signs the VAPID token and posts it.
//! Control sees that daemon X notified device Y, and how big; never what.
//!
//! Control sends notices of its own too (#104): a device or a person
//! waiting for the account's owners. Those it encrypts itself, and they
//! say only that something waits; approving happens on the page, with
//! the fingerprint.
//!
//! Each person chooses what they're pushed about (`prefs`): chat, their
//! machines (what daemons relay), their projects (the platform's inbox,
//! deploys and runs). Control holds back what they turned off. Notices
//! about their account (a device or a person waiting, a new agent token)
//! always go.
//!
//! Endpoints are only the browsers' push services (so control can't be
//! made to post anywhere else), plus `--push-host` for tests, and control's
//! own live endpoints, where a device with no push service (the desktop
//! app) listens while it runs (`push_live.rs`).

use std::sync::Arc;

use arugula_control_wire::{PushAnswer, PushRequest, PushSubs};
use arugula_e2e::{now_ms, push::PushSub};
use arugula_platform::hub::{PushOptions, Pushed};
use axum::{Json, extract::State, http::StatusCode};
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD as B64};
use p256::{
    SecretKey,
    ecdsa::{Signature, SigningKey, signature::Signer},
};
use serde::Deserialize;
use serde_json::{Value, json};
use tracing::{info, warn};

use crate::{
    ApiError, App,
    auth::{DaemonAuth, Session},
    err,
};

type R = Result<Json<Value>, ApiError>;

/// What a push is about, for what a person chose to hear (`prefs`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Kind {
    /// Team channels, DMs and pane threads (the platform's `chat-…`).
    Chat,
    /// What a daemon relays: a pane or an agent that needs you.
    Machines,
    /// The rest of the platform's: inbox items, deploys, runs, workspaces.
    Projects,
    /// Who and what may act for the account: never turned off.
    Account,
}

impl Kind {
    /// The kinds a person may turn off, by the names the page uses.
    const CHOSEN: [(Kind, &'static str); 3] =
        [(Kind::Chat, "chat"), (Kind::Machines, "machines"), (Kind::Projects, "projects")];

    /// The platform's push by its `tag`.
    pub(crate) fn of_platform(message: &Value) -> Kind {
        let tag = message.get("tag").and_then(Value::as_str).unwrap_or_default();
        if tag.starts_with("chat-") {
            Kind::Chat
        } else if tag.starts_with("agent-token-") || tag.starts_with("client-secret-reads-") {
            Kind::Account
        } else {
            Kind::Projects
        }
    }

    fn name(self) -> Option<&'static str> {
        Self::CHOSEN.iter().find(|(k, _)| *k == self).map(|(_, n)| *n)
    }
}

/// `account` turned pushes about `kind` off.
pub(crate) fn muted(app: &App, account: &str, kind: Kind) -> anyhow::Result<bool> {
    let Some(name) = kind.name() else { return Ok(false) };
    Ok(app.db.push_muted(account)?.iter().any(|m| m == name))
}

/// `GET /api/push/prefs`: which kinds of push the account hears.
pub async fn prefs(State(app): State<Arc<App>>, s: Session) -> R {
    prefs_of(&app, &s.account)
}

fn prefs_of(app: &App, account: &str) -> R {
    let muted = app.db.push_muted(account)?;
    let on: serde_json::Map<String, Value> =
        Kind::CHOSEN.iter().map(|(_, n)| ((*n).to_owned(), json!(!muted.iter().any(|m| m == n)))).collect();
    Ok(Json(Value::Object(on)))
}

/// `POST /api/push/prefs`: `{ "chat": true, "machines": false, … }`; a kind
/// left out stays as it was.
pub async fn set_prefs(State(app): State<Arc<App>>, s: Session, Json(b): Json<serde_json::Map<String, Value>>) -> R {
    let mut muted = app.db.push_muted(&s.account)?;
    for (k, v) in &b {
        let (Some((_, name)), Some(on)) = (Kind::CHOSEN.iter().find(|(_, n)| n == k), v.as_bool()) else {
            return Err(err(StatusCode::BAD_REQUEST, "chat, machines or projects: true or false"));
        };
        muted.retain(|m| m != name);
        if !on {
            muted.push((*name).to_owned());
        }
    }
    let muted: Vec<&str> = Kind::CHOSEN.iter().map(|(_, n)| *n).filter(|n| muted.iter().any(|m| m == n)).collect();
    app.db.set_push_muted(&s.account, &muted)?;
    // The daemons that serve them drop (or take back) their subscriptions now.
    let mut ds: Vec<String> = app.db.daemons(&s.account)?.into_iter().map(|d| d.id).collect();
    ds.extend(crate::teams::reachable(&app, &s.account)?);
    app.relay.nudge(&ds);
    prefs_of(&app, &s.account)
}

/// Push services browsers use.
const SERVICES: &[&str] =
    &["fcm.googleapis.com", "push.services.mozilla.com", "web.push.apple.com", "notify.windows.com"];

pub struct Vapid {
    key: SecretKey,
}

impl Vapid {
    /// Control's key, made on first start and kept in the database.
    pub fn load(db: &crate::db::Db) -> anyhow::Result<Self> {
        if let Some(hex_key) = db.setting("vapid")? {
            let key = SecretKey::from_slice(&hex::decode(hex_key)?).map_err(|_| anyhow::anyhow!("bad VAPID key"))?;
            return Ok(Self { key });
        }
        let key = loop {
            if let Ok(k) = SecretKey::from_slice(&arugula_e2e::random::<32>()) {
                break k;
            }
        };
        db.set_setting("vapid", &hex::encode(key.to_bytes()))?;
        Ok(Self { key })
    }

    pub fn public(&self) -> String {
        B64.encode(self.key.public_key().to_sec1_bytes())
    }

    pub(crate) fn jwt(&self, audience: &str, subject: &str) -> String {
        let header = B64.encode(br#"{"typ":"JWT","alg":"ES256"}"#);
        let exp = now_ms() / 1000 + 12 * 3600;
        let claims = B64.encode(json!({ "aud": audience, "exp": exp, "sub": subject }).to_string());
        let input = format!("{header}.{claims}");
        let sig: Signature = SigningKey::from(self.key.clone()).sign(input.as_bytes());
        format!("{input}.{}", B64.encode(sig.to_bytes()))
    }
}

/// An endpoint at a real push service (or one allowed for tests).
pub(crate) fn allowed_endpoint(app: &App, endpoint: &str) -> bool {
    if crate::push_live::is_live(app, endpoint) {
        return true;
    }
    let Ok(u) = url::Url::parse(endpoint) else { return false };
    let Some(host) = u.host_str() else { return false };
    let hostport = match u.port() {
        Some(p) => format!("{host}:{p}"),
        None => host.to_owned(),
    };
    if app.cfg.push_hosts.iter().any(|h| h == &hostport) {
        return true;
    }
    u.scheme() == "https" && SERVICES.iter().any(|s| host == *s || host.ends_with(&format!(".{s}")))
}

#[derive(Deserialize)]
pub struct Subscribe {
    sub: PushSub,
}

/// Push subscriptions one account may have, its devices' together.
const SUBS_MOST: usize = 50;

pub async fn subscribe(State(app): State<Arc<App>>, s: Session, Json(b): Json<Subscribe>) -> R {
    let sub = b.sub;
    if sub.account != s.account || !allowed_endpoint(&app, &sub.endpoint) {
        return Err(err(StatusCode::BAD_REQUEST, "a subscription at a push service, for your account"));
    }
    let Some((cert, true)) = app.db.device(&s.account, &sub.device)? else {
        return Err(err(StatusCode::FORBIDDEN, "subscribe from an approved device"));
    };
    if !sub.signed_by(&cert) {
        return Err(err(StatusCode::FORBIDDEN, "the subscription isn't signed by that device"));
    }
    // A live endpoint (`push_live.rs`): kept with the token its device
    // listens with, which only this answer shows.
    let live = crate::push_live::is_live(&app, &sub.endpoint);
    let listen = if live {
        Some(crate::push_live::subscribe(&app, &sub, SUBS_MOST)?)
    } else {
        // A push service's: the same endpoint again from another account
        // takes it over, as a browser that signs into another account
        // keeps its endpoint. (Whoever takes one can't read what's sent.)
        let mine = app.db.push_sub_account(&sub.endpoint)?.as_deref() == Some(s.account.as_str());
        if !mine && app.db.push_sub_count(&s.account)? >= SUBS_MOST {
            return Err(err(StatusCode::TOO_MANY_REQUESTS, "too many push subscriptions; turn some off"));
        }
        app.db.put_push_sub(&sub.endpoint, &s.account, &serde_json::to_string(&sub)?)?;
        None
    };
    // The daemons that serve them pick it up now.
    let mut ds: Vec<String> = app.db.daemons(&s.account)?.into_iter().map(|d| d.id).collect();
    ds.extend(crate::teams::reachable(&app, &s.account)?);
    app.relay.nudge(&ds);
    app.analytics.capture(&s.account, "push turned on", json!({ "service": push_service(&app, &sub.endpoint) }));
    match listen {
        Some(listen) => Ok(Json(json!({ "listen": listen }))),
        None => Ok(Json(json!({}))),
    }
}

/// Which push service an endpoint is at (its host), never the endpoint.
fn push_service(app: &App, endpoint: &str) -> String {
    if crate::push_live::is_live(app, endpoint) {
        return "live".into();
    }
    url::Url::parse(endpoint).ok().and_then(|u| u.host_str().map(str::to_owned)).unwrap_or_default()
}

#[derive(Deserialize)]
pub struct Unsubscribe {
    endpoint: String,
}

pub async fn unsubscribe(State(app): State<Arc<App>>, s: Session, Json(b): Json<Unsubscribe>) -> R {
    // Only the account's own: another's live streams stay open.
    if app.db.push_sub_account(&b.endpoint)?.as_deref() == Some(s.account.as_str()) {
        app.live.close(&b.endpoint);
    }
    app.db.drop_push_sub(&b.endpoint, Some(&s.account))?;
    app.analytics.capture(&s.account, "push turned off", json!({}));
    Ok(Json(json!({})))
}

/// The accounts a daemon serves: its own, its team's, and those it lets in
/// that control routes to it (teams.rs): never someone who has no say in
/// it, whatever list it sent.
fn served(app: &App, daemon: &str, own: &str) -> anyhow::Result<Vec<String>> {
    let mut a = vec![own.to_owned()];
    let rel = crate::teams::Relations::of(app, daemon, own)?;
    for x in app.db.daemon_accounts(daemon)? {
        if rel.routes(app, &x)? {
            a.push(x);
        }
    }
    if let Some(team) = app.db.daemon_team(daemon)?
        && let Some(body) = app.db.latest_roster(&team)?
    {
        let r: arugula_e2e::team::Roster = serde_json::from_str(&body)?;
        a.extend(r.members.into_iter().map(|m| m.account));
    }
    a.sort();
    a.dedup();
    Ok(a)
}

/// Subscriptions of the people a daemon serves, for it to check and use:
/// not those of anyone who turned their machines' pushes off.
pub async fn daemon_subs(State(app): State<Arc<App>>, d: DaemonAuth) -> R {
    let mut subs = Vec::new();
    for a in served(&app, &d.cert.device, &d.cert.account)? {
        if muted(&app, &a, Kind::Machines)? {
            continue;
        }
        crate::push_live::forget_idle(&app, &a)?;
        // Passed on as stored (signed by the device; control never reads it).
        subs.extend(app.db.push_subs(&a)?);
    }
    crate::reply(&PushSubs::<Value> { subs })
}

/// An hour, when a daemon doesn't say how long a push service keeps it.
const HOUR: u32 = 3600;

/// Post an encrypted notification with control's VAPID signature; its
/// push service's status. A subscription that's gone is forgotten.
async fn post(app: &App, endpoint: &str, body: Vec<u8>, ttl: u32, urgency: Option<&str>) -> Result<u16, ApiError> {
    // A live endpoint: its device's open stream, not a push service.
    if crate::push_live::is_live(app, endpoint) {
        return Ok(app.live.send(endpoint, body));
    }
    let url = url::Url::parse(endpoint).map_err(|_| err(StatusCode::BAD_REQUEST, "endpoint"))?;
    // RFC 8292: the push resource's origin, with its port if it has one.
    let audience = url.origin().ascii_serialization();
    let urgency = match urgency {
        Some(u @ ("very-low" | "low" | "normal" | "high")) => u.to_owned(),
        _ => "high".to_owned(),
    };
    let res = app
        .http
        .post(url)
        .header("TTL", ttl.min(86_400).to_string())
        .header("Urgency", urgency)
        .header("Content-Encoding", "aes128gcm")
        .header("Content-Type", "application/octet-stream")
        .header(
            "Authorization",
            format!("vapid t={}, k={}", app.vapid.jwt(&audience, &app.cfg.public_url), app.vapid.public()),
        )
        .body(body)
        .send()
        .await
        .map_err(|e| {
            warn!(error = %e, "push service unreachable");
            err(StatusCode::BAD_GATEWAY, "the push service didn't answer")
        })?;
    let status = res.status().as_u16();
    if status == 404 || status == 410 {
        app.db.drop_push_sub(endpoint, None)?;
    }
    Ok(status)
}

/// Tell these accounts' devices (those with push on) that something waits
/// for them (#104): `title`, `body`, and a tap that opens control's page,
/// where the prompt is. In the background; a failure is only logged.
pub fn notify(app: &Arc<App>, accounts: Vec<String>, tag: &str, title: String, body: String) {
    notify_with(app, accounts, tag, title, body, serde_json::Map::new());
}

/// [`notify`], with more for the page beside the words (chat's room, for
/// the tap to open it).
pub fn notify_with(
    app: &Arc<App>,
    accounts: Vec<String>,
    tag: &str,
    title: String,
    body: String,
    more: serde_json::Map<String, Value>,
) {
    let app = app.clone();
    let mut msg = more;
    for (k, v) in [("title", json!(title)), ("body", json!(body)), ("tag", json!(tag)), ("control", json!(true))] {
        msg.insert(k.into(), v);
    }
    let msg = Value::Object(msg).to_string();
    tokio::spawn(async move {
        for account in accounts {
            let subs = match app.db.push_subs(&account) {
                Ok(s) => s,
                Err(e) => {
                    warn!(error = %e, "push subscriptions");
                    continue;
                }
            };
            for sub in subs {
                let Ok(sub) = serde_json::from_value::<PushSub>(sub) else { continue };
                if let Err(e) = send_own(&app, &sub, msg.as_bytes()).await {
                    warn!(error = e.1, %account, "control's notice didn't go");
                }
            }
        }
    });
}

async fn send_own(app: &App, sub: &PushSub, msg: &[u8]) -> Result<(), ApiError> {
    app.limits.check_account(crate::limit::ACCOUNT_PUSHES, &sub.account)?;
    app.limits.check_all(crate::limit::PUSHES)?;
    let bad = |_| err(StatusCode::BAD_REQUEST, "subscription keys");
    let (ua, auth) = (B64.decode(&sub.p256dh).map_err(bad)?, B64.decode(&sub.auth).map_err(bad)?);
    let body = arugula_e2e::push::encrypt(msg, &ua, &auth, &arugula_e2e::push::ephemeral(), &arugula_e2e::random())
        .map_err(|e| err(StatusCode::BAD_REQUEST, &e.to_string()))?;
    let status = post(app, &sub.endpoint, body, 3600, Some("high")).await?;
    info!(account = sub.account, status, "control's notice sent");
    Ok(())
}

/// Push `msg` to an account's devices for the platform (`HubPort::push`,
/// `HubPort::push_with`): each subscription signed by one of its approved
/// devices that isn't revoked, or with `any_subscription` every one it has
/// (as [`notify_with`] does, for chat's pushes), encrypted here as control's
/// own notices are, kept by a push service for `ttl` (an hour by default).
/// Waits for each push service, and says what they did.
/// Nothing goes for a kind the account turned off ([`Kind::of_platform`]).
pub(crate) async fn push_account(app: &App, account: &str, msg: &[u8], options: PushOptions) -> anyhow::Result<Pushed> {
    let kind = serde_json::from_slice(msg).map_or(Kind::Projects, |m| Kind::of_platform(&m));
    if muted(app, account, kind)? {
        return Ok(Pushed::default());
    }
    let revoked: std::collections::HashSet<String> =
        app.db.revocations(account)?.into_iter().map(|r| r.device).collect();
    let ttl = options.ttl.map_or(HOUR, |t| u32::try_from(t.as_secs()).unwrap_or(u32::MAX));
    let mut out = Pushed::default();
    for value in app.db.push_subs(account)? {
        let Ok(sub) = serde_json::from_value::<PushSub>(value) else { continue };
        // Every one, as `notify_with` sends them: each was checked when it
        // was stored (`subscribe`).
        if !options.any_subscription {
            let Some((cert, true)) = app.db.device(account, &sub.device)? else { continue };
            if !sub.signed_by(&cert) || revoked.contains(&sub.device) || !allowed_endpoint(app, &sub.endpoint) {
                continue;
            }
        }
        let sent = async {
            app.limits.check_account(crate::limit::ACCOUNT_PUSHES, account)?;
            app.limits.check_all(crate::limit::PUSHES)?;
            let bad = |_| err(StatusCode::BAD_REQUEST, "subscription keys");
            let (ua, auth) = (B64.decode(&sub.p256dh).map_err(bad)?, B64.decode(&sub.auth).map_err(bad)?);
            let body =
                arugula_e2e::push::encrypt(msg, &ua, &auth, &arugula_e2e::push::ephemeral(), &arugula_e2e::random())
                    .map_err(|e| err(StatusCode::BAD_REQUEST, &e.to_string()))?;
            post(app, &sub.endpoint, body, ttl, Some("high")).await
        };
        match sent.await {
            Ok(404 | 410) => out.gone += 1,
            Ok(200..=299) => out.sent += 1,
            Ok(status) => {
                warn!(%account, status, "the platform's push was refused");
                out.failed += 1;
            }
            Err(e) => {
                warn!(%account, error = e.1, "the platform's push didn't go");
                out.failed += 1;
            }
        }
    }
    Ok(out)
}

pub async fn daemon_send(State(app): State<Arc<App>>, d: DaemonAuth, Json(b): Json<PushRequest>) -> R {
    let Some(account) = app.db.push_sub_account(&b.endpoint)? else {
        return Err(err(StatusCode::NOT_FOUND, "no such subscription"));
    };
    if !served(&app, &d.cert.device, &d.cert.account)?.contains(&account) {
        return Err(err(StatusCode::FORBIDDEN, "not someone this daemon serves"));
    }
    // A daemon that hasn't fetched their subscriptions since they turned
    // their machines' pushes off: held back, and done as far as it knows.
    if muted(&app, &account, Kind::Machines)? {
        return crate::reply(&PushAnswer { status: 204 });
    }
    // A brake on how much one daemon has control relay, and on all of them.
    app.limits.check_daemon(crate::limit::DAEMON_PUSHES, &d.cert.device)?;
    app.limits.check_account(crate::limit::ACCOUNT_PUSHES, &account)?;
    app.limits.check_all(crate::limit::PUSHES)?;
    let body = base64::engine::general_purpose::STANDARD
        .decode(&b.body)
        .map_err(|_| err(StatusCode::BAD_REQUEST, "body: base64"))?;
    if body.len() > 4096 {
        return Err(err(StatusCode::BAD_REQUEST, "a notification is at most 4 KB"));
    }
    let status = post(&app, &b.endpoint, body, b.ttl.unwrap_or(HOUR), b.urgency.as_deref()).await?;
    // Only who and how it went; never what (we couldn't read it anyway).
    info!(daemon = d.cert.device, %account, status, "push relayed");
    app.analytics.capture(
        &account,
        "push relayed",
        json!({ "status": status, "ok": (200..300).contains(&status), "own_machine": account == d.cert.account }),
    );
    crate::reply(&PushAnswer { status })
}

#[cfg(test)]
mod tests {
    #[test]
    fn push_services_only() {
        let ok = |e: &str| {
            let u = url::Url::parse(e).unwrap();
            u.scheme() == "https"
                && super::SERVICES
                    .iter()
                    .any(|s| u.host_str().unwrap() == *s || u.host_str().unwrap().ends_with(&format!(".{s}")))
        };
        assert!(ok("https://fcm.googleapis.com/fcm/send/x"));
        assert!(ok("https://updates.push.services.mozilla.com/wpush/v2/x"));
        assert!(ok("https://web.push.apple.com/x"));
        assert!(!ok("https://evil.example/fcm.googleapis.com"));
        assert!(!ok("http://fcm.googleapis.com/x"));
        assert!(!ok("https://169.254.169.254/latest"));
    }
}
