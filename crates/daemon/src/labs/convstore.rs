//! S38 prototype (#979, throwaway, behind `chat`): a conversation kept in
//! control as ciphertext, mirrored on every one of the owner's machines,
//! with its lead run by whichever machine holds the lease.
//!
//! - **Moving one in** ([`upload`], `POST /api/s38/convs/{id}/upload`): a
//!   new key, wrapped to each of the account's devices; the record and every
//!   message sealed and put in control, in order, so control's numbers are
//!   the message ids; and this machine, its home, takes the lease first.
//! - **Mirrored** ([`start`]): each second, every machine of the account
//!   lists the conversations in control, unwraps a new one's key with its
//!   own device key, and takes what's new: the record (its lead stays the
//!   machine's own) and the log's entries, kept here as the timeline is
//!   (`Api::ThreadIngest`). The local timeline is a cache of control's.
//! - **Posting** ([`post`]): checked and made up here (`Api::ThreadCompose`),
//!   sealed and appended in control, then taken back from the log, so
//!   every machine numbers it the same.
//! - **The lease:** one machine at a time runs the lead. It renews every
//!   five seconds; one that finds it free or expired takes it, with a
//!   fencing token one higher. What only the lead's machine writes (the
//!   lead's replies, its links' events) carries the token, and control
//!   refuses a stale one. The holder hands each `@lead` past the lease's
//!   cursor to its lead and moves the cursor, so the next holder carries on
//!   where it stopped. A machine that loses the lease stops its lead and
//!   its links' watchers.
//!
//! Not built: other accounts' devices (members), browsers reading control
//! directly, key rotation, signed entries, the record's merge.

use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::{Arc, LazyLock, Mutex},
    time::Duration,
};

use arugula_proto::{ConvId, Conversation, ThreadMsg, ThreadTarget};
use axum::{
    Json, Router,
    extract::{Path as UrlPath, State},
    http::StatusCode,
    routing::{get, post as http_post},
};
use base64::Engine;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use tracing::{info, warn};

use crate::{
    acl::Principal,
    mux::{Api, Posted, ThreadError, ThreadPost},
    server::App,
};

/// How long before the lease's expiry (by this machine's wall clock) it
/// stops counting itself the holder: a margin for clocks and the network.
const MARGIN_MS: u64 = 3_000;

/// How often the holder renews.
const RENEW_MS: u64 = 5_000;

#[derive(Clone, Serialize, Deserialize)]
struct Side {
    key: String,
    ver: u64,
    /// The record as last put in or taken from control.
    synced: String,
}

#[derive(Clone, Copy, Debug)]
struct Held {
    token: u64,
    /// Wall-clock ms: it counts itself the holder until then.
    until: u64,
    sent: u64,
    cursor: u64,
}

struct Local {
    side: Side,
    key: [u8; 32],
    held: Option<Held>,
}

static STATE: LazyLock<Mutex<HashMap<ConvId, Local>>> = LazyLock::new(Default::default);

/// One sync at a time: entries go in in order.
static SYNC: LazyLock<tokio::sync::Mutex<()>> = LazyLock::new(Default::default);

fn now() -> u64 {
    crate::store::now_ms()
}

fn b64(b: &[u8]) -> String {
    base64::engine::general_purpose::STANDARD.encode(b)
}

fn unb64(s: &str) -> Option<Vec<u8>> {
    base64::engine::general_purpose::STANDARD.decode(s).ok()
}

fn dir(app: &App) -> PathBuf {
    app.control.state_dir().join("s38")
}

fn side_path(d: &Path, id: ConvId) -> PathBuf {
    d.join(format!("{id}.json"))
}

fn save_side(app: &App, id: ConvId, s: &Side) {
    let d = dir(app);
    let _ = crate::store::private_dir(&d);
    if let Err(e) = crate::store::write_atomic(&side_path(&d, id), &serde_json::to_vec(s).unwrap_or_default()) {
        warn!(conv = %id, error = %e, "s38: can't keep the conversation's key");
    }
}

