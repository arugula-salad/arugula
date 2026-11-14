//! End-to-end channels from client devices (M17, M18): the responder side
//! of `arugula_e2e::channel`, over a WebSocket at `/e2e` (the direct
//! path) or a stream from control's relay.
//!
//! The client's Noise key must belong to a device this daemon trusts (its
//! certificate chains back to the account root pinned at join; see
//! `control.rs`). Then the channel is what `/ws` and the API are for a
//! tailnet client: the protocol's messages go to the mux as a client of
//! their own, and requests are answered by the daemon's own API router.
//! If the device stops being trusted (revoked), its channels close.

use std::sync::Arc;

use arugula_e2e::{
    Kind,
    channel::{Channel, MAX_MSG, MAX_WIRE, Msg, Refusal, RequestHead, Responder, ResponseHead, Welcome, prologue},
};
use arugula_proto::{Frame, FrameKind};
use axum::{
    Router,
    body::{Body, HttpBody},
    extract::{
        State,
        ws::{Message, WebSocket, WebSocketUpgrade},
    },
    http::{Request, header},
    response::Response,
};
use futures_util::{SinkExt, StreamExt};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt, DuplexStream},
    sync::mpsc,
};
use tower::ServiceExt;
use tracing::{debug, info, warn};

use crate::{
    mux::Cmd,
    pane::{ClientRx, Subscriber, ToClient, client_queue},
    server::App,
};

/// A relayed channel's pane output goes at most this often (#856): a busy
/// pane's thousands of small reads a second become one message per pane
/// each time, so control's relay moves about 60 messages a second for it,
/// not 4,000. Output after a quiet spell goes at once (a keystroke's echo).
const COALESCE: std::time::Duration = std::time::Duration::from_millis(16);
/// The most output one coalesced frame carries.
const COALESCE_MAX: usize = 32 * 1024;
/// The most queued items taken together.
const COALESCE_ITEMS: usize = 512;

pub const PATH: &str = "/e2e";

/// Wire messages queued for the socket, per channel.
const OUT_QUEUE: usize = 256;

/// `/e2e`: the direct path. The handshake is the authentication, so this
/// needs no tailnet identity or origin.
pub async fn ws(State(app): State<Arc<App>>, upgrade: WebSocketUpgrade) -> Response {
    upgrade.max_message_size(MAX_WIRE).on_upgrade(move |socket| serve_ws(app, socket))
}

async fn serve_ws(app: Arc<App>, socket: WebSocket) {
    let (mut tx, mut rx) = socket.split();
    let (in_tx, in_rx) = mpsc::channel::<Vec<u8>>(OUT_QUEUE);
    let (out_tx, mut out_rx) = mpsc::channel::<Vec<u8>>(OUT_QUEUE);
    let reader = async move {
        while let Some(Ok(m)) = rx.next().await {
            match m {
                Message::Binary(b) => {
                    if in_tx.send(b.to_vec()).await.is_err() {
                        break;
                    }
                }
                Message::Close(_) => break,
                _ => {}
            }
        }
    };
    let writer = async move {
        while let Some(w) = out_rx.recv().await {
            if tx.send(Message::Binary(w.into())).await.is_err() {
                break;
            }
        }
        let _ = tx.close().await;
    };
    let w = crate::task::spawn(writer);
    tokio::select! {
        r = serve(app, in_rx, out_tx, false) => log_end(r),
        _ = reader => {}
    }
    drain(w).await;
}

/// The channel ended: whatever was queued still goes out, for a moment,
/// and then the socket's writer stops whatever still holds its queue.
async fn drain(mut w: tokio::task::JoinHandle<()>) {
    if tokio::time::timeout(std::time::Duration::from_secs(2), &mut w).await.is_err() {
        w.abort();
    }
}

/// A stream from control's relay: `len (u32 BE) ‖ Noise message` frames.
pub async fn serve_stream(app: Arc<App>, stream: DuplexStream) {
    let (mut rd, mut wr) = tokio::io::split(stream);
    let (in_tx, in_rx) = mpsc::channel::<Vec<u8>>(OUT_QUEUE);
    let (out_tx, mut out_rx) = mpsc::channel::<Vec<u8>>(OUT_QUEUE);
    let reader = async move {
        let mut len = [0u8; 4];
        while rd.read_exact(&mut len).await.is_ok() {
            let n = u32::from_be_bytes(len) as usize;
            if n > MAX_WIRE {
                break;
            }
            let mut b = vec![0u8; n];
            if rd.read_exact(&mut b).await.is_err() || in_tx.send(b).await.is_err() {
                break;
            }
        }
    };
    let writer = async move {
        while let Some(w) = out_rx.recv().await {
            let mut f = Vec::with_capacity(4 + w.len());
            f.extend_from_slice(&(w.len() as u32).to_be_bytes());
            f.extend_from_slice(&w);
            if wr.write_all(&f).await.is_err() {
                break;
            }
        }
        let _ = wr.shutdown().await;
    };
    let w = crate::task::spawn(writer);
    // Through control's relay: its output is coalesced (#856).
    tokio::select! {
        r = serve(app, in_rx, out_tx, true) => log_end(r),
        _ = reader => {}
    }
    drain(w).await;
}

