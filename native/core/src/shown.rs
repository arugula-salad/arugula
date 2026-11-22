//! Which sessions the apps show, and their machines in order: the web's
//! `inShown` (web/src/shell/project-view.ts) and `machineSections` /
//! `pinnedCards` (web/src/app/machine-list.ts), once for both apps (Arugula
//! #1355). Machines and Home show only the current project's sessions; a
//! session placed in no project is its machine owner's Default: yours, on
//! your own machine, in your personal project (#1161).
//!
//! What crosses is JSON, as everything else in [`crate::ffi`]: a machine's
//! sessions come as `aru_link_panes` hands them out.

use std::cmp::Ordering;

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// The project the app shows.
#[derive(Debug, Clone, Deserialize)]
pub struct At {
    pub id: String,
    /// It's the reader's personal project, where a session placed nowhere shows.
    #[serde(default)]
    pub personal: bool,
}

/// Whether a session in `project` (its id; none: placed nowhere), on a
/// machine that's `yours` or not, shows in `at`. Everything shows without one.
pub fn in_shown(project: Option<&str>, yours: bool, at: Option<&At>) -> bool {
    let Some(at) = at else { return true };
    match project {
        Some(p) => p == at.id,
        None => yours && at.personal,
    }
}

/// Whether a pane shows in `at`: its session's, as [`in_shown`], or the
/// card its agent works on is the project's. A pane on another project's
/// card isn't placed nowhere.
pub fn pane_shown(session_project: Option<&str>, on_issue_project: Option<&str>, yours: bool, at: Option<&At>) -> bool {
    let Some(a) = at else { return true };
    if on_issue_project.is_some_and(|p| p == a.id) {
        return true;
    }
    match (session_project, on_issue_project) {
        (None, Some(_)) => false,
        (p, _) => in_shown(p, yours, at),
    }
}

/// A machine as the directory lists it, with its sessions when a link to it
/// has them (`aru_link_panes`' JSON).
#[derive(Debug, Clone, Deserialize)]
pub struct Host {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub online: bool,
    /// Whose it is when it isn't yours: their name. None: yours.
    #[serde(default)]
    pub owner: Option<String>,
    /// A cloud workspace, not someone's computer.
    #[serde(default)]
    pub hosted: bool,
    #[serde(default)]
    pub panes: Option<Value>,
}

/// One of a session's tabs, its panes (their ids) in layout order.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SessionTab {
    pub id: u64,
    pub name: Option<String>,
    pub panes: Vec<u64>,
}

/// A session's card.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct SessionCard {
    pub id: u64,
    pub name: String,
    /// Its project, `{id, name}`; none: placed nowhere.
    pub project: Option<Value>,
    pub tabs: Vec<SessionTab>,
    pub panes: usize,
    /// How many of its panes need you.
    pub needs_you: usize,
    /// `needs_input`, `working` or `idle`.
    pub state: &'static str,
}

/// A machine's section.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct MachineSection {
    pub id: String,
    pub name: String,
    pub online: bool,
    pub cloud: bool,
    /// Whose it is when it isn't yours.
    pub owner: Option<String>,
    /// Its sessions in the project, needs you first.
    pub sessions: Vec<SessionCard>,
}

/// A session someone pinned: its machine's id and its own.
#[derive(Debug, Clone, PartialEq, Deserialize, Serialize)]
pub struct Pin {
    pub machine: String,
    pub session: u64,
}

/// It needs you: it asks, or it failed (web/src/shared/needs-you.ts).
pub fn needs_you(pane: &Value) -> bool {
    let attention = pane["attention"].as_str();
    let failed = matches!(pane["reason"]["kind"].as_str(), Some("failed" | "exited"));
    attention == Some("needs_input") || (attention == Some("done") && failed)
}