/// Whether conversation `id` is kept in control.
pub fn stored(id: ConvId) -> bool {
    STATE.lock().unwrap().contains_key(&id)
}

/// The lease's token, while this machine holds it.
pub fn leading(id: ConvId) -> Option<u64> {
    let s = STATE.lock().unwrap();
    let h = s.get(&id)?.held?;
    (now() < h.until).then_some(h.token)
}

fn key_of(id: ConvId) -> Option<[u8; 32]> {
    STATE.lock().unwrap().get(&id).map(|l| l.key)
}

/// This machine: its device id and name in control.
fn me(app: &App) -> Option<(String, String)> {
    let e = app.control.enrolled()?;
    Some((e.saved.cert.device.clone(), e.saved.cert.name.clone()))
}

/// The record as control keeps it: no lead (that's a machine's own), and
/// every link naming its machine, this one too.
fn record_of(app: &App, c: &Conversation) -> String {
    let mut c = c.clone();
    c.lead = None;
    if let Some((id, name)) = me(app) {
        for l in &mut c.links {
            if l.machine.is_none() {
                l.machine = Some(id.clone());
                l.machine_name = name.clone();
            }
        }
    }
    serde_json::to_string(&c).unwrap_or_default()
}

/// A record from control, as this machine keeps it: its own links are
/// "here".
fn localize(app: &App, mut c: Conversation) -> Conversation {
    if let Some((id, _)) = me(app) {
        for l in &mut c.links {
            if l.machine.as_deref() == Some(id.as_str()) {
                l.machine = None;
            }
        }
    }
    c
}

async fn conv(app: &App, id: ConvId) -> Option<Conversation> {
    app.mux.api(Api::Convs).await?.into_iter().find(|c| c.id == id)
}

async fn messages(app: &App, id: ConvId) -> Vec<ThreadMsg> {
    match app.mux.api(|r| Api::ThreadGet(ThreadTarget::Conv(id), Principal::Owner, r)).await {
        Some(Ok(m)) => m,
        _ => Vec::new(),
    }
}

async fn call(app: &App, path: &str, body: Option<Value>) -> Result<Value, (u16, String)> {
    match app.control.s38(path, body.as_ref()).await {
        Ok((s, v)) if s < 300 => Ok(v),
        Ok((s, v)) => Err((s, v["error"].as_str().unwrap_or("refused").to_owned())),
        Err(e) => Err((502, format!("control: {e}"))),
    }
}

// ---------------------------------------------------------------- moving in

/// Move conversation `id` (kept here, its home) into control: what M81's
/// conversations would do on upgrade (S38 q7).
pub async fn upload(app: &Arc<App>, id: ConvId) -> Result<Value, (u16, String)> {
    if stored(id) {
        return Err((409, "it's in control already".into()));
    }
    let c = conv(app, id).await.ok_or((404, "no such conversation".to_owned()))?;
    let e = app.control.enrolled().ok_or((409, "this machine isn't in control".to_owned()))?;
    let key = arugula_e2e::conv::new_key();
    let cid = id.to_string();
    // Wrapped to each device of the account this machine trusts: its
    // machines, browsers and CLIs (a recovery code has no X25519 key).
    let mut keys = serde_json::Map::new();
    for (dev, cert) in &e.trusted.devices {
        if cert.kind == arugula_e2e::Kind::Recovery {
            continue;
        }
        let Ok(pub_) = arugula_e2e::keys::hex32(&cert.noise) else { continue };
        if let Ok(w) = arugula_e2e::conv::wrap(&key, &cid, dev, &pub_) {
            keys.insert(dev.clone(), json!(b64(&w)));
        }
    }
    let record = record_of(app, &c);
    let sealed = arugula_e2e::conv::seal(&key, &cid, "record", record.as_bytes());
    call(app, &format!("/api/daemon/s38/convs/{cid}"), Some(json!({ "ver": 1, "record": b64(&sealed), "keys": keys })))
        .await?;
    let msgs = messages(app, id).await;
    for m in &msgs {
        let ct = arugula_e2e::conv::seal(&key, &cid, "msg", &serde_json::to_vec(m).unwrap_or_default());
        let v = call(app, &format!("/api/daemon/s38/convs/{cid}/log"), Some(json!({ "ct": b64(&ct) }))).await?;
        if v["seq"].as_u64() != Some(m.id) {
            return Err((500, format!("message {} came back numbered {}", m.id, v["seq"])));
        }
    }
    let side = Side { key: hex::encode(key), ver: 1, synced: record };
    save_side(app, id, &side);
    STATE.lock().unwrap().insert(id, Local { side, key, held: None });
    // The home is the lead's first machine; its cursor is the last
    // message, since the lead here has seen what came before.
    let cursor = msgs.last().map_or(0, |m| m.id);
    take(app, id, None, Some(cursor)).await;
    info!(conv = %id, messages = msgs.len(), devices = keys.len(), "s38: conversation moved into control");
    Ok(json!({ "conv": cid, "messages": msgs.len(), "wrapped_to": keys.keys().collect::<Vec<_>>(), "leading": leading(id) }))
}