fn log_end(r: anyhow::Result<()>) {
    if let Err(e) = r {
        debug!(error = %e, "channel ended");
    }
}

/// Seals and queues messages, one whole message at a time: the nonces
/// (and a message's chunks) must reach the socket in order.
pub(crate) struct Out {
    pub(crate) ch: Arc<Channel>,
    q: tokio::sync::Mutex<mpsc::Sender<Vec<u8>>>,
}

impl Out {
    pub(crate) fn new(ch: Channel, q: mpsc::Sender<Vec<u8>>) -> Self {
        Self { ch: Arc::new(ch), q: tokio::sync::Mutex::new(q) }
    }

    pub(crate) async fn put(&self, m: &Msg) -> anyhow::Result<()> {
        let q = self.q.lock().await;
        for w in self.ch.seal(m)? {
            q.send(w).await.map_err(|_| anyhow::anyhow!("socket gone"))?;
        }
        Ok(())
    }
}

async fn serve(
    app: Arc<App>,
    mut inbound: mpsc::Receiver<Vec<u8>>,
    out: mpsc::Sender<Vec<u8>>,
    relayed: bool,
) -> anyhow::Result<()> {
    // Before anyone's looked up: a change from here on is one this channel
    // sees, however soon it comes.
    let changed = app.control.changed.subscribe();
    let m1 = tokio::time::timeout(std::time::Duration::from_secs(15), inbound.recv())
        .await
        .map_err(|_| anyhow::anyhow!("no handshake"))?
        .ok_or_else(|| anyhow::anyhow!("closed before the handshake"))?;
    let enrolled = app.control.enrolled().ok_or_else(|| anyhow::anyhow!("not enrolled in control"))?;
    let (responder, who, payload) = Responder::read(&enrolled.keys, &prologue(&enrolled.keys.id()), &m1)?;
    // A machine token (aru-141): control's word for who this is, not a
    // device's key. Before the others, whose payloads are JSON too.
    if let Some(hello) = arugula_control_wire::machine_token::TokenHello::decode(&payload) {
        return serve_token(app, &enrolled, responder, hello, inbound, out, relayed, changed).await;
    }
    // A public preview (M187): control, for a stranger, with a grant
    // signed with its hosted key for one public port.
    if let Ok(hello) = arugula_e2e::preview::PublicHello::decode(&payload) {
        return crate::public_preview::serve(app, &enrolled, responder, who, hello, inbound, out).await;
    }
    // A block's frame (M50): a one-off key and its grant, not a device.
    if !payload.is_empty() {
        return crate::block_channel::serve(app, &enrolled.keys.id(), responder, who, &payload, inbound, out).await;
    }
    let Some((device, principal)) = app.control.device(&who) else {
        warn!(key = hex::encode(&who[..8]), "refused a channel from a device this daemon doesn't trust");
        // Said, not just hung up (#913): a browser approved a moment ago
        // gets here before this daemon has fetched its certificate, and a
        // hang-up looks like a broken route, so it would try a worse one and
        // stay on it. Queued before the socket closes, as a block's is.
        let (m2, _) = responder.finish(&Refusal::unknown_device().encode())?;
        out.send(m2).await?;
        anyhow::bail!("unknown device");
    };
    // The daemon's clock, for the block grants this device mints (M50).
    let (m2, ch) = responder.finish(&Welcome { now: arugula_e2e::now_ms() }.encode())?;
    out.send(m2).await?;
    let out = Arc::new(Out::new(ch, out));
    let caller = Caller { account: device.account.clone(), device: device.device.clone(), name: device.name.clone() };
    // #399: another machine, for this one's agents: requests to `/api/a2a/`
    // and nothing else (no mux, no panes, no hands). M184: on a hosted
    // machine, a project person's own machine drives it: any request.
    if device.kind == Kind::Daemon {
        let full = app.control.drives(&who);
        return serve_machine(app, inbound, out, who, device, principal, caller, full, changed).await;
    }
    let recheck_who = who;
    let recheck_was = principal.clone();
    let still = move |app: &App| no_longer(app.control.device(&recheck_who).map(|(_, p)| p), &recheck_was);
    let log = Opened {
        account: device.account.clone(),
        name: device.name.clone(),
        role: "device".into(),
        by: "device",
        token_role: None,
    };
    let hands = (device.device.clone(), device.name.clone());
    run_client(app, inbound, out, principal, caller, hands, log, relayed, changed, false, still).await
}

/// How often a token channel checks the machine's remote-input switch,
/// which `sharing.json` holds and nothing announces.
const RECHECK: std::time::Duration = std::time::Duration::from_secs(2);

