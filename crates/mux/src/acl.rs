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
    /// A team daemon's members' roles on every session (M19), from the
    /// team's signed roster.
    team: RwLock<std::collections::HashMap<String, Role>>,
    /// Members of teams sessions were shared with (M30), by team, each with
    /// their role in it, from rosters this daemon verified.
    shared_teams: RwLock<std::collections::HashMap<String, std::collections::HashMap<String, Role>>>,
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
            shared_teams: Default::default(),
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

    /// Someone's role on a session: a grant, else their team role.
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
                let team = self.team.read().unwrap().get(id).copied();
                // Shared with a team they're in: the grant's role, at most
                // their role in the team.
                let teams = self.shared_teams.read().unwrap();
                let via_team = self
                    .grants
                    .read()
                    .unwrap()
                    .iter()
                    .filter(|g| g.session == session)
                    .filter_map(|g| {
                        let in_team = teams.get(g.principal.strip_prefix("team:")?)?.get(id)?;
                        Some(g.role.min(*in_team))
                    })
                    .max();
                granted.max(team).max(via_team)
            }
        }
    }

    /// A team daemon's members' roles (from its verified roster).
    pub fn set_team_roles(&self, roles: std::collections::HashMap<String, Role>) {
        *self.team.write().unwrap() = roles;
    }

    /// Members of the teams sessions were shared with (M30).
    pub fn set_shared_teams(&self, teams: std::collections::HashMap<String, std::collections::HashMap<String, Role>>) {
        *self.shared_teams.write().unwrap() = teams;
    }

    pub fn team_role(&self, p: &Principal) -> Option<Role> {
        self.team.read().unwrap().get(p.id()).copied()
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