// ---------------------------------------------------------------- posting

/// `post`, as `Api::ThreadPost` would answer it: for a conversation kept
/// in control, appended there and taken back; anything else, as before.
pub async fn post(app: &App, post: ThreadPost) -> Option<Result<Posted, ThreadError>> {
    post_fenced(app, post, None).await
}

async fn post_fenced(app: &App, post: ThreadPost, force: Option<u64>) -> Option<Result<Posted, ThreadError>> {
    let ThreadTarget::Conv(id) = post.target else { return app.mux.api(|r| Api::ThreadPost(post, r)).await };
    let Some(key) = key_of(id) else { return app.mux.api(|r| Api::ThreadPost(post, r)).await };
    // The lead's replies and its links' events (posted as the lead or the
    // link) are the lead's machine's. A card's answer is a person's.
    let fenced = post.as_agent.is_some();
    let fence = match (force, fenced) {
        (Some(t), _) => Some(t),
        (None, true) => match leading(id) {
            Some(t) => Some(t),
            None => {
                warn!(conv = %id, "s38: not posting as the lead: this machine doesn't hold the lead's lease");
                return Some(Err(ThreadError(409, "this machine doesn't hold the lead's lease".into())));
            }
        },
        (None, false) => None,
    };
    let (msg, to_agent, unreached) = match app.mux.api(|r| Api::ThreadCompose(post, r)).await? {
        Ok(p) => p,
        Err(e) => return Some(Err(e)),
    };
    let cid = id.to_string();
    let ct = arugula_e2e::conv::seal(&key, &cid, "msg", &serde_json::to_vec(&msg).unwrap_or_default());
    let v = match call(app, &format!("/api/daemon/s38/convs/{cid}/log"), Some(json!({ "ct": b64(&ct), "fence": fence })))
        .await
    {
        Ok(v) => v,
        Err((s, why)) => {
            if fence.is_some() {
                warn!(conv = %id, fence, why, "s38: control refused the lead's post (fenced)");
            }
            return Some(Err(ThreadError(s, why)));
        }
    };
    let seq = v["seq"].as_u64().unwrap_or(0);
    sync_log(app, id).await;
    let kept = messages(app, id).await.into_iter().find(|m| m.id == seq).unwrap_or(ThreadMsg { id: seq, ..msg });
    Some(Ok((kept, to_agent, unreached)))
}

