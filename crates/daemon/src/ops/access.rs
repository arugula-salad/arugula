//! `access.get` (Arugula #681): who can reach this machine, and as what, in
//! one answer, composed from what the machine already holds and the owner
//! already reads: the session shares and links in `acl.json` (`GET
//! /api/acl`), the roster it checked itself (`control.rs`), trust live now
//! (the mux), the read-only pane links (`GET /api/shares`) and the ssh
//! invites (`GET /api/guests`). Read-only, the owner's on every way in
//! (`Access::Owner`: the same policy table for the socket, the tailnet and
//! control), and nothing is stored or sent to control for it.
//!
//! The machine's agents' powers are control's word (`GET /api/grants`):
//! the page and `arugula access` ask control for those themselves and label
//! them, so nothing of control's is read as the machine's decision.
//!
//! [`compose`] is the whole of it, from plain pieces, so the tests below
//! read a team, a hosted and a shared-with-a-team machine without one.

use arugula_core::Role;
use arugula_e2e::team::TeamRole;
use arugula_proto::{
    PaneId,
    api::{AccessRow, AccessSession, AccessTeam, AccessView, Empty, GuestInvite, PaneSummary, Share},
    op::ops::AccessGet,
    sharing,
};

use super::{Cx, Handle, OpError};
use crate::{
    acl::{Grant, Principal},
    control::RosterView,
    mux::Api,
};

/// Whose word a row is.
const THIS_MACHINE: &str = "this_machine";
const ROSTER: &str = "roster";

/// What the view is composed from.
pub struct Pieces {
    /// The machine's name (`GET /api/host`'s).
    pub name: String,
    /// What to call the owner.
    pub owner: String,
    /// The enrollment, if joined to control.
    pub roster: Option<RosterView>,
    /// `acl.json`.
    pub grants: Vec<Grant>,
    /// Read-only links to one pane (`/api/shares`).
    pub shares: Vec<Share>,
    /// The ssh invites (`None`: this build has no guest ssh).
    pub guests: Option<Vec<GuestInvite>>,
    /// The Guest SSH access setting.
    pub guest_ssh: bool,
    /// Who is trusted with which pane until when.
    pub trust: Vec<(PaneId, String, u64)>,
    pub panes: Vec<PaneSummary>,
}

impl Handle for AccessGet {
    async fn handle(cx: &Cx<'_>, _: (), _: Empty) -> Result<AccessView, OpError> {
        let app = cx.app;
        let panes = app.mux.api(Api::Panes).await.unwrap_or_default();
        let trust = app.mux.api(Api::Trusted).await.unwrap_or_default();
        let owner =
            app.mux.api(|r| Api::Who(Principal::Owner, r)).await.map(|d| d.name).unwrap_or_else(|| "owner".into());
        let dir = app.control.state_dir();
        Ok(compose(Pieces {
            name: app.hosts.name().to_owned(),
            owner,
            roster: app.control.roster_view(),
            grants: app.acl.list(),
            shares: app.shares.list(),
            guests: crate::lab_hooks::labs().guests(),
            guest_ssh: sharing::guest_ssh(dir),
            trust,
            panes,
        }))
    }
}

fn row(kind: &str, word: &str, who: impl Into<String>, role: Role) -> AccessRow {
    AccessRow {
        kind: kind.into(),
        word: word.into(),
        who: who.into(),
        principal: None,
        role,
        session: None,
        pane: None,
        from_ms: None,
        until_ms: None,
        note: None,
    }
}

/// A member's role on sessions, as `control.rs`'s `Enrolled::build` gives
/// it: an owner is the owner; a hosted machine's editor has full use, as
/// an owner (Jake, 2026-10-21); a locked team lets only owners in.
fn member_role(role: TeamRole, hosted: bool, locked: bool) -> Option<Role> {
    match role {
        TeamRole::Owner => Some(Role::Owner),
        _ if locked => None,
        TeamRole::Editor if hosted => Some(Role::Owner),
        TeamRole::Editor => Some(Role::Editor),
        TeamRole::Viewer => Some(Role::Viewer),
    }
}

