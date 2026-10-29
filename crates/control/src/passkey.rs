//! Passkeys (WebAuthn) as a first-class sign-in: on their own (a new
//! account with no GitHub), or added to an account. Like GitHub sign-in,
//! a passkey opens control's API only; devices are still approved by
//! devices.
//!
//! Verified here rather than with a WebAuthn crate (which brings OpenSSL,
//! and the static builds can't have it). Only what's needed:
//!
//! - registration with `attestation: "none"` (the authenticator's make
//!   and model aren't checked; the key is what matters);
//! - ES256 (P-256), Ed25519 and RS256 credential keys (RS256 for the
//!   Windows Hello setups and security keys that only do RSA, #205);
//! - user verification required, the RP id is control's host, and the
//!   origin must be control's own.
//!
//! #764: the page asks for the `prf` extension too, and says whether the
//! passkey gave it (`prf`), kept per passkey. Its output never comes here:
//! the page derives its device keys from it. Control says what keeps the
//! passkey, for the page to name that device.
//!
//! **Adding one to an account takes more than its session** (attestation
//! is `none`, so a passkey made in software could claim to verify anyone):
//! - an account with a passkey confirms with one of them: a `webauthn.get`
//!   with user verification over a challenge that names the account and
//!   the new passkey's id (`arugula add passkey v1`), asked for once the new
//!   one is made (`/auth/passkey/register/step-up`);
//! - an account with none needs one of its browsers or phones (a browser
//!   all the way back to its root) to sign for it, or a GitHub sign-in from
//!   the last ten minutes ([`REAUTH_MS`]); the page says which device it is
//!   as it starts, and control sends the person through GitHub again when
//!   it can't sign and the sign-in is older (`reason: "reauth"`). Browsers
//!   an app window approved before #1343 can't sign, so their accounts go
//!   through GitHub.
//!
//! Every device of the account hears of it (a push), and the passkey keeps
//! which device's sign-in added it, and which sign-in. Only the page does
//! any of this, by its cookie: a CLI's signature is refused before it gets
//! here (routes.toml, `clients::gate`).
//!
//! Every ceremony refuses one from inside another site's frame
//! (`crossOrigin`, `topOrigin`).

use std::{
    collections::HashMap,
    net::SocketAddr,
    sync::{Arc, Mutex},
};

use arugula_e2e::now_ms;
use axum::{
    Json,
    extract::{ConnectInfo, State},
    http::{HeaderMap, StatusCode, header},
    response::{IntoResponse, Response},
};
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD as B64};
use ciborium::value::Value as Cbor;
use serde::Deserialize;
use serde_json::json;
use sha2::{Digest, Sha256};

use crate::{
    ApiError, App, Site, auth,
    db::{Passkey, SessionInfo},
    err, refusal,
};

const CHALLENGE_TTL_MS: u64 = 5 * 60 * 1000;
const ES256: i64 = -7;
const EDDSA: i64 = -8;
const RS256: i64 = -257;

/// Flags in authenticator data.
const UP: u8 = 0x01;
const UV: u8 = 0x04;
const AT: u8 = 0x40;

/// How old a GitHub sign-in may be to add a passkey to an account that
/// has none.
pub const REAUTH_MS: u64 = 10 * 60 * 1000;
/// The reason code when it's older.
pub const REAUTH: &str = "reauth";

#[derive(Clone)]
enum Purpose {
    /// Register: for this account, or a new one by this name (#102).
    Register {
        account: Option<String>,
        name: String,
    },
    Login,
    /// One of `account`'s passkeys agreeing to add `credential` to it.
    AddPasskey {
        account: String,
        credential: String,
    },
}

#[derive(Default)]
pub struct Challenges {
    open: Mutex<HashMap<String, (Purpose, u64)>>,
}

impl Challenges {
    fn issue(&self, p: Purpose) -> String {
        self.issue_as(B64.encode(arugula_e2e::random::<32>()), p)
    }

    /// Issue `c` (a hash over what it's for) as a challenge.
    fn issue_as(&self, c: String, p: Purpose) -> String {
        let now = now_ms();
        let mut open = self.open.lock().unwrap();
        open.retain(|_, (_, at)| now - *at < CHALLENGE_TTL_MS);
        open.insert(c.clone(), (p, now));
        c
    }

    /// Each challenge works once.
    fn take(&self, c: &str) -> Option<Purpose> {
        let (p, at) = self.open.lock().unwrap().remove(c)?;
        (now_ms() - at < CHALLENGE_TTL_MS).then_some(p)
    }
}

/// Passkeys are made for the site they're made on (#507): one made at an
/// old URL works only there.
fn rp_id(site: &Site) -> &str {
    site.host()
}

fn bad(why: &str) -> ApiError {
    err(StatusCode::BAD_REQUEST, why)
}

/// Optional session, by its cookie only: registering adds to it, or starts
/// a new account. Its token's hash, and its row.
fn session_of(app: &App, headers: &HeaderMap) -> Option<(String, SessionInfo)> {
    let hash = auth::hash(auth::cookie(headers, auth::SESSION_COOKIE)?);
    let info = app.db.session_info(&hash, now_ms()).ok().flatten()?;
    Some((hash, info))
}

/// The desktop app's window, signed in by a browser's hand-over: it makes
/// no passkeys (its keys may be a program's on that computer).
fn from_app(info: &SessionInfo) -> Result<(), ApiError> {
    if info.via.as_deref() == Some(auth::Via::App.as_str()) {
        return Err(refusal(
            StatusCode::FORBIDDEN,
            "app_window",
            "add a passkey in a browser or on a phone: the desktop app's window doesn't",
        ));
    }
    Ok(())
}

/// The account's browsers and phones that are ones all the way back to its
/// root (`api::browser_all_the_way`): one of them signs for a passkey
/// added to an account that has any.
fn browser_devices(app: &App, account: &str) -> anyhow::Result<Vec<arugula_e2e::Cert>> {
    let Some(root) = app.db.account(account)?.and_then(|a| a.root) else { return Ok(vec![]) };
    let (certs, _) = app.db.devices(account)?;
    let revs = app.db.revocations(account)?;
    let trust = arugula_e2e::Trust { account: account.to_owned(), root };
    let now = trust.evaluate(&certs, &revs);
    let children = crate::legacy_app::children(&app.db, account)?;
    Ok(now
        .devices
        .into_values()
        .filter(|c| c.is_browser() && crate::api::browser_all_the_way(&trust, &certs, &c.device, &children))
        .collect())
}

/// What one of the account's browsers or phones signs to add a passkey:
/// the account, the new passkey's id and the registration's challenge. The
/// page's `addPasskeyDeviceBody` is the same text.
pub fn add_passkey_device_body(account: &str, credential: &str, challenge: &str) -> String {
    format!("arugula add passkey device v1\naccount {account}\ncredential {credential}\nchallenge {challenge}\n")
}

/// A device's signature over [`add_passkey_device_body`].
#[derive(Deserialize, Clone)]
pub struct DeviceSig {
    id: String,
    sig: String,
}

/// A GitHub sign-in from the last [`REAUTH_MS`]: what adding a first
/// passkey to an account with no browser or phone takes.
pub(crate) fn fresh_github(info: &SessionInfo) -> bool {
    info.via.as_deref() == Some(auth::Via::Github.as_str()) && now_ms().saturating_sub(info.created) <= REAUTH_MS
}

fn reauth() -> ApiError {
    refusal(
        StatusCode::FORBIDDEN,
        REAUTH,
        "to add a passkey to an account with none, sign in with GitHub again first: it takes a sign-in from the last ten minutes",
    )
}

/// The challenge an account's passkey signs to add `credential` to it: the
/// hash of what it's for, so the assertion says so.
fn add_passkey_challenge(account: &str, credential: &str) -> String {
    let nonce = hex::encode(arugula_e2e::random::<16>());
    B64.encode(Sha256::digest(
        format!("arugula add passkey v1\naccount {account}\ncredential {credential}\nnonce {nonce}\n").as_bytes(),
    ))
}

fn same_origin(app: &App, headers: &HeaderMap) -> Result<(), ApiError> {
    match headers.get(header::ORIGIN).and_then(|o| o.to_str().ok()) {
        Some(o) if o == app.cfg.site(headers).origin => Ok(()),
        _ => Err(err(StatusCode::FORBIDDEN, "cross-origin request refused")),
    }
}

#[derive(Deserialize, Default)]
pub struct Start {
    /// A new account's name: what teammates see (#102).
    #[serde(default)]
    name: Option<String>,
    /// The page's own device, when it's one of the account's: whether it
    /// may sign for a first passkey, or the page signs in with GitHub again
    /// first.
    #[serde(default)]
    device: Option<String>,
}