/// Take what's new in conversation `id`'s log.
async fn sync_log(app: &App, id: ConvId) {
    let _one = SYNC.lock().await;
    let Some(key) = key_of(id) else { return };
    let cid = id.to_string();
    loop {
        let last = messages(app, id).await.last().map_or(0, |m| m.id);
        let Ok(v) = call(app, &format!("/api/daemon/s38/convs/{cid}/log?after={last}"), None).await else { return };
        let entries = v["entries"].as_array().cloned().unwrap_or_default();
        if entries.is_empty() {
            return;
        }
        for e in entries {
            let seq = e["seq"].as_u64().unwrap_or(0);
            let Some(ct) = e["ct"].as_str().and_then(unb64) else { return };
            let msg = arugula_e2e::conv::open(&key, &cid, "msg", &ct)
                .ok()
                .and_then(|p| serde_json::from_slice::<ThreadMsg>(&p).ok());
            let Some(mut msg) = msg else {
                warn!(conv = %id, seq, "s38: an entry that doesn't open");
                return;
            };
            msg.id = seq;
            let notify = leading(id).is_some();
            match app.mux.api(|r| Api::ThreadIngest(ThreadTarget::Conv(id), msg, notify, r)).await {
                Some(Ok(_)) => {}
                Some(Err(e)) => {
                    warn!(conv = %id, seq, error = e.1, "s38: can't keep an entry");
                    return;
                }
                None => return,
            }
        }
    }
}

// ---------------------------------------------------------------- the lease

/// Take (or, with `token`, renew) the lease. `cursor`: the last `@lead`
/// handed to the lead.
async fn take(app: &Arc<App>, id: ConvId, token: Option<u64>, cursor: Option<u64>) -> bool {
    let sent = now();
    let cid = id.to_string();
    let had = STATE.lock().unwrap().get(&id).and_then(|l| l.held);
    match call(app, &format!("/api/daemon/s38/convs/{cid}/lease"), Some(json!({ "token": token, "cursor": cursor })))
        .await
    {
        Ok(v) => {
            let l = &v["lease"];
            let ttl = v["ttl"].as_u64().unwrap_or(15_000);
            let h = Held {
                token: l["token"].as_u64().unwrap_or(0),
                until: sent + ttl.saturating_sub(MARGIN_MS),
                sent,
                cursor: l["cursor"].as_u64().unwrap_or(0),
            };
            if let Some(local) = STATE.lock().unwrap().get_mut(&id) {
                local.held = Some(h);
            }
            if had.is_none_or(|o| o.token != h.token) {
                info!(conv = %id, token = h.token, cursor = h.cursor, "s38: this machine took the lead's lease");
                gained(app, id).await;
            }
            true
        }
        Err((s, why)) => {
            if had.is_some() {
                warn!(conv = %id, status = s, why, "s38: lost the lead's lease");
                lost(app, id).await;
            }
            false
        }
    }
}

/// This machine runs the lead now: its links' watchers start here. Its
/// lead block starts at the first `@lead` it's handed (`lead::tell`).
async fn gained(app: &Arc<App>, id: ConvId) {
    let Some(c) = conv(app, id).await else { return };
    for l in c.links.iter().filter(|l| l.closed.is_none()) {
        super::events::watch(app, id, l);
    }
}

/// Another machine runs the lead now: stop this one's, and its links'
/// watchers, so nothing here acts for the conversation.
async fn lost(app: &App, id: ConvId) {
    if let Some(local) = STATE.lock().unwrap().get_mut(&id) {
        local.held = None;
    }
    super::events::stop(id, None);
    super::lead::stop(id);
    if let Some(l) = conv(app, id).await.and_then(|c| c.lead) {
        let _ = app.mux.api(|r| Api::Close(l.block, r)).await;
        info!(conv = %id, block = l.block, "s38: closed this machine's lead: another machine holds the lease");
    }
}

/// Hand the lead each `@lead` after the lease's cursor, in order, and move
/// the cursor past it.
async fn hand_on(app: &Arc<App>, id: ConvId) {
    let Some(h) = STATE.lock().unwrap().get(&id).and_then(|l| l.held) else { return };
    if leading(id).is_none() {
        return;
    }
    let msgs = messages(app, id).await;
    for m in msgs.iter().filter(|m| m.id > h.cursor && m.to_agent && !m.agent) {
        info!(conv = %id, msg = m.id, token = h.token, "s38: handing an @lead to the lead here");
        if let Err(e) = super::lead::tell(app, id, m).await {
            warn!(conv = %id, msg = m.id, error = e, "s38: the lead didn't take it");
            return;
        }
        if let Some(local) = STATE.lock().unwrap().get_mut(&id)
            && let Some(held) = local.held.as_mut()
        {
            held.cursor = m.id;
        }
        // The cursor is control's, so the next holder starts after it.
        take(app, id, Some(h.token), Some(m.id)).await;
    }
}

