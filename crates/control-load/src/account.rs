//! A person, as their browser page is to control: a new account made with
//! a passkey (made in software here), the browser as its first device, and
//! that browser approving the account's machines as they join.

use std::{sync::Arc, time::Instant};

use anyhow::{Context as _, bail};
use arugula_control_wire as wire;
use arugula_e2e::{Cert, DeviceKeys, cert::Kind, now_ms};
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD as B64};
use ciborium::value::Value as Cbor;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

use crate::{Ctx, keys};

#[derive(Serialize, Deserialize)]
pub struct Account {
    pub account: String,
    /// The session cookie's token.
    pub session: String,
    /// The browser's keys, as `keys::to_text` writes them.
    pub browser: String,
    #[serde(skip)]
    pub keys: Option<Arc<DeviceKeys>>,
}

impl Account {
    pub fn keys(&self) -> &DeviceKeys {
        self.keys.as_ref().expect("keys loaded")
    }

    pub fn cookie(&self) -> String {
        format!("ilg_session={}", self.session)
    }
}

/// A request as the page sends it: its origin and its cookie.
fn page(ctx: &Ctx, b: reqwest::RequestBuilder, cookie: Option<&str>) -> reqwest::RequestBuilder {
    let b = b.header("origin", &ctx.origin);
    match cookie {
        Some(c) => b.header("cookie", c),
        None => b,
    }
}

/// Send, time it under `name`, count its status, and read its JSON.
pub async fn call(ctx: &Ctx, name: &str, b: reqwest::RequestBuilder) -> anyhow::Result<(Value, Vec<String>)> {
    let t = Instant::now();
    let r = b.send().await;
    ctx.stats.time(name, t);
    let r = match r {
        Ok(r) => r,
        Err(e) => {
            ctx.stats.inc(&format!("{name}:error"));
            return Err(e.into());
        }
    };
    let status = r.status();
    if !status.is_success() {
        ctx.stats.inc(&format!("{name}:{}", status.as_u16()));
        let body = r.text().await.unwrap_or_default();
        bail!("{name}: {status}: {}", body.chars().take(200).collect::<String>());
    }
    let cookies =
        r.headers().get_all("set-cookie").iter().filter_map(|v| v.to_str().ok().map(str::to_owned)).collect::<Vec<_>>();
    let v = r.json::<Value>().await.unwrap_or(Value::Null);
    Ok((v, cookies))
}

/// The attestation a platform authenticator would give for a new Ed25519
/// passkey: `none`, with the person verified.
fn attestation(rp_id: &str, cred_id: &[u8], public: &[u8; 32]) -> Vec<u8> {
    let mut data = Sha256::digest(rp_id.as_bytes()).to_vec();
    // User present, user verified, attested credential data.
    data.push(0x01 | 0x04 | 0x40);
    data.extend_from_slice(&0u32.to_be_bytes());
    data.extend_from_slice(&[0u8; 16]);
    data.extend_from_slice(&(cred_id.len() as u16).to_be_bytes());
    data.extend_from_slice(cred_id);
    let key = Cbor::Map(vec![
        (Cbor::Integer(1.into()), Cbor::Integer(1.into())),
        (Cbor::Integer(3.into()), Cbor::Integer((-8).into())),
        (Cbor::Integer((-1).into()), Cbor::Integer(6.into())),
        (Cbor::Integer((-2).into()), Cbor::Bytes(public.to_vec())),
    ]);
    ciborium::into_writer(&key, &mut data).expect("cbor");
    let att = Cbor::Map(vec![
        (Cbor::Text("fmt".into()), Cbor::Text("none".into())),
        (Cbor::Text("attStmt".into()), Cbor::Map(vec![])),
        (Cbor::Text("authData".into()), Cbor::Bytes(data)),
    ]);
    let mut out = vec![];
    ciborium::into_writer(&att, &mut out).expect("cbor");
    out
}

