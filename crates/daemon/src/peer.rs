//! This daemon as a client of another machine (#399): the account's own
//! machines, its teams', and its teammates' that offer agents, reached
//! through control's relay and signed with this daemon's own key, as the
//! CLI reaches them with its own (`crates/cli/src/control.rs`).
//!
//! - **Which machines** come from control's directory. A machine of another
//!   account is trusted only when its certificate chains to the root a
//!   checked roster names for that account (a teammate's), never to one
//!   control alone vouches for.
//! - **What it may do there** is the other machine's to decide: a daemon is
//!   let in for `/api/a2a/` only, as its account (`e2e.rs`).
//! - **One request a channel** for now: a catalog asks each machine once
//!   a refresh. Tasks (M78) will want a channel that stays.

use std::time::Duration;

use arugula_e2e::{
    Cert, Kind, Revocation, Trust,
    channel::{Initiator, Msg, RequestHead, prologue},
};
use futures_util::{SinkExt, StreamExt};
use serde::Deserialize;
use tokio_tungstenite::tungstenite::Message;

use crate::control::Control;

/// How long one request to another machine may take, the handshake
/// included.
pub const TIMEOUT: Duration = Duration::from_secs(20);

/// A machine this daemon may reach.
#[derive(Debug, Clone)]
pub struct Machine {
    pub id: String,
    pub name: String,
    /// Its account (this one's, for its own machines).
    pub account: String,
    /// Who owns it, to show (empty for this account's own).
    pub owner: String,
    pub team: Option<String>,
    pub online: bool,
    cert: Cert,
}

#[derive(Deserialize)]
struct Chain {
    trust: Option<Trust>,
    #[serde(default)]
    certs: Vec<Cert>,
    #[serde(default)]
    revocations: Vec<Revocation>,
}

/// The machines control lists that this daemon trusts, itself left out.
pub async fn machines(control: &Control) -> anyhow::Result<Vec<Machine>> {
    let (e, v) = control.directory().await?;
    let own = e.saved.cert.account.clone();
    let mut out = Vec::new();
    for d in v["daemons"].as_array().into_iter().flatten() {
        let Some(id) = d["id"].as_str() else { continue };
        if id == e.saved.cert.device {
            continue;
        }
        let account = d["account"].as_str().unwrap_or(&own).to_owned();
        let cert = if account == own {
            e.trusted.get(id).cloned()
        } else {
            let Some(root) = e.known_root(&account) else { continue };
            let Ok(chain) = serde_json::from_value::<Chain>(d["chain"].clone()) else { continue };
            // Control's word for the root counts only where a roster agrees.
            if chain.trust.as_ref().is_none_or(|t| t.account != account || t.root != root) {
                continue;
            }
            Trust { account: account.clone(), root }.evaluate(&chain.certs, &chain.revocations).get(id).cloned()
        };
        let Some(cert) = cert.filter(|c| c.kind == Kind::Daemon) else { continue };
        out.push(Machine {
            id: id.to_owned(),
            name: d["name"].as_str().unwrap_or(id).to_owned(),
            owner: d["owner_name"].as_str().unwrap_or_default().to_owned(),
            team: d["team"].as_str().map(str::to_owned),
            online: d["online"].as_bool().unwrap_or(false),
            account,
            cert,
        });
    }
    Ok(out)
}

/// The machine named `which` (its name, or its id), as [`machines`] lists
/// them.
pub async fn find(control: &Control, which: &str) -> anyhow::Result<Machine> {
    let all = machines(control).await?;
    let found: Vec<Machine> = all.iter().filter(|m| m.id == which || m.name == which).cloned().collect();
    match found.as_slice() {
        [one] => Ok(one.clone()),
        [] => anyhow::bail!("no machine {which} that this one reaches (list kind agents names them)"),
        _ => anyhow::bail!("more than one machine is called {which}: name it by id"),
    }
}

/// `GET path` on `m`, through control's relay: its status and body.
pub async fn get(control: &Control, m: &Machine, path: &str) -> anyhow::Result<(u16, Vec<u8>)> {
    request(control, m, "GET", path, None, &[]).await
}

/// A request to `m` through control's relay: a JSON body, and headers from
/// those the channel carries (`A2A-Version`, say). Its status and body.
pub async fn request(
    control: &Control,
    m: &Machine,
    method: &str,
    path: &str,
    body: Option<&serde_json::Value>,
    headers: &[(&str, &str)],
) -> anyhow::Result<(u16, Vec<u8>)> {
    let head = RequestHead {
        method: method.into(),
        path: path.into(),
        content_type: body.map(|_| "application/json".to_owned()),
        stream: false,
        headers: headers.iter().map(|(k, v)| ((*k).to_owned(), (*v).to_owned())).collect(),
    };
    let body = body.map(|b| serde_json::to_vec(b).unwrap_or_default()).unwrap_or_default();
    tokio::time::timeout(TIMEOUT, send(control, m, head, body))
        .await
        .map_err(|_| anyhow::anyhow!("{} didn't answer in time", m.name))?
}

async fn send(control: &Control, m: &Machine, head: RequestHead, body: Vec<u8>) -> anyhow::Result<(u16, Vec<u8>)> {
    let e = control.enrolled().ok_or_else(|| anyhow::anyhow!("this machine isn't in control"))?;
    let relay = format!("/api/relay/c/{}", m.id);
    let base = e.saved.url.trim_end_matches('/');
    let ws_base = base.replacen("https://", "wss://", 1).replacen("http://", "ws://", 1);
    let url = reqwest::Url::parse(&format!("{ws_base}{relay}"))?;
    let (name, auth) = control.auth_for(&e, "GET", &relay);
    let mut ws = crate::dial::open_ws(&url, &[(name, &auth), ("origin", base)]).await?;
    // Noise IK, this daemon the initiator.
    let (init, m1) = Initiator::start(&e.keys, &m.cert.noise_key(), &prologue(&m.cert.device))?;
    ws.send(Message::Binary(m1.into())).await?;
    let m2 = loop {
        match ws.next().await {
            Some(Ok(Message::Binary(b))) => break b,
            Some(Ok(Message::Close(_))) | None => anyhow::bail!("{} closed during the handshake (not let in?)", m.name),
            Some(Ok(_)) => {}
            Some(Err(err)) => return Err(err.into()),
        }
    };
    let (_, ch) = init.finish(&m2)?;
    for w in ch.seal(&Msg::Request { id: 1, head, body })? {
        ws.send(Message::Binary(w.into())).await?;
    }
    let mut answer = Vec::new();
    loop {
        let w = match ws.next().await {
            Some(Ok(Message::Binary(b))) => b,
            Some(Ok(Message::Close(_))) | None => anyhow::bail!("{} closed before answering", m.name),
            Some(Ok(_)) => continue,
            Some(Err(err)) => return Err(err.into()),
        };
        // Anything but our answer isn't for a machine's channel.
        if let Some(Msg::Response { id: 1, head, body: part }) = ch.open(&w)? {
            answer.extend(part);
            if !head.more {
                let _ = ws.close(None).await;
                return Ok((head.status, answer));
            }
        }
    }
}