pub async fn register_start(
    State(app): State<Arc<App>>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    body: Option<Json<Start>>,
) -> Result<Json<serde_json::Value>, ApiError> {
    same_origin(&app, &headers)?;
    let session = session_of(&app, &headers);
    let account = session.as_ref().map(|(_, i)| i.account.clone());
    // Signed in: one more for the account, confirmed with a passkey it has,
    // or (with none) a fresh GitHub sign-in.
    let (have, browsers) = match &account {
        Some(a) => (app.db.passkey_ids(a)?, browser_devices(&app, a)?),
        None => (vec![], vec![]),
    };
    let body = body.map(|Json(b)| b);
    // A first passkey: this page's device signs for it if it's a browser
    // or a phone all the way back to the root, or else the session is a
    // GitHub sign-in from the last ten minutes. Browsers an app window
    // approved before it had a kind of its own don't count, so an account
    // whose browsers all came that way signs in with GitHub again.
    let this_page = body.as_ref().and_then(|b| b.device.as_deref());
    let page_signs = this_page.is_some_and(|d| browsers.iter().any(|c| c.device == d));
    let first_by_github = have.is_empty() && session.as_ref().is_some_and(|(_, i)| fresh_github(i));
    if let Some((_, info)) = &session {
        from_app(info)?;
        if have.is_empty() && !page_signs && !fresh_github(info) {
            return Err(reauth());
        }
    }
    let name = match &account {
        Some(a) => app.db.account(a)?.map(|x| x.name).unwrap_or_default(),
        None => {
            let asked = body.and_then(|b| b.name).unwrap_or_default();
            let name = crate::api::display_name(&asked).map_err(|_| bad("your name, as teammates will see it"))?;
            // A new account each time: limited.
            app.limits.check(crate::limit::ACCOUNTS, app.limits.client_ip(peer, &headers))?;
            name
        }
    };
    let user_id = arugula_e2e::random::<16>().to_vec();
    let challenge = app.passkeys.issue(Purpose::Register { account: account.clone(), name: name.clone() });
    let name = if name.is_empty() { "Arugula".to_owned() } else { name };
    Ok(Json(json!({
        "challenge": challenge,
        "rp": { "id": rp_id(app.cfg.site(&headers)), "name": "Arugula" },
        "user": { "id": B64.encode(&user_id), "name": name, "displayName": name },
        "pubKeyCredParams": [
            { "type": "public-key", "alg": EDDSA },
            { "type": "public-key", "alg": ES256 },
            { "type": "public-key", "alg": RS256 },
        ],
        "authenticatorSelection": { "residentKey": "required", "userVerification": "required" },
        "attestation": "none",
        "timeout": CHALLENGE_TTL_MS,
        // Once it's made, one of these confirms it (`register_step_up`).
        "step_up": if have.is_empty() { None } else { Some("passkey") },
        // And one of the account's browsers or phones signs for it
        // (`add_passkey_device_body`).
        // (Asked whenever this page can; needed unless a GitHub sign-in
        // vouches for a first one.)
        "device_sig": page_signs || (!browsers.is_empty() && !first_by_github),
    })))
}

#[derive(Deserialize)]
pub struct StepUp {
    /// The new passkey's id (base64url, as `rawId`), made but not added.
    id: String,
}

/// `POST /auth/passkey/register/step-up {id}`: for an account with
/// passkeys, the `webauthn.get` options one of them signs to add the new
/// one, `id`: a challenge naming the account and `id`, and the account's
/// passkeys to choose from.
pub async fn register_step_up(
    State(app): State<Arc<App>>,
    headers: HeaderMap,
    Json(b): Json<StepUp>,
) -> Result<Json<serde_json::Value>, ApiError> {
    same_origin(&app, &headers)?;
    let (_, info) = session_of(&app, &headers).ok_or_else(|| err(StatusCode::UNAUTHORIZED, "sign in first"))?;
    from_app(&info)?;
    let have = app.db.passkey_ids(&info.account)?;
    if have.is_empty() {
        return Err(err(StatusCode::CONFLICT, "this account has no passkey to confirm with"));
    }
    if b.id.is_empty() || b.id.len() > 1024 || B64.decode(&b.id).is_err() || have.contains(&b.id) {
        return Err(bad("not a new passkey's id"));
    }
    let challenge = add_passkey_challenge(&info.account, &b.id);
    app.passkeys
        .issue_as(challenge.clone(), Purpose::AddPasskey { account: info.account.clone(), credential: b.id.clone() });
    Ok(Json(json!({
        "challenge": challenge,
        "rpId": rp_id(app.cfg.site(&headers)),
        "userVerification": "required",
        "timeout": CHALLENGE_TTL_MS,
        "allowCredentials": have.iter().map(|id| json!({ "type": "public-key", "id": id })).collect::<Vec<_>>(),
    })))
}

#[derive(Deserialize)]
pub struct Registration {
    /// base64url, as `PublicKeyCredential.rawId`.
    id: String,
    #[serde(rename = "clientDataJSON")]
    client_data: String,
    #[serde(rename = "attestationObject")]
    attestation: String,
    /// Whether the browser says the passkey gives PRF output (#764).
    #[serde(default)]
    prf: Option<bool>,
    /// For an account with passkeys: one of them agreeing to this one
    /// (`register_step_up`'s challenge).
    #[serde(default)]
    step_up: Option<Assertion>,
    /// For an account with a browser or a phone: one of them signing for
    /// this one ([`add_passkey_device_body`]).
    #[serde(default)]
    device: Option<DeviceSig>,
}

/// A ceremony's client data, as the browser wrote it. Only one run in
/// control's own page parses: one from inside another site's frame
/// (`crossOrigin`, or a `topOrigin` that isn't `origin`) doesn't, whoever
/// reads it.
#[derive(Deserialize)]
#[serde(try_from = "Framed")]
struct ClientData {
    kind: String,
    challenge: String,
    origin: String,
}

/// [`ClientData`] with what says where it ran.
#[derive(Deserialize)]
struct Framed {
    #[serde(rename = "type")]
    kind: String,
    challenge: String,
    origin: String,
    /// The ceremony ran in a frame of another origin's page.
    #[serde(rename = "crossOrigin", default)]
    cross_origin: Option<bool>,
    /// The top page's origin, when it isn't the frame's.
    #[serde(rename = "topOrigin", default)]
    top_origin: Option<String>,
}

impl Framed {
    /// Not from inside another site's frame: `crossOrigin` isn't true, and
    /// a `topOrigin` is `origin` itself. Why not, or `Ok`.
    fn ok(&self) -> Result<(), &'static str> {
        if self.cross_origin == Some(true) {
            return Err("a passkey asked for from inside another site's frame");
        }
        if self.top_origin.as_deref().is_some_and(|t| t != self.origin) {
            return Err("a passkey asked for under another site's page");
        }
        Ok(())
    }
}

impl TryFrom<Framed> for ClientData {
    type Error = &'static str;

    fn try_from(f: Framed) -> Result<Self, Self::Error> {
        f.ok()?;
        Ok(ClientData { kind: f.kind, challenge: f.challenge, origin: f.origin })
    }
}

fn client_data(site: &Site, b64: &str, kind: &str) -> Result<(ClientData, Vec<u8>), ApiError> {
    let raw = B64.decode(b64).map_err(|_| bad("bad clientDataJSON"))?;
    let f: Framed = serde_json::from_slice(&raw).map_err(|_| bad("bad clientDataJSON"))?;
    let cd = ClientData::try_from(f).map_err(|why| err(StatusCode::FORBIDDEN, why))?;
    if cd.kind != kind {
        return Err(bad("wrong WebAuthn ceremony"));
    }
    if cd.origin != site.origin {
        return Err(err(StatusCode::FORBIDDEN, "passkey from another site"));
    }
    Ok((cd, raw))
}

/// rpIdHash, flags, signCount; the rest.
fn auth_data<'a>(site: &Site, d: &'a [u8]) -> Result<(u8, u32, &'a [u8]), ApiError> {
    if d.len() < 37 {
        return Err(bad("short authenticator data"));
    }
    if d[..32] != Sha256::digest(rp_id(site).as_bytes())[..] {
        return Err(bad("passkey for another site"));
    }
    let flags = d[32];
    if flags & UP == 0 || flags & UV == 0 {
        return Err(bad("the passkey must verify you (PIN, fingerprint or face)"));
    }
    Ok((flags, u32::from_be_bytes(d[33..37].try_into().unwrap()), &d[37..]))
}

fn map_get(m: &[(Cbor, Cbor)], k: i64) -> Option<&Cbor> {
    m.iter().find(|(key, _)| key.as_integer().and_then(|i| i64::try_from(i).ok()) == Some(k)).map(|(_, v)| v)
}