/// The view, from the pieces: the owner first, then the roster's people,
/// then what `acl.json` says session by session, then links to a pane, ssh
/// guests and trust, each saying whose word it is.
pub fn compose(p: Pieces) -> AccessView {
    let Pieces { name, owner, roster, grants, shares, guests, guest_ssh, trust, panes } = p;
    let mut sessions: Vec<AccessSession> = Vec::new();
    for s in &panes {
        if !sessions.iter().any(|x| x.id == s.session) {
            sessions.push(AccessSession { id: s.session, name: s.session_name.clone() });
        }
    }
    let session_of = |pane: PaneId| panes.iter().find(|s| s.info.id == pane).map(|s| s.session);
    let mut rows = Vec::new();
    let mut notes = Vec::new();

    let hosted = roster.as_ref().is_some_and(|r| r.hosted);
    let kind = match &roster {
        None => "local",
        Some(r) if r.hosted => "hosted",
        Some(r) if r.team_id.is_some() => "team",
        Some(_) => "personal",
    };
    let team = roster.as_ref().and_then(|r| r.team.as_ref()).map(|t| AccessTeam {
        id: t.id.clone(),
        name: t.name.clone(),
        locked: t.locked,
    });

    // Not here, said plainly: the machines that may call this machine's
    // agents (`agent_callers`, A2A only) and MCP tokens.
    notes.push(
        "Not listed: the machines that may call this machine's agents and nothing else, and MCP tokens (`arugula mcp tokens`)."
            .into(),
    );

    // A deploy's box: the project's people aren't admitted, as
    // `Enrolled::build` decides, nor a session's link or an ssh guest
    // (`Control::admits_links_and_guests`), and control routes nobody to
    // it. What the box holds is listed as held: a session link or a guest
    // says it admits no one there; a tailnet share, a pane link and trust
    // say they admit only the tailnet and local processes, which a hosted
    // box reached through control's relay doesn't have.
    let nobody = roster.as_ref().is_some_and(|r| r.nobody);
    if let Some(r) = roster.as_ref().filter(|r| r.nobody) {
        let what = if r.purpose.as_deref() == Some("both") { "a deploy and a snapshot" } else { "one deploy" };
        notes.push(format!(
            "A box made for {what}: the project's people aren't admitted here, not even its owners, nor is anyone a session was shared with by account or team, nor anyone holding a session's link or an ssh invite, and Arugula cloud routes nobody to it."
        ));
    }
    if roster.as_ref().is_some_and(|r| r.purpose.as_deref() == Some("snapshot")) {
        notes.push(
            "A box building a snapshot: its project's people are admitted here, as on a workspace, and Arugula cloud routes nobody to it."
                .into(),
        );
    }

    // The owner: this machine's, or a hosted machine's project's people.
    match &roster {
        Some(_) if nobody => {}
        Some(r) if r.hosted && r.team.is_none() => {
            // A personal project's box: no roster; the account's devices,
            // pinned at enrollment and checked by this machine.
            let who = if r.login.is_empty() { r.account.clone() } else { r.login.clone() };
            let mut o = row("owner", THIS_MACHINE, who, Role::Owner);
            o.principal = Some(format!("account:{}", r.account));
            o.note = Some("the project's account, pinned when this box enrolled: its own devices".into());
            rows.push(o);
        }
        Some(r) if r.hosted => {}
        Some(r) => {
            // Named as control knows the account, once it said; the
            // `--owner` login, or "owner", until then.
            let who = if r.login.is_empty() { owner.clone() } else { r.login.clone() };
            let mut o = row("owner", THIS_MACHINE, who, Role::Owner);
            o.principal = Some(format!("account:{}", r.account));
            o.note = Some("this machine's owner, joined to Arugula cloud: the account's own devices".into());
            rows.push(o);
        }
        None => {
            let mut o = row("owner", THIS_MACHINE, owner.clone(), Role::Owner);
            o.note = Some("not joined to Arugula cloud: a local user, or the tailnet login that owns it".into());
            rows.push(o);
        }
    }

    // The roster's people: the machine's own team's, checked here. A
    // workspace's notes are for a workspace: a snapshot's or a deploy's
    // box said what it is above.
    if let Some(r) = roster.as_ref().filter(|r| !r.nobody)
        && let Some(t) = &r.team
    {
        if t.locked {
            notes.push(format!("The team {} is locked: only its owners get in.", t.name));
        }
        if hosted && r.purpose.as_deref() == Some("people") {
            notes.push(if t.locked {
                "A Cloud workspace of a locked team: its owners get in, with full use; its editors and viewers don't until it's unlocked."
                    .into()
            } else {
                "A Cloud workspace: its project's people get in by their team role, and an editor has full use of it, as an owner does."
                    .into()
            });
            notes.push(
                "Their own machines drive it as the owner too, while Arugula cloud holds a live cloud.drive power for them: that part is control's word."
                    .into(),
            );
        }
        for m in &t.members {
            if !hosted && m.account == r.account {
                continue;
            }
            let Some(role) = member_role(m.role, hosted, t.locked) else { continue };
            let kind = if m.role == TeamRole::Owner { "team_owner" } else { "team_role" };
            let mut x = row(kind, ROSTER, m.name.clone(), role);
            x.principal = Some(format!("account:{}", m.account));
            x.note = Some(match m.role {
                TeamRole::Owner if hosted => "an owner of the project: full use".into(),
                TeamRole::Owner => "an owner of the team: an owner here, with every owner route".into(),
                TeamRole::Editor if hosted => "an editor of the project: full use, as an owner".into(),
                TeamRole::Editor => {
                    "types by role in every session, no trust needed; no files, new sessions or sharing".into()
                }
                TeamRole::Viewer => "watches every session (not a private pane)".into(),
            });
            rows.push(x);
        }
    }

    // acl.json, session by session: people and teams shared with, and links.
    let mut acl: Vec<&Grant> = grants.iter().collect();
    acl.sort_by_key(|g| (g.session, g.at));
    for g in acl {
        // An account's or a team's share is admitted through `build`, which
        // admits no one on a deploy's box: not listed there.
        if nobody && (g.principal.starts_with("account:") || g.principal.starts_with("team:")) {
            continue;
        }
        if let Some(rest) = g.principal.strip_prefix("team:") {
            let shared = roster.as_ref().and_then(|r| r.shared.get(rest));
            match shared {
                Some(t) => {
                    for m in &t.members {
                        if roster.as_ref().is_some_and(|r| r.account == m.account)
                            || (t.locked && m.role != TeamRole::Owner)
                        {
                            continue;
                        }
                        let in_team = if m.role == TeamRole::Viewer { Role::Viewer } else { Role::Editor };
                        let mut x = row("team_share", THIS_MACHINE, m.name.clone(), g.role.min(in_team));
                        x.principal = Some(format!("account:{}", m.account));
                        x.session = Some(g.session);
                        x.from_ms = g.from.as_ref().map(|_| g.at);
                        x.note = Some(format!(
                            "through the team {}, shared {}: at most their role in it, by the roster",
                            t.name,
                            as_word(g.role)
                        ));
                        rows.push(x);
                    }
                }
                None => {
                    let mut x = row("team_share", THIS_MACHINE, g.name.clone(), g.role);
                    x.principal = Some(g.principal.clone());
                    x.session = Some(g.session);
                    x.from_ms = g.from.as_ref().map(|_| g.at);
                    x.note = Some(
                        "a team whose roster isn't checked here yet: nobody gets in through it until it is".into(),
                    );
                    rows.push(x);
                }
            }
        } else if g.key.is_some() {
            let mut x = row("link", THIS_MACHINE, g.name.clone(), Role::Viewer);
            x.principal = Some(g.principal.clone());
            x.session = Some(g.session);
            x.from_ms = g.from.as_ref().map(|_| g.at);
            x.until_ms = g.expires;
            x.note = Some(if nobody {
                "a read-only link to the session, held: a deploy's box admits no link".into()
            } else {
                "a read-only link to the session".into()
            });
            rows.push(x);
        } else {
            let who = if g.name.is_empty() {
                g.principal.split_once(':').map_or(g.principal.as_str(), |(_, n)| n)
            } else {
                &g.name
            };
            let mut x = row("share", THIS_MACHINE, who, g.role);
            x.principal = Some(g.principal.clone());
            x.session = Some(g.session);
            x.from_ms = g.from.as_ref().map(|_| g.at);
            x.note = Some(match (g.principal.split_once(':').map(|(k, _)| k), g.role) {
                (Some("tailnet"), _) if nobody => {
                    "a tailnet login: on a deploy's box only the tailnet and local processes reach it".into()
                }
                (Some("tailnet"), _) => "a tailnet login: everyone signed in to Tailscale as it".into(),
                (_, Role::Owner) => "an owner of the session, not of the machine".into(),
                _ => "an Arugula account's devices".into(),
            });
            rows.push(x);
        }
    }

    // Read-only links to one pane (`arugula share`).
    for s in &shares {
        let mut x = row("link", THIS_MACHINE, "someone with a link", Role::Viewer);
        x.pane = Some(s.pane);
        x.session = session_of(s.pane);
        x.until_ms = Some(s.expires_ms);
        x.note = Some(if nobody {
            "a read-only link to this pane: on a deploy's box only the tailnet and local processes reach it".into()
        } else {
            "a read-only link to this pane".into()
        });
        rows.push(x);
    }

    // ssh guests, where the setting is on (as `GET /api/guests` lists them).
    if guest_ssh && let Some(guests) = &guests {
        for g in guests {
            let who = if g.label.is_empty() { "guest" } else { g.label.as_str() };
            let mut x = row("guest", THIS_MACHINE, who, if g.rw { Role::Editor } else { Role::Viewer });
            x.pane = Some(g.pane);
            x.session = session_of(g.pane);
            x.until_ms = Some(g.expires_ms);
            let mut parts = vec![if g.rw { "over ssh, may type" } else { "over ssh, watches" }.to_owned()];
            if nobody {
                parts.push("held: a deploy's box admits no guest".into());
            }
            parts.push(if g.reusable {
                "any number of logins".into()
            } else if g.used {
                "its one login used".into()
            } else {
                "one login".into()
            });
            if g.sessions > 0 {
                parts.push(format!("{} connected now", g.sessions));
            }
            x.note = Some(parts.join("; "));
            rows.push(x);
        }
    }

    // Trust: minutes to type into one pane here.
    for (pane, id, until) in &trust {
        let name = grants
            .iter()
            .find(|g| &g.principal == id && !g.name.is_empty())
            .map(|g| g.name.clone())
            .unwrap_or_else(|| id.split_once(':').map_or(id.clone(), |(_, n)| n.to_owned()));
        let mut x = row("trust", THIS_MACHINE, name, Role::Editor);
        x.principal = Some(id.clone());
        x.pane = Some(*pane);
        x.session = session_of(*pane);
        x.until_ms = Some(*until);
        x.note = Some("trusted to type into this pane for now".into());
        rows.push(x);
    }

    AccessView {
        kind: kind.into(),
        purpose: roster.as_ref().and_then(|r| r.purpose.clone()),
        name,
        machine: roster.as_ref().map(|r| r.device.clone()),
        control: roster.as_ref().map(|r| r.url.clone()),
        team,
        guest_ssh: guests.as_ref().map(|_| guest_ssh),
        sessions,
        rows,
        notes,
    }
}

