//! Principals and their grants (M12).
//!
//! Every request has an author. The **owner** is the daemon's: a local
//! process, the tailnet login that owns it, or a device of the control
//! account it joined. Anyone else is a **user**, known by a principal id
//! (`tailnet:alice@example.com`, or `account:<id>` from control) and
//! reaching only the sessions granted to them, with the role granted.
//!
//! Grants are data: `<state>/acl.json`, written atomically, and every
//! change is appended to `<state>/audit.jsonl` (who, what, when).
//!
//! On a team's machine its members watch the sessions in that team's
//! projects (Arugula #1162): a member's team role, the roster's or a machine
//! token's, counts only on such a session, and as a viewer's at most.
//! Sessions in a person's own project, another team's or none, and any whose
//! project's team isn't known yet, are the owner's and whoever they're
//! shared with: a share with a person stays wherever the session goes, and
//! a share with a team counts only while the session is in its project.
//! A cloud workspace is its project's, and a box joined for a team is the
//! team's (Arugula #1182): there the team role counts on every session.
//!
//! A team owner is the owner (`Principal::Owner`) only of a machine that's
//! the team's. On a person's machine in a team they're a teammate (Arugula
//! #1180): their owner's role is their team role, which watches as anyone's
//! does. Managing the machine (its tokens, removing it, whose box it is)
//! stays theirs in control. The daemon decides which they are
//! (`control.rs`'s `Enrolled::owner_role_owns`).

use std::{
    collections::BTreeMap,
    io::Write,
    path::{Path, PathBuf},
    sync::{RwLock, RwLockWriteGuard},
};

use arugula_core::{PaneId, Role, SessionId};
use arugula_proto::api::NotifyPref;
use serde::{Deserialize, Serialize};
use tracing::warn;

use crate::store::{ReplaceError, now_ms, replace_atomic, write_atomic};

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Principal {
    Owner,
    /// `id` as in grants; `name` to show (a login); a picture, if the
    /// tailnet gave one.
    User {
        id: String,
        name: String,
        pic: Option<String>,
    },
}

impl Principal {
    pub fn tailnet(login: &str) -> Self {
        Principal::User { id: format!("tailnet:{login}"), name: login.to_owned(), pic: None }
    }

    pub fn with_pic(self, pic: Option<String>) -> Self {
        match self {
            Principal::User { id, name, .. } => Principal::User { id, name, pic },
            p => p,
        }
    }

    pub fn id(&self) -> &str {
        match self {
            Principal::Owner => "owner",
            Principal::User { id, .. } => id,
        }
    }

    pub fn is_owner(&self) -> bool {
        matches!(self, Principal::Owner)
    }
}

/// What a remote channel holds for its principal while it's open
/// (aru-141): [`Acl::hold`], then [`Acl::release`] with the same.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Hold {
    /// A role on every session in place of the team roster's, when
    /// `replaces`: a machine token's (none for a guest's).
    pub role: Option<Role>,
    pub replaces: bool,
    /// It opened with remote input off: while it's open its principal is
    /// a viewer at most, on every session.
    pub viewer_only: bool,
    /// Its role counts on every session here, not only on the team's
    /// projects' (Arugula #1210): the machine's owner, with remote input
    /// off, watching their own machine.
    pub everywhere: bool,
}

impl Hold {
    /// A machine token's channel: its role in place of the roster's.
    pub fn token(role: Option<Role>, viewer_only: bool) -> Self {
        Self { role, replaces: true, viewer_only, everywhere: false }
    }