/// A COSE key as (alg, public key bytes): SEC1 for P-256, raw for
/// Ed25519, X.509 SubjectPublicKeyInfo DER for RSA.
fn cose_key(v: &Cbor) -> Result<(i64, Vec<u8>), ApiError> {
    let m = v.as_map().ok_or_else(|| bad("bad credential key"))?;
    let int = |k| map_get(m, k).and_then(|v| v.as_integer()).and_then(|i| i64::try_from(i).ok());
    let bytes = |k| map_get(m, k).and_then(|v| v.as_bytes()).cloned();
    match (int(1), int(3)) {
        // EC2, ES256, P-256.
        (Some(2), Some(ES256)) if int(-1) == Some(1) => {
            let (x, y) = bytes(-2).zip(bytes(-3)).ok_or_else(|| bad("bad P-256 key"))?;
            let mut sec1 = vec![4u8];
            sec1.extend_from_slice(&x);
            sec1.extend_from_slice(&y);
            p256::ecdsa::VerifyingKey::from_sec1_bytes(&sec1).map_err(|_| bad("bad P-256 key"))?;
            Ok((ES256, sec1))
        }
        // OKP, EdDSA, Ed25519.
        (Some(1), Some(EDDSA)) if int(-1) == Some(6) => Ok((EDDSA, bytes(-2).ok_or_else(|| bad("bad Ed25519 key"))?)),
        // RSA, RS256 (PKCS#1 v1.5 with SHA-256): n is -1, e is -2.
        (Some(3), Some(RS256)) => {
            use aws_lc_rs::encoding::AsDer;
            let (n, e) = bytes(-1).zip(bytes(-2)).ok_or_else(|| bad("bad RSA key"))?;
            let strip = |b: &[u8]| b.iter().position(|&x| x != 0).map(|i| b[i..].to_vec()).unwrap_or_default();
            let key = aws_lc_rs::rsa::PublicKeyComponents { n: strip(&n), e: strip(&e) };
            // 2048 bits at least, as verification will insist.
            if key.n.len() < 256 {
                return Err(bad("RSA passkey shorter than 2048 bits"));
            }
            let der = key.as_der().map_err(|_| bad("bad RSA key"))?;
            Ok((RS256, der.as_ref().to_vec()))
        }
        _ => Err(bad("unsupported passkey algorithm (ES256, Ed25519 or RS256 only)")),
    }
}

/// What keeps a passkey, from the AAGUID its authenticator reports (all
/// zeros when it won't say). A few common ones, from the community list at
/// github.com/passkeydeveloper/passkey-authenticator-aaguids.
fn provider(aaguid: &str) -> Option<&'static str> {
    Some(match aaguid {
        "fbfc3007154e4ecc8c0b6e020557d7bd" | "dd4ec289e01d41c9bb8970fa845d4bf2" => "iCloud Keychain",
        "ea9b8d664d011d213ce4b6b48cb575d4" => "Google Password Manager",
        "adce000235bcc60a648b0b25f1f05503" => "Chrome on Mac",
        "08987058cadc4b81b6e130de50dcbe96"
        | "9ddd1817af5a4672a2b93e3dd95000a9"
        | "6028b017b1d44c02b4b3afcdafc96bb2" => "Windows Hello",
        "bada5566a7aa401fbd9645619a55120d" => "1Password",
        "d548826e79b4db40a3d811116f7e8349" => "Bitwarden",
        _ => return None,
    })
}

pub async fn register_finish(State(app): State<Arc<App>>, headers: HeaderMap, Json(r): Json<Registration>) -> Response {
    match register(&app, &headers, r).await {
        Ok((account, provider, new_session)) => {
            let mut res = Json(json!({ "account": account, "provider": provider })).into_response();
            if let Some(cookie) = new_session {
                res.headers_mut().append(header::SET_COOKIE, cookie);
            }
            res
        }
        Err(e) => e.into_response(),
    }
}

async fn register(
    app: &Arc<App>,
    headers: &HeaderMap,
    r: Registration,
) -> Result<(String, Option<&'static str>, Option<header::HeaderValue>), ApiError> {
    same_origin(app, headers)?;
    let site = app.cfg.site(headers);
    let (cd, _) = client_data(site, &r.client_data, "webauthn.create")?;
    let Some(Purpose::Register { account, name }) = app.passkeys.take(&cd.challenge) else {
        return Err(bad("that request expired; try again"));
    };
    // One more for a signed-in account: its own session still (not the
    // desktop app's window), and confirmed: a browser or a phone of its
    // signs, a passkey it has agrees, or (with neither) a fresh GitHub
    // sign-in.
    let added_by = match &account {
        Some(a) => {
            let (hash, info) = session_of(app, headers)
                .filter(|(_, i)| i.account == *a)
                .ok_or_else(|| err(StatusCode::UNAUTHORIZED, "sign in again"))?;
            from_app(&info)?;
            let device =
                confirm_adding(app, site, a, &r.id, &cd.challenge, &info, r.step_up.as_ref(), r.device.as_ref())?;
            Some((hash, info, device))
        }
        None => None,
    };
    let att: Cbor = ciborium::from_reader(B64.decode(&r.attestation).map_err(|_| bad("bad attestation"))?.as_slice())
        .map_err(|_| bad("bad attestation"))?;
    let m = att.as_map().ok_or_else(|| bad("bad attestation"))?;
    let data = m
        .iter()
        .find(|(k, _)| k.as_text() == Some("authData"))
        .and_then(|(_, v)| v.as_bytes())
        .ok_or_else(|| bad("no authenticator data"))?;
    let (flags, _count, rest) = auth_data(site, data)?;
    if flags & AT == 0 || rest.len() < 18 {
        return Err(bad("no credential in the attestation"));
    }
    let id_len = u16::from_be_bytes([rest[16], rest[17]]) as usize;
    let cred_id = rest.get(18..18 + id_len).ok_or_else(|| bad("short credential"))?;
    if B64.encode(cred_id) != r.id {
        return Err(bad("credential id mismatch"));
    }
    let key: Cbor = ciborium::from_reader(&rest[18 + id_len..]).map_err(|_| bad("bad credential key"))?;
    let (alg, public) = cose_key(&key)?;
    let now = now_ms();
    // Signed in: add it to the account. Not: it is a new account.
    let (account, cookie) = match account {
        Some(a) => (a, None),
        None => {
            let a = app.db.account_for("passkey", &r.id, "", &auth::new_account_id(), now)?;
            app.db.set_name(&a, &name)?;
            app.analytics.signed_in(&a, "passkey", true);
            (a.clone(), Some(auth::start_session(app, &a, auth::Via::Passkey)?))
        }
    };
    app.db.add_passkey(&r.id, &account, alg, &public, now)?;
    let aaguid = hex::encode(&rest[..16]);
    let provider = provider(&aaguid);
    app.db.note_passkey(&r.id, &crate::account::agent(headers), provider)?;
    if let Some(prf) = r.prf {
        app.db.passkey_prf(&r.id, prf)?;
    }
    if let Some((hash, info, device)) = &added_by {
        // The device is the one that signed for it, not what the sign-in
        // said it is.
        app.db.note_passkey_added(&r.id, device.as_deref(), &hash[..crate::account::SESSION_ID_LEN])?;
        tell_devices(app, &account, device.as_deref(), info, provider);
    }
    app.analytics.capture(
        &account,
        "passkey added",
        json!({ "provider": provider, "prf": r.prf, "new_account": cookie.is_some() }),
    );
    Ok((account, provider, cookie))
}

/// Adding `credential` to `account` (registered over `challenge`), which
/// `info` is signed in to. A first passkey: one of its browsers or phones
/// signs for it (`device`), or the session is a GitHub sign-in from the
/// last ten minutes. One more: a passkey it has agrees (`step_up`: UV, over
/// the challenge [`register_step_up`] made for this account and this
/// credential), and with a browser or a phone, one of them signs too. The
/// device that signed, if one did.
#[allow(clippy::too_many_arguments)]
fn confirm_adding(
    app: &App,
    site: &Site,
    account: &str,
    credential: &str,
    challenge: &str,
    info: &SessionInfo,
    step_up: Option<&Assertion>,
    device: Option<&DeviceSig>,
) -> Result<Option<String>, ApiError> {
    let browsers = browser_devices(app, account)?;
    let signed = match device {
        Some(d) => browsers.iter().find(|c| {
            c.device == d.id
                && arugula_e2e::cert::verify_hex(
                    &c.sign,
                    add_passkey_device_body(account, credential, challenge).as_bytes(),
                    &d.sig,
                )
        }),
        None => None,
    };
    let have = app.db.passkey_ids(account)?;
    // A first one: a browser or a phone signed for it, or the session is a
    // fresh GitHub sign-in.
    if have.is_empty() {
        if signed.is_none() && !fresh_github(info) {
            return Err(reauth());
        }
        return Ok(signed.map(|c| c.device.clone()));
    }
    if !browsers.is_empty() && signed.is_none() {
        return Err(refusal(
            StatusCode::FORBIDDEN,
            "device_sig",
            "add a passkey from a browser or a phone that's one of your devices: it signs for the new passkey",
        ));
    }
    let no = || refusal(StatusCode::FORBIDDEN, "step_up", "confirm with one of your passkeys to add another");
    let a = step_up.ok_or_else(no)?;
    let (cd, raw) = client_data(site, &a.client_data, "webauthn.get")?;
    match app.passkeys.take(&cd.challenge) {
        Some(Purpose::AddPasskey { account: for_account, credential: for_credential })
            if for_account == account && for_credential == credential => {}
        _ => return Err(no()),
    }
    if a.id == credential {
        return Err(no());
    }
    let (pk, count) = asserted(app, site, a, &raw)?;
    if pk.account != account {
        return Err(no());
    }
    app.db.passkey_used(&a.id, count, now_ms())?;
    Ok(signed.map(|c| c.device.clone()))
}

