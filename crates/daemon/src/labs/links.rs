//! A conversation's links (#856): panes it drives, here or on another
//! machine, each by a grant kept on the pane's machine.
//!
//! - **On the pane's machine**, a grant (`<state>/conv-grants.json`) says
//!   that one daemon, the conversation's home, may drive one pane here for
//!   one conversation. Someone who may drive the pane makes it from their
//!   own device (`conv.grant`, which needs an editor's role and drive
//!   trust), so "you can only link a pane you may drive" is checked where
//!   the pane is. It's listed on the pane and revoked there.
//! - **Over the relay**, the home's channel gets in (`e2e.rs`) for a granted
//!   pane's prompt, wait, capture, events and card answers, and nothing
//!   else ([`linked`]). Those run with the grant maker's rights on the pane:
//!   no route on that list needs more than an editor's.
//! - **At home**, a link (`ConvLink`) names the machine by device id, the
//!   pane and its epoch. Linking checks that the pane's machine lets the
//!   home in for it; [`statuses`] reads each link's state, and closes for
//!   good one its machine refuses.
//!
//! A revoked grant, or one whose pane went, is kept for a week, so the home
//! hears "revoked" instead of nothing (a teammate's machine lets the home's
//! daemon in only while some grant names it).

use std::{path::Path, sync::Mutex, time::Duration};

use arugula_proto::{
    Attention, ConvGrant, ConvId, ConvLink, ConvLinkState, ConvLinkStatus, Conversation, PaneId,
    api::{ConvGrantRequest, ConvLinkRequest, WaitResult},
};
use tracing::{info, warn};

use crate::{
    acl::Principal,
    mux::{Api, ConvReq},
    server::App,
};

/// A refusal: an HTTP status and what to say.
pub type Refusal = (u16, String);

/// How long a revoked grant is kept, for the home to hear it was.
const KEEP_REVOKED_MS: u64 = 7 * 86_400 * 1000;

/// How long a link's machine has to answer how the link is.
const STATUS_WAIT: Duration = Duration::from_secs(8);

/// How long linking waits for control to route the home here, and for the
/// pane's machine to answer.
const LINK_WAIT: Duration = Duration::from_secs(15);

/// The grants file is read and written whole; this keeps two changes from
/// crossing.
static FILE: Mutex<()> = Mutex::new(());

fn path(dir: &Path) -> std::path::PathBuf {
    dir.join("conv-grants.json")
}

fn load(dir: &Path) -> Vec<ConvGrant> {
    std::fs::read(path(dir)).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default()
}

fn save(dir: &Path, g: &[ConvGrant]) -> Result<(), Refusal> {
    let b = serde_json::to_vec_pretty(g).map_err(|e| (500, e.to_string()))?;
    crate::store::write_atomic(&path(dir), &b).map_err(|e| (500, format!("can't save the grant: {e}")))
}

/// Change the grants under the file's lock: `f` returns what to answer.
fn change<T>(dir: &Path, f: impl FnOnce(&mut Vec<ConvGrant>) -> Result<T, Refusal>) -> Result<T, Refusal> {
    let _held = FILE.lock().unwrap();
    let mut g = load(dir);
    let before = g.clone();
    let out = f(&mut g)?;
    let now = crate::store::now_ms();
    g.retain(|x| x.revoked.is_none_or(|at| now.saturating_sub(at) < KEEP_REVOKED_MS));
    if g != before {
        save(dir, &g)?;
    }
    Ok(out)
}

/// This machine as a grant names it: its device id in control, or `here`
/// when it isn't in one.
fn me(app: &App) -> String {
    app.control.enrolled().map(|e| e.saved.cert.device.clone()).unwrap_or_else(|| "here".into())
}

/// The daemons a live grant here lets in (`control.rs` adds them to the
/// machines it lets reach this one, for their links only), this one left
/// out; and those whose grants were revoked lately, so they hear it.
pub fn granted_devices(dir: &Path) -> Vec<String> {
    let mut d: Vec<String> = load(dir).into_iter().map(|g| g.device).collect();
    d.sort();
    d.dedup();
    d
}