/// A channel opened with a machine token (aru-141): let in as the token's
/// role says, capped to a viewer's while remote input is off here.
#[allow(clippy::too_many_arguments)]
async fn serve_token(
    app: Arc<App>,
    enrolled: &crate::control::Enrolled,
    responder: Responder,
    hello: arugula_control_wire::machine_token::TokenHello,
    inbound: mpsc::Receiver<Vec<u8>>,
    out: mpsc::Sender<Vec<u8>>,
    relayed: bool,
    changed: tokio::sync::watch::Receiver<u64>,
) -> anyhow::Result<()> {
    let claims = match app.control.check_machine_token(&hello.machine_token).await {
        Ok(c) => c,
        Err(why) => {
            warn!(%why, "refused a channel's machine token");
            let refusal = Refusal { ok: false, refused: "machine-token".into(), message: why.to_string() };
            let (m2, _) = responder.finish(&refusal.encode())?;
            out.send(m2).await?;
            anyhow::bail!("machine token refused: {why}");
        }
    };
    let input = arugula_proto::sharing::remote_input(app.control.state_dir());
    let grant = token_grant(&claims, enrolled.saved.is_hosted(), input);
    let (m2, ch) = responder.finish(&Welcome { now: arugula_e2e::now_ms() }.encode())?;
    out.send(m2).await?;
    let out = Arc::new(Out::new(ch, out));
    let id = format!("account:{}", claims.sub);
    if let Some(r) = grant.role {
        app.acl.hold_token_role(&id, r);
    }
    let caller = Caller { account: claims.sub.clone(), device: id.clone(), name: claims.name.clone() };
    let log = Opened {
        account: claims.sub.clone(),
        name: claims.name.clone(),
        role: grant.shown.into(),
        by: "token",
        token_role: grant.role.map(|_| id.clone()),
    };
    // A role that could type ends when remote input goes off here; the
    // client opens a new channel and gets a viewer's.
    let could_type = grant.could_type;
    let still = move |app: &App| {
        (could_type && !arugula_proto::sharing::remote_input(app.control.state_dir()))
            .then_some("remote input turned off")
    };
    run_client(
        app,
        inbound,
        out,
        grant.principal,
        caller,
        (id, claims.name.clone()),
        log,
        relayed,
        changed,
        true,
        still,
    )
    .await
}

/// What a machine token gives on this machine.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct TokenGrant {
    pub(crate) principal: crate::acl::Principal,
    /// The role on every session the ACL holds for the channel, if any.
    pub(crate) role: Option<arugula_core::Role>,
    /// The role to log.
    pub(crate) shown: &'static str,
    /// Whether it may type or act (more than watch).
    pub(crate) could_type: bool,
}

/// What `claims` give here. The owner is this machine's owner; on a hosted
/// machine an editor is too, as a team roster's editor is (M184). With
/// remote input off, nobody through control is more than a viewer.
pub(crate) fn token_grant(
    claims: &arugula_control_wire::machine_token::MachineClaims,
    hosted: bool,
    remote_input: bool,
) -> TokenGrant {
    use arugula_control_wire::machine_token::TokenRole;
    use arugula_core::Role;
    let user =
        || crate::acl::Principal::User { id: format!("account:{}", claims.sub), name: claims.name.clone(), pic: None };
    let types = matches!(claims.role, TokenRole::Owner | TokenRole::Editor);
    if types && !remote_input {
        return TokenGrant { principal: user(), role: Some(Role::Viewer), shown: "viewer", could_type: false };
    }
    match claims.role {
        TokenRole::Owner => {
            TokenGrant { principal: crate::acl::Principal::Owner, role: None, shown: "owner", could_type: true }
        }
        TokenRole::Editor if hosted => {
            TokenGrant { principal: crate::acl::Principal::Owner, role: None, shown: "editor", could_type: true }
        }
        TokenRole::Editor => {
            TokenGrant { principal: user(), role: Some(Role::Editor), shown: "editor", could_type: true }
        }
        TokenRole::Viewer => {
            TokenGrant { principal: user(), role: Some(Role::Viewer), shown: "viewer", could_type: false }
        }
        // Only what's shared with `account:<sub>`.
        TokenRole::Guest => TokenGrant { principal: user(), role: None, shown: "guest", could_type: false },
    }
}

/// What the remote sessions log says of a channel, and the token role to
/// let go of when it closes.
struct Opened {
    account: String,
    name: String,
    role: String,
    by: &'static str,
    token_role: Option<String>,
}