// ---------------------------------------------------------------- the mirror

/// What's kept here already (the keys), before anything resumes, and then
/// the mirror, every second.
pub fn start(app: &Arc<App>) {
    if !super::on(app.control.state_dir(), arugula_proto::flags::CHAT) {
        return;
    }
    for entry in std::fs::read_dir(dir(app)).into_iter().flatten().flatten() {
        let Some(id) = entry
            .path()
            .file_stem()
            .and_then(|s| s.to_str())
            .and_then(|s| serde_json::from_value::<ConvId>(json!(s)).ok())
        else {
            continue;
        };
        let Some(side) = std::fs::read(entry.path()).ok().and_then(|b| serde_json::from_slice::<Side>(&b).ok()) else {
            continue;
        };
        let Ok(key) = arugula_e2e::keys::hex32(&side.key) else { continue };
        STATE.lock().unwrap().insert(id, Local { side, key, held: None });
    }
    let app = app.clone();
    tokio::spawn(async move {
        loop {
            if let Err(e) = tick(&app).await {
                tracing::debug!(error = e, "s38: mirror");
            }
            tokio::time::sleep(Duration::from_secs(1)).await;
        }
    });
}

async fn tick(app: &Arc<App>) -> Result<(), String> {
    let (me_id, _) = me(app).ok_or("not in control")?;
    let v = call(app, "/api/daemon/s38/convs", None).await.map_err(|e| e.1)?;
    let control_now = v["now"].as_u64().unwrap_or(0);
    for h in v["convs"].as_array().cloned().unwrap_or_default() {
        let Ok(id) = serde_json::from_value::<ConvId>(h["id"].clone()) else { continue };
        let cid = id.to_string();
        let ver = h["ver"].as_u64().unwrap_or(0);
        // New here: unwrap its key with this machine's own.
        if !stored(id) {
            let e = app.control.enrolled().ok_or("not in control")?;
            let Some(w) = h["keys"][&me_id].as_str().and_then(unb64) else {
                warn!(conv = %id, "s38: a conversation with no key for this machine");
                continue;
            };
            let key = arugula_e2e::conv::unwrap(&w, &cid, &me_id, &e.keys.noise_private, &e.keys.noise_public)
                .map_err(|e| e.to_string())?;
            let side = Side { key: hex::encode(key), ver: 0, synced: String::new() };
            STATE.lock().unwrap().insert(id, Local { side, key, held: None });
            info!(conv = %id, "s38: a conversation in control, mirrored here");
        }
        let (key, side) = {
            let s = STATE.lock().unwrap();
            let l = s.get(&id).ok_or("gone")?;
            (l.key, l.side.clone())
        };
        // The record: theirs if newer, else ours if we changed it.
        if ver > side.ver {
            let ct = h["record"].as_str().and_then(unb64).ok_or("record")?;
            let plain = arugula_e2e::conv::open(&key, &cid, "record", &ct).map_err(|e| e.to_string())?;
            let c: Conversation = serde_json::from_slice(&plain).map_err(|e| e.to_string())?;
            let synced = String::from_utf8_lossy(&plain).into_owned();
            app.mux.api(|r| Api::ConvPut(localize(app, c), r)).await.ok_or("shutting down")?.map_err(|e| e.1)?;
            let side = Side { ver, synced, ..side };
            save_side(app, id, &side);
            if let Some(l) = STATE.lock().unwrap().get_mut(&id) {
                l.side = side;
            }
        } else if let Some(c) = conv(app, id).await {
            let now_rec = record_of(app, &c);
            if now_rec != side.synced {
                let sealed = arugula_e2e::conv::seal(&key, &cid, "record", now_rec.as_bytes());
                let body = json!({ "ver": side.ver + 1, "record": b64(&sealed), "keys": h["keys"] });
                if call(app, &format!("/api/daemon/s38/convs/{cid}"), Some(body)).await.is_ok() {
                    let side = Side { ver: side.ver + 1, synced: now_rec, ..side };
                    save_side(app, id, &side);
                    if let Some(l) = STATE.lock().unwrap().get_mut(&id) {
                        l.side = side;
                    }
                }
            }
        }
        sync_log(app, id).await;
        // The lease: renew ours, or take a free one.
        let held = STATE.lock().unwrap().get(&id).and_then(|l| l.held);
        match held {
            Some(h) if now().saturating_sub(h.sent) >= RENEW_MS || now() >= h.until => {
                take(app, id, Some(h.token), Some(h.cursor)).await;
            }
            Some(_) => {}
            None => {
                let free = h["lease"].is_null() || h["lease"]["expires"].as_u64().unwrap_or(0) <= control_now;
                if free {
                    take(app, id, None, None).await;
                }
            }
        }
        hand_on(app, id).await;
    }
    Ok(())
}