/// Every device of the account hears that a passkey was added: by the
/// device that signed for it, or else by how the session signed in and
/// what it said it runs in.
fn tell_devices(app: &Arc<App>, account: &str, device: Option<&str>, info: &SessionInfo, provider: Option<&str>) {
    let from = match device.and_then(|d| app.db.device(account, d).ok().flatten()) {
        Some((c, _)) => format!("\"{}\"", c.name),
        None => {
            let how = match info.via.as_deref() {
                Some("github") => "a GitHub sign-in",
                Some("passkey") => "a passkey sign-in",
                _ => "a sign-in",
            };
            let ua: String = info.agent.chars().take(120).collect();
            if ua.is_empty() { how.to_owned() } else { format!("{how} ({ua})") }
        }
    };
    let what = provider.map_or_else(|| "A passkey".to_owned(), |p| format!("A passkey ({p})"));
    crate::push::notify(
        app,
        vec![account.to_owned()],
        "control-passkey",
        "A passkey was added to your account".into(),
        format!("{what} was added from {from}. Not you? Remove it under Sign-in, and the device that added it."),
    );
}

pub async fn login_start(
    State(app): State<Arc<App>>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
) -> Result<Json<serde_json::Value>, ApiError> {
    same_origin(&app, &headers)?;
    app.limits.check(crate::limit::SIGN_INS, app.limits.client_ip(peer, &headers))?;
    let challenge = app.passkeys.issue(Purpose::Login);
    Ok(Json(
        json!({ "challenge": challenge, "rpId": rp_id(app.cfg.site(&headers)), "userVerification": "required", "timeout": CHALLENGE_TTL_MS }),
    ))
}

#[derive(Deserialize, Clone)]
pub struct Assertion {
    id: String,
    #[serde(rename = "clientDataJSON")]
    client_data: String,
    #[serde(rename = "authenticatorData")]
    authenticator_data: String,
    signature: String,
    /// The browser got PRF output from it (#764).
    #[serde(default)]
    prf: bool,
}

pub async fn login_finish(State(app): State<Arc<App>>, headers: HeaderMap, Json(a): Json<Assertion>) -> Response {
    match login(&app, &headers, a) {
        Ok((cookie, pk)) => {
            app.analytics.signed_in(&pk.account, "passkey", false);
            // Which account, and what keeps the passkey: the page names a
            // device whose keys came from it by that (#764).
            let mut res = Json(json!({ "account": pk.account, "provider": pk.provider })).into_response();
            res.headers_mut().append(header::SET_COOKIE, cookie);
            res
        }
        Err(e) => e.into_response(),
    }
}

fn login(app: &Arc<App>, headers: &HeaderMap, a: Assertion) -> Result<(header::HeaderValue, Passkey), ApiError> {
    same_origin(app, headers)?;
    let site = app.cfg.site(headers);
    let (cd, raw) = client_data(site, &a.client_data, "webauthn.get")?;
    if !matches!(app.passkeys.take(&cd.challenge), Some(Purpose::Login)) {
        return Err(bad("that request expired; try again"));
    }
    let (pk, count) = asserted(app, site, &a, &raw)?;
    app.db.passkey_used(&a.id, count, now_ms())?;
    if a.prf {
        app.db.passkey_prf(&a.id, true)?;
    }
    let via = auth::kept_via(app, headers, auth::Via::Passkey);
    Ok((auth::start_session(app, &pk.account, via)?, pk))
}

/// A registered passkey's assertion (its client data `raw`, checked by the
/// caller): the signature checks out with its key, the authenticator
/// verified the person, and its counter didn't go backwards. The passkey,
/// and the counter to keep.
fn asserted(app: &App, site: &Site, a: &Assertion, raw: &[u8]) -> Result<(Passkey, u32), ApiError> {
    let pk =
        app.db.passkey(&a.id)?.ok_or_else(|| err(StatusCode::UNAUTHORIZED, "this passkey isn't registered here"))?;
    let data = B64.decode(&a.authenticator_data).map_err(|_| bad("bad authenticator data"))?;
    let (_, count, _) = auth_data(site, &data)?;
    let sig = B64.decode(&a.signature).map_err(|_| bad("bad signature"))?;
    if !signed(&pk, &data, raw, &sig)? {
        return Err(err(StatusCode::UNAUTHORIZED, "the passkey's signature doesn't check out"));
    }
    // A counter that goes backwards means a cloned authenticator. Synced
    // passkeys report 0 throughout.
    if count != 0 && pk.sign_count != 0 && count <= pk.sign_count {
        return Err(err(StatusCode::UNAUTHORIZED, "this passkey's counter went backwards; it may have been copied"));
    }
    Ok((pk, count))
}

/// Whether `sig` is the passkey's over `data ‖ SHA-256(client data)`.
fn signed(pk: &Passkey, data: &[u8], raw: &[u8], sig: &[u8]) -> Result<bool, ApiError> {
    let mut msg = data.to_vec();
    msg.extend_from_slice(&Sha256::digest(raw));
    Ok(match pk.alg {
        ES256 => {
            use p256::ecdsa::signature::Verifier;
            let key = p256::ecdsa::VerifyingKey::from_sec1_bytes(&pk.public).map_err(|_| bad("stored key"))?;
            let s = p256::ecdsa::Signature::from_der(sig).map_err(|_| bad("bad signature"))?;
            key.verify(&msg, &s).is_ok()
        }
        EDDSA => {
            let key: [u8; 32] = pk.public.as_slice().try_into().map_err(|_| bad("stored key"))?;
            let sig: [u8; 64] = sig.try_into().map_err(|_| bad("bad signature"))?;
            ed25519_dalek::VerifyingKey::from_bytes(&key)
                .map(|k| k.verify_strict(&msg, &ed25519_dalek::Signature::from_bytes(&sig)).is_ok())
                .unwrap_or(false)
        }
        RS256 => {
            aws_lc_rs::signature::UnparsedPublicKey::new(&aws_lc_rs::signature::RSA_PKCS1_2048_8192_SHA256, &pk.public)
                .verify(&msg, sig)
                .is_ok()
        }
        _ => false,
    })
}