/// A client's channel once it's let in, as `principal`: the mux's client
/// and the daemon's API, until the socket ends or `still` says why it no
/// longer may (checked when access changes, before each request, and with
/// `poll` every [`RECHECK`]).
#[allow(clippy::too_many_arguments)]
async fn run_client(
    app: Arc<App>,
    mut inbound: mpsc::Receiver<Vec<u8>>,
    out: Arc<Out>,
    principal: crate::acl::Principal,
    caller: Caller,
    hands: (String, String),
    log: Opened,
    relayed: bool,
    mut changed: tokio::sync::watch::Receiver<u64>,
    poll: bool,
    still: impl Fn(&App) -> Option<&'static str>,
) -> anyhow::Result<()> {
    let ch = out.ch.clone();
    // An owner here through control has a name of their own (M30).
    let name = caller.owner_name(&app, &principal);
    let client = app.new_client_id();
    info!(
        device = hands.0,
        account = caller.account,
        name = hands.1,
        who = principal.id(),
        by = log.by,
        client,
        "channel open"
    );
    let (data_tx, mut data_rx) = client_queue();
    let (ctrl_tx, mut ctrl_rx) = mpsc::unbounded_channel();
    app.hands.connect(client, ctrl_tx.clone(), principal.is_owner(), Some(hands.clone()));
    app.mux.send(Cmd::Connect {
        sub: Subscriber { client, data: data_tx, ctrl: ctrl_tx, principal: principal.clone(), name },
    });
    let path = if relayed { "relay" } else { "direct" };
    crate::remote_log::append(
        app.control.state_dir(),
        &crate::remote_log::Entry {
            event: "open",
            at: arugula_e2e::now_ms(),
            client,
            account: &log.account,
            name: &log.name,
            role: &log.role,
            path,
            by: log.by,
            secs: None,
        },
    );
    // The callers drop this future when the socket's reader ends first,
    // which is how most channels end: leave the mux and the hands then too.
    let _left =
        Leave { app: app.clone(), client, device: hands.0.clone(), opened: std::time::Instant::now(), log, path };
    let router = crate::server::channel_router(app.clone());
    // Its requests (a follow can run for as long as the channel does) end
    // with it: dropping the set aborts them.
    let mut requests = tokio::task::JoinSet::new();
    // When this relayed channel last sent pane output (#856).
    let mut last_output: Option<tokio::time::Instant> = None;
    let mut tick = tokio::time::interval(RECHECK);
    tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    loop {
        tokio::select! {
            Some(_) = requests.join_next(), if !requests.is_empty() => {}
            w = inbound.recv() => {
                let Some(w) = w else { break Ok(()) };
                match ch.open(&w) {
                    Ok(None) => {}
                    Ok(Some(Msg::Text(t))) => {
                        if let Err(e) = crate::server::handle(&app, client, Message::Text(t.into())) {
                            debug!(client, error = %e, "bad client message");
                        }
                    }
                    Ok(Some(Msg::Binary(b))) => {
                        if let Err(e) = crate::server::handle(&app, client, Message::Binary(b.into())) {
                            debug!(client, error = %e, "bad client message");
                        }
                    }
                    Ok(Some(Msg::Request { id, head, body })) => {
                        // By what's true now, not at the handshake: the mux's
                        // word that access went may still be on its way here.
                        if still(&app).is_some() {
                            break Ok(());
                        }
                        requests.spawn(answer(router.clone(), out.clone(), id, head, body, principal.clone(), caller.clone()));
                    }
                    Ok(Some(Msg::Response { .. })) => break Err(anyhow::anyhow!("a client doesn't answer requests")),
                    Err(e) => break Err(e),
                }
            }
            Some(o) = ctrl_rx.recv() => {
                let mut end = None;
                for o in crate::server::ctrl_batch(o, &mut ctrl_rx) {
                    let Some(m) = to_msg(o) else {
                        end = Some(Ok(()));
                        break;
                    };
                    if let Err(e) = out.put(&m).await {
                        end = Some(Err(e));
                        break;
                    }
                }
                if let Some(r) = end { break r }
            }
            Some(o) = data_rx.recv() => {
                let batch = if relayed { coalesced(o, &mut data_rx, &mut last_output).await } else { vec![o] };
                let mut end = None;
                for o in batch {
                    let Some(m) = to_msg(o) else {
                        end = Some(Ok(()));
                        break;
                    };
                    if let Err(e) = out.put(&m).await {
                        end = Some(Err(e));
                        break;
                    }
                }
                if let Some(r) = end { break r }
            }
            Ok(()) = changed.changed() => {
                if let Some(why) = still(&app) {
                    info!(device = hands.0, "{why}: closing its channel");
                    // Say why, as the mux does when a grant goes (M30): the
                    // page drops what it showed instead of keeping it greyed.
                    // Someone whose standing changed just reconnects.
                    if why == UNTRUSTED {
                        let message = "your access was removed".to_owned();
                        let m = Msg::Text(serde_json::to_string(&arugula_proto::ServerMsg::Error { id: None, message }).expect("serialize"));
                        let _ = out.put(&m).await;
                    }
                    break Ok(());
                }
            }
            _ = tick.tick(), if poll => {
                if let Some(why) = still(&app) {
                    info!(device = hands.0, "{why}: closing its channel");
                    break Ok(());
                }
            }
        }
    }
}

