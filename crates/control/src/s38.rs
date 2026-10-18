//! S38 prototype (#979, throwaway): a conversation's record kept here as
//! ciphertext, and the lease on the machine that runs its lead.
//!
//! Daemons only, and only the conversation's own account's (the owner's
//! machines, #978): control checks whose it is and who holds the lease,
//! and orders the log. It never has the key (`arugula_e2e::conv`).
//!
//! - `GET  /api/daemon/s38/convs`: the account's conversations, each with
//!   its sealed record, wrapped keys, last entry and lease.
//! - `POST /api/daemon/s38/convs/{id}`: the record, version `ver`.
//! - `POST /api/daemon/s38/convs/{id}/log`: append a sealed entry; with
//!   `fence`, only while the writer holds the lease with that token.
//! - `GET  /api/daemon/s38/convs/{id}/log?after=N`: the entries after N.
//! - `POST /api/daemon/s38/convs/{id}/lease`: take, renew or release it.

use std::sync::Arc;

use axum::{
    Json,
    extract::{Path, Query, State},
    http::StatusCode,
};
use base64::Engine;
use serde::Deserialize;
use serde_json::{Value, json};

use crate::{ApiError, App, auth::DaemonAuth, db::s38::Refused, err};

type R = Result<Json<Value>, ApiError>;

/// How long a lease lasts unrenewed. Holders renew every third of it.
pub const TTL_MS: u64 = 15_000;

fn now() -> u64 {
    arugula_e2e::now_ms()
}

fn unb64(s: &str) -> Result<Vec<u8>, ApiError> {
    base64::engine::general_purpose::STANDARD.decode(s).map_err(|_| err(StatusCode::BAD_REQUEST, "not base64"))
}

fn refused(r: Refused) -> ApiError {
    match r {
        Refused::NoSuch => err(StatusCode::NOT_FOUND, "no such conversation"),
        Refused::NotYours => err(StatusCode::FORBIDDEN, "not your account's conversation"),
        Refused::Stale(v) => err(StatusCode::CONFLICT, &format!("the record is at version {v}")),
        Refused::Held(l) => err(
            StatusCode::CONFLICT,
            &format!("the lead's lease is {}'s (token {}), until {}", l.holder, l.token, l.expires),
        ),
    }
}

pub async fn list(State(app): State<Arc<App>>, d: DaemonAuth) -> R {
    let heads = app.db.s38_heads(&d.cert.account)?;
    Ok(Json(json!({ "convs": heads, "now": now(), "ttl": TTL_MS })))
}

#[derive(Deserialize)]
pub struct Put {
    ver: u64,
    record: String,
    keys: Value,
}

pub async fn put(State(app): State<Arc<App>>, d: DaemonAuth, Path(id): Path<String>, Json(b): Json<Put>) -> R {
    let record = unb64(&b.record)?;
    app.db.s38_put(&d.cert.account, &id, b.ver, &record, &b.keys, now())?.map_err(refused)?;
    Ok(Json(json!({ "ver": b.ver })))
}

#[derive(Deserialize)]
pub struct Append {
    ct: String,
    #[serde(default)]
    fence: Option<u64>,
}

pub async fn append(State(app): State<Arc<App>>, d: DaemonAuth, Path(id): Path<String>, Json(b): Json<Append>) -> R {
    let ct = unb64(&b.ct)?;
    if ct.len() > 64 * 1024 {
        return Err(err(StatusCode::PAYLOAD_TOO_LARGE, "an entry is 64 KB at most"));
    }
    let seq = app.db.s38_append(&d.cert.account, &id, &d.cert.device, b.fence, &ct, now())?.map_err(refused)?;
    Ok(Json(json!({ "seq": seq })))
}

#[derive(Deserialize)]
pub struct After {
    #[serde(default)]
    after: u64,
}

pub async fn log(State(app): State<Arc<App>>, d: DaemonAuth, Path(id): Path<String>, Query(q): Query<After>) -> R {
    let entries = app.db.s38_log(&d.cert.account, &id, q.after, 500)?.map_err(refused)?;
    Ok(Json(json!({ "entries": entries })))
}

#[derive(Deserialize)]
pub struct LeaseReq {
    #[serde(default)]
    token: Option<u64>,
    #[serde(default)]
    cursor: Option<u64>,
    #[serde(default)]
    release: bool,
}

pub async fn lease(State(app): State<Arc<App>>, d: DaemonAuth, Path(id): Path<String>, Json(b): Json<LeaseReq>) -> R {
    let l = app
        .db
        .s38_lease(&d.cert.account, &id, &d.cert.device, b.token, b.cursor, b.release, TTL_MS, now())?
        .map_err(refused)?;
    Ok(Json(json!({ "lease": l, "now": now(), "ttl": TTL_MS })))
}