// ---------------------------------------------------------------- routes

/// The prototype's own routes (the owner's: `authz`'s default).
pub fn routes(r: Router<Arc<App>>) -> Router<Arc<App>> {
    r.route("/api/s38/convs/{id}", get(state_get))
        .route("/api/s38/convs/{id}/upload", http_post(upload_post))
        .route("/api/s38/convs/{id}/probe", http_post(probe_post))
}

type R = Result<Json<Value>, (StatusCode, String)>;

fn status((s, why): (u16, String)) -> (StatusCode, String) {
    (StatusCode::from_u16(s).unwrap_or(StatusCode::BAD_REQUEST), why)
}

fn parse(id: &str) -> Result<ConvId, (StatusCode, String)> {
    serde_json::from_value(json!(id)).map_err(|_| (StatusCode::BAD_REQUEST, "not a conversation id".into()))
}

async fn upload_post(State(app): State<Arc<App>>, UrlPath(id): UrlPath<String>) -> R {
    upload(&app, parse(&id)?).await.map(Json).map_err(status)
}

async fn state_get(State(app): State<Arc<App>>, UrlPath(id): UrlPath<String>) -> R {
    let id = parse(&id)?;
    let (ver, held) = {
        let s = STATE.lock().unwrap();
        let l = s.get(&id).ok_or((StatusCode::NOT_FOUND, "not kept in control".into()))?;
        (l.side.ver, l.held)
    };
    let lead = conv(&app, id).await.and_then(|c| c.lead).map(|l| l.block);
    Ok(Json(json!({
        "ver": ver,
        "leading": leading(id),
        "held": held.map(|h| json!({ "token": h.token, "until": h.until, "cursor": h.cursor })),
        "lead_block": lead,
        "messages": messages(&app, id).await.len(),
    })))
}

#[derive(Deserialize)]
struct Probe {
    token: u64,
    text: String,
}

/// Post as the lead with a given token: what a machine that slept through
/// losing the lease would send. Control should refuse a stale token.
async fn probe_post(State(app): State<Arc<App>>, UrlPath(id): UrlPath<String>, Json(p): Json<Probe>) -> R {
    let id = parse(&id)?;
    let post = ThreadPost {
        target: ThreadTarget::Conv(id),
        who: Principal::Owner,
        acting: None,
        as_agent: Some(arugula_proto::Driver { who: super::lead::WHO.into(), name: super::lead::WHO.into() }),
        text: p.text,
        quote: None,
        event: None,
    };
    match post_fenced(&app, post, Some(p.token)).await {
        Some(Ok((m, _, _))) => Ok(Json(json!({ "posted": m.id }))),
        Some(Err(e)) => Err(status((e.0, e.1))),
        None => Err((StatusCode::SERVICE_UNAVAILABLE, "shutting down".into())),
    }
}