/// The device on the other end of a channel (S34, #399): which account it
/// is, whatever its principal (every team owner is `Owner` on a team box).
/// Handlers that decide by account (the agents') read it.
#[derive(Debug, Clone)]
#[cfg_attr(not(feature = "labs"), allow(dead_code))]
pub struct Caller {
    pub account: String,
    pub device: String,
    pub name: String,
}

impl Caller {
    /// What an owner here through control goes by (M30): their account's
    /// login, or a team box's owner by name. Its channel's client and its
    /// requests both say it, so an answer and a typed command name one
    /// person the same way (UI audit #194). None for anyone else.
    pub fn owner_name(&self, app: &App, who: &crate::acl::Principal) -> Option<String> {
        who.is_owner().then(|| app.control.name_of_account(&self.account)).flatten()
    }
}

/// The request headers a channel carries besides `Content-Type` (#400).
const PASSED_HEADERS: [&str; 2] = ["A2A-Version", "A2A-Extensions"];

/// The response headers a channel carries besides `Content-Type` (#933): a
/// file read's size and offset.
const PASSED_BACK: [&str; 2] = ["x-arugula-size", "x-arugula-offset"];

/// Whether `path` is one an agent-calling daemon may ask for: under
/// `/api/a2a/`, as the router will see it (decoded, no `..`).
fn agents_only(path: &str) -> bool {
    let path = path.split('?').next().unwrap_or_default();
    crate::authz::segments(path).is_some_and(|s| {
        s.len() > 2 && s[0] == "api" && s[1] == "a2a" && !s.iter().any(|x| x == ".." || x == "." || x.contains('/'))
    })
}

/// A daemon's channel (#399): its requests under `/api/a2a/` answered as
/// its account, anything else refused, and closed when it's no longer let
/// in. `full` (M184, a hosted machine's driver): every request, as its
/// principal says, and closed when it no longer drives this one.
#[allow(clippy::too_many_arguments)]
async fn serve_machine(
    app: Arc<App>,
    mut inbound: mpsc::Receiver<Vec<u8>>,
    out: Arc<Out>,
    who: [u8; 32],
    device: arugula_e2e::Cert,
    principal: crate::acl::Principal,
    caller: Caller,
    full: bool,
    mut changed: tokio::sync::watch::Receiver<u64>,
) -> anyhow::Result<()> {
    let what = if full { "driving channel" } else { "agent channel" };
    info!(device = device.device, account = device.account, name = device.name, "{what} open");
    let router = crate::server::channel_router(app.clone());
    let ch = out.ch.clone();
    let mut requests = tokio::task::JoinSet::new();
    let r = loop {
        tokio::select! {
            Some(_) = requests.join_next(), if !requests.is_empty() => {}
            w = inbound.recv() => {
                let Some(w) = w else { break Ok(()) };
                match ch.open(&w) {
                    Ok(None) => {}
                    Ok(Some(Msg::Request { id, head, body })) if full || agents_only(&head.path) => {
                        requests.spawn(answer(router.clone(), out.clone(), id, head, body, principal.clone(), caller.clone()));
                    }
                    Ok(Some(Msg::Request { id, .. })) => {
                        let body = serde_json::to_vec(&serde_json::json!({ "error": "a machine reaches only the agents here (/api/a2a/)" })).unwrap();
                        let m = Msg::Response { id, head: ResponseHead { status: 403, content_type: Some("application/json".to_owned()), more: false, headers: Default::default() }, body };
                        if let Err(e) = out.put(&m).await { break Err(e) }
                    }
                    Ok(Some(_)) => break Err(anyhow::anyhow!("a machine's channel carries requests only")),
                    Err(e) => break Err(e),
                }
            }
            Ok(()) = changed.changed() => {
                let gone = no_longer(app.control.device(&who).map(|(_, p)| p), &principal);
                if gone.is_some() || (full && !app.control.drives(&who)) {
                    info!(device = device.device, "machine no longer let in: closing its {what}");
                    break Ok(());
                }
            }
        }
    };
    info!(device = device.device, "{what} closed");
    r
}

pub(crate) const UNTRUSTED: &str = "device no longer trusted";

/// Why a channel opened as `was` closes now that its device resolves to
/// `now` (None: no longer trusted): it's gone, or it's someone else or has
/// another standing here (an owner made a viewer, say). The client opens a
/// new one as whoever it is now. None: it carries on.
pub(crate) fn no_longer(now: Option<crate::acl::Principal>, was: &crate::acl::Principal) -> Option<&'static str> {
    match now {
        None => Some(UNTRUSTED),
        Some(p) if p.id() != was.id() || p.is_owner() != was.is_owner() => Some("device's standing changed"),
        Some(_) => None,
    }
}

/// A channel's client leaving the mux and the hands, however the channel ends.
struct Leave {
    app: Arc<App>,
    client: arugula_proto::ClientId,
    device: String,
    /// So "channel closed" says how long it lasted: a device that keeps
    /// reopening its channel stands out in the log (#369).
    opened: std::time::Instant,
    /// For the remote sessions log, and the token role to let go of.
    log: Opened,
    path: &'static str,
}