    /// The machine's owner through control with remote input off (Arugula
    /// #1210): a viewer of every session, typing into none.
    pub fn owner_watching() -> Self {
        Self { role: Some(Role::Viewer), replaces: true, viewer_only: true, everywhere: true }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Grant {
    pub session: SessionId,
    pub principal: String,
    pub name: String,
    pub role: Role,
    pub by: String,
    pub at: u64,
    /// A "from now" share (M13): each pane's stream offset when it was
    /// granted. Output before it is never sent: no history, no scrollback.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub from: Option<BTreeMap<PaneId, u64>>,
    /// For an `account:` principal (M19): the root device the owner saw
    /// for them when sharing; their devices must chain back to it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub root: Option<String>,
    /// A read-only link (M19): its X25519 public key (hex), and when it
    /// stops working (ms).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub key: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expires: Option<u64>,
}

#[derive(Default, Serialize, Deserialize)]
struct File {
    grants: Vec<Grant>,
}

/// The grants in memory after a write: `before` if the file wasn't replaced,
/// as they are if it was. Whether it was.
fn settle(written: &Result<(), ReplaceError>, g: &mut Vec<Grant>, before: Vec<Grant>) -> bool {
    if let Err(ReplaceError::NotReplaced(_)) = written {
        *g = before;
        return false;
    }
    true
}

#[derive(Debug)]
pub struct Acl {
    path: PathBuf,
    audit: PathBuf,
    grants: RwLock<Vec<Grant>>,
    /// A team daemon's members' roles (M19), from the team's signed
    /// roster: on the sessions in the team's projects.
    team: RwLock<std::collections::HashMap<String, Role>>,
    /// The team this machine is in (Arugula #1162), when it's a team's.
    machine_team: RwLock<Option<String>>,
    /// It's a cloud workspace (its project's) or a box joined for a team
    /// (the team's, Arugula #1182): the team's role counts on every session.
    hosted: RwLock<bool>,
    /// The team whose project each session is in, for the sessions in a
    /// team's project control named (Arugula #1162).
    session_teams: RwLock<std::collections::HashMap<SessionId, String>>,
    /// Members of teams sessions were shared with (M30), by team, each with
    /// their role in it, from rosters this daemon verified.
    shared_teams: RwLock<std::collections::HashMap<String, std::collections::HashMap<String, Role>>>,
    /// What the remote channels open now hold for their principals
    /// (aru-141), by principal id: one [`Hold`] per channel.
    held: RwLock<std::collections::HashMap<String, Vec<Hold>>>,
    /// Who wants "needs you" notifications about what (M29), by principal
    /// id; in `notify.json`.
    notify_path: PathBuf,
    notify: RwLock<BTreeMap<String, NotifyPref>>,
}

impl Acl {
    pub fn open(state_dir: &Path) -> Self {
        let path = state_dir.join("acl.json");
        let grants = match std::fs::read(&path) {
            Ok(b) => serde_json::from_slice::<File>(&b).map(|f| f.grants).unwrap_or_else(|e| {
                warn!(error = %e, "acl.json is unreadable; no one but the owner gets in");
                Vec::new()
            }),
            Err(_) => Vec::new(),
        };
        let notify_path = state_dir.join("notify.json");
        let notify = std::fs::read(&notify_path).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default();
        Self {
            path,
            audit: state_dir.join("audit.jsonl"),
            grants: RwLock::new(grants),
            team: Default::default(),
            machine_team: Default::default(),
            hosted: Default::default(),
            session_teams: Default::default(),
            shared_teams: Default::default(),
            held: Default::default(),
            notify_path,
            notify: RwLock::new(notify),
        }
    }

    /// Someone's role on a session, by principal id (`owner`, or a grant's).
    pub fn role_of(&self, id: &str, session: SessionId) -> Option<Role> {
        if id == "owner" {
            return Some(Role::Owner);
        }
        self.role(&Principal::User { id: id.to_owned(), name: String::new(), pic: None }, session)
    }

    /// Whether `id` is told when an agent in `session` needs someone (M29):
    /// the owner always; anyone else who may edit it and opted in.
    pub fn notifies(&self, id: &str, session: Option<SessionId>) -> bool {
        if id == "owner" {
            return true;
        }
        let Some(session) = session else { return false };
        if self.role_of(id, session).is_none_or(|r| r < Role::Editor) {
            return false;
        }
        self.notify.read().unwrap().get(id).is_some_and(|p| p.all || p.sessions.contains(&session))
    }

    pub fn notify_pref(&self, id: &str) -> NotifyPref {
        self.notify.read().unwrap().get(id).cloned().unwrap_or_default()
    }

    /// Opt in or out: one session, or all of them (`session: None`).
    pub fn set_notify(&self, id: &str, session: Option<SessionId>, on: bool) -> std::io::Result<NotifyPref> {
        let mut n = self.notify.write().unwrap();
        let p = n.entry(id.to_owned()).or_default();
        match session {
            None => p.all = on,
            Some(s) if on => {
                p.sessions.insert(s);
            }
            Some(s) => {
                p.sessions.remove(&s);
            }
        }
        let out = p.clone();
        write_atomic(&self.notify_path, &serde_json::to_vec_pretty(&*n).map_err(std::io::Error::other)?)?;
        Ok(out)
    }

    /// Someone's role on a session: a grant, else their team role, on a
    /// session in the machine's team's project.
    pub fn role(&self, p: &Principal, session: SessionId) -> Option<Role> {
        match p {
            Principal::Owner => Some(Role::Owner),
            Principal::User { id, .. } => {
                let granted = self
                    .grants
                    .read()
                    .unwrap()
                    .iter()
                    .find(|g| g.session == session && &g.principal == id)
                    .map(|g| g.role);
                let project_team = self.session_team(session);
                let team = if *self.hosted.read().unwrap() || self.everywhere(id) {
                    self.team_role_of(id)
                } else {
                    self.team_role_of(id).filter(|_| self.team_watches(session)).map(|r| r.min(Role::Viewer))
                };
                // Shared with a team they're in, while the session is in
                // that team's project: the grant's role, at most their role
                // in the team.
                let teams = self.shared_teams.read().unwrap();
                let via_team = self
                    .grants
                    .read()
                    .unwrap()
                    .iter()
                    .filter(|g| g.session == session)
                    .filter_map(|g| {
                        let t = g.principal.strip_prefix("team:")?;
                        if project_team.as_deref() != Some(t) {
                            return None;
                        }
                        let in_team = teams.get(t)?.get(id)?;
                        Some(g.role.min(*in_team))
                    })
                    .max();
                let role = granted.max(team).max(via_team);
                // Remote input off (aru-141): nothing raises them past a
                // viewer, not a grant on this session, not a team's.
                if self.viewer_only(id) { role.min(Some(Role::Viewer)) } else { role }
            }
        }
    }

    /// A team daemon's members' roles (from its verified roster).
    pub fn set_team_roles(&self, roles: std::collections::HashMap<String, Role>) {
        *self.team.write().unwrap() = roles;
    }

    /// Whether this machine's team gets `session` by its members' roles: a
    /// cloud workspace's every session, else one in the team's project
    /// (Arugula #1162), as a viewer's at most.
    pub fn team_watches(&self, session: SessionId) -> bool {
        if *self.hosted.read().unwrap() {
            return true;
        }
        let team = self.machine_team.read().unwrap();
        team.is_some() && self.session_teams.read().unwrap().get(&session) == team.as_ref()
    }

    /// The team whose project `session` is in, as control named it.
    pub fn session_team(&self, session: SessionId) -> Option<String> {
        self.session_teams.read().unwrap().get(&session).cloned()
    }

    /// The team this machine is in, if any (Arugula #1162), and whether
    /// it's the team's own (a cloud workspace, or a box joined for it).
    pub fn set_machine_team(&self, team: Option<String>, hosted: bool) {
        *self.machine_team.write().unwrap() = team;
        *self.hosted.write().unwrap() = hosted;
    }

    /// The team whose project each session is in (Arugula #1162): the
    /// sessions in a team's project, as control named it. Whether that
    /// changed.
    pub fn set_session_teams(&self, teams: impl IntoIterator<Item = (SessionId, String)>) -> bool {
        let teams: std::collections::HashMap<SessionId, String> = teams.into_iter().collect();
        let mut now = self.session_teams.write().unwrap();
        let changed = *now != teams;
        *now = teams;
        changed
    }

    /// Members of the teams sessions were shared with (M30).
    pub fn set_shared_teams(&self, teams: std::collections::HashMap<String, std::collections::HashMap<String, Role>>) {
        *self.shared_teams.write().unwrap() = teams;
    }

    pub fn team_role(&self, p: &Principal) -> Option<Role> {
        self.team_role_of(p.id())
    }

    /// `id`'s role in the machine's team, which counts on the sessions in
    /// the team's projects ([`Acl::role`]): its machine token's, while a channel
    /// opened with one is open (control's word for its role now, aru-141,
    /// Arugula #1005), else the team roster's; at most a viewer's while a
    /// channel that opened with remote input off is.
    fn team_role_of(&self, id: &str) -> Option<Role> {
        let held = self.held.read().unwrap();
        let holds = held.get(id).map(Vec::as_slice).unwrap_or_default();
        let role = if holds.iter().any(|h| h.replaces) {
            holds.iter().filter(|h| h.replaces).filter_map(|h| h.role).max()
        } else {
            self.team.read().unwrap().get(id).copied()
        };
        if holds.iter().any(|h| h.viewer_only) { role.min(Some(Role::Viewer)) } else { role }
    }

    /// Whether a channel of `id`'s holds a role on every session (the
    /// owner watching, Arugula #1210).
    fn everywhere(&self, id: &str) -> bool {
        self.held.read().unwrap().get(id).is_some_and(|h| h.iter().any(|h| h.everywhere))
    }

    /// Whether a channel of `id`'s that opened with remote input off is
    /// open: nothing makes them more than a viewer here then.
    fn viewer_only(&self, id: &str) -> bool {
        self.held.read().unwrap().get(id).is_some_and(|h| h.iter().any(|h| h.viewer_only))
    }

    /// A remote channel opened for `id` holds `hold` until
    /// [`Acl::release`]: a machine token's role on every session in place
    /// of the team roster's, the highest of its channels' (none, for a
    /// guest's), and with remote input off, a viewer's at most, whatever
    /// grants or rosters say.
    pub fn hold(&self, id: &str, hold: Hold) {
        self.held.write().unwrap().entry(id.to_owned()).or_default().push(hold);
    }

    /// The channel that held `hold` for `id` closed.
    pub fn release(&self, id: &str, hold: Hold) {
        let mut held = self.held.write().unwrap();
        if let Some(h) = held.get_mut(id) {
            if let Some(i) = h.iter().position(|x| *x == hold) {
                h.remove(i);
            }
            if h.is_empty() {
                held.remove(id);
            }
        }
    }

    /// Until when read-only links may reach this daemon (M19).
    pub fn links_until(&self) -> Option<u64> {
        let now = now_ms();
        self.grants.read().unwrap().iter().filter_map(|g| g.key.as_ref().and(g.expires)).filter(|e| *e > now).max()
    }

    /// The read-only link whose key this is, while it works.
    pub fn link_by_key(&self, key_hex: &str) -> Option<Grant> {
        let now = now_ms();
        self.grants
            .read()
            .unwrap()
            .iter()
            .find(|g| g.key.as_deref() == Some(key_hex) && g.expires.is_some_and(|e| e > now))
            .cloned()
    }

    /// A read-only link to `session` for `key` until `expires`, from now on
    /// unless `from` says otherwise.
    pub fn add_link(
        &self,
        session: SessionId,
        key: &str,
        expires: u64,
        from: Option<BTreeMap<PaneId, u64>>,
    ) -> std::io::Result<String> {
        let id = format!("link:{}", &hex_sha(key)[..12]);
        let mut g = self.grants.write().unwrap();
        let before = g.clone();
        g.retain(|x| x.principal != id);
        g.push(Grant {
            session,
            principal: id.clone(),
            name: "someone with a link".into(),
            role: Role::Viewer,
            by: "owner".into(),
            at: now_ms(),
            from,
            root: None,
            key: Some(key.to_owned()),
            expires: Some(expires),
        });
        let entry = serde_json::json!({ "at": now_ms(), "by": "owner", "action": "link", "session": session, "principal": id, "expires": expires });
        self.commit(g, before, entry)?;
        Ok(id)
    }

    /// The read-only links that still work, oldest first.
    pub fn links(&self) -> Vec<Grant> {
        let now = now_ms();
        self.grants
            .read()
            .unwrap()
            .iter()
            .filter(|g| g.key.is_some() && g.expires.is_some_and(|e| e > now))
            .cloned()
            .collect()
    }

    /// End the read-only link with this principal now, as `by` (false: no
    /// such link). Anything else's grant isn't a link's, and stays.
    pub fn revoke_link(&self, principal: &str, by: &str) -> std::io::Result<bool> {
        let mut g = self.grants.write().unwrap();
        let Some(gone) = g.iter().find(|x| x.principal == principal && x.key.is_some()).cloned() else {
            return Ok(false);
        };
        let before = g.clone();
        g.retain(|x| x.principal != principal);
        let entry = serde_json::json!({
            "at": now_ms(), "by": by, "action": "revoke",
            "session": gone.session, "principal": principal, "name": gone.name, "role": null,
        });
        self.commit(g, before, entry)?;
        Ok(true)
    }

    /// Write the grants as `g` has them, with its lock held: every change
    /// and its write are one step, or one change's older copy could land on
    /// top of another's. The grants in memory end up as the file has them,
    /// and the log says what the file says. An error may mean it happened
    /// all the same (the file changed, and that didn't reach the disk for
    /// certain), so whoever gets one still tells the mux and control.
    fn commit(
        &self,
        mut g: RwLockWriteGuard<'_, Vec<Grant>>,
        before: Vec<Grant>,
        entry: serde_json::Value,
    ) -> std::io::Result<()> {
        let written = replace_atomic(&self.path, &serde_json::to_vec_pretty(&File { grants: g.clone() }).unwrap());
        let changed = settle(&written, &mut g, before);
        drop(g);
        if changed {
            self.log(entry);
        }
        written.map_err(std::io::Error::from)
    }

    /// Links that ran out go (true if any did).
    pub fn prune_links(&self) -> bool {
        let now = now_ms();
        let gone: Vec<Grant> =
            self.grants.read().unwrap().iter().filter(|g| g.expires.is_some_and(|e| e <= now)).cloned().collect();
        for g in &gone {
            let _ = self.set(g.session, &g.principal, &g.name, None, "expired");
        }
        !gone.is_empty()
    }

    /// For a "from now" share: where `pane`'s output may start for them.
    pub fn floor(&self, p: &Principal, session: SessionId, pane: PaneId) -> Option<u64> {
        let Principal::User { id, .. } = p else { return None };
        let g = self.grants.read().unwrap();
        let from = g.iter().find(|g| g.session == session && &g.principal == id)?.from.as_ref()?;
        // A pane made after the share has nothing from before it.
        Some(from.get(&pane).copied().unwrap_or(0))
    }

    /// For a "from now" share (M13): when it was made (ms), for what has
    /// a time and no offset (a pane's events).
    pub fn share_at(&self, p: &Principal, session: SessionId) -> Option<u64> {
        let Principal::User { id, .. } = p else { return None };
        let g = self.grants.read().unwrap();
        let grant = g.iter().find(|g| g.session == session && &g.principal == id)?;
        grant.from.as_ref().map(|_| grant.at)
    }

    /// Whether this principal may connect at all.
    pub fn knows(&self, p: &Principal) -> bool {
        if p.is_owner()
            || self.grants.read().unwrap().iter().any(|g| g.principal == p.id())
            || self.team_role(p).is_some()
        {
            return true;
        }
        // In a team something here was shared with (M30).
        let teams = self.shared_teams.read().unwrap();
        self.grants
            .read()
            .unwrap()
            .iter()
            .filter_map(|g| g.principal.strip_prefix("team:"))
            .any(|t| teams.get(t).is_some_and(|m| m.contains_key(p.id())))
    }

    pub fn list(&self) -> Vec<Grant> {
        self.grants.read().unwrap().clone()
    }

    /// Grant `role` (or revoke, with `None`), as `by`.
    pub fn set(
        &self,
        session: SessionId,
        principal: &str,
        name: &str,
        role: Option<Role>,
        by: &str,
    ) -> std::io::Result<()> {
        self.set_from(session, principal, name, role, by, None)
    }

    /// `from`: a "from now" share. Changing only the role of one keeps its
    /// floor.
    pub fn set_from(
        &self,
        session: SessionId,
        principal: &str,
        name: &str,
        role: Option<Role>,
        by: &str,
        from: Option<BTreeMap<PaneId, u64>>,
    ) -> std::io::Result<()> {
        self.set_full(session, principal, name, role, by, from, None)
    }

    /// With the root an `account:` principal's devices chain back to. A
    /// grant held keeps its own.
    #[allow(clippy::too_many_arguments)]
    pub fn set_full(
        &self,
        session: SessionId,
        principal: &str,
        name: &str,
        role: Option<Role>,
        by: &str,
        from: Option<BTreeMap<PaneId, u64>>,
        root: Option<String>,
    ) -> std::io::Result<()> {
        let mut g = self.grants.write().unwrap();
        let before = g.clone();
        let old = g.iter().find(|x| x.session == session && x.principal == principal).cloned();
        let kept = old.as_ref().and_then(|x| x.from.clone());
        // A role change keeps when the grant began (where a "from now"
        // share's thread in control's chat starts); a first grant, or one
        // made after a revoke, begins now.
        let at = match &old {
            Some(x) if from.is_none() => x.at,
            _ => now_ms(),
        };
        let root = root.or_else(|| old.and_then(|x| x.root));
        g.retain(|x| !(x.session == session && x.principal == principal));
        if let Some(role) = role {
            g.push(Grant {
                session,
                principal: principal.into(),
                name: name.into(),
                role,
                by: by.into(),
                at,
                from: from.or(kept),
                root,
                key: None,
                expires: None,
            });
        }
        let entry = serde_json::json!({
            "at": now_ms(), "by": by, "action": if role.is_some() { "grant" } else { "revoke" },
            "session": session, "principal": principal, "name": name, "role": role,
        });
        self.commit(g, before, entry)
    }

    /// A session closed: its grants go with it. Whether it had any.
    pub fn forget_session(&self, session: SessionId) -> bool {
        let gone: Vec<Grant> = self.grants.read().unwrap().iter().filter(|g| g.session == session).cloned().collect();
        for g in &gone {
            if let Err(e) = self.set(session, &g.principal, &g.name, None, "session closed") {
                warn!(error = %e, "can't drop a closed session's grant");
            }
        }
        !gone.is_empty()
    }

    /// Something else worth auditing (M29: who answered an agent, who sent
    /// it a follow-up).
    pub fn record(&self, entry: serde_json::Value) {
        self.log(entry);
    }

    fn log(&self, entry: serde_json::Value) {
        let line = format!("{entry}\n");
        let r = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.audit)
            .and_then(|mut f| f.write_all(line.as_bytes()));
        if let Err(e) = r {
            warn!(error = %e, "can't write the audit log");
        }
    }

    /// The audit log, newest last.
    pub fn audit(&self) -> Vec<serde_json::Value> {
        std::fs::read_to_string(&self.audit)
            .unwrap_or_default()
            .lines()
            .filter_map(|l| serde_json::from_str(l).ok())
            .collect()
    }
}

fn hex_sha(s: &str) -> String {
    use sha2::{Digest, Sha256};
    hex::encode(Sha256::digest(s.as_bytes()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn grants_persist_and_are_audited() {
        let dir = std::env::temp_dir().join(format!("arugula-acl-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let alice = Principal::tailnet("alice@example.com");
        let acl = Acl::open(&dir);
        assert!(!acl.knows(&alice));
        acl.set(3, alice.id(), "alice", Some(Role::Viewer), "owner").unwrap();
        acl.set(3, alice.id(), "alice", Some(Role::Editor), "owner").unwrap();
        let again = Acl::open(&dir);
        assert_eq!(again.role(&alice, 3), Some(Role::Editor));
        assert_eq!(again.role(&alice, 4), None);
        assert_eq!(again.role(&Principal::Owner, 4), Some(Role::Owner));
        assert!(again.forget_session(3));
        assert!(!again.knows(&alice));
        assert!(!again.forget_session(3));
        let log = again.audit();
        assert_eq!(log.iter().map(|e| e["action"].as_str().unwrap()).collect::<Vec<_>>(), ["grant", "grant", "revoke"]);
        std::fs::remove_dir_all(dir).unwrap();
    }

    /// A machine token's role (aru-141) stands in for the team roster's
    /// while its channel is open, lower or higher: a viewer's token isn't
    /// raised by the roster, and a guest's (someone control took out) gets
    /// nothing from it. A grant on a session still counts. On a session in
    /// the team's project either one watches (Arugula #1162).
    #[test]
    fn a_machine_tokens_role_stands_in_for_the_rosters() {
        let dir = std::env::temp_dir().join(format!("arugula-acl-token-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let acl = Acl::open(&dir);
        acl.set_machine_team(Some("t1".into()), false);
        acl.set_session_teams([(3, "t1".to_owned())]);
        let bea = Principal::User { id: "account:b1".into(), name: "bea".into(), pic: None };
        acl.set_team_roles([("account:b1".to_owned(), Role::Editor)].into());
        assert_eq!((acl.team_role(&bea), acl.role(&bea, 3)), (Some(Role::Editor), Some(Role::Viewer)));
        let (viewer, editor) = (Hold::token(Some(Role::Viewer), false), Hold::token(Some(Role::Editor), false));
        acl.hold("account:b1", viewer);
        assert_eq!((acl.role(&bea, 3), acl.team_role(&bea)), (Some(Role::Viewer), Some(Role::Viewer)));
        // A second channel, an editor's: the higher one holds while it's
        // open, and goes with it.
        acl.hold("account:b1", editor);
        assert_eq!(acl.team_role(&bea), Some(Role::Editor));
        acl.release("account:b1", editor);
        assert_eq!(acl.team_role(&bea), Some(Role::Viewer));
        acl.release("account:b1", viewer);
        // A guest's token: nothing from the roster, only what's shared.
        let guest = Hold::token(None, false);
        acl.hold("account:b1", guest);
        assert_eq!((acl.team_role(&bea), acl.role(&bea, 3)), (None, None));
        acl.set(4, "account:b1", "bea", Some(Role::Viewer), "owner").unwrap();
        assert_eq!(acl.role(&bea, 4), Some(Role::Viewer));
        // Closed: the roster's again, for a device's channel.
        acl.release("account:b1", guest);
        assert_eq!((acl.team_role(&bea), acl.role(&bea, 3)), (Some(Role::Editor), Some(Role::Viewer)));
        std::fs::remove_dir_all(dir).unwrap();
    }

    /// A channel that opened with remote input off (aru-141) makes its
    /// principal a viewer at most while it's open: an editor's grant on a
    /// session, a team's share and the roster don't raise it, and a
    /// session they have nothing on stays shut.
    #[test]
    fn remote_input_off_holds_everyone_to_a_viewer() {
        let dir = std::env::temp_dir().join(format!("arugula-acl-viewer-only-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let acl = Acl::open(&dir);
        acl.set_machine_team(Some("t1".into()), false);
        acl.set_session_teams([(4, "t1".to_owned()), (5, "t1".to_owned())]);
        let bea = Principal::User { id: "account:b1".into(), name: "bea".into(), pic: None };
        acl.set(3, "account:b1", "bea", Some(Role::Editor), "owner").unwrap();
        acl.set(4, "team:t1", "t1", Some(Role::Editor), "owner").unwrap();
        acl.set_shared_teams([("t1".to_owned(), [("account:b1".to_owned(), Role::Editor)].into())].into());
        assert_eq!((acl.role(&bea, 3), acl.role(&bea, 4)), (Some(Role::Editor), Some(Role::Editor)));
        // A guest's token, remote input off.
        let guest = Hold::token(None, true);
        acl.hold("account:b1", guest);
        assert_eq!(
            (acl.role(&bea, 3), acl.role(&bea, 4), acl.role(&bea, 5)),
            (Some(Role::Viewer), Some(Role::Viewer), None)
        );
        // An editor's token that opened with it on doesn't lift it.
        let editor = Hold::token(Some(Role::Editor), false);
        acl.hold("account:b1", editor);
        assert_eq!((acl.role(&bea, 3), acl.team_role(&bea)), (Some(Role::Viewer), Some(Role::Viewer)));
        acl.release("account:b1", editor);
        acl.release("account:b1", guest);
        assert_eq!(acl.role(&bea, 3), Some(Role::Editor));
        // A device's channel holds no role, only the cap: the roster's
        // editor watches.
        acl.set_team_roles([("account:b1".to_owned(), Role::Editor)].into());
        let device = Hold { viewer_only: true, ..Hold::default() };
        acl.hold("account:b1", device);
        assert_eq!((acl.team_role(&bea), acl.role(&bea, 5)), (Some(Role::Viewer), Some(Role::Viewer)));
        acl.release("account:b1", device);
        assert_eq!((acl.team_role(&bea), acl.role(&bea, 5)), (Some(Role::Editor), Some(Role::Viewer)));
        std::fs::remove_dir_all(dir).unwrap();
    }

    /// Arugula #1210: the machine's owner through control with remote
    /// input off watches every session on it, team or not, and types into
    /// none; another account's token with it off still gets only what's
    /// shared with it, or its team's.
    #[test]
    fn with_remote_input_off_the_owner_watches_every_session() {
        let (dir, acl) = opened("owner-watching");
        acl.set_session_teams([(1, "t1".to_owned())]);
        let me = Principal::User { id: "account:a1".into(), name: "me".into(), pic: None };
        let them = Principal::User { id: "account:b1".into(), name: "bea".into(), pic: None };
        acl.set(2, "account:a1", "me", Some(Role::Editor), "owner").unwrap();
        assert_eq!((acl.role(&me, 1), acl.role(&me, 2), acl.role(&me, 3)), (None, Some(Role::Editor), None));
        let watching = Hold::owner_watching();
        acl.hold("account:a1", watching);
        assert_eq!(
            (acl.role(&me, 1), acl.role(&me, 2), acl.role(&me, 3)),
            (Some(Role::Viewer), Some(Role::Viewer), Some(Role::Viewer))
        );
        // Someone else's owner-sized token, held to a viewer: as before.
        acl.hold("account:b1", Hold::token(Some(Role::Viewer), true));
        assert_eq!((acl.role(&them, 1), acl.role(&them, 3)), (None, None));
        acl.release("account:a1", watching);
        assert_eq!((acl.role(&me, 1), acl.role(&me, 3)), (None, None));
        std::fs::remove_dir_all(dir).unwrap();
    }

    fn opened(name: &str) -> (std::path::PathBuf, Acl) {
        let dir = std::env::temp_dir().join(format!("arugula-acl-{name}-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let acl = Acl::open(&dir);
        (dir, acl)
    }

    /// Arugula #1162: on a machine of team t1, its members watch the
    /// sessions in t1's projects, an editor's role and an owner-sized token
    /// included, and nothing else: not a session in a person's own project,
    /// in another team's, or in none.
    #[test]
    fn a_team_watches_its_projects_sessions_and_no_others() {
        let (dir, acl) = opened("team-project");
        acl.set_machine_team(Some("t1".into()), false);
        // 1 in t1's project, 2 in t2's; 3 in the owner's own, 4 in none:
        // neither has a team.
        acl.set_session_teams([(1, "t1".to_owned()), (2, "t2".to_owned())]);
        let (bea, cy) = (
            Principal::User { id: "account:b1".into(), name: "bea".into(), pic: None },
            Principal::User { id: "account:c1".into(), name: "cy".into(), pic: None },
        );
        // Bea by the roster, an editor; Cy by an editor's machine token.
        acl.set_team_roles([("account:b1".to_owned(), Role::Editor)].into());
        acl.hold("account:c1", Hold::token(Some(Role::Editor), false));
        for who in [&bea, &cy] {
            assert_eq!([1, 2, 3, 4].map(|s| acl.role(who, s)), [Some(Role::Viewer), None, None, None], "{}", who.id());
        }
        // Someone outside the team: nothing.
        let dee = Principal::User { id: "account:d1".into(), name: "dee".into(), pic: None };
        assert_eq!(acl.role(&dee, 1), None);
        // The owner: everything.
        assert_eq!([1, 2, 3, 4].map(|s| acl.role(&Principal::Owner, s)), [Some(Role::Owner); 4]);
        std::fs::remove_dir_all(dir).unwrap();
    }

    /// Arugula #1162: who watches a session follows its project. Into a
    /// team's project, the team watches; out of it, it doesn't.
    #[test]
    fn a_project_change_grants_and_revokes_the_teams_watch() {
        let (dir, acl) = opened("project-change");
        acl.set_machine_team(Some("t1".into()), false);
        acl.set_team_roles([("account:b1".to_owned(), Role::Viewer)].into());
        let bea = Principal::User { id: "account:b1".into(), name: "bea".into(), pic: None };
        assert!(!acl.set_session_teams([]));
        assert_eq!(acl.role(&bea, 7), None, "in the owner's own project");
        assert!(acl.set_session_teams([(7, "t1".to_owned())]), "moved into t1's project");
        assert_eq!(acl.role(&bea, 7), Some(Role::Viewer));
        assert!(!acl.set_session_teams([(7, "t1".to_owned())]), "nothing changed");
        assert!(acl.set_session_teams([(7, "t2".to_owned())]), "moved to another team's");
        assert_eq!(acl.role(&bea, 7), None);
        acl.set_session_teams([(7, "t1".to_owned())]);
        assert!(acl.set_session_teams([]), "back in the owner's own");
        assert_eq!(acl.role(&bea, 7), None);
        // The machine left the team: none of its old projects' sessions.
        acl.set_session_teams([(7, "t1".to_owned())]);
        acl.set_machine_team(None, false);
        assert_eq!(acl.role(&bea, 7), None);
        std::fs::remove_dir_all(dir).unwrap();
    }

    /// A cloud workspace is its project's, and a box joined for a team the
    /// team's (Arugula #1182): its team's members keep their role on every
    /// session, whatever its project, an editor's token's included.
    #[test]
    fn a_cloud_workspaces_team_keeps_its_role_everywhere() {
        let (dir, acl) = opened("hosted");
        acl.set_machine_team(Some("t1".into()), true);
        acl.set_team_roles([("account:c1".to_owned(), Role::Viewer)].into());
        acl.hold("account:b1", Hold::token(Some(Role::Editor), false));
        assert_eq!(acl.role_of("account:c1", 1), Some(Role::Viewer), "no project's team known");
        acl.set_session_teams([(2, "t2".to_owned())]);
        assert_eq!(acl.role_of("account:b1", 1), Some(Role::Editor), "in no team's project");
        assert_eq!(acl.role_of("account:b1", 2), Some(Role::Editor), "in another team's");
        // A person's machine in the team: #1162's rule.
        acl.set_machine_team(Some("t1".into()), false);
        assert_eq!((acl.role_of("account:b1", 1), acl.role_of("account:b1", 2)), (None, None));
        std::fs::remove_dir_all(dir).unwrap();
    }

    /// Arugula #1162, decision 4: a share with a person stays wherever the
    /// session goes, a teammate's too, and raises them past watching. A
    /// share with a team counts only while the session is in that team's
    /// project, so a team share made before doesn't open a session in the
    /// owner's own project.
    #[test]
    fn a_persons_share_stays_and_a_teams_follows_the_project() {
        let (dir, acl) = opened("shares");
        acl.set_machine_team(Some("t1".into()), false);
        acl.set_team_roles([("account:b1".to_owned(), Role::Viewer)].into());
        acl.set_shared_teams([("t1".to_owned(), [("account:c1".to_owned(), Role::Editor)].into())].into());
        let (bea, cy) = (
            Principal::User { id: "account:b1".into(), name: "bea".into(), pic: None },
            Principal::User { id: "account:c1".into(), name: "cy".into(), pic: None },
        );
        acl.set(5, "account:b1", "bea", Some(Role::Editor), "owner").unwrap();
        acl.set(5, "team:t1", "t1", Some(Role::Editor), "owner").unwrap();
        // In the owner's own project: Bea's share stands, t1's gives Cy
        // nothing.
        assert_eq!((acl.role(&bea, 5), acl.role(&cy, 5)), (Some(Role::Editor), None));
        // In t1's project: both count, and Bea's is more than the watch.
        acl.set_session_teams([(5, "t1".to_owned())]);
        assert_eq!((acl.role(&bea, 5), acl.role(&cy, 5)), (Some(Role::Editor), Some(Role::Editor)));
        // In t2's: Bea's still stands.
        acl.set_session_teams([(5, "t2".to_owned())]);
        assert_eq!((acl.role(&bea, 5), acl.role(&cy, 5)), (Some(Role::Editor), None));
        std::fs::remove_dir_all(dir).unwrap();
    }

    /// Arugula #1162: an older control names no project's team, so no
    /// session has one: a team's members get nothing from the team, only
    /// what's shared with them.
    #[test]
    fn with_no_team_known_the_team_gets_nothing() {
        let (dir, acl) = opened("old-control");
        acl.set_machine_team(Some("t1".into()), false);
        acl.set_team_roles([("account:b1".to_owned(), Role::Editor)].into());
        acl.hold("account:c1", Hold::token(Some(Role::Editor), false));
        acl.set_session_teams([]);
        for id in ["account:b1", "account:c1"] {
            assert_eq!(acl.role_of(id, 1), None, "{id}");
        }
        acl.set(1, "account:c1", "cy", Some(Role::Viewer), "owner").unwrap();
        assert_eq!(acl.role_of("account:c1", 1), Some(Role::Viewer), "shared with them");
        std::fs::remove_dir_all(dir).unwrap();
    }

    /// Arugula #1180: a team owner on a teammate's machine, by the roster or
    /// an owner's machine token, gets what any teammate gets: a viewer's
    /// watch on the sessions in the team's projects, nothing on the
    /// teammate's own, and what's shared with them.
    #[test]
    fn a_team_owner_on_a_teammates_machine_watches_as_a_teammate() {
        let (dir, acl) = opened("team-owner");
        acl.set_machine_team(Some("t1".into()), false);
        // 1 in t1's project; 2 in the teammate's own; 3 in t2's.
        acl.set_session_teams([(1, "t1".to_owned()), (3, "t2".to_owned())]);
        let (ola, oz) = (
            Principal::User { id: "account:o1".into(), name: "ola".into(), pic: None },
            Principal::User { id: "account:o2".into(), name: "oz".into(), pic: None },
        );
        // Ola by the roster, Oz by an owner's token.
        acl.set_team_roles([("account:o1".to_owned(), Role::Owner)].into());
        acl.hold("account:o2", Hold::token(Some(Role::Owner), false));
        for who in [&ola, &oz] {
            assert_eq!(acl.team_role(who), Some(Role::Owner), "{}", who.id());
            assert_eq!([1, 2, 3].map(|s| acl.role(who, s)), [Some(Role::Viewer), None, None], "{}", who.id());
            assert!(!acl.notifies(who.id(), Some(1)), "a watcher isn't told: {}", who.id());
        }
        // Shared with Ola on the teammate's own session: that share.
        acl.set(2, "account:o1", "ola", Some(Role::Editor), "owner").unwrap();
        assert_eq!(acl.role(&ola, 2), Some(Role::Editor));
        // The machine's own account is still its owner.
        assert_eq!([1, 2, 3].map(|s| acl.role(&Principal::Owner, s)), [Some(Role::Owner); 3]);
        // On a machine that's the team's (Arugula #1182), the owner's role
        // counts on every session.
        acl.set_machine_team(Some("t1".into()), true);
        assert_eq!([1, 2, 3].map(|s| acl.role(&oz, s)), [Some(Role::Owner); 3]);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn revoking_a_link_ends_that_link_and_nothing_else() {
        let dir = std::env::temp_dir().join(format!("arugula-acl-link-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let acl = Acl::open(&dir);
        let until = now_ms() + 60_000;
        let (a, b) = ("a".repeat(64), "b".repeat(64));
        let first = acl.add_link(3, &a, until + 1000, None).unwrap();
        let second = acl.add_link(4, &b, until, None).unwrap();
        acl.set(3, "tailnet:alice@example.com", "alice", Some(Role::Viewer), "owner").unwrap();
        assert_eq!(acl.links().len(), 2);
        assert_eq!(acl.links_until(), Some(until + 1000));

        // Not a link: its grant stays, whatever the principal.
        assert!(!acl.revoke_link("tailnet:alice@example.com", "owner").unwrap());
        assert!(!acl.revoke_link("link:nothing", "owner").unwrap());

        assert!(acl.revoke_link(&first, "owner").unwrap());
        assert!(acl.link_by_key(&a).is_none());
        assert!(acl.link_by_key(&b).is_some());
        assert!(!acl.knows(&Principal::User { id: first.clone(), name: String::new(), pic: None }));
        assert!(acl.knows(&Principal::tailnet("alice@example.com")));
        // What control is told follows the links left, down to none.
        assert_eq!(acl.links_until(), Some(until));
        assert!(!acl.revoke_link(&first, "owner").unwrap());
        assert!(acl.revoke_link(&second, "owner").unwrap());
        assert_eq!(acl.links_until(), None);
        assert!(acl.links().is_empty());
        // It stays gone across a restart, and the log says so.
        let again = Acl::open(&dir);
        assert!(again.link_by_key(&a).is_none());
        let log = again.audit();
        assert_eq!(
            log.iter()
                .filter(|e| e["principal"] == first.as_str())
                .map(|e| e["action"].as_str().unwrap())
                .collect::<Vec<_>>(),
            ["link", "revoke"]
        );
        std::fs::remove_dir_all(dir).unwrap();
    }

    /// Nobody sees a change to the grants before it's written: a change
    /// another made meanwhile would be written over by this one's older
    /// copy. Each change here stops at its write, on a FIFO in place of the
    /// write's temporary file (where a sync then fails, so it's rolled back).
    #[cfg(target_os = "linux")]
    #[test]
    fn a_change_to_the_grants_shows_only_once_it_is_written() {
        use std::{
            sync::{Arc, mpsc},
            time::Duration,
        };
        type Case = (&'static str, fn(&Acl, &str), fn(&Acl, &str) -> bool);
        let dir = std::env::temp_dir().join(format!("arugula-acl-write-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let acl = Arc::new(Acl::open(&dir));
        let until = now_ms() + 60_000;
        let live = acl.add_link(3, &"a".repeat(64), until, None).unwrap();
        let fifo = dir.join("acl.tmp");
        let cases: [Case; 3] = [
            (
                "a mint",
                |acl, _| drop(acl.add_link(3, &"b".repeat(64), now_ms() + 60_000, None)),
                |acl, _| acl.link_by_key(&"b".repeat(64)).is_some(),
            ),
            (
                "a revoke",
                |acl, live| drop(acl.revoke_link(live, "owner")),
                |acl, _| acl.link_by_key(&"a".repeat(64)).is_none(),
            ),
            (
                "a share",
                |acl, _| drop(acl.set(3, "tailnet:alice@example.com", "alice", Some(Role::Viewer), "owner")),
                |acl, _| acl.knows(&Principal::tailnet("alice@example.com")),
            ),
        ];
        for (what, change, shows) in cases {
            let made = std::process::Command::new("mkfifo").arg(&fifo).status().unwrap();
            assert!(made.success());
            let (a, l) = (acl.clone(), live.clone());
            let writer = std::thread::spawn(move || change(&a, &l));
            let (told, heard) = mpsc::channel();
            let (a, l) = (acl.clone(), live.clone());
            std::thread::spawn(move || {
                while !shows(&a, &l) {
                    std::thread::sleep(Duration::from_millis(5));
                }
                let _ = told.send(());
            });
            assert!(heard.recv_timeout(Duration::from_millis(400)).is_err(), "{what} showed before it was written");
            // The write goes on, and can't finish on a FIFO: nothing changed.
            std::fs::read(&fifo).unwrap();
            writer.join().unwrap();
            std::fs::remove_file(&fifo).unwrap();
            assert!(!shows(&acl, &live), "{what} that wasn't written is still there");
            change(&acl, &live);
            heard.recv_timeout(Duration::from_secs(5)).unwrap();
        }
        // Written, and so what comes back after a restart.
        let again = Acl::open(&dir);
        assert!(again.link_by_key(&"a".repeat(64)).is_none());
        assert!(again.link_by_key(&"b".repeat(64)).is_some());
        assert!(again.knows(&Principal::tailnet("alice@example.com")));
        std::fs::remove_dir_all(dir).unwrap();
    }

    fn grant_named(principal: &str) -> Grant {
        Grant {
            session: 3,
            principal: principal.into(),
            name: String::new(),
            role: Role::Viewer,
            by: "owner".into(),
            at: 0,
            from: None,
            root: None,
            key: None,
            expires: None,
        }
    }

    /// What's in memory after a write: back as it was only if the file is.
    #[test]
    fn memory_follows_whether_the_file_was_replaced() {
        let failed = || std::io::Error::other("no");
        let (before, after) = (vec![grant_named("old")], vec![grant_named("new")]);
        for (written, changed, held) in [
            (Ok(()), true, &after),
            (Err(ReplaceError::NotSynced(failed())), true, &after),
            (Err(ReplaceError::NotReplaced(failed())), false, &before),
        ] {
            let mut g = after.clone();
            assert_eq!(settle(&written, &mut g, before.clone()), changed, "{written:?}");
            assert_eq!(&g, held, "{written:?}");
        }
    }

    /// The file is replaced and then the directory can't be synced (it can't
    /// be opened, in a directory we may write to but not read): the change
    /// stands in memory as it does on disk, in the log too, and the caller
    /// hears of the error. Root reads any directory, so there's nothing to see.
    #[cfg(unix)]
    #[test]
    fn a_change_that_replaced_the_file_stands_though_its_sync_failed() {
        use std::os::unix::fs::PermissionsExt;
        let dir = std::env::temp_dir().join(format!("arugula-acl-unsynced-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let acl = Acl::open(&dir);
        let until = now_ms() + 60_000;
        let (a, b) = ("a".repeat(64), "b".repeat(64));
        let live = acl.add_link(3, &a, until, None).unwrap();
        let mode = |m| std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(m)).unwrap();

        mode(0o300);
        if std::fs::File::open(&dir).is_ok() {
            mode(0o700);
            return eprintln!("skipped: this user reads every directory");
        }
        let made = acl.add_link(3, &b, until, None);
        let revoked = acl.revoke_link(&live, "owner");
        let shared = acl.set(3, "tailnet:alice@example.com", "alice", Some(Role::Viewer), "owner");
        mode(0o700);
        assert!(made.is_err() && revoked.is_err() && shared.is_err(), "each hears of it");

        let check = |acl: &Acl| {
            assert!(acl.link_by_key(&b).is_some(), "the link made");
            assert!(acl.link_by_key(&a).is_none(), "the link revoked");
            assert!(acl.knows(&Principal::tailnet("alice@example.com")), "the share");
        };
        check(&acl);
        check(&Acl::open(&dir));
        let log = acl.audit();
        assert_eq!(
            log.iter().map(|e| e["action"].as_str().unwrap()).collect::<Vec<_>>(),
            ["link", "link", "revoke", "grant"]
        );
        std::fs::remove_dir_all(dir).unwrap();
    }

    /// A link made while another is revoked: the revoked one stays gone
    /// after a restart. No pause here, so a failure may take some rounds.
    #[test]
    fn a_revoked_link_stays_gone_with_a_link_made_at_the_same_time() {
        use std::sync::{Arc, Barrier};
        let dir = std::env::temp_dir().join(format!("arugula-acl-race-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let acl = Arc::new(Acl::open(&dir));
        let until = now_ms() + 60_000;
        for round in 0..60u32 {
            let (old, new) = (format!("{:064x}", 2 * round), format!("{:064x}", 2 * round + 1));
            let id = acl.add_link(3, &old, until, None).unwrap();
            let start = Arc::new(Barrier::new(2));
            let mint = {
                let (acl, start, new) = (acl.clone(), start.clone(), new.clone());
                std::thread::spawn(move || {
                    start.wait();
                    acl.add_link(3, &new, until, None).unwrap()
                })
            };
            start.wait();
            assert!(acl.revoke_link(&id, "owner").unwrap(), "round {round}");
            let made = mint.join().unwrap();
            let again = Acl::open(&dir);
            assert!(again.link_by_key(&old).is_none(), "round {round}: a revoked link came back");
            assert!(again.link_by_key(&new).is_some(), "round {round}: the new link was lost");
            assert!(acl.revoke_link(&made, "owner").unwrap());
        }
        std::fs::remove_dir_all(dir).unwrap();
    }

    /// A role change keeps when a grant began (where a "from now" share's
    /// threads start); a grant made again after a revoke begins now.
    #[test]
    fn a_role_change_keeps_when_the_grant_began() {
        let dir = std::env::temp_dir().join(format!("arugula-acl-at-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let alice = Principal::tailnet("alice@example.com");
        let acl = Acl::open(&dir);
        let from: BTreeMap<PaneId, u64> = [(1, 5)].into();
        acl.set_from(3, alice.id(), "alice", Some(Role::Viewer), "owner", Some(from.clone())).unwrap();
        let first = acl.share_at(&alice, 3).unwrap();
        std::thread::sleep(std::time::Duration::from_millis(5));
        acl.set(3, alice.id(), "alice", Some(Role::Editor), "owner").unwrap();
        assert_eq!(acl.role(&alice, 3), Some(Role::Editor));
        assert_eq!(acl.share_at(&alice, 3), Some(first));
        assert_eq!(acl.floor(&alice, 3, 1), Some(5));
        acl.set(3, alice.id(), "alice", None, "owner").unwrap();
        std::thread::sleep(std::time::Duration::from_millis(5));
        acl.set_from(3, alice.id(), "alice", Some(Role::Viewer), "owner", Some(from)).unwrap();
        assert!(acl.share_at(&alice, 3).unwrap() > first);
        std::fs::remove_dir_all(dir).unwrap();
    }
}
