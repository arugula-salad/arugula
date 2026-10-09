//! The team's agent catalog (M77, #399): every agent offered on this
//! machine, the account's other machines, its teams' machines and its
//! teammates' (those that offer agents), as cards, read live from each.
//!
//! - **Live from each machine**, through control's relay as this daemon
//!   (`peer.rs`): `GET /api/a2a/agents` there. Control never sees a card.
//! - **Cached per machine** in `<state>/agent-catalog.json`: a refresh asks
//!   every online machine at once, and a machine that's offline (or doesn't
//!   answer) keeps the cards it had, marked so.
//! - **Fresh enough** is a minute: a read older than that refreshes first,
//!   and one refresh at a time runs (the others wait for it).

use std::{path::Path, sync::Arc};

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{server::App, store::now_ms};

/// The cache, in the state dir.
pub const FILE: &str = "agent-catalog.json";

/// A catalog older than this is refreshed before it's read.
pub const FRESH_MS: u64 = 60_000;

/// One machine's agents.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Shelf {
    /// The machine's id in control (this one's own, for `here`).
    pub machine: String,
    pub name: String,
    /// Who owns it, to show; empty for this account's own.
    #[serde(default)]
    pub owner: String,
    #[serde(default)]
    pub account: String,
    #[serde(default)]
    pub team: Option<String>,
    /// This machine.
    #[serde(default)]
    pub here: bool,
    /// It answered on the last refresh (its cards are current).
    #[serde(default)]
    pub online: bool,
    /// Its A2A agent cards.
    #[serde(default)]
    pub agents: Vec<Value>,
    /// When its cards were read.
    #[serde(default)]
    pub fetched_ms: Option<u64>,
    /// Why its cards are old or missing.
    #[serde(default)]
    pub note: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Catalog {
    pub machines: Vec<Shelf>,
    #[serde(default)]
    pub refreshed_ms: u64,
    /// Why only this machine is in it (not in control, say).
    #[serde(default)]
    pub note: Option<String>,
}

impl Catalog {
    /// Every agent, with its machine: `(shelf, card)`.
    pub fn agents(&self) -> impl Iterator<Item = (&Shelf, &Value)> {
        self.machines.iter().flat_map(|s| s.agents.iter().map(move |c| (s, c)))
    }

    /// One line an agent reads: `NAME on MACHINE (OWNER): description`.
    pub fn line(s: &Shelf, c: &Value) -> String {
        let who = if s.owner.is_empty() { String::new() } else { format!(", {}'s", s.owner) };
        let stale = if s.online { "" } else { " [offline: as last seen]" };
        let model = c["skills"][0]["tags"]
            .as_array()
            .and_then(|t| t.iter().filter_map(Value::as_str).find(|t| *t != "claude-code"))
            .map(|m| format!(" ({m})"))
            .unwrap_or_default();
        format!(
            "{} on {}{who}{model}{stale}: {}",
            c["name"].as_str().unwrap_or("?"),
            s.name,
            c["description"].as_str().unwrap_or("")
        )
    }
}

pub fn read(state_dir: &Path) -> Catalog {
    std::fs::read(state_dir.join(FILE)).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default()
}

fn save(state_dir: &Path, c: &Catalog) {
    if let Ok(b) = serde_json::to_vec_pretty(c)
        && let Err(e) = crate::store::write_atomic(&state_dir.join(FILE), &b)
    {
        tracing::warn!(error = %e, "can't keep the agent catalog");
    }
}

/// One refresh at a time.
static REFRESHING: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

/// The catalog, refreshed first if it's older than `max_age_ms` (0: now).
pub async fn get(app: &Arc<App>, max_age_ms: u64) -> Catalog {
    let state = app.control.state_dir().to_owned();
    let c = read(&state);
    if now_ms().saturating_sub(c.refreshed_ms) < max_age_ms.max(1) && !c.machines.is_empty() {
        return c;
    }
    let _one = REFRESHING.lock().await;
    // Someone else's refresh may have just finished.
    let c = read(&state);
    if max_age_ms > 0 && now_ms().saturating_sub(c.refreshed_ms) < max_age_ms && !c.machines.is_empty() {
        return c;
    }
    let next = refresh(app, &c).await;
    save(&state, &next);
    next
}