impl Drop for Leave {
    fn drop(&mut self) {
        self.app.mux.send(Cmd::Disconnect { client: self.client });
        self.app.hands.disconnect(self.client);
        if let Some(id) = &self.log.token_role {
            self.app.acl.drop_token_role(id);
        }
        let lasted_ms = self.opened.elapsed().as_millis() as u64;
        info!(device = self.device, client = self.client, lasted_ms, "channel closed");
        crate::remote_log::append(
            self.app.control.state_dir(),
            &crate::remote_log::Entry {
                event: "close",
                at: arugula_e2e::now_ms(),
                client: self.client,
                account: &self.log.account,
                name: &self.log.name,
                role: &self.log.role,
                path: self.path,
                by: self.log.by,
                secs: Some(lasted_ms / 1000),
            },
        );
    }
}

/// What a relayed channel sends for `first` and whatever is queued behind
/// it (#856). Pane output within [`COALESCE`] of the last that went waits
/// out the rest of it, so a busy pane's output goes in one message per
/// pane per [`COALESCE`]; then everything queued is taken, in order, with
/// each pane's back-to-back output joined ([`join_output`]).
async fn coalesced(first: ToClient, rx: &mut ClientRx, last: &mut Option<tokio::time::Instant>) -> Vec<ToClient> {
    let output = matches!(&first, ToClient::Frame(b) if b.first() == Some(&(FrameKind::Output as u8)));
    if output && let Some(at) = *last {
        tokio::time::sleep_until(at + COALESCE).await;
    }
    let mut items = vec![first];
    while items.len() < COALESCE_ITEMS {
        let Some(o) = rx.try_recv() else { break };
        items.push(o);
    }
    let items = join_output(items);
    if items.iter().any(|o| matches!(o, ToClient::Frame(b) if b.first() == Some(&(FrameKind::Output as u8)))) {
        *last = Some(tokio::time::Instant::now());
    }
    items
}

/// `items` in order, with each run of one pane's output whose offsets follow
/// on joined into one frame (up to [`COALESCE_MAX`]): the client writes the
/// same bytes at the same offsets either way.
fn join_output(items: Vec<ToClient>) -> Vec<ToClient> {
    let mut out: Vec<ToClient> = Vec::with_capacity(items.len());
    // The output frame being built, not yet in `out`.
    let mut open: Option<Frame> = None;
    for o in items {
        let f = match &o {
            ToClient::Frame(b) => Frame::decode(b).ok().filter(|f| f.kind == FrameKind::Output),
            _ => None,
        };
        match (f, open.as_mut()) {
            (Some(f), Some(cur))
                if f.pane == cur.pane
                    && f.offset == cur.offset + cur.data.len() as u64
                    && cur.data.len() + f.data.len() <= COALESCE_MAX =>
            {
                cur.data.extend_from_slice(&f.data);
            }
            (Some(f), _) => {
                if let Some(cur) = open.replace(f) {
                    out.push(ToClient::Frame(cur.encode()));
                }
            }
            (None, _) => {
                if let Some(cur) = open.take() {
                    out.push(ToClient::Frame(cur.encode()));
                }
                out.push(o);
            }
        }
    }
    if let Some(cur) = open {
        out.push(ToClient::Frame(cur.encode()));
    }
    out
}

/// `None`: hang up.
fn to_msg(o: ToClient) -> Option<Msg> {
    match o {
        ToClient::Frame(bytes) => Some(Msg::Binary(bytes)),
        ToClient::Msg(m) => Some(Msg::Text(serde_json::to_string(&m).expect("serialize"))),
        ToClient::Json(t) => Some(Msg::Text(t)),
        ToClient::Close => None,
    }
}

async fn answer(
    router: Router,
    out: Arc<Out>,
    id: u32,
    head: RequestHead,
    body: Vec<u8>,
    who: crate::acl::Principal,
    caller: Caller,
) {
    let stream = head.stream;
    let (status, content_type, headers, body) = match call(router, head, body, who, caller).await {
        Ok(r) => r,
        Err(e) => {
            let body = serde_json::to_vec(&serde_json::json!({ "error": e.to_string() })).unwrap();
            let head = ResponseHead {
                status: 400,
                content_type: Some("application/json".to_owned()),
                more: false,
                headers: Default::default(),
            };
            let _ = out.put(&Msg::Response { id, head, body }).await;
            return;
        }
    };
    let say = |status, content_type, more, body| Msg::Response {
        id,
        head: ResponseHead { status, content_type, more, headers: headers.clone() },
        body,
    };
    // A body of no fixed length (a follow) goes in parts when the client
    // can take them; otherwise whole, as before.
    if !stream || body.size_hint().exact().is_some() {
        let body = match axum::body::to_bytes(body, MAX_MSG - 1024).await {
            Ok(b) => b.to_vec(),
            Err(e) => {
                let body = serde_json::to_vec(&serde_json::json!({ "error": e.to_string() })).unwrap();
                let _ = out.put(&say(400, Some("application/json".to_owned()), false, body)).await;
                return;
            }
        };
        let _ = out.put(&say(status, content_type, false, body)).await;
        return;
    }
    if out.put(&say(status, content_type.clone(), true, Vec::new())).await.is_err() {
        return;
    }
    let mut parts = body.into_data_stream();
    while let Some(Ok(b)) = parts.next().await {
        // The channel went: nobody to tell.
        if out.put(&say(status, content_type.clone(), true, b.to_vec())).await.is_err() {
            return;
        }
    }
    let _ = out.put(&say(status, content_type, false, Vec::new())).await;
}