/// A machine's sessions that show in `at`, each with its tabs and their
/// panes; a session with no pane isn't there. Needs you first, then by id.
pub fn sessions_of(panes: &Value, yours: bool, at: Option<&At>) -> Vec<SessionCard> {
    let list = |k: &str| panes[k].as_array().map(Vec::as_slice).unwrap_or_default();
    let pane = |id: u64| list("panes").iter().chain(list("agents")).find(|p| p["id"].as_u64() == Some(id));
    let mut cards: Vec<SessionCard> = list("sessions")
        .iter()
        .filter_map(|s| {
            let project = s["project"].as_object().map(|_| s["project"].clone());
            if !in_shown(project.as_ref().and_then(|p| p["id"].as_str()), yours, at) {
                return None;
            }
            let tabs: Vec<SessionTab> = s["tabs"]
                .as_array()?
                .iter()
                .filter_map(|tid| {
                    let t = list("tabs").iter().find(|t| t["id"] == *tid)?;
                    let ps: Vec<u64> = t["cells"]
                        .as_array()?
                        .iter()
                        .filter_map(|c| c["pane"].as_u64())
                        .filter(|&id| pane(id).is_some())
                        .collect();
                    let (id, name) = (t["id"].as_u64()?, t["name"].as_str().map(String::from));
                    (!ps.is_empty()).then_some(SessionTab { id, name, panes: ps })
                })
                .collect();
            let all: Vec<&Value> = tabs.iter().flat_map(|t| &t.panes).filter_map(|&id| pane(id)).collect();
            if all.is_empty() {
                return None;
            }
            let needs = all.iter().filter(|p| needs_you(p)).count();
            let state = if needs > 0 {
                "needs_input"
            } else if all.iter().any(|p| p["attention"] == "working") {
                "working"
            } else {
                "idle"
            };
            Some(SessionCard {
                id: s["id"].as_u64()?,
                name: s["name"].as_str().unwrap_or_default().to_string(),
                project,
                tabs,
                panes: all.len(),
                needs_you: needs,
                state,
            })
        })
        .collect();
    cards.sort_by_key(|c| (c.state != "needs_input", c.id));
    cards
}

/// The phone's order of machines: online before offline, yours before
/// teammates', the one you used last (`recent`, its id), then by name. A
/// phone is never one of them, so none comes first as "this computer".
fn machine_cmp(a: &MachineSection, b: &MachineSection, recent: Option<&str>) -> Ordering {
    let last = |m: &MachineSection| recent == Some(m.id.as_str());
    b.online
        .cmp(&a.online)
        .then(a.owner.is_some().cmp(&b.owner.is_some()))
        .then(last(b).cmp(&last(a)))
        .then_with(|| a.name.cmp(&b.name))
}

fn section(h: &Host, at: Option<&At>) -> MachineSection {
    MachineSection {
        id: h.id.clone(),
        name: h.name.clone(),
        online: h.online,
        cloud: h.hosted,
        owner: h.owner.clone(),
        sessions: h.panes.as_ref().map(|p| sessions_of(p, h.owner.is_none(), at)).unwrap_or_default(),
    }
}

/// The machines in the phone's order, sessions or not.
pub fn machine_order(hosts: &[Host], recent: Option<&str>) -> Vec<MachineSection> {
    let mut all: Vec<MachineSection> = hosts.iter().map(|h| section(h, None)).collect();
    all.sort_by(|a, b| machine_cmp(a, b, recent));
    all
}

/// The machines with sessions in `at`, in the phone's order, and `quiet`,
/// those with none, by name.
pub fn machine_sections(
    hosts: &[Host],
    at: Option<&At>,
    recent: Option<&str>,
) -> (Vec<MachineSection>, Vec<MachineSection>) {
    let (mut machines, mut quiet): (Vec<_>, Vec<_>) =
        hosts.iter().map(|h| section(h, at)).partition(|m| !m.sessions.is_empty());
    machines.sort_by(|a, b| machine_cmp(a, b, recent));
    quiet.sort_by(|a, b| a.name.cmp(&b.name));
    (machines, quiet)
}

