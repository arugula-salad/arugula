//! Public previews (M187, aru-097): a port of this hosted machine that a
//! project's owner or editor made public, which control serves to anyone
//! at `<id>.<preview domain>` (`crates/control/src/previews.rs`).
//!
//! Control opens a block channel here for each request, with a grant
//! signed with its hosted key (the key this machine's delegation names)
//! for one port and the preview's id (`arugula_e2e::preview`). This
//! machine lets it in only when the grant checks out and control, asked
//! at most every [`PUBLIC_FOR`], says that port is public under that id
//! now; then it answers the channel's frames as a block channel's
//! (`block_channel::frame`), through the block proxy with
//! `sites::Via::Public`, to that port only (never one of the daemon's
//! own). A machine that isn't hosted serves none.
//!
//! An open channel (a WebSocket) is checked again every [`PUBLIC_FOR`] and
//! closed once the port isn't public, so turning a preview off takes it
//! down within a minute. If control can't be asked, nothing is served.

use std::{
    collections::HashMap,
    sync::{Arc, LazyLock},
    time::{Duration, Instant},
};

use arugula_e2e::{
    block::{Answer, Expect, Refused},
    channel::{Msg, Responder},
    now_ms,
    preview::PublicHello,
};
use tokio::sync::{Mutex, mpsc};
use tracing::{info, warn};

use crate::{
    block_channel::{Stream, abort, drop_finished, frame},
    control::Enrolled,
    e2e::Out,
    ports::Target,
    server::App,
    sites::{self, Upstream, Via},
};

/// How long control's answer to "which ports are public" is trusted.
pub const PUBLIC_FOR: Duration = Duration::from_secs(20);

/// Ports public now and their preview ids, as control said.
type Ports = Vec<(u16, String)>;

/// Control's last answer, and when.
static PUBLIC: LazyLock<Mutex<Option<(Instant, Ports)>>> = LazyLock::new(|| Mutex::new(None));

/// The ports public now and their ids: control's answer from the last
/// [`PUBLIC_FOR`], or asked now. `None` when it can't be asked.
async fn public(app: &App) -> Option<Ports> {
    let mut held = PUBLIC.lock().await;
    if let Some((at, ports)) = held.as_ref()
        && at.elapsed() < PUBLIC_FOR
    {
        return Some(ports.clone());
    }
    match app.control.public_ports().await {
        Ok(ports) => {
            *held = Some((Instant::now(), ports.clone()));
            Some(ports)
        }
        Err(e) => {
            warn!(error = %format!("{e:#}"), "couldn't ask control which ports are public");
            *held = None;
            None
        }
    }
}

async fn is_public(app: &App, port: u16, id: &str) -> bool {
    public(app).await.is_some_and(|ports| ports.iter().any(|(p, i)| *p == port && i == id))
}

/// Whether `hello` lets `remote` in, as this machine sees it now.
async fn check(app: &App, e: &Enrolled, remote: &[u8; 32], hello: &PublicHello) -> Result<(), Refused> {
    let g = &hello.public;
    let want = Expect { daemon: &e.keys.id(), remote, now: now_ms() };
    let key = e.saved.hosted.as_ref().map(|h| h.delegation.key.as_str());
    // Everything but whether it's public, before control is asked.
    g.check(&want, key, |_, _| true)?;
    let sites = sites::get().ok_or(Refused::NoBlock)?;
    sites.allowed(&Target::Local(g.port)).map_err(|_| Refused::NotPublic)?;
    if !is_public(app, g.port, &g.preview).await {
        return Err(Refused::NotPublic);
    }
    Ok(())
}

/// A channel whose message 1 carried a public grant: answers the
/// handshake, then serves the port until something ends it.
pub(crate) async fn serve(
    app: Arc<App>,
    e: &Enrolled,
    responder: Responder,
    remote: [u8; 32],
    hello: PublicHello,
    mut inbound: mpsc::Receiver<Vec<u8>>,
    out: mpsc::Sender<Vec<u8>>,
) -> anyhow::Result<()> {
    let verdict = check(&app, e, &remote, &hello).await;
    let answer = match &verdict {
        Ok(()) => Answer::ok(),
        Err(why) => Answer::refused(*why),
    };
    let (m2, ch) = responder.finish(&answer.encode())?;
    out.send(m2).await?;
    if let Err(why) = verdict {
        info!(port = hello.public.port, code = why.code(), "refused a public preview");
        return Ok(());
    }
    let (port, id) = (hello.public.port, hello.public.preview);
    let sites = sites::get().ok_or_else(|| anyhow::anyhow!("no block sites"))?;
    let site = sites.public_site(port);
    let out = Arc::new(Out::new(ch, out));
    let ch = out.ch.clone();
    let up = Arc::new(Upstream::default());
    let mut streams: HashMap<u32, Stream> = HashMap::new();
    let mut again = tokio::time::interval_at(tokio::time::Instant::now() + PUBLIC_FOR, PUBLIC_FOR);
    let r = loop {
        let w = tokio::select! {
            w = inbound.recv() => w,
            _ = again.tick() => {
                if !is_public(&app, port, &id).await {
                    info!(port, "no longer public: closing its preview's channel");
                    break Ok(());
                }
                continue;
            }
        };
        let Some(w) = w else { break Ok(()) };
        let f = match ch.open(&w) {
            Ok(None) => continue,
            Ok(Some(Msg::Binary(f))) => f,
            Ok(Some(_)) => break Err(anyhow::anyhow!("a public preview's channel carries frames only")),
            Err(e) => break Err(e),
        };
        drop_finished(&mut streams);
        if let Err(e) = frame(&sites, &site, &up, &out, &mut streams, &f, Via::Public) {
            break Err(e);
        }
    };
    abort(streams);
    r
}