async fn refresh(app: &Arc<App>, last: &Catalog) -> Catalog {
    let state = app.control.state_dir();
    let here_name = app.hosts.name().to_owned();
    let here_id = app.control.enrolled().map(|e| e.saved.cert.device.clone()).unwrap_or_default();
    let (cards, _) = super::cards(&here_name, state, &crate::home());
    let mut machines = vec![Shelf {
        machine: here_id,
        name: here_name,
        here: true,
        online: true,
        agents: cards,
        fetched_ms: Some(now_ms()),
        ..Default::default()
    }];
    let peers = match crate::peer::machines(&app.control).await {
        Ok(p) => p,
        Err(e) => {
            return Catalog {
                machines,
                refreshed_ms: now_ms(),
                note: Some(format!("only this machine: control's directory can't be read ({e})")),
            };
        }
    };
    let asks = peers.iter().map(|m| async move {
        let before = last.machines.iter().find(|s| s.machine == m.id).cloned().unwrap_or_default();
        let mut s = Shelf {
            machine: m.id.clone(),
            name: m.name.clone(),
            owner: m.owner.clone(),
            account: m.account.clone(),
            team: m.team.clone(),
            agents: before.agents.clone(),
            fetched_ms: before.fetched_ms,
            ..Default::default()
        };
        if !m.online {
            s.note = Some("offline: its agents as last seen".into());
            return s;
        }
        match crate::peer::get(&app.control, m, "/api/a2a/agents").await {
            Ok((200, body)) => {
                let v: Value = serde_json::from_slice(&body).unwrap_or_default();
                s.agents = v["agents"].as_array().cloned().unwrap_or_default();
                s.online = true;
                s.fetched_ms = Some(now_ms());
            }
            // The flag is off there, or it's older than M76: nothing offered.
            Ok((404, _)) => {
                s.agents.clear();
                s.online = true;
                s.fetched_ms = Some(now_ms());
            }
            Ok((status, body)) => {
                let said =
                    serde_json::from_slice::<Value>(&body).ok().and_then(|v| v["error"].as_str().map(str::to_owned));
                s.note = Some(format!(
                    "it said {status}{}: its agents as last seen",
                    said.map(|w| format!(" ({w})")).unwrap_or_default()
                ));
            }
            Err(e) => s.note = Some(format!("not reached ({e:#}): its agents as last seen")),
        }
        s
    });
    let mut others: Vec<Shelf> = futures_util::future::join_all(asks).await;
    // A machine with nothing offered, now or before, isn't worth a row.
    others.retain(|s| !s.agents.is_empty() || (s.note.is_some() && s.fetched_ms.is_some()));
    others.sort_by(|a, b| (a.owner.as_str(), a.name.as_str()).cmp(&(b.owner.as_str(), b.name.as_str())));
    machines.extend(others);
    Catalog { machines, refreshed_ms: now_ms(), note: None }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn a_line_says_where_whose_and_whether_its_current() {
        let card =
            json!({ "name": "fixer", "description": "Fixes tests", "skills": [{ "tags": ["claude-code", "haiku"] }] });
        let s = Shelf { name: "ale-box".into(), owner: "ale".into(), online: false, ..Default::default() };
        assert_eq!(Catalog::line(&s, &card), "fixer on ale-box, ale's (haiku) [offline: as last seen]: Fixes tests");
        let s = Shelf { name: "geek".into(), online: true, ..Default::default() };
        assert_eq!(Catalog::line(&s, &card), "fixer on geek (haiku): Fixes tests");
    }
}