/// Who a grant's requests run as: the person who made it.
fn principal(g: &ConvGrant) -> Principal {
    if g.by == "owner" {
        Principal::Owner
    } else {
        Principal::User { id: g.by.clone(), name: g.by_name.clone(), pic: None }
    }
}

/// `POST /api/panes/{id}/conv-grants`: let `req`'s home drive `pane` for
/// its conversation, as `by` (whom the route already checked may drive it).
pub async fn grant(app: &App, pane: PaneId, req: ConvGrantRequest, by: &Principal) -> Result<ConvGrant, Refusal> {
    let p = app.mux.api(|r| Api::Pane(pane, r)).await.flatten().ok_or((404, format!("no pane %{pane}")))?;
    let here = me(app);
    let device = req.device.filter(|d| !d.trim().is_empty()).unwrap_or_else(|| here.clone());
    let by_name = crate::api::who_is(app, by.clone()).await.map(|d| d.name).unwrap_or_else(|| by.id().to_owned());
    let g = ConvGrant {
        device: device.clone(),
        device_name: req.device_name.unwrap_or_default(),
        pane,
        epoch: p.epoch,
        conv: req.conv,
        conv_title: req.conv_title.unwrap_or_default(),
        by: by.id().to_owned(),
        by_name,
        at: crate::store::now_ms(),
        revoked: None,
    };
    let out = g.clone();
    change(app.control.state_dir(), move |all| {
        all.retain(|x| !(x.device == g.device && x.pane == g.pane && x.conv == g.conv));
        all.push(g);
        Ok(())
    })?;
    info!(pane, device, conv = %out.conv, by = out.by, "a conversation may drive a pane here");
    // Another machine gets in once control routes it here and this daemon
    // trusts it: before the home asks.
    if device != here && !app.control.refresh_now(LINK_WAIT).await {
        warn!("control didn't answer: the conversation's home may not reach this pane yet");
    }
    Ok(out)
}

/// The live grants on `pane`, those whose pane went marked revoked first.
pub async fn grants(app: &App, pane: PaneId) -> Vec<ConvGrant> {
    sweep(app).await;
    load(app.control.state_dir()).into_iter().filter(|g| g.pane == pane && g.revoked.is_none()).collect()
}

/// `POST /api/panes/{id}/conv-grants/revoke`: what's left on the pane.
pub async fn revoke(app: &App, pane: PaneId, device: &str, conv: ConvId) -> Result<Vec<ConvGrant>, Refusal> {
    let now = crate::store::now_ms();
    change(app.control.state_dir(), |all| {
        let g = all
            .iter_mut()
            .find(|g| g.pane == pane && g.device == device && g.conv == conv && g.revoked.is_none())
            .ok_or((404, "no such grant on this pane".to_owned()))?;
        g.revoked = Some(now);
        Ok(())
    })?;
    info!(pane, device, conv = %conv, "a conversation's grant on a pane was revoked");
    Ok(grants(app, pane).await)
}

/// Mark revoked the grants whose pane isn't here any more: a pane's number
/// isn't reused while the daemon runs, but a grant mustn't outlive it.
async fn sweep(app: &App) {
    let dir = app.control.state_dir();
    if load(dir).iter().all(|g| g.revoked.is_some()) {
        return;
    }
    let Some(panes) = app.mux.api(Api::Panes).await else { return };
    let now = crate::store::now_ms();
    let _ = change(dir, |all| {
        for g in all.iter_mut().filter(|g| g.revoked.is_none()) {
            if !panes.iter().any(|p| p.info.id == g.pane) {
                g.revoked = Some(now);
            }
        }
        Ok(())
    });
}

/// The header a home names the conversation with, on each request for a
/// linked pane: a grant is for one conversation, not for every one the home
/// keeps.
pub const CONV_HEADER: &str = "Arugula-Conv";

/// What a home daemon's request for a linked pane comes to.
pub enum Linked {
    /// Let through, as this person.
    Yes(Principal),
    /// Its grant was revoked, or its pane went: say so (410).
    Revoked,
}

