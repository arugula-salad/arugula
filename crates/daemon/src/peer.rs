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
//! - **One standing channel per machine** (#856): every request to it goes
//!   down the same relay socket, numbered (`Msg::Request { id }`), and the
//!   answers come back by number. It closes when it's been idle a while,
//!   and a request on one that died is tried once more on a new one.

use std::{
    collections::HashMap,
    sync::{
        Arc, LazyLock, Mutex,
        atomic::{AtomicBool, AtomicU32, Ordering},
    },
    time::{Duration, Instant},
};

use arugula_e2e::{
    Cert, Kind, Revocation, Trust,
    channel::{Channel, Initiator, Msg, Refusal, RequestHead, ResponseHead, prologue},
};
use futures_util::{SinkExt, StreamExt, stream::SplitSink};
use serde::Deserialize;
use tokio::sync::mpsc;
use tokio_tungstenite::tungstenite::Message;

use crate::control::{Control, Enrolled};

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
        if !d["hosted"].is_null() {
            match accept_hosted(&e, id, &d["hosted"]) {
                // M184: its project's machine, accepted for that project only.
                Ok((project, cert)) => out.push(Machine {
                    id: id.to_owned(),
                    name: d["name"].as_str().unwrap_or(id).to_owned(),
                    account: project,
                    owner: String::new(),
                    team: d["team"].as_str().map(str::to_owned),
                    online: d["online"].as_bool().unwrap_or(false),
                    cert,
                }),
                Err(why) => tracing::debug!(machine = id, %why, "not trusting a hosted machine"),
            }
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

/// A hosted machine (M178), if this daemon accepts it: its project and the
/// certificate its channel is opened with. Checked as
/// the page and the CLI check it (`arugula_e2e::hosted::accept`), against
/// what this daemon holds itself, never control's word: this account's
/// own chain for a personal project; for a team's, a roster checked from
/// its pin (this machine's own team's, or one the owner's browser pinned
/// here, #233).
fn accept_hosted(e: &Enrolled, id: &str, v: &serde_json::Value) -> anyhow::Result<(String, Cert)> {
    use arugula_e2e::hosted::{Delegation, DelegationRevoke, HostedCert, Owner, Owners, accept};
    let cert: HostedCert = serde_json::from_value(v["cert"].clone())?;
    let d: Delegation = serde_json::from_value(v["delegation"].clone())?;
    let revokes: Vec<DelegationRevoke> = serde_json::from_value(v["revocations"].clone()).unwrap_or_default();
    anyhow::ensure!(cert.device == id, "its certificate is another machine's");
    let me = &e.saved.cert.account;
    let accepted = match d.owner().ok_or_else(|| anyhow::anyhow!("its delegation names no owner"))? {
        Owner::Team(team) => {
            let (roster, certs) = match (&e.saved.roster, e.saved.shared_teams.get(&team)) {
                (Some(r), _) if r.team == team => (r, &e.saved.team_certs),
                (_, Some(t)) => (&t.roster, &t.certs),
                _ => anyhow::bail!(
                    "its team isn't one this machine knows: open Control in a browser of this account once, which pins your teams here"
                ),
            };
            accept(me, &cert, &d, &revokes, &Owners::Team { roster, certs })
        }
        Owner::Account(_) => accept(
            me,
            &cert,
            &d,
            &revokes,
            &Owners::Account { trust: &e.saved.trust, certs: &e.saved.certs, revocations: &e.saved.revocations },
        ),
    }
    .map_err(|r| anyhow::anyhow!("refused: {}", r.code()))?;
    // Its keys, to open a channel with; never one of an account's.
    let c = Cert {
        v: 1,
        account: accepted.project.clone(),
        device: cert.device.clone(),
        kind: Kind::Daemon,
        name: cert.name.clone(),
        noise: cert.noise.clone(),
        sign: cert.sign.clone(),
        created: cert.created,
        approver: String::new(),
        sig: String::new(),
    };
    Ok((accepted.project, c))
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

/// A JSON `POST` to `m` that may take up to `limit` (M184: an MCP call
/// forwarded to a hosted machine, which may wait on a turn): its status
/// and body.
pub async fn post_waiting(
    control: &Control,
    m: &Machine,
    path: &str,
    body: &serde_json::Value,
    limit: Duration,
) -> anyhow::Result<(u16, Vec<u8>)> {
    let head = RequestHead {
        method: "POST".into(),
        path: path.into(),
        content_type: Some("application/json".to_owned()),
        stream: false,
        headers: Default::default(),
    };
    let body = serde_json::to_vec(body)?;
    tokio::time::timeout(limit + TIMEOUT, send(control, m, head, body))
        .await
        .map_err(|_| anyhow::anyhow!("{} didn't answer in time", m.name))?
}

/// How long a standing channel stays open with nothing asked on it.
const IDLE: Duration = Duration::from_secs(300);

/// What a request on a channel came to.
enum Asked {
    Answer(u16, Vec<u8>),
    /// The channel went before anything came back: worth one more try, on a
    /// new one, if this one wasn't new.
    Gone,
}

/// Where an answer's parts go: to the request waiting for it.
type Parts = mpsc::UnboundedSender<(ResponseHead, Vec<u8>)>;

/// A standing channel to one machine (#856).
struct Conn {
    ch: Arc<Channel>,
    sink: tokio::sync::Mutex<SplitSink<crate::dial::Ws, Message>>,
    /// The requests waiting for their answer's parts, by id.
    waiting: Mutex<HashMap<u32, Parts>>,
    next: AtomicU32,
    alive: AtomicBool,
    used: Mutex<Instant>,
}

impl Conn {
    async fn ask(&self, head: RequestHead, body: Vec<u8>) -> anyhow::Result<Asked> {
        let mut answer = Vec::new();
        let status = self
            .ask_parts(head, body, |_, part| {
                answer.extend(part);
                true
            })
            .await?;
        Ok(match status {
            Some(status) => Asked::Answer(status, answer),
            None => Asked::Gone,
        })
    }

    /// Ask, handing each part of the answer to `part` as it comes (its
    /// status, and the bytes) until the last, or until `part` says stop.
    /// `None`: the channel went before anything came back.
    async fn ask_parts(
        &self,
        head: RequestHead,
        body: Vec<u8>,
        mut part: impl FnMut(u16, Vec<u8>) -> bool,
    ) -> anyhow::Result<Option<u16>> {
        let id = self.next.fetch_add(1, Ordering::Relaxed);
        let (tx, mut rx) = mpsc::unbounded_channel();
        self.waiting.lock().unwrap().insert(id, tx);
        *self.used.lock().unwrap() = Instant::now();
        let sent = async {
            // Sealed and sent under one lock: the nonces reach the socket in
            // order.
            let mut sink = self.sink.lock().await;
            for w in self.ch.seal(&Msg::Request { id, head, body })? {
                sink.send(Message::Binary(w.into())).await?;
            }
            anyhow::Ok(())
        }
        .await;
        if sent.is_err() {
            self.waiting.lock().unwrap().remove(&id);
            self.alive.store(false, Ordering::SeqCst);
            return Ok(None);
        }
        let mut status = None;
        let r = loop {
            match rx.recv().await {
                Some((head, bytes)) => {
                    status = Some(head.status);
                    *self.used.lock().unwrap() = Instant::now();
                    if !part(head.status, bytes) || !head.more {
                        break Ok(status);
                    }
                }
                None if status.is_none() => break Ok(None),
                None => break Err(anyhow::anyhow!("the channel closed partway through the answer")),
            }
        };
        // Whatever still comes for it (a follow cut short) is dropped.
        self.waiting.lock().unwrap().remove(&id);
        *self.used.lock().unwrap() = Instant::now();
        r
    }
}

type Slot = Arc<tokio::sync::Mutex<Option<Arc<Conn>>>>;

/// This daemon's device, the other's, and the other's key.
type Pair = (String, String, String);

/// The standing channels, by pair.
static CHANNELS: LazyLock<Mutex<HashMap<Pair, Slot>>> = LazyLock::new(Default::default);

/// This daemon's standing channel to `m`, opened if there's none; and
/// whether it's new.
async fn channel(control: &Control, m: &Machine) -> anyhow::Result<(Arc<Conn>, bool)> {
    let me = control.enrolled().ok_or_else(|| anyhow::anyhow!("this machine isn't in control"))?;
    let key = (me.saved.cert.device.clone(), m.id.clone(), m.cert.noise.clone());
    let slot = CHANNELS.lock().unwrap().entry(key).or_default().clone();
    let mut slot = slot.lock().await;
    if let Some(c) = slot.as_ref().filter(|c| c.alive.load(Ordering::SeqCst)) {
        return Ok((c.clone(), false));
    }
    let c = open(control, m).await?;
    *slot = Some(c.clone());
    Ok((c, true))
}

async fn open(control: &Control, m: &Machine) -> anyhow::Result<Arc<Conn>> {
    let e = control.enrolled().ok_or_else(|| anyhow::anyhow!("this machine isn't in control"))?;
    let relay = format!("/api/relay/c/{}", m.id);
    let base = e.saved.url.trim_end_matches('/');
    let ws_base = base.replacen("https://", "wss://", 1).replacen("http://", "ws://", 1);
    let url = reqwest::Url::parse(&format!("{ws_base}{relay}"))?;
    let (name, auth) = control.auth_for(&e, "GET", &relay);
    // A hosted machine that's asleep is woken (M182): a channel is opened
    // for something an agent or a person asked of the machine.
    let headers = [(name, auth.as_str()), ("origin", base), ("x-arugula-wake", "1")];
    let mut ws = tokio::time::timeout(TIMEOUT, crate::dial::open_ws(&url, &headers))
        .await
        .map_err(|_| anyhow::anyhow!("{} didn't answer in time", m.name))??;
    // Noise IK, this daemon the initiator.
    let (init, m1) = Initiator::start(&e.keys, &m.cert.noise_key(), &prologue(&m.cert.device))?;
    ws.send(Message::Binary(m1.into())).await?;
    let m2 = tokio::time::timeout(TIMEOUT, async {
        loop {
            match ws.next().await {
                Some(Ok(Message::Binary(b))) => break Ok(b),
                Some(Ok(Message::Close(_))) | None => {
                    anyhow::bail!("{} closed during the handshake (not let in?)", m.name)
                }
                Some(Ok(_)) => {}
                Some(Err(err)) => break Err(err.into()),
            }
        }
    })
    .await
    .map_err(|_| anyhow::anyhow!("{} didn't answer the handshake in time", m.name))??;
    let (said, ch) = init.finish(&m2)?;
    if let Some(r) = Refusal::decode(&said) {
        anyhow::bail!("{} refused: {} (not let in?)", m.name, r.message);
    }
    let (sink, mut stream) = ws.split();
    let conn = Arc::new(Conn {
        ch: Arc::new(ch),
        sink: tokio::sync::Mutex::new(sink),
        waiting: Mutex::new(HashMap::new()),
        next: AtomicU32::new(1),
        alive: AtomicBool::new(true),
        used: Mutex::new(Instant::now()),
    });
    // The reader: each answer's parts to whoever asked, until the socket
    // goes or the channel has been idle a while.
    let c = conn.clone();
    let machine = m.name.clone();
    crate::task::spawn(async move {
        let mut tick = tokio::time::interval(Duration::from_secs(30));
        loop {
            tokio::select! {
                w = stream.next() => {
                    let w = match w {
                        Some(Ok(Message::Binary(b))) => b,
                        Some(Ok(Message::Close(_))) | None | Some(Err(_)) => break,
                        Some(Ok(_)) => continue,
                    };
                    match c.ch.open(&w) {
                        Ok(Some(Msg::Response { id, head, body })) => {
                            let to = c.waiting.lock().unwrap().get(&id).cloned();
                            if let Some(to) = to {
                                let _ = to.send((head, body));
                            }
                        }
                        // Anything else isn't for a machine's channel.
                        Ok(_) => {}
                        Err(e) => {
                            tracing::debug!(machine, error = %e, "a machine's channel sent something unreadable");
                            break;
                        }
                    }
                }
                _ = tick.tick() => {
                    let idle = c.used.lock().unwrap().elapsed() > IDLE;
                    if idle && c.waiting.lock().unwrap().is_empty() {
                        break;
                    }
                }
            }
        }
        c.alive.store(false, Ordering::SeqCst);
        // Whoever still waits hears it went.
        c.waiting.lock().unwrap().clear();
        let _ = c.sink.lock().await.close().await;
    });
    Ok(conn)
}

async fn send(control: &Control, m: &Machine, head: RequestHead, body: Vec<u8>) -> anyhow::Result<(u16, Vec<u8>)> {
    // Once on the standing channel; once more on a new one if that had
    // died unnoticed.
    for _ in 0..2 {
        let (conn, new) = channel(control, m).await?;
        match conn.ask(head.clone(), body.clone()).await? {
            Asked::Answer(status, body) => return Ok((status, body)),
            Asked::Gone if new => break,
            Asked::Gone => conn.alive.store(false, Ordering::SeqCst),
        }
    }
    anyhow::bail!("{} closed before answering", m.name)
}