async fn call(
    router: Router,
    head: RequestHead,
    body: Vec<u8>,
    who: crate::acl::Principal,
    caller: Caller,
) -> anyhow::Result<(u16, Option<String>, std::collections::BTreeMap<String, String>, Body)> {
    anyhow::ensure!(head.path.starts_with("/api/"), "only the API is reachable this way");
    let mut req = Request::builder().method(head.method.as_str()).uri(head.path.as_str());
    if let Some(ct) = &head.content_type {
        req = req.header(header::CONTENT_TYPE, ct);
    }
    // The few other headers a caller may send (#400): A2A's service
    // parameters. Anything else is dropped, so nothing that means
    // something to the router or authz comes from a channel.
    for (k, v) in &head.headers {
        if PASSED_HEADERS.iter().any(|h| h.eq_ignore_ascii_case(k)) {
            req = req.header(k.as_str(), v.as_str());
        }
    }
    let mut req = req.body(Body::from(body))?;
    // Who's asking: the API's checks (authz.rs) go by it.
    req.extensions_mut().insert(who);
    // Which account's device it is (a team owner is `Owner` above, whichever
    // account they are).
    req.extensions_mut().insert(caller);
    let res = router.oneshot(req).await?;
    let status = res.status().as_u16();
    let ct = res.headers().get(header::CONTENT_TYPE).and_then(|v| v.to_str().ok()).map(str::to_owned);
    // The answer's headers a client pages by (#933), nothing else.
    let headers = PASSED_BACK
        .iter()
        .filter_map(|h| Some(((*h).to_owned(), res.headers().get(*h)?.to_str().ok()?.to_owned())))
        .collect();
    Ok((status, ct, headers, res.into_body()))
}

#[cfg(test)]
mod passed_back_tests {
    use super::*;

    /// A file read's size and offset cross the channel with its bytes, and
    /// nothing else the router sets does (#933).
    #[tokio::test]
    async fn a_reads_size_and_offset_cross_the_channel() {
        let router = Router::new().route(
            "/api/x",
            axum::routing::get(|| async {
                ([("x-arugula-size", "3145728"), ("x-arugula-offset", "0"), ("set-cookie", "a=b")], "bytes")
            }),
        );
        let head = RequestHead {
            method: "GET".into(),
            path: "/api/x".into(),
            content_type: None,
            stream: false,
            headers: Default::default(),
        };
        let caller = Caller { account: String::new(), device: String::new(), name: String::new() };
        let (_, _, headers, _) = call(router, head, Vec::new(), crate::acl::Principal::Owner, caller).await.unwrap();
        let sent: Vec<_> = headers.iter().map(|(k, v)| (k.as_str(), v.as_str())).collect();
        assert_eq!(sent, [("x-arugula-offset", "0"), ("x-arugula-size", "3145728")]);
    }
}

#[cfg(test)]
mod coalesce_tests {
    use super::*;

    fn out(pane: u32, offset: u64, data: &[u8]) -> ToClient {
        ToClient::Frame(Frame { kind: FrameKind::Output, pane, offset, data: data.to_vec() }.encode())
    }

    fn frames(items: &[ToClient]) -> Vec<(u32, u64, Vec<u8>)> {
        items
            .iter()
            .map(|o| match o {
                ToClient::Frame(b) => {
                    let f = Frame::decode(b).unwrap();
                    (f.pane, f.offset, f.data)
                }
                ToClient::Json(j) => (0, 0, j.as_bytes().to_vec()),
                _ => (0, 0, vec![]),
            })
            .collect()
    }

    /// A pane's output that follows on is one frame; another pane's, a gap,
    /// or anything else in between keeps the order and starts a new one.
    #[test]
    fn back_to_back_output_is_joined_in_order() {
        let items = vec![
            out(1, 0, b"ab"),
            out(1, 2, b"cd"),
            out(2, 0, b"x"),
            out(1, 4, b"ef"),
            out(1, 9, b"gap"),
            ToClient::Json("{}".into()),
            out(1, 12, b"h"),
        ];
        assert_eq!(
            frames(&join_output(items)),
            vec![
                (1, 0, b"abcd".to_vec()),
                (2, 0, b"x".to_vec()),
                (1, 4, b"ef".to_vec()),
                (1, 9, b"gap".to_vec()),
                (0, 0, b"{}".to_vec()),
                (1, 12, b"h".to_vec()),
            ]
        );
    }