/// Whether `device`'s request (`path`, with its query, and `body`), for
/// conversation `conv`, is for a pane a grant here lets it drive: prompt,
/// wait, capture, its events, or an answer to its card. `None`: not a
/// linked request at all.
pub async fn linked(
    app: &App,
    device: &str,
    conv: Option<&str>,
    method: &str,
    path: &str,
    body: &[u8],
) -> Option<Linked> {
    let conv: ConvId = conv?.parse().ok()?;
    let pane = linked_pane(method, path, body)?;
    sweep(app).await;
    let all = load(app.control.state_dir());
    let mine: Vec<&ConvGrant> = all.iter().filter(|g| g.device == device && g.pane == pane && g.conv == conv).collect();
    if let Some(g) = mine.iter().find(|g| g.revoked.is_none()) {
        return Some(Linked::Yes(principal(g)));
    }
    (!mine.is_empty()).then_some(Linked::Revoked)
}

/// The pane a request is for, if it's one a grant can let through. The path
/// must be written plainly (nothing encoded), so the router sees the same.
fn linked_pane(method: &str, path: &str, body: &[u8]) -> Option<PaneId> {
    let (path, query) = path.split_once('?').unwrap_or((path, ""));
    let s: Vec<&str> = path.strip_prefix('/')?.split('/').collect();
    let id = |s: &str| s.bytes().all(|b| b.is_ascii_digit()).then(|| s.parse::<PaneId>().ok()).flatten();
    match (method, s.as_slice()) {
        ("POST", ["api", "panes", n, "prompt"]) | ("GET", ["api", "panes", n, "wait" | "capture"]) => id(n),
        ("GET", ["api", "events"]) => {
            // Exactly one pane, named plainly; the other keys the route reads.
            let mut pane = None;
            for kv in query.split('&').filter(|kv| !kv.is_empty()) {
                let (k, v) = kv.split_once('=').unwrap_or((kv, ""));
                match k {
                    "pane" if pane.is_none() => pane = Some(id(v)?),
                    "type" | "follow" | "since" => {}
                    _ => return None,
                }
            }
            pane
        }
        ("POST", ["api", "attention", "act"]) if query.is_empty() => {
            let v: serde_json::Value = serde_json::from_slice(body).ok()?;
            if v.get("panes").is_some_and(|p| !p.as_array().is_some_and(Vec::is_empty)) {
                return None;
            }
            v["pane"].as_u64()?.try_into().ok()
        }
        _ => None,
    }
}

// ---------------------------------------------------------------- home

/// The conversation `id`, if `who` is in it.
async fn conv(app: &App, id: ConvId, who: &Principal) -> Result<Conversation, Refusal> {
    app.mux
        .api(|r| Api::Conv(ConvReq::Get { id }, who.clone(), r))
        .await
        .ok_or((503, "shutting down".into()))?
        .map_err(|e| (e.0, e.1))
}

/// Whether `machine` names this one.
fn is_here(app: &App, machine: &Option<String>) -> bool {
    machine.as_ref().is_none_or(|m| m.is_empty() || *m == me(app))
}