/// A passkey's assertion over a challenge the platform made (M218: a
/// deploy's approval, whose challenge is SHA-256 of the approval's bytes),
/// checked for the platform (`HubPort::verify_passkey`): the passkey is
/// registered to `account`, the ceremony is a `webauthn.get` from one of
/// control's own sites over exactly `challenge` (base64url), the
/// authenticator verified the person (UV), the signature checks out with
/// the passkey's key, and its counter didn't go backwards. Nothing is
/// signed in. Why not, or `Ok`.
pub fn verify_for(
    app: &App,
    account: &str,
    challenge: &str,
    a: &arugula_platform_api::hub::PasskeyAssertion,
) -> anyhow::Result<Result<(), String>> {
    let no = |why: &str| Ok(Err(why.to_owned()));
    let Some(pk) = app.db.passkey(&a.id)? else { return no("that passkey isn't registered here") };
    if pk.account != account {
        return no("that passkey isn't one of yours");
    }
    let Ok(raw) = B64.decode(&a.client_data_json) else { return no("bad clientDataJSON") };
    let Ok(cd) = serde_json::from_slice::<ClientData>(&raw) else { return no("bad clientDataJSON") };
    if cd.kind != "webauthn.get" {
        return no("wrong WebAuthn ceremony");
    }
    if cd.challenge != challenge {
        return no("the passkey signed something other than this approval");
    }
    let Some(site) = app.cfg.sites.iter().find(|s| s.origin == cd.origin) else {
        return no("a passkey from another site");
    };
    // The sign-in's own checks: the signature, UV, and the counter.
    let assertion = Assertion {
        id: a.id.clone(),
        client_data: a.client_data_json.clone(),
        authenticator_data: a.authenticator_data.clone(),
        signature: a.signature.clone(),
        prf: false,
    };
    let count = match asserted(app, site, &assertion, &raw) {
        Ok((_, count)) => count,
        Err(e) => return Ok(Err(e.1)),
    };
    app.db.passkey_used(&a.id, count, now_ms())?;
    Ok(Ok(()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cose_keys() {
        let ed = Cbor::Map(vec![
            (Cbor::from(1), Cbor::from(1)),
            (Cbor::from(3), Cbor::from(-8)),
            (Cbor::from(-1), Cbor::from(6)),
            (Cbor::from(-2), Cbor::Bytes(vec![7; 32])),
        ]);
        assert_eq!(cose_key(&ed).unwrap(), (EDDSA, vec![7; 32]));
        // RSA with no modulus or exponent.
        let rsa = Cbor::Map(vec![(Cbor::from(1), Cbor::from(3)), (Cbor::from(3), Cbor::from(-257))]);
        assert!(cose_key(&rsa).is_err());
        // An algorithm nobody offered.
        let ps256 = Cbor::Map(vec![(Cbor::from(1), Cbor::from(3)), (Cbor::from(3), Cbor::from(-37))]);
        assert!(cose_key(&ps256).is_err());
    }

    fn rsa_cose(k: &aws_lc_rs::rsa::KeyPair) -> Cbor {
        use aws_lc_rs::signature::KeyPair as _;
        let c = aws_lc_rs::rsa::PublicKeyComponents::<Vec<u8>>::from(k.public_key());
        Cbor::Map(vec![
            (Cbor::from(1), Cbor::from(3)),
            (Cbor::from(3), Cbor::from(RS256)),
            (Cbor::from(-1), Cbor::Bytes(c.n)),
            (Cbor::from(-2), Cbor::Bytes(c.e)),
        ])
    }

    /// M218: the platform's check of a passkey over a deploy approval's
    /// bytes. Its owner's passkey, over that challenge, with UV, from
    /// control's site: yes. Anything else: why not. Nobody is signed in.
    #[test]
    fn verify_for_checks_whose_what_and_how() {
        use ed25519_dalek::Signer as _;
        let app = App::for_tests("https://control.test");
        let key = ed25519_dalek::SigningKey::from_bytes(&[9; 32]);
        app.db.add_passkey("pk1", "acct-a", EDDSA, &key.verifying_key().to_bytes(), 1).unwrap();
        let challenge = B64.encode(Sha256::digest(b"arugula deploy approval v1\n..."));
        let make = |id: &str, ch: &str, origin: &str, flags: u8| {
            let client = json!({ "type": "webauthn.get", "challenge": ch, "origin": origin }).to_string();
            let mut data = Sha256::digest(b"control.test").to_vec();
            data.push(flags);
            data.extend_from_slice(&0u32.to_be_bytes());
            let mut msg = data.clone();
            msg.extend_from_slice(&Sha256::digest(client.as_bytes()));
            arugula_platform_api::hub::PasskeyAssertion {
                id: id.into(),
                client_data_json: B64.encode(client.as_bytes()),
                authenticator_data: B64.encode(&data),
                signature: B64.encode(key.sign(&msg).to_bytes()),
            }
        };
        let site = "https://control.test";
        let check = |account: &str, a| verify_for(&app, account, &challenge, &a).unwrap();
        assert_eq!(check("acct-a", make("pk1", &challenge, site, UP | UV)), Ok(()));
        assert!(check("acct-b", make("pk1", &challenge, site, UP | UV)).is_err(), "another account's");
        assert!(check("acct-a", make("pk1", "b3RoZXI", site, UP | UV)).is_err(), "another challenge");
        assert!(check("acct-a", make("pk1", &challenge, site, UP)).is_err(), "no user verification");
        assert!(check("acct-a", make("pk1", &challenge, "https://evil.test", UP | UV)).is_err(), "another site");
        assert!(check("acct-a", make("pk-none", &challenge, site, UP | UV)).is_err(), "not registered");
        let mut forged = make("pk1", &challenge, site, UP | UV);
        forged.signature = B64.encode([0u8; 64]);
        assert!(check("acct-a", forged).is_err(), "a bad signature");
    }

    #[tokio::test]
    async fn rs256_is_offered() {
        let app = App::for_tests("http://control.test");
        let mut h = HeaderMap::new();
        h.insert(header::ORIGIN, app.cfg.origin.parse().unwrap());
        let opts = register_start(
            State(app.start()),
            ConnectInfo("127.0.0.1:1".parse().unwrap()),
            h,
            Some(Json(Start { name: Some("Sam Stranger".into()), device: None })),
        )
        .await
        .unwrap();
        let algs: Vec<i64> =
            opts.0["pubKeyCredParams"].as_array().unwrap().iter().map(|p| p["alg"].as_i64().unwrap()).collect();
        assert_eq!(algs, [EDDSA, ES256, RS256]);
    }

    /// An RSA passkey registers (a new account) and signs in.
    #[tokio::test]
    async fn rs256_registers_and_signs_in() {
        use aws_lc_rs::{rand::SystemRandom, rsa::KeySize, signature::RSA_PKCS1_SHA256};
        let app = App::for_tests("http://control.test").start();
        let mut h = HeaderMap::new();
        h.insert(header::ORIGIN, app.cfg.origin.parse().unwrap());
        h.insert(header::USER_AGENT, "Mozilla/5.0 (Windows NT 10.0; Win64; x64) Chrome/140.0".parse().unwrap());
        let client = |kind: &str, challenge: &str| {
            let cd = json!({ "type": kind, "challenge": challenge, "origin": app.cfg.origin });
            B64.encode(serde_json::to_vec(&cd).unwrap())
        };
        let rp = Sha256::digest(rp_id(&app.cfg.sites[0]).as_bytes()).to_vec();

        let k = aws_lc_rs::rsa::KeyPair::generate(KeySize::Rsa2048).unwrap();
        let cred = vec![9u8; 16];
        let mut data = rp.clone();
        data.push(UP | UV | AT);
        data.extend_from_slice(&0u32.to_be_bytes());
        // Windows Hello's AAGUID.
        data.extend_from_slice(&hex::decode("6028b017b1d44c02b4b3afcdafc96bb2").unwrap());
        data.extend_from_slice(&(cred.len() as u16).to_be_bytes());
        data.extend_from_slice(&cred);
        ciborium::into_writer(&rsa_cose(&k), &mut data).unwrap();
        let att = Cbor::Map(vec![
            (Cbor::from("fmt"), Cbor::from("none")),
            (Cbor::from("attStmt"), Cbor::Map(vec![])),
            (Cbor::from("authData"), Cbor::Bytes(data)),
        ]);
        let mut att_bytes = vec![];
        ciborium::into_writer(&att, &mut att_bytes).unwrap();
        let challenge = app.passkeys.issue(Purpose::Register { account: None, name: "Sam Stranger".into() });
        let r = Registration {
            id: B64.encode(&cred),
            client_data: client("webauthn.create", &challenge),
            attestation: B64.encode(&att_bytes),
            prf: None,
            step_up: None,
            device: None,
        };
        let (account, provider, cookie) = register(&app, &h, r).await.unwrap();
        assert_eq!(provider, Some("Windows Hello"));
        assert!(cookie.is_some());
        let stored = app.db.passkey(&B64.encode(&cred)).unwrap().unwrap();
        assert_eq!((stored.alg, stored.account.as_str()), (RS256, account.as_str()));

        let sign_in = |sig_with: &aws_lc_rs::rsa::KeyPair| {
            let challenge = app.passkeys.issue(Purpose::Login);
            let cd = client("webauthn.get", &challenge);
            let mut data = rp.clone();
            data.push(UP | UV);
            data.extend_from_slice(&0u32.to_be_bytes());
            let mut msg = data.clone();
            msg.extend_from_slice(&Sha256::digest(B64.decode(&cd).unwrap()));
            let mut sig = vec![0; sig_with.public_modulus_len()];
            sig_with.sign(&RSA_PKCS1_SHA256, &SystemRandom::new(), &msg, &mut sig).unwrap();
            login(
                &app,
                &h,
                Assertion {
                    id: B64.encode(&cred),
                    client_data: cd,
                    authenticator_data: B64.encode(&data),
                    signature: B64.encode(&sig),
                    prf: true,
                },
            )
        };
        // Listed by what keeps it and the browser that added it (#208).
        let listed = &app.db.passkeys(&account).unwrap()[0];
        assert_eq!(listed.provider.as_deref(), Some("Windows Hello"));
        assert!(listed.agent.contains("Windows NT"));
        assert_eq!(listed.used, None);
        assert_eq!(listed.prf, None, "not known until a browser says");
        assert!(sign_in(&k).is_ok());
        let listed = &app.db.passkeys(&account).unwrap()[0];
        assert!(listed.used.is_some());
        // #764: it gave PRF output at sign-in.
        assert_eq!(listed.prf, Some(true));
        // Another key's signature doesn't.
        let other = aws_lc_rs::rsa::KeyPair::generate(KeySize::Rsa2048).unwrap();
        assert!(sign_in(&other).is_err());
    }

    /// #507: control at two URLs. A passkey is made for the site it's made
    /// on, signs in there, and isn't taken at the other.
    #[tokio::test]
    async fn a_passkey_belongs_to_its_site() {
        use aws_lc_rs::{rand::SystemRandom, rsa::KeySize, signature::RSA_PKCS1_SHA256};
        let mut app = App::for_tests("http://control.test");
        app.cfg.sites.push(crate::Site::new("http://old.test").unwrap());
        let app = app.start();
        let at = |host: &str| {
            let mut h = HeaderMap::new();
            h.insert(header::HOST, host.parse().unwrap());
            h.insert(header::ORIGIN, format!("http://{host}").parse().unwrap());
            h
        };
        let (old, new) = (at("old.test"), at("control.test"));
        let client = |kind: &str, challenge: &str, origin: &str| {
            B64.encode(serde_json::to_vec(&json!({ "type": kind, "challenge": challenge, "origin": origin })).unwrap())
        };
        let rp = |host: &str| Sha256::digest(host.as_bytes()).to_vec();

        // Each site names itself as the passkey's site.
        let opts = login_start(State(app.clone()), ConnectInfo("127.0.0.1:1".parse().unwrap()), old.clone());
        assert_eq!(opts.await.unwrap().0["rpId"], "old.test");
        let opts = login_start(State(app.clone()), ConnectInfo("127.0.0.1:1".parse().unwrap()), new.clone());
        assert_eq!(opts.await.unwrap().0["rpId"], "control.test");
        // A page of one site can't start one at the other.
        let mut crossed = old.clone();
        crossed.insert(header::ORIGIN, "http://control.test".parse().unwrap());
        assert!(login_start(State(app.clone()), ConnectInfo("127.0.0.1:1".parse().unwrap()), crossed).await.is_err());

        // Made at the old site.
        let k = aws_lc_rs::rsa::KeyPair::generate(KeySize::Rsa2048).unwrap();
        let cred = vec![7u8; 16];
        let mut data = rp("old.test");
        data.push(UP | UV | AT);
        data.extend_from_slice(&0u32.to_be_bytes());
        data.extend_from_slice(&[0; 16]);
        data.extend_from_slice(&(cred.len() as u16).to_be_bytes());
        data.extend_from_slice(&cred);
        ciborium::into_writer(&rsa_cose(&k), &mut data).unwrap();
        let att = Cbor::Map(vec![
            (Cbor::from("fmt"), Cbor::from("none")),
            (Cbor::from("attStmt"), Cbor::Map(vec![])),
            (Cbor::from("authData"), Cbor::Bytes(data)),
        ]);
        let mut att_bytes = vec![];
        ciborium::into_writer(&att, &mut att_bytes).unwrap();
        let challenge = app.passkeys.issue(Purpose::Register { account: None, name: "Sam".into() });
        let r = Registration {
            id: B64.encode(&cred),
            client_data: client("webauthn.create", &challenge, "http://old.test"),
            attestation: B64.encode(&att_bytes),
            prf: Some(false),
            step_up: None,
            device: None,
        };
        register(&app, &old, r).await.unwrap();

        let sign_in = |h: &HeaderMap, origin: &str, rp_host: &str| {
            let challenge = app.passkeys.issue(Purpose::Login);
            let cd = client("webauthn.get", &challenge, origin);
            let mut data = rp(rp_host);
            data.push(UP | UV);
            data.extend_from_slice(&0u32.to_be_bytes());
            let mut msg = data.clone();
            msg.extend_from_slice(&Sha256::digest(B64.decode(&cd).unwrap()));
            let mut sig = vec![0; k.public_modulus_len()];
            k.sign(&RSA_PKCS1_SHA256, &SystemRandom::new(), &msg, &mut sig).unwrap();
            let a = Assertion {
                id: B64.encode(&cred),
                client_data: cd,
                authenticator_data: B64.encode(&data),
                signature: B64.encode(&sig),
                prf: false,
            };
            login(&app, h, a)
        };
        assert!(sign_in(&old, "http://old.test", "old.test").is_ok());
        // At the new site, as a browser would present it there, or as the
        // old site's: refused either way.
        assert!(sign_in(&new, "http://control.test", "old.test").is_err());
        assert!(sign_in(&new, "http://old.test", "old.test").is_err());
    }

    /// A software authenticator with an Ed25519 passkey, for the
    /// ceremonies below, on `control.test`.
    struct Authn {
        key: ed25519_dalek::SigningKey,
        id: Vec<u8>,
    }

    const ORIGIN: &str = "http://control.test";

    impl Authn {
        fn new(seed: u8) -> Self {
            Authn { key: ed25519_dalek::SigningKey::from_bytes(&[seed; 32]), id: vec![seed; 16] }
        }

        fn id(&self) -> String {
            B64.encode(&self.id)
        }

        fn client(kind: &str, challenge: &str, extra: serde_json::Value) -> String {
            let mut cd = json!({ "type": kind, "challenge": challenge, "origin": ORIGIN });
            cd.as_object_mut().unwrap().extend(extra.as_object().unwrap().clone());
            B64.encode(serde_json::to_vec(&cd).unwrap())
        }

        fn create(&self, challenge: &str, step_up: Option<Assertion>) -> Registration {
            let mut data = Sha256::digest(b"control.test").to_vec();
            data.push(UP | UV | AT);
            data.extend_from_slice(&0u32.to_be_bytes());
            data.extend_from_slice(&[0; 16]);
            data.extend_from_slice(&(self.id.len() as u16).to_be_bytes());
            data.extend_from_slice(&self.id);
            let cose = Cbor::Map(vec![
                (Cbor::from(1), Cbor::from(1)),
                (Cbor::from(3), Cbor::from(EDDSA)),
                (Cbor::from(-1), Cbor::from(6)),
                (Cbor::from(-2), Cbor::Bytes(self.key.verifying_key().to_bytes().to_vec())),
            ]);
            ciborium::into_writer(&cose, &mut data).unwrap();
            let att = Cbor::Map(vec![
                (Cbor::from("fmt"), Cbor::from("none")),
                (Cbor::from("attStmt"), Cbor::Map(vec![])),
                (Cbor::from("authData"), Cbor::Bytes(data)),
            ]);
            let mut att_bytes = vec![];
            ciborium::into_writer(&att, &mut att_bytes).unwrap();
            Registration {
                id: self.id(),
                client_data: Self::client("webauthn.create", challenge, json!({})),
                attestation: B64.encode(&att_bytes),
                prf: None,
                step_up,
                device: None,
            }
        }

        fn get(&self, challenge: &str, flags: u8, extra: serde_json::Value) -> Assertion {
            use ed25519_dalek::Signer as _;
            let cd = Self::client("webauthn.get", challenge, extra);
            let mut data = Sha256::digest(b"control.test").to_vec();
            data.push(flags);
            data.extend_from_slice(&0u32.to_be_bytes());
            let mut msg = data.clone();
            msg.extend_from_slice(&Sha256::digest(B64.decode(&cd).unwrap()));
            Assertion {
                id: self.id(),
                client_data: cd,
                authenticator_data: B64.encode(&data),
                signature: B64.encode(self.key.sign(&msg).to_bytes()),
                prf: false,
            }
        }
    }

    /// `account`, signed in `via`, `age_ms` ago: the page's headers.
    fn signed_in(app: &App, account: &str, via: auth::Via, age_ms: u64) -> HeaderMap {
        app.db.account_for("github", &format!("gh-{account}"), account, account, 1).unwrap();
        let t = auth::token();
        let at = now_ms() - age_ms;
        app.db.add_session(&auth::hash(&t), account, at, at + 3_600_000).unwrap();
        app.db.set_session_via(&auth::hash(&t), via.as_str()).unwrap();
        let mut h = HeaderMap::new();
        h.insert(header::ORIGIN, ORIGIN.parse().unwrap());
        h.insert(header::COOKIE, format!("{}={t}", auth::SESSION_COOKIE).parse().unwrap());
        h
    }

    async fn start(app: &Arc<App>, h: &HeaderMap) -> Result<serde_json::Value, ApiError> {
        start_on(app, h, None).await
    }

    /// Started by a page that says it's `device`.
    async fn start_on(app: &Arc<App>, h: &HeaderMap, device: Option<&str>) -> Result<serde_json::Value, ApiError> {
        let body = Start { name: None, device: device.map(str::to_owned) };
        register_start(State(app.clone()), ConnectInfo("127.0.0.1:1".parse().unwrap()), h.clone(), Some(Json(body)))
            .await
            .map(|j| j.0)
    }

    async fn step_up(app: &Arc<App>, h: &HeaderMap, id: &str) -> Result<serde_json::Value, ApiError> {
        register_step_up(State(app.clone()), h.clone(), Json(StepUp { id: id.into() })).await.map(|j| j.0)
    }

    fn reason(e: ApiError) -> Option<&'static str> {
        e.2
    }

    /// An account with no passkey adds one with a GitHub sign-in from the
    /// last ten minutes, not an older one, nor another kind of sign-in.
    #[tokio::test]
    async fn a_first_passkey_takes_a_fresh_github_sign_in() {
        let app = App::for_tests(ORIGIN).start();
        for (via, age) in [(auth::Via::Github, REAUTH_MS + 60_000), (auth::Via::Passkey, 0)] {
            let h = signed_in(&app, "acct-old", via, age);
            assert_eq!(start(&app, &h).await.map_err(reason).err(), Some(Some(REAUTH)), "{via:?}, {age} ms old");
        }
        let h = signed_in(&app, "acct-fresh", auth::Via::Github, 60_000);
        let opts = start(&app, &h).await.unwrap();
        assert!(opts["step_up"].is_null(), "none to confirm with");
        let a = Authn::new(1);
        let (account, _, cookie) =
            register(&app, &h, a.create(opts["challenge"].as_str().unwrap(), None)).await.unwrap();
        assert_eq!((account.as_str(), cookie.is_none()), ("acct-fresh", true));
        // Kept with which sign-in added it.
        let session = &auth::hash(auth::cookie(&h, auth::SESSION_COOKIE).unwrap())[..crate::account::SESSION_ID_LEN];
        let added: (Option<String>, Option<String>) = app
            .db
            .conn()
            .query_row("SELECT added_device, added_session FROM passkeys WHERE id = ?1", [a.id()], |r| {
                Ok((r.get(0)?, r.get(1)?))
            })
            .unwrap();
        assert_eq!(added, (None, Some(session.to_owned())));
        // Its GitHub sign-in went stale before it finished: refused there too.
        let h = signed_in(&app, "acct-slow", auth::Via::Github, 0);
        let opts = start(&app, &h).await.unwrap();
        app.db.conn().execute("UPDATE sessions SET created = 1 WHERE account = 'acct-slow'", []).unwrap();
        let r = register(&app, &h, Authn::new(2).create(opts["challenge"].as_str().unwrap(), None)).await;
        assert_eq!(r.map_err(reason).err(), Some(Some(REAUTH)));
    }

    /// An account with a passkey adds another only when one of its own
    /// agrees, with user verification, over the challenge made for this
    /// account and this new passkey.
    #[tokio::test]
    async fn another_passkey_takes_one_the_account_has() {
        let app = App::for_tests(ORIGIN).start();
        let mine = Authn::new(7);
        app.db.account_for("github", "gh-acct-a", "a", "acct-a", 1).unwrap();
        app.db.add_passkey(&mine.id(), "acct-a", EDDSA, &mine.key.verifying_key().to_bytes(), 1).unwrap();
        let theirs = Authn::new(8);
        app.db.account_for("github", "gh-acct-b", "b", "acct-b", 1).unwrap();
        app.db.add_passkey(&theirs.id(), "acct-b", EDDSA, &theirs.key.verifying_key().to_bytes(), 1).unwrap();
        // A fresh GitHub sign-in isn't enough here.
        let h = signed_in(&app, "acct-a", auth::Via::Github, 0);
        let new = Authn::new(9);
        async fn attempt(
            app: &Arc<App>,
            h: &HeaderMap,
            new: &Authn,
            step: Option<Assertion>,
        ) -> Result<String, ApiError> {
            let opts = start(app, h).await.unwrap();
            assert_eq!(opts["step_up"], "passkey");
            register(app, h, new.create(opts["challenge"].as_str().unwrap(), step)).await.map(|r| r.0)
        }
        let attempt = |step| attempt(&app, &h, &new, step);
        assert_eq!(attempt(None).await.map_err(reason).err(), Some(Some("step_up")));
        async fn ch(app: &Arc<App>, h: &HeaderMap, id: &str) -> String {
            step_up(app, h, id).await.unwrap()["challenge"].as_str().unwrap().to_owned()
        }
        let (app_, h_) = (&app, &h);
        let ch = move |id: String| async move { ch(app_, h_, &id).await };
        // Over a challenge made for another new passkey.
        let other = ch(Authn::new(10).id()).await;
        assert!(attempt(Some(mine.get(&other, UP | UV, json!({})))).await.is_err(), "another new passkey's");
        // Another account's passkey.
        let c = ch(new.id()).await;
        assert!(attempt(Some(theirs.get(&c, UP | UV, json!({})))).await.is_err(), "another account's passkey");
        // Without user verification.
        let c = ch(new.id()).await;
        assert!(attempt(Some(mine.get(&c, UP, json!({})))).await.is_err(), "no user verification");
        // From inside another site's frame.
        let c = ch(new.id()).await;
        let framed = mine.get(&c, UP | UV, json!({ "crossOrigin": true, "topOrigin": "https://evil.test" }));
        assert!(attempt(Some(framed)).await.is_err(), "framed");
        // A made-up challenge.
        assert!(attempt(Some(mine.get("bm90IG91cnM", UP | UV, json!({})))).await.is_err(), "not one control made");
        // The new passkey can't vouch for itself, nor ask for a challenge.
        assert!(step_up(&app, &h, &mine.id()).await.is_err(), "not a new passkey");
        // Its own: added, once.
        let c = ch(new.id()).await;
        let step = mine.get(&c, UP | UV, json!({}));
        assert_eq!(attempt(Some(step.clone())).await.unwrap(), "acct-a");
        assert_eq!(app.db.passkey_count("acct-a").unwrap(), 2);
        assert!(app.db.passkeys("acct-b").unwrap().len() == 1, "nothing changed for the other account");
        // Its challenge works once.
        let again = Authn::new(11);
        let opts = start(&app, &h).await.unwrap();
        let r = register(&app, &h, again.create(opts["challenge"].as_str().unwrap(), Some(step))).await;
        assert!(r.is_err(), "a step-up is good once");
    }

    /// Every device of the account hears that a passkey was added, naming
    /// the device whose sign-in added it.
    #[tokio::test]
    async fn adding_a_passkey_tells_every_device() {
        use arugula_e2e::{Cert, DeviceKeys, Kind};
        let mut app = App::for_tests(ORIGIN);
        let h = signed_in(&app, "acct-p", auth::Via::Github, 0);
        let keys = DeviceKeys::generate();
        let mut root = Cert::new(&keys, "acct-p", Kind::Browser, "Firefox on Linux");
        root.sign_with(&keys);
        app.db.put_device(&root, true, now_ms()).unwrap();
        app.db
            .set_session_device(&auth::hash(auth::cookie(&h, auth::SESSION_COOKIE).unwrap()), "acct-p", &keys.id())
            .unwrap();
        let (mut phone, host) = crate::push_notices::Phone::subscribe(&app, "acct-p", &keys, 5).await;
        app.cfg.push_hosts = vec![host];
        let app = app.start();
        let opts = start_on(&app, &h, Some(&keys.id())).await.unwrap();
        assert_eq!(opts["device_sig"], true);
        let challenge = opts["challenge"].as_str().unwrap();
        let a = Authn::new(3);
        let mut r = a.create(challenge, None);
        r.device = Some(DeviceSig {
            id: keys.id(),
            sig: hex::encode(keys.signature(add_passkey_device_body("acct-p", &a.id(), challenge).as_bytes())),
        });
        register(&app, &h, r).await.unwrap();
        let n = phone.next().await.expect("a push");
        assert_eq!(n["title"], "A passkey was added to your account");
        assert!(n["body"].as_str().unwrap().contains("\"Firefox on Linux\""), "{n}");
        assert_eq!(app.db.passkeys("acct-p").unwrap()[0].added_device.as_deref(), Some(keys.id().as_str()));
    }

    /// Without a fresh GitHub sign-in, an account with a browser or a phone
    /// adds a passkey only with one of them signing for it: not a CLI's or
    /// the app's key, not for another passkey or another registration. Its
    /// push names that device, not what the sign-in said it is.
    #[tokio::test]
    async fn a_browser_of_the_account_signs_for_a_new_passkey() {
        use arugula_e2e::{Cert, DeviceKeys, Kind};
        let app = App::for_tests(ORIGIN).start();
        let h = signed_in(&app, "acct-d", auth::Via::Github, REAUTH_MS + 60_000);
        let root = DeviceKeys::generate();
        let mut rc = Cert::new(&root, "acct-d", Kind::Browser, "Safari on iPhone");
        rc.sign_with(&root);
        app.db.put_device(&rc, true, now_ms()).unwrap();
        let other = |kind| {
            let k = DeviceKeys::generate();
            let mut c = Cert::new(&k, "acct-d", kind, "x");
            c.sign_with(&root);
            app.db.put_device(&c, true, now_ms()).unwrap();
            k
        };
        let (cli, appk) = (other(Kind::Cli), other(Kind::App));
        let sig = |k: &DeviceKeys, cred: &str, ch: &str| DeviceSig {
            id: k.id(),
            sig: hex::encode(k.signature(add_passkey_device_body("acct-d", cred, ch).as_bytes())),
        };
        let page = root.id();
        assert_eq!(start(&app, &h).await.map_err(reason).err(), Some(Some(REAUTH)), "a page that isn't one");
        async fn attempt_with(
            app: &Arc<App>,
            h: &HeaderMap,
            page: &str,
            seed: u8,
            device: &dyn Fn(&str, &str) -> Option<DeviceSig>,
        ) -> Result<String, ApiError> {
            let a = Authn::new(seed);
            let opts = start_on(app, h, Some(page)).await.unwrap();
            assert_eq!(opts["device_sig"], true);
            let ch = opts["challenge"].as_str().unwrap().to_owned();
            let mut r = a.create(&ch, None);
            r.device = device(&a.id(), &ch);
            register(app, h, r).await.map(|r| r.0)
        }
        assert_eq!(
            attempt_with(&app, &h, &page, 20, &|_, _| None).await.map_err(reason).err(),
            Some(Some(REAUTH)),
            "no device's signature, and the GitHub sign-in is stale"
        );
        assert!(attempt_with(&app, &h, &page, 21, &|c, ch| Some(sig(&cli, c, ch))).await.is_err(), "a CLI's key");
        assert!(attempt_with(&app, &h, &page, 22, &|c, ch| Some(sig(&appk, c, ch))).await.is_err(), "the app's key");
        assert!(
            attempt_with(&app, &h, &page, 23, &|_, ch| Some(sig(&root, "another", ch))).await.is_err(),
            "for another passkey"
        );
        assert!(
            attempt_with(&app, &h, &page, 24, &|c, _| Some(sig(&root, c, "another"))).await.is_err(),
            "for another registration"
        );
        assert_eq!(attempt_with(&app, &h, &page, 25, &|c, ch| Some(sig(&root, c, ch))).await.unwrap(), "acct-d");
        assert_eq!(app.db.passkeys("acct-d").unwrap()[0].added_device.as_deref(), Some(root.id().as_str()));
    }

    /// An account whose browsers an app window approved before #1343 (so
    /// none of them can sign) adds its first passkey after a fresh GitHub
    /// sign-in, and not with an older one; a second still takes a passkey
    /// it has.
    #[tokio::test]
    async fn a_first_passkey_without_a_browser_that_signs_takes_github() {
        use arugula_e2e::{Cert, DeviceKeys, Kind};
        let app = App::for_tests(ORIGIN).start();
        let stale = signed_in(&app, "acct-e", auth::Via::Github, REAUTH_MS + 60_000);
        let root = DeviceKeys::generate();
        let mut rc = Cert::new(&root, "acct-e", Kind::Browser, "Arugula app on geek");
        rc.sign_with(&root);
        app.db.put_device(&rc, true, now_ms()).unwrap();
        let firefox = DeviceKeys::generate();
        let mut fc = Cert::new(&firefox, "acct-e", Kind::Browser, "Firefox on Mac");
        fc.sign_with(&root);
        app.db.put_device(&fc, true, now_ms()).unwrap();
        assert!(browser_devices(&app, "acct-e").unwrap().is_empty(), "approved by an app window");
        let r = start_on(&app, &stale, Some(&firefox.id())).await;
        assert_eq!(r.map_err(reason).err(), Some(Some(REAUTH)), "a stale sign-in goes through GitHub again");
        let fresh = signed_in(&app, "acct-e", auth::Via::Github, 60_000);
        let opts = start_on(&app, &fresh, Some(&firefox.id())).await.unwrap();
        assert_eq!((opts["device_sig"].clone(), opts["step_up"].clone()), (json!(false), json!(null)));
        let a = Authn::new(30);
        let (account, _, _) =
            register(&app, &fresh, a.create(opts["challenge"].as_str().unwrap(), None)).await.unwrap();
        assert_eq!(account, "acct-e");
        // A second one: GitHub isn't enough.
        let opts = start_on(&app, &fresh, Some(&firefox.id())).await.unwrap();
        assert_eq!(opts["step_up"], "passkey");
        let r = register(&app, &fresh, Authn::new(31).create(opts["challenge"].as_str().unwrap(), None)).await;
        assert_eq!(r.map_err(reason).err(), Some(Some("step_up")));
    }

    /// A browser that the desktop app's window let in, from before the app
    /// had a kind, signs for a new passkey when control stored and approved
    /// both before #1343 shipped (`legacy_app`); not one the window let in
    /// after, nor through a window stored after, and never the window
    /// itself.
    #[tokio::test]
    async fn a_browser_a_grandfathered_app_window_let_in_signs_for_a_passkey() {
        use arugula_e2e::{Cert, DeviceKeys, Kind};

        use crate::legacy_app::APP_KIND_SHIPPED_MS as CUTOFF;
        let app = App::for_tests(ORIGIN).start();
        let h = signed_in(&app, "acct-g", auth::Via::Github, REAUTH_MS + 60_000);
        let root = DeviceKeys::generate();
        let mut rc = Cert::new(&root, "acct-g", Kind::Browser, "Safari on iPhone");
        rc.sign_with(&root);
        app.db.put_device(&rc, true, 1).unwrap();
        let put = |name: &str, by: &DeviceKeys, at: u64| {
            let k = DeviceKeys::generate();
            let mut c = Cert::new(&k, "acct-g", Kind::Browser, name);
            c.sign_with(by);
            app.db.put_device(&c, true, at).unwrap();
            k
        };
        let window = put("Arugula app on jake-air", &root, CUTOFF - 1);
        let firefox = put("Firefox on Mac", &window, CUTOFF - 1);
        let late = put("Arugula app on jake-mini", &root, CUTOFF);
        let chrome = put("Chrome on Mac", &late, CUTOFF - 1);
        let edge = put("Edge", &window, now_ms());
        let mut can: Vec<String> = browser_devices(&app, "acct-g").unwrap().into_iter().map(|c| c.device).collect();
        can.sort();
        let mut want = vec![root.id(), firefox.id()];
        want.sort();
        assert_eq!(can, want, "the root and Firefox: not either window, nor Chrome or Edge");
        // The page asking is the root's; the device signing for it varies.
        async fn sign(app: &Arc<App>, h: &HeaderMap, page: &str, k: &DeviceKeys, seed: u8) -> Result<String, ApiError> {
            let a = Authn::new(seed);
            let opts = start_on(app, h, Some(page)).await.unwrap();
            let ch = opts["challenge"].as_str().unwrap().to_owned();
            let mut r = a.create(&ch, None);
            r.device = Some(DeviceSig {
                id: k.id(),
                sig: hex::encode(k.signature(add_passkey_device_body("acct-g", &a.id(), &ch).as_bytes())),
            });
            register(app, h, r).await.map(|r| r.0)
        }
        let page = root.id();
        assert!(sign(&app, &h, &page, &window, 50).await.is_err(), "the window itself");
        assert!(sign(&app, &h, &page, &chrome, 51).await.is_err(), "through a window stored after");
        assert!(sign(&app, &h, &page, &edge, 53).await.is_err(), "what the window let in after");
        assert_eq!(sign(&app, &h, &page, &firefox, 52).await.unwrap(), "acct-g");
        assert_eq!(app.db.passkeys("acct-g").unwrap()[0].added_device.as_deref(), Some(firefox.id().as_str()));
    }

    /// The desktop app's window signing in with a passkey stays the app's
    /// session; a browser's becomes a passkey sign-in.
    #[tokio::test]
    async fn a_passkey_sign_in_in_the_apps_window_stays_the_apps() {
        let app = App::for_tests(ORIGIN).start();
        let mine = Authn::new(40);
        let h = signed_in(&app, "acct-x", auth::Via::App, 0);
        app.db.add_passkey(&mine.id(), "acct-x", EDDSA, &mine.key.verifying_key().to_bytes(), 1).unwrap();
        let via_of = |h: &HeaderMap| {
            let challenge = app.passkeys.issue(Purpose::Login);
            let (cookie, _) = login(&app, h, mine.get(&challenge, UP | UV, json!({}))).unwrap();
            let t = cookie.to_str().unwrap().split(';').next().unwrap().split_once('=').unwrap().1.to_owned();
            app.db.session_info(&auth::hash(&t), now_ms()).unwrap().unwrap().via
        };
        assert_eq!(via_of(&h).as_deref(), Some("app"));
        let browser = signed_in(&app, "acct-y", auth::Via::Github, 0);
        assert_eq!(via_of(&browser).as_deref(), Some("passkey"));
        let mut none = HeaderMap::new();
        none.insert(header::ORIGIN, ORIGIN.parse().unwrap());
        assert_eq!(via_of(&none).as_deref(), Some("passkey"));
    }

    /// The desktop app's window makes no passkeys.
    #[tokio::test]
    async fn the_apps_window_makes_no_passkeys() {
        let app = App::for_tests(ORIGIN).start();
        let h = signed_in(&app, "acct-w", auth::Via::App, 0);
        assert_eq!(start(&app, &h).await.map_err(reason).err(), Some(Some("app_window")));
        let mine = Authn::new(30);
        app.db.add_passkey(&mine.id(), "acct-w", EDDSA, &mine.key.verifying_key().to_bytes(), 1).unwrap();
        assert_eq!(step_up(&app, &h, &Authn::new(31).id()).await.map_err(reason).err(), Some(Some("app_window")));
        let challenge = app.passkeys.issue(Purpose::Register { account: Some("acct-w".into()), name: "w".into() });
        let r = register(&app, &h, Authn::new(31).create(&challenge, None)).await;
        assert_eq!(r.map_err(reason).err(), Some(Some("app_window")));
    }

    /// Every ceremony refuses one run in a frame of another site's page,
    /// and so does anything else that reads its client data.
    #[test]
    fn framed_ceremonies_are_refused() {
        let site = crate::Site::new(ORIGIN).unwrap();
        let cd = |extra: serde_json::Value| Authn::client("webauthn.get", "x", extra);
        let parses =
            |extra: serde_json::Value| serde_json::from_slice::<ClientData>(&B64.decode(cd(extra)).unwrap()).is_ok();
        for ok in [json!({}), json!({ "crossOrigin": false }), json!({ "topOrigin": ORIGIN })] {
            assert!(client_data(&site, &cd(ok.clone()), "webauthn.get").is_ok(), "{ok}");
            assert!(parses(ok));
        }
        for framed in [json!({ "crossOrigin": true }), json!({ "topOrigin": "https://evil.test" })] {
            let e = client_data(&site, &cd(framed.clone()), "webauthn.get").err().unwrap();
            assert_eq!(e.0, StatusCode::FORBIDDEN, "{framed}");
            assert!(!parses(framed));
        }
    }

    #[test]
    fn short_rsa_keys_are_refused() {
        let mut short = rsa_cose(&aws_lc_rs::rsa::KeyPair::generate(aws_lc_rs::rsa::KeySize::Rsa2048).unwrap());
        if let Cbor::Map(m) = &mut short {
            m[2].1 = Cbor::Bytes(vec![0xc5; 128]);
        }
        assert!(cose_key(&short).is_err());
    }
}