/// The sessions pinned, in pin order (each as its pin), and the machines
/// without them; a machine whose sessions were all pinned is left out.
pub fn pinned_cards(machines: &[MachineSection], pins: &[Pin]) -> (Vec<Pin>, Vec<MachineSection>) {
    let pinned: Vec<Pin> = pins
        .iter()
        .filter(|p| machines.iter().any(|m| m.id == p.machine && m.sessions.iter().any(|s| s.id == p.session)))
        .cloned()
        .collect();
    let rest = machines
        .iter()
        .filter_map(|m| {
            let sessions: Vec<SessionCard> = m
                .sessions
                .iter()
                .filter(|s| !pinned.iter().any(|p| p.machine == m.id && p.session == s.id))
                .cloned()
                .collect();
            (!sessions.is_empty()).then(|| MachineSection { sessions, ..m.clone() })
        })
        .collect();
    (pinned, rest)
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn at(id: &str, personal: bool) -> At {
        At { id: id.into(), personal }
    }

    #[test]
    fn a_session_shows_in_its_own_project_only() {
        let a = at("prj_a", false);
        assert!(in_shown(Some("prj_a"), true, Some(&a)));
        assert!(!in_shown(Some("prj_b"), true, Some(&a)));
        assert!(in_shown(Some("prj_b"), false, None), "everything shows without a project");
    }

    #[test]
    fn a_session_placed_nowhere_is_yours_on_your_machine_in_your_personal_project() {
        let me = at("prj_me", true);
        assert!(in_shown(None, true, Some(&me)));
        assert!(!in_shown(None, false, Some(&me)), "a teammate's unplaced session isn't yours");
        assert!(!in_shown(None, true, Some(&at("prj_team", false))));
    }

    #[test]
    fn a_pane_shows_by_its_session_or_its_card() {
        let a = at("prj_a", false);
        let me = at("prj_me", true);
        assert!(pane_shown(Some("prj_b"), Some("prj_a"), false, Some(&a)), "on this project's card");
        assert!(!pane_shown(Some("prj_b"), None, true, Some(&a)));
        assert!(pane_shown(None, None, true, Some(&me)));
        assert!(!pane_shown(None, None, false, Some(&me)), "a teammate's unplaced pane");
        assert!(!pane_shown(None, Some("prj_a"), true, Some(&me)), "on another project's card");
    }

    /// Two sessions, one in prj_a and one placed nowhere, and a third in
    /// prj_b named as the first: each with a tab of one pane.
    fn panes() -> Value {
        json!({
            "panes": [
                {"id": 1, "tab": 10, "attention": "needs_input"},
                {"id": 2, "tab": 20, "attention": "working"},
                {"id": 3, "tab": 30, "attention": "done", "reason": {"kind": "failed"}},
            ],
            "agents": [{"id": 4, "tab": 10, "attention": "idle"}],
            "tabs": [
                {"id": 10, "name": "main", "cells": [{"pane": 1}, {"pane": 4}, {"pane": 99}]},
                {"id": 20, "name": null, "cells": [{"pane": 2}]},
                {"id": 30, "name": "x", "cells": [{"pane": 3}]},
                {"id": 40, "name": "empty", "cells": []},
            ],
            "sessions": [
                {"id": 1, "name": "work", "project": {"id": "prj_a", "name": "A"}, "tabs": [10, 40]},
                {"id": 2, "name": "loose", "project": null, "tabs": [20]},
                {"id": 3, "name": "work", "project": {"id": "prj_b", "name": "B"}, "tabs": [30]},
            ],
        })
    }

    #[test]
    fn a_machine_keeps_the_projects_sessions_and_counts_only_theirs() {
        let s = sessions_of(&panes(), true, Some(&at("prj_a", false)));
        assert_eq!(s.len(), 1, "another project's session of the same name isn't there");
        assert_eq!(s[0].name, "work");
        assert_eq!(s[0].project, Some(json!({"id": "prj_a", "name": "A"})));
        assert_eq!(s[0].tabs, vec![SessionTab { id: 10, name: Some("main".into()), panes: vec![1, 4] }]);
        assert_eq!((s[0].panes, s[0].needs_you, s[0].state), (2, 1, "needs_input"));

        let ids = |s: Vec<SessionCard>| s.iter().map(|c| c.id).collect::<Vec<_>>();
        assert_eq!(ids(sessions_of(&panes(), true, Some(&at("prj_me", true)))), vec![2]);
        assert_eq!(ids(sessions_of(&panes(), false, Some(&at("prj_me", true)))), Vec::<u64>::new());
        // Without a project, every one, needs you first.
        assert_eq!(ids(sessions_of(&panes(), true, None)), vec![1, 3, 2]);
    }

    fn host(id: &str, online: bool, owner: Option<&str>) -> Host {
        Host {
            id: id.into(),
            name: id.into(),
            online,
            owner: owner.map(Into::into),
            hosted: false,
            panes: Some(panes()),
        }
    }

    #[test]
    fn machines_go_yours_online_first_the_last_used_first_then_teammates_by_name() {
        let hosts = [
            host("zed", true, Some("sam")),
            host("off", false, None),
            host("bee", true, None),
            host("ant", true, Some("kim")),
            host("cat", true, None),
        ];
        let order = |recent| machine_order(&hosts, recent).into_iter().map(|m| m.id).collect::<Vec<_>>();
        assert_eq!(order(None), ["bee", "cat", "ant", "zed", "off"]);
        assert_eq!(order(Some("cat")), ["cat", "bee", "ant", "zed", "off"]);
        // The last used is first of yours only: a teammate's doesn't pass yours.
        assert_eq!(order(Some("zed")), ["bee", "cat", "zed", "ant", "off"]);
    }

    #[test]
    fn sections_leave_a_machine_with_nothing_in_the_project_quiet() {
        let mut empty = host("aaa", true, None);
        empty.panes = None;
        let hosts = [host("zed", true, Some("sam")), host("bee", true, None), empty];
        let (machines, quiet) = machine_sections(&hosts, Some(&at("prj_me", true)), None);
        // On a teammate's machine, the unplaced session isn't in your Default.
        assert_eq!(machines.iter().map(|m| m.id.as_str()).collect::<Vec<_>>(), ["bee"]);
        assert_eq!(quiet.iter().map(|m| m.id.as_str()).collect::<Vec<_>>(), ["aaa", "zed"]);
        assert_eq!(machines[0].owner, None);
        assert_eq!(quiet[1].owner.as_deref(), Some("sam"));
    }

    #[test]
    fn pinned_sessions_come_out_of_their_machines() {
        let (machines, _) = machine_sections(&[host("bee", true, None), host("cat", true, None)], None, None);
        let pins = [
            Pin { machine: "cat".into(), session: 2 },
            Pin { machine: "bee".into(), session: 9 },
            Pin { machine: "bee".into(), session: 1 },
        ];
        let (pinned, rest) = pinned_cards(&machines, &pins);
        assert_eq!(pinned, [pins[0].clone(), pins[2].clone()], "in pin order; a session not there isn't pinned");
        let ids = |m: &MachineSection| m.sessions.iter().map(|s| s.id).collect::<Vec<_>>();
        assert_eq!(rest.iter().map(ids).collect::<Vec<_>>(), [vec![3, 2], vec![1, 3]]);

        let one = [Pin { machine: "bee".into(), session: 1 }];
        let (_, rest) =
            pinned_cards(&machine_sections(&[host("bee", true, None)], Some(&at("prj_a", false)), None).0, &one);
        assert!(rest.is_empty(), "a machine whose sessions were all pinned is left out");
    }
}