    /// A joined frame stops at [`COALESCE_MAX`].
    #[test]
    fn joined_output_is_bounded() {
        let chunk = vec![b'x'; COALESCE_MAX / 2 + 1];
        let items = vec![out(1, 0, &chunk), out(1, chunk.len() as u64, &chunk)];
        assert_eq!(join_output(items).len(), 2);
    }

    /// A busy pane's thousand small reads go as a few messages, and output
    /// after a quiet spell goes at once.
    #[tokio::test]
    async fn a_busy_panes_output_goes_once_per_interval() {
        let (tx, mut rx) = client_queue();
        let mut last = None;
        let first = out(1, 0, b"a");
        let t = tokio::time::Instant::now();
        let sent = coalesced(first, &mut rx, &mut last).await;
        assert_eq!(sent.len(), 1);
        assert!(t.elapsed() < COALESCE, "the first goes at once");
        for i in 1..1000u64 {
            tx.try_send(ToClient::Frame(
                Frame { kind: FrameKind::Output, pane: 1, offset: i, data: b"a".to_vec() }.encode(),
            ))
            .unwrap();
        }
        let next = rx.recv().await.unwrap();
        let sent = coalesced(next, &mut rx, &mut last).await;
        assert!(last.unwrap() >= t + COALESCE, "the next waits out the interval");
        assert_eq!(frames(&sent), vec![(1, 1, vec![b'a'; COALESCE_ITEMS])]);
    }
}

#[cfg(test)]
mod tests {
    use super::{UNTRUSTED, agents_only, no_longer};
    use crate::acl::Principal;

    /// A channel is as whoever its device is now: gone, someone else, or
    /// an owner no longer (or newly) one closes it; a new name doesn't.
    #[test]
    fn a_channel_closes_when_its_device_stands_differently() {
        let user = |id: &str, name: &str| Principal::User { id: id.into(), name: name.into(), pic: None };
        let dee = user("account:d1", "dee");
        assert_eq!(no_longer(None, &Principal::Owner), Some(UNTRUSTED));
        assert_eq!(no_longer(Some(Principal::Owner), &Principal::Owner), None);
        assert!(no_longer(Some(dee.clone()), &Principal::Owner).is_some(), "an owner made a viewer");
        assert!(no_longer(Some(Principal::Owner), &dee).is_some(), "a viewer made an owner");
        assert!(no_longer(Some(user("account:e1", "dee")), &dee).is_some());
        assert_eq!(no_longer(Some(user("account:d1", "Dee D.")), &dee), None);
    }

    #[test]
    fn a_machine_reaches_only_the_agents() {
        for ok in ["/api/a2a/agents", "/api/a2a/agents/fixer/card", "/api/a2a/agents/fixer?x=1"] {
            assert!(agents_only(ok), "{ok}");
        }
        for no in [
            "/api/a2a",
            "/api/panes",
            "/api/a2a/../panes",
            "/api/a2a/%2e%2e/panes",
            "/api/a2a/agents/%2F..%2Fpanes",
            "/api/blocks/1/call/send",
            "/e2e",
        ] {
            assert!(!agents_only(no), "{no}");
        }
    }
}

#[cfg(test)]
mod token_grants {
    use arugula_control_wire::machine_token::{MachineClaims, TokenRole, USE};
    use arugula_core::Role;

    use super::token_grant;
    use crate::acl::Principal;

    fn claims(role: TokenRole) -> MachineClaims {
        MachineClaims {
            iss: "i".into(),
            sub: "acct".into(),
            aud: "m".into(),
            use_: USE.into(),
            role,
            name: "bee".into(),
            iat: 0,
            exp: 300,
            jti: "j".into(),
        }
    }

    #[test]
    fn each_role_gives_what_it_says() {
        let user = Principal::User { id: "account:acct".into(), name: "bee".into(), pic: None };
        let g = |r, hosted, input| {
            let g = token_grant(&claims(r), hosted, input);
            (g.principal, g.role, g.could_type)
        };
        assert_eq!(g(TokenRole::Owner, false, true), (Principal::Owner, None, true));
        assert_eq!(g(TokenRole::Editor, false, true), (user.clone(), Some(Role::Editor), true));
        // A hosted machine's editors have full use of it (M184).
        assert_eq!(g(TokenRole::Editor, true, true), (Principal::Owner, None, true));
        assert_eq!(g(TokenRole::Viewer, false, true), (user.clone(), Some(Role::Viewer), false));
        assert_eq!(g(TokenRole::Guest, false, true), (user.clone(), None, false));
        // Remote input off: nobody through control types.
        assert_eq!(g(TokenRole::Owner, false, false), (user.clone(), Some(Role::Viewer), false));
        assert_eq!(g(TokenRole::Editor, true, false), (user.clone(), Some(Role::Viewer), false));
        assert_eq!(g(TokenRole::Guest, false, false), (user, None, false));
    }
}