/// Sign up as a new person: a passkey, the session it starts, the browser
/// as the account's first device, and the pages' first calls (the
/// platform's among them).
pub async fn sign_up(ctx: &Ctx, name: &str) -> anyhow::Result<Account> {
    let started = Instant::now();
    let http = &ctx.http;
    let (opts, _) = call(
        ctx,
        "signup.passkey_start",
        page(ctx, http.post(ctx.at("/auth/passkey/register")), None).json(&json!({ "name": name })),
    )
    .await?;
    let challenge = opts["challenge"].as_str().context("no challenge")?;
    let rp_id = opts["rp"]["id"].as_str().context("no rp id")?;
    let cred_id = arugula_e2e::random::<16>();
    let public = ed25519_public();
    let client_data = json!({ "type": "webauthn.create", "challenge": challenge, "origin": ctx.origin }).to_string();
    let body = json!({
        "id": B64.encode(cred_id),
        "clientDataJSON": B64.encode(client_data),
        "attestationObject": B64.encode(attestation(rp_id, &cred_id, &public)),
        "prf": false,
    });
    let (made, cookies) = call(
        ctx,
        "signup.passkey_finish",
        page(ctx, http.post(ctx.at("/auth/passkey/register/finish")), None).json(&body),
    )
    .await?;
    let account = made["account"].as_str().context("no account")?.to_owned();
    let session = cookies
        .iter()
        .find_map(|c| c.strip_prefix("ilg_session=").map(|v| v.split(';').next().unwrap_or("").to_owned()))
        .context("no session cookie")?;
    let keys = DeviceKeys::generate();
    let mut a = Account { account, session, browser: keys::to_text(&keys), keys: Some(Arc::new(keys)) };
    let cookie = a.cookie();
    call(ctx, "page.me", page(ctx, http.get(ctx.at("/api/me")), Some(&cookie))).await?;
    // The first device: this browser, self-signed.
    let mut cert = Cert::new(a.keys(), &a.account, Kind::Browser, "load test browser");
    cert.approver = cert.device.clone();
    cert.sign_with(a.keys());
    let (r, _) = call(
        ctx,
        "signup.first_device",
        page(ctx, http.post(ctx.at("/api/devices")), Some(&cookie)).json(&json!({ "cert": cert })),
    )
    .await?;
    if r["approved"] != json!(true) {
        bail!("the first device wasn't approved: {r}");
    }
    // What a new page loads first, the platform's projects among it.
    first_load(ctx, &cookie).await;
    ctx.stats.time("signup.total", started);
    a.browser = keys::to_text(a.keys());
    Ok(a)
}

/// The calls a signed-in page makes as it loads, and every refresh
/// (`control.ts`'s `refresh`, every 10 s): control's own.
pub async fn page_refresh(ctx: &Ctx, cookie: &str) {
    let http = &ctx.http;
    let devices = call(ctx, "page.devices", page(ctx, http.get(ctx.at("/api/devices")), Some(cookie)));
    let directory = call(ctx, "page.directory", page(ctx, http.get(ctx.at("/api/directory")), Some(cookie)));
    let _ = tokio::join!(devices, directory);
}

/// The platform's (behind the hub's gateway, on its Postgres): what the
/// platform page asks first.
pub async fn platform_refresh(ctx: &Ctx, cookie: &str) {
    let http = &ctx.http;
    let projects = call(ctx, "platform.projects", page(ctx, http.get(ctx.at("/api/projects")), Some(cookie)));
    let inbox = call(ctx, "platform.inbox", page(ctx, http.get(ctx.at("/api/inbox")), Some(cookie)));
    let _ = tokio::join!(projects, inbox);
}

async fn first_load(ctx: &Ctx, cookie: &str) {
    page_refresh(ctx, cookie).await;
    platform_refresh(ctx, cookie).await;
}

fn ed25519_public() -> [u8; 32] {
    // Any Ed25519 key does: registration checks only that it's one.
    DeviceKeys::generate().sign_public()
}

/// A machine joining `a`'s account: it asks, the browser approves its code,
/// and it polls for its certificate.
pub async fn enrol(ctx: &Ctx, a: &Account, name: &str) -> anyhow::Result<DeviceKeys> {
    let started = Instant::now();
    let http = &ctx.http;
    let keys = DeviceKeys::generate();
    let ask = Cert { account: String::new(), ..Cert::new(&keys, "", Kind::Daemon, name) };
    let ms = now_ms();
    let proof = wire::JoinProof {
        ms,
        sig: hex::encode(keys.signature(arugula_e2e::cert::join_proof_body(&ask, ms).as_bytes())),
    };
    let req =
        wire::JoinRequest { cert: ask.clone(), urls: vec![], team: None, features: String::new(), proof: Some(proof) };
    let (started_join, _) = call(ctx, "enrol.join", http.post(ctx.at(wire::JOIN)).json(&req)).await?;
    let started_join: wire::JoinStarted = serde_json::from_value(started_join)?;
    // The browser approves it: the machine's certificate, signed by it.
    let mut cert = Cert { account: a.account.clone(), created: now_ms(), approver: a.keys().id(), ..ask };
    cert.sign_with(a.keys());
    let path = format!("/api/joins/{}/approve", started_join.code);
    call(ctx, "enrol.approve", page(ctx, http.post(ctx.at(&path)), Some(&a.cookie())).json(&json!({ "cert": cert })))
        .await?;
    let (poll, _) =
        call(ctx, "enrol.poll", http.get(ctx.at(&wire::JoinPoll::path(&started_join.code, &started_join.poll))))
            .await?;
    if poll["approved"] != json!(true) {
        bail!("not approved: {poll}");
    }
    ctx.stats.time("enrol.total", started);
    Ok(keys)
}