fn as_word(role: Role) -> &'static str {
    match role {
        Role::Owner => "as an owner",
        Role::Editor => "as an editor",
        Role::Viewer => "as a viewer",
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::*;
    use crate::control::{RosterMember, RosterTeam};

    fn member(account: &str, name: &str, role: TeamRole) -> RosterMember {
        RosterMember { account: account.into(), name: name.into(), role }
    }

    fn acme(locked: bool) -> RosterTeam {
        RosterTeam {
            id: "t1".into(),
            name: "Acme".into(),
            locked,
            members: vec![
                member("a1", "alex", TeamRole::Owner),
                member("b1", "bea", TeamRole::Editor),
                member("c1", "cy", TeamRole::Viewer),
                member("o2", "ola", TeamRole::Owner),
            ],
        }
    }

    fn pieces(roster: Option<RosterView>) -> Pieces {
        Pieces {
            name: "box".into(),
            owner: "alex".into(),
            roster,
            grants: vec![],
            shares: vec![],
            guests: None,
            guest_ssh: false,
            trust: vec![],
            panes: vec![],
        }
    }

    fn enrolled(team: Option<RosterTeam>, hosted: bool) -> RosterView {
        RosterView {
            hosted,
            purpose: hosted.then(|| "people".to_owned()),
            nobody: false,
            team_id: team.as_ref().map(|t| t.id.clone()),
            account: "a1".into(),
            login: "alex".into(),
            device: "dev1".into(),
            url: "https://control.example".into(),
            team,
            shared: BTreeMap::new(),
        }
    }

    fn grant(session: u32, principal: &str, name: &str, role: Role) -> Grant {
        Grant {
            session,
            principal: principal.into(),
            name: name.into(),
            role,
            by: "owner".into(),
            at: 5,
            from: None,
            root: None,
            key: None,
            expires: None,
        }
    }

    fn by_kind<'a>(v: &'a AccessView, kind: &str) -> Vec<&'a AccessRow> {
        v.rows.iter().filter(|r| r.kind == kind).collect()
    }

    /// A machine joined to no control: its owner alone, the machine's word.
    #[test]
    fn a_local_machine_has_its_owner_and_what_it_shared() {
        let mut p = pieces(None);
        p.grants = vec![grant(3, "tailnet:friend@example.com", "friend@example.com", Role::Editor)];
        p.trust = vec![(7, "tailnet:friend@example.com".into(), 99_999)];
        let v = compose(p);
        assert_eq!((v.kind.as_str(), v.machine, v.team, v.guest_ssh), ("local", None, None, None));
        let kinds: Vec<&str> = v.rows.iter().map(|r| r.kind.as_str()).collect();
        assert_eq!(kinds, ["owner", "share", "trust"]);
        assert!(v.rows.iter().all(|r| r.word == THIS_MACHINE), "{:?}", v.rows);
        assert_eq!((v.rows[0].who.as_str(), v.rows[0].role), ("alex", Role::Owner));
        assert_eq!((v.rows[1].session, v.rows[1].role), (Some(3), Role::Editor));
        assert_eq!(
            (v.rows[2].pane, v.rows[2].until_ms, v.rows[2].who.as_str()),
            (Some(7), Some(99_999), "friend@example.com")
        );
    }

    /// A team machine: the roster's other owners are owners here, an
    /// editor types by role, a viewer watches; the own account is the
    /// owner row, not a member row; every one of them is the roster's word.
    #[test]
    fn a_team_machine_lists_the_roster_by_role() {
        let v = compose(pieces(Some(enrolled(Some(acme(false)), false))));
        assert_eq!(v.kind, "team");
        assert_eq!((v.machine.as_deref(), v.control.as_deref()), (Some("dev1"), Some("https://control.example")));
        assert_eq!(v.team.as_ref().map(|t| (t.name.as_str(), t.locked)), Some(("Acme", false)));
        let people: Vec<(&str, &str, Role, &str)> =
            v.rows.iter().map(|r| (r.kind.as_str(), r.who.as_str(), r.role, r.word.as_str())).collect();
        assert_eq!(
            people,
            [
                ("owner", "alex", Role::Owner, THIS_MACHINE),
                ("team_role", "bea", Role::Editor, ROSTER),
                ("team_role", "cy", Role::Viewer, ROSTER),
                ("team_owner", "ola", Role::Owner, ROSTER),
            ]
        );
        assert!(v.rows[1].session.is_none() && v.rows[1].pane.is_none(), "every session");
        // Only what every view says: what isn't listed.
        assert_eq!(v.notes.len(), 1, "{:?}", v.notes);
        assert!(v.notes[0].starts_with("Not listed:"), "{:?}", v.notes);
    }

    /// Locked: only the owners get in, and the view says so.
    #[test]
    fn a_locked_team_keeps_only_its_owners() {
        let v = compose(pieces(Some(enrolled(Some(acme(true)), false))));
        let who: Vec<&str> = v.rows.iter().map(|r| r.who.as_str()).collect();
        assert_eq!(who, ["alex", "ola"]);
        assert!(v.team.as_ref().is_some_and(|t| t.locked));
        assert!(v.notes.contains(&"The team Acme is locked: only its owners get in.".to_owned()), "{:?}", v.notes);
    }

    /// A hosted machine of a team's project: its people by role, an editor
    /// with full use, as an owner (Jake, 2026-10-21), and no owner row of
    /// the machine's own.
    #[test]
    fn a_hosted_machines_editors_have_full_use() {
        let v = compose(pieces(Some(enrolled(Some(acme(false)), true))));
        assert_eq!(v.kind, "hosted");
        let people: Vec<(&str, &str, Role)> =
            v.rows.iter().map(|r| (r.kind.as_str(), r.who.as_str(), r.role)).collect();
        assert_eq!(
            people,
            [
                ("team_owner", "alex", Role::Owner),
                ("team_role", "bea", Role::Owner),
                ("team_role", "cy", Role::Viewer),
                ("team_owner", "ola", Role::Owner),
            ]
        );
        assert_eq!(v.rows[1].note.as_deref(), Some("an editor of the project: full use, as an owner"));
        assert!(v.notes.iter().any(|n| n.contains("an editor has full use")), "{:?}", v.notes);
        assert!(v.notes.iter().any(|n| n.contains("control's word")), "{:?}", v.notes);
        // Locked: the editor is out too, and the notes don't say they have full use.
        let v = compose(pieces(Some(enrolled(Some(acme(true)), true))));
        let who: Vec<&str> = v.rows.iter().map(|r| r.who.as_str()).collect();
        assert_eq!(who, ["alex", "ola"]);
        assert!(v.notes.iter().any(|n| n.contains("locked team: its owners get in")), "{:?}", v.notes);
        assert!(!v.notes.iter().any(|n| n.contains("an editor has full use")), "{:?}", v.notes);
    }

    /// A snapshot's box admits its project's people as a workspace does
    /// (`Enrolled::build` closes only on a deploy); the view says that
    /// control routes nobody to it, and nothing of a workspace's.
    #[test]
    fn a_snapshots_box_admits_its_people_and_says_control_routes_nobody() {
        let mut r = enrolled(Some(acme(false)), true);
        r.purpose = Some("snapshot".into());
        let v = compose(pieces(Some(r)));
        assert_eq!(v.purpose.as_deref(), Some("snapshot"));
        let who: Vec<&str> = v.rows.iter().map(|r| r.who.as_str()).collect();
        assert_eq!(who, ["alex", "bea", "cy", "ola"]);
        let said = |what: &str| v.notes.iter().any(|n| n.contains(what));
        assert!(said("admitted here") && said("routes nobody"), "{:?}", v.notes);
        assert!(!said("A Cloud workspace") && !said("Their own machines"), "{:?}", v.notes);
    }

    /// A hosted machine of a personal project: the account alone, and no
    /// roster to be the word of it: this machine's.
    #[test]
    fn a_personal_projects_hosted_machine_is_its_accounts() {
        let v = compose(pieces(Some(enrolled(None, true))));
        assert_eq!(v.kind, "hosted");
        assert_eq!(v.rows.len(), 1);
        assert_eq!((v.rows[0].kind.as_str(), v.rows[0].who.as_str(), v.rows[0].role), ("owner", "alex", Role::Owner));
        assert_eq!((v.rows[0].principal.as_deref(), v.rows[0].word.as_str()), (Some("account:a1"), THIS_MACHINE));
    }

    /// A box for one deploy: none of the project's people, and a sentence
    /// saying so; what the box holds of its own is still listed, since
    /// `Enrolled::build` isn't what checks a share, a link or a guest.
    #[test]
    fn a_deploys_box_admits_no_person_but_lists_what_it_holds() {
        let mut r = enrolled(Some(acme(false)), true);
        r.purpose = Some("deploy".into());
        r.nobody = true;
        let mut p = pieces(Some(r));
        p.grants = vec![
            grant(3, "tailnet:friend@example.com", "friend@example.com", Role::Editor),
            grant(3, "account:z1", "zed", Role::Viewer),
            grant(3, "team:t9", "Nine", Role::Viewer),
            Grant {
                key: Some("ab".repeat(32)),
                expires: Some(9_000),
                ..grant(3, "link:abc", "someone with a link", Role::Viewer)
            },
        ];
        p.shares = vec![Share { id: 1, pane: 9, created_ms: 1, expires_ms: 8_000, token: None, path: None, url: None }];
        p.trust = vec![(9, "tailnet:friend@example.com".into(), 99_999)];
        let v = compose(p);
        // The account's and the team's shares go through `build`, which
        // admits no one here: not listed. A session's link is held and
        // admits no one, and says so. The rest is checked elsewhere.
        let kinds: Vec<&str> = v.rows.iter().map(|r| r.kind.as_str()).collect();
        assert_eq!(kinds, ["share", "link", "link", "trust"]);
        let tailnet_only = "only the tailnet and local processes reach it";
        assert!(v.rows[0].note.as_deref().unwrap().contains(tailnet_only), "the tailnet share: {:?}", v.rows[0]);
        let pane_link = v.rows.iter().find(|r| r.kind == "link" && r.pane.is_some()).unwrap();
        assert!(pane_link.note.as_deref().unwrap().contains(tailnet_only), "the pane link: {pane_link:?}");
        let session_link = v.rows.iter().find(|r| r.principal.as_deref() == Some("link:abc")).unwrap();
        assert_eq!(
            session_link.note.as_deref(),
            Some("a read-only link to the session, held: a deploy's box admits no link")
        );
        assert!(v.rows.iter().all(|r| r.principal.as_deref() != Some("account:z1")), "{:?}", v.rows);
        assert_eq!(v.purpose.as_deref(), Some("deploy"));
        let said = |what: &str| v.notes.iter().any(|n| n.contains(what));
        assert!(
            said("A box made for one deploy")
                && said("aren't admitted here")
                && said("a session's link")
                && said("routes nobody"),
            "{:?}",
            v.notes
        );
        assert!(!said("reach no one") && !said("A Cloud workspace"), "{:?}", v.notes);
    }

    /// Nothing secret: a link's key, a pane link's token and an invite's
    /// token never reach the answer, whatever the pieces carry.
    #[test]
    fn no_key_or_token_is_in_the_answer() {
        let mut p = pieces(None);
        let mut link = grant(3, "link:abc", "someone with a link", Role::Viewer);
        link.key = Some("0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef".into());
        link.expires = Some(7_000);
        p.grants = vec![link];
        p.shares = vec![Share {
            id: 1,
            pane: 9,
            created_ms: 1,
            expires_ms: 8_000,
            token: Some("ils_secret_token".into()),
            path: Some("/share/ils_secret_token".into()),
            url: Some("https://x/share/ils_secret_token".into()),
        }];
        p.guest_ssh = true;
        p.guests = Some(vec![GuestInvite {
            token: Some("ilg_secret".into()),
            command: Some("ssh -p 1 ilg_secret@h".into()),
            known_hosts: Some("h ssh-ed25519 KEY".into()),
            fingerprint: Some("SHA256:fp".into()),
            ..serde_json::from_value(serde_json::json!({ "id": 1, "pane": 9, "expires_ms": 9000 })).unwrap()
        }]);
        let text = serde_json::to_string(&compose(p)).unwrap();
        for secret in ["0123456789abcdef", "ils_secret", "ilg_secret", "KEY", "SHA256", "\"key\"", "\"token\""] {
            assert!(!text.contains(secret), "{secret} in {text}");
        }
    }

    /// A session shared with a team (M30): each member at most their role
    /// in it, from the roster checked here; the own account isn't listed;
    /// a team whose roster hasn't checked out is one row that says so.
    #[test]
    fn a_session_shared_with_a_team_is_its_members_capped() {
        let mut r = enrolled(None, false);
        r.shared.insert("t1".into(), acme(false));
        let mut p = pieces(Some(r));
        p.grants = vec![
            grant(2, "team:t1", "Acme", Role::Editor),
            grant(4, "team:t9", "Nine", Role::Viewer),
            grant(2, "account:z1", "zed", Role::Owner),
        ];
        let v = compose(p);
        let shared: Vec<(&str, Role, Option<u32>)> =
            by_kind(&v, "team_share").iter().map(|r| (r.who.as_str(), r.role, r.session)).collect();
        assert_eq!(
            shared,
            [
                ("bea", Role::Editor, Some(2)),
                ("cy", Role::Viewer, Some(2)),
                ("ola", Role::Editor, Some(2)),
                ("Nine", Role::Viewer, Some(4))
            ]
        );
        assert!(by_kind(&v, "team_share")[3].note.as_deref().unwrap().contains("isn't checked here yet"));
        let zed = by_kind(&v, "share")[0];
        assert_eq!(
            (zed.who.as_str(), zed.role, zed.note.as_deref()),
            ("zed", Role::Owner, Some("an owner of the session, not of the machine"))
        );
    }

    /// Links and guests: a session link from `acl.json` with its end, a
    /// pane link from `/api/shares` with its session, and ssh invites only
    /// where the setting is on, as `GET /api/guests` answers.
    #[test]
    fn links_and_guests_say_where_and_until_when() {
        let mut p = pieces(None);
        let mut link = grant(3, "link:abc", "someone with a link", Role::Viewer);
        link.key = Some("k".into());
        link.expires = Some(7_000);
        link.from = Some(BTreeMap::new());
        p.grants = vec![link];
        p.shares = vec![Share { id: 1, pane: 9, created_ms: 1, expires_ms: 8_000, token: None, path: None, url: None }];
        let pane = |id: u32, session: u32| -> PaneSummary {
            serde_json::from_value(serde_json::json!({
                "session": session, "session_name": format!("s{session}"), "tab": 1, "tab_name": null,
                "id": id, "epoch": 1, "cwd": null, "command": null, "running": true, "policy": { "kind": "shell" },
            }))
            .unwrap()
        };
        p.panes = vec![pane(9, 3), pane(10, 4)];
        let invite = GuestInvite {
            id: 1,
            pane: 10,
            rw: true,
            reusable: false,
            label: "sam".into(),
            created_ms: 1,
            expires_ms: 9_000,
            used: false,
            sessions: 1,
            token: None,
            command: None,
            known_hosts: None,
            fingerprint: None,
            host: None,
            port: None,
            relay: false,
            jump: None,
        };
        p.guests = Some(vec![invite]);
        // The setting is off: the invite isn't listed, as `/api/guests` refuses.
        let v = compose(p);
        assert_eq!(v.guest_ssh, Some(false));
        assert!(by_kind(&v, "guest").is_empty());
        let links: Vec<[Option<u64>; 4]> = by_kind(&v, "link")
            .iter()
            .map(|r| [r.session.map(u64::from), r.pane.map(u64::from), r.from_ms, r.until_ms])
            .collect();
        assert_eq!(links, [[Some(3), None, Some(5), Some(7_000)], [Some(3), Some(9), None, Some(8_000)]]);
        assert_eq!(v.sessions.iter().map(|s| (s.id, s.name.as_str())).collect::<Vec<_>>(), [(3, "s3"), (4, "s4")]);

        let mut p = pieces(None);
        p.guest_ssh = true;
        p.guests = Some(vec![GuestInvite {
            reusable: true,
            sessions: 2,
            ..serde_json::from_value(
                serde_json::json!({ "id": 1, "pane": 10, "rw": false, "label": "", "expires_ms": 9000 }),
            )
            .unwrap()
        }]);
        let v = compose(p);
        assert_eq!(v.guest_ssh, Some(true));
        let g = by_kind(&v, "guest")[0];
        assert_eq!((g.who.as_str(), g.role, g.pane, g.until_ms), ("guest", Role::Viewer, Some(10), Some(9_000)));
        assert_eq!(g.note.as_deref(), Some("over ssh, watches; any number of logins; 2 connected now"));
    }
}