/// `POST /api/convs/{id}/links`: link a pane the pane's machine granted this
/// conversation, as `who` (someone in it).
pub async fn link(app: &App, id: ConvId, req: ConvLinkRequest, who: &Principal) -> Result<Conversation, Refusal> {
    let c = conv(app, id, who).await?;
    if c.floor(who.id()).is_none() {
        return Err((403, "only the people in a conversation link panes to it".into()));
    }
    let pane = req.pane;
    let (machine, machine_name, epoch) = if is_here(app, &req.machine) {
        let g = grants(app, pane)
            .await
            .into_iter()
            .find(|g| g.device == me(app) && g.conv == id)
            .ok_or((403, format!("%{pane} hasn't been granted to this conversation: grant it on the pane first")))?;
        (None, app.hosts.name().to_owned(), g.epoch)
    } else {
        let mid = req.machine.clone().unwrap_or_default();
        let m = crate::peer::find_by_id(&app.control, &mid)
            .await
            .map_err(|e| (502, e.to_string()))?
            .ok_or((404, "this machine doesn't reach that one: is it in your account or your team?".to_owned()))?;
        // The pane's machine lets this one in for it only by its grant.
        let path = format!("/api/panes/{pane}/wait?until=idle&timeout=0");
        let conv = id.to_string();
        let names = [(CONV_HEADER, conv.as_str())];
        let (status, body) = crate::peer::request_within(&app.control, &m, "GET", &path, None, &names, LINK_WAIT)
            .await
            .map_err(|e| (502, format!("{} didn't answer: {e}", m.name)))?;
        if status >= 400 {
            return Err((403, format!("{} refused %{pane}: {}", m.name, said(&body))));
        }
        (Some(m.id), m.name, req.epoch.unwrap_or_default())
    };
    let link = ConvLink {
        name: String::new(),
        machine,
        machine_name,
        pane,
        epoch: req.epoch.unwrap_or(epoch),
        title: req.title.unwrap_or_default().trim().chars().take(100).collect(),
        by: String::new(),
        by_name: String::new(),
        at: 0,
        closed: None,
    };
    let c = app
        .mux
        .api(|r| Api::Conv(ConvReq::Link { id, link }, who.clone(), r))
        .await
        .ok_or((503, "shutting down".to_owned()))?
        .map_err(|e| (e.0, e.1))?;
    Ok(c)
}

/// What a refusal's body says.
fn said(body: &[u8]) -> String {
    serde_json::from_slice::<serde_json::Value>(body)
        .ok()
        .and_then(|v| v["error"].as_str().map(str::to_owned))
        .unwrap_or_else(|| String::from_utf8_lossy(body).chars().take(200).collect())
}

/// `GET /api/convs/{id}/links`: how each link is now. A link whose pane
/// went, or whose machine refuses it, is closed for good here.
pub async fn statuses(app: &App, id: ConvId, who: &Principal) -> Result<Vec<ConvLinkStatus>, Refusal> {
    let c = conv(app, id, who).await?;
    let open: Vec<&ConvLink> = c.links.iter().filter(|l| l.closed.is_none()).collect();
    let machines = if open.iter().any(|l| !is_here(app, &l.machine)) {
        crate::peer::machines(&app.control).await.unwrap_or_default()
    } else {
        Vec::new()
    };
    let checks = open.iter().map(|l| status(app, id, l, &machines));
    let mut now: Vec<ConvLinkStatus> = futures_util::future::join_all(checks).await;
    for l in c.links.iter().filter(|l| l.closed.is_some()) {
        now.push(ConvLinkStatus {
            name: l.name.clone(),
            state: ConvLinkState::Closed,
            attention: None,
            why: l.closed.clone(),
        });
    }
    let order = |n: &str| c.links.iter().position(|l| l.name == n);
    now.sort_by_key(|s| order(&s.name));
    Ok(now)
}

async fn status(app: &App, id: ConvId, l: &ConvLink, machines: &[crate::peer::Machine]) -> ConvLinkStatus {
    let s = |state, attention, why: Option<String>| ConvLinkStatus { name: l.name.clone(), state, attention, why };
    let close = |why: String| async move {
        let req = ConvReq::LinkClosed { id, name: l.name.clone(), why: why.clone() };
        let _ = app.mux.api(|r| Api::Conv(req, Principal::Owner, r)).await;
        s(ConvLinkState::Closed, None, Some(why))
    };
    let pane = l.pane;
    let path = format!("/api/panes/{pane}/wait?until=idle&timeout=0");
    if is_here(app, &l.machine) {
        let here = me(app);
        if !grants(app, pane).await.iter().any(|g| g.device == here && g.conv == id) {
            let gone = app.mux.api(|r| Api::Pane(pane, r)).await.flatten().is_none();
            return close(if gone { "its pane closed".into() } else { "its grant was revoked".into() }).await;
        }
        // What it's doing now: a wait that doesn't wait.
        let a = match tokio::time::timeout(Duration::ZERO, crate::api::wait_attention(app, pane, false)).await {
            Ok(Ok(WaitResult::Attention { state, .. })) => Some(state),
            Ok(_) => None,
            Err(_) => Some(Attention::Working),
        };
        return s(ConvLinkState::Live, a, None);
    }
    let mid = l.machine.clone().unwrap_or_default();
    let Some(m) = machines.iter().find(|m| m.id == mid) else {
        return s(ConvLinkState::Offline, None, Some(format!("{} isn't reachable from here", l.machine_name)));
    };
    if !m.online {
        return s(ConvLinkState::Offline, None, Some(format!("{} is offline", m.name)));
    }
    let conv = id.to_string();
    let names = [(CONV_HEADER, conv.as_str())];
    match crate::peer::request_within(&app.control, m, "GET", &path, None, &names, STATUS_WAIT).await {
        Ok((status, body)) if status < 400 => {
            let a = match serde_json::from_slice::<WaitResult>(&body) {
                Ok(WaitResult::Attention { state, .. }) => Some(state),
                Ok(WaitResult::Timeout) => Some(Attention::Working),
                _ => None,
            };
            s(ConvLinkState::Live, a, None)
        }
        // Refused on a channel it got in on: no grant for it there now.
        Ok((410, _)) => close(format!("{} revoked it, or its pane closed", m.name)).await,
        Ok((403 | 404, body)) => close(format!("{} refused it: {}", m.name, said(&body))).await,
        Ok((status, body)) => {
            s(ConvLinkState::Offline, None, Some(format!("{} said {status}: {}", m.name, said(&body))))
        }
        Err(e) => s(ConvLinkState::Offline, None, Some(e.to_string())),
    }
}

#[cfg(test)]
mod tests {
    use super::linked_pane;

    #[test]
    fn a_grant_lets_through_its_pane_s_prompt_wait_capture_events_and_act_only() {
        let none: &[u8] = b"";
        for (method, path, body, pane) in [
            ("POST", "/api/panes/7/prompt", none, 7),
            ("GET", "/api/panes/7/wait?until=idle&timeout=0", none, 7),
            ("GET", "/api/panes/7/capture", none, 7),
            ("GET", "/api/events?pane=7&type=attention,exit&follow=1", none, 7),
            ("POST", "/api/attention/act", br#"{"action":"allow","pane":7}"#.as_slice(), 7),
            ("POST", "/api/attention/act", br#"{"action":"allow","pane":7,"panes":[]}"#.as_slice(), 7),
        ] {
            assert_eq!(linked_pane(method, path, body), Some(pane), "{method} {path}");
        }
        for (method, path, body) in [
            // Anything else on the pane, or another verb on the route.
            ("POST", "/api/panes/7/send", none),
            ("POST", "/api/panes/7/keys", none),
            ("POST", "/api/panes/7/close", none),
            ("GET", "/api/panes/7/prompt", none),
            ("POST", "/api/panes/7/capture", none),
            ("GET", "/api/panes", none),
            // Written so the router might read it otherwise.
            ("GET", "/api/panes/%37/capture", none),
            ("GET", "/api/panes/7/../8/capture", none),
            ("GET", "/api/panes/+7/capture", none),
            ("GET", "//api/panes/7/capture", none),
            // Events for every pane, two panes, or a key the route doesn't take.
            ("GET", "/api/events", none),
            ("GET", "/api/events?follow=1", none),
            ("GET", "/api/events?pane=7&pane=8", none),
            ("GET", "/api/events?pane=7&host=x", none),
            ("GET", "/api/events?p%61ne=7", none),
            // A card answer for several panes, or none named.
            ("POST", "/api/attention/act", br#"{"action":"allow","panes":[7,8]}"#.as_slice()),
            ("POST", "/api/attention/act", br#"{"action":"allow","pane":7,"panes":[8]}"#.as_slice()),
            ("POST", "/api/attention/act", br#"{"action":"allow"}"#.as_slice()),
            ("POST", "/api/attention/act?pane=7", br#"{"action":"allow","pane":7}"#.as_slice()),
        ] {
            assert_eq!(linked_pane(method, path, body), None, "{method} {path}");
        }
    }
}
