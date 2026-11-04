//! The host list (M4a): other daemons a client can switch to. The home
//! daemon keeps it in `hosts.json`; clients fetch it and then talk to each
//! host directly, so terminal bytes never pass through here.
//!
//! A host gets on the list by being added (`arugula hosts add`, by the
//! owner): a tailnet host, reached at its URLs, or an ssh host (M51). The
//! home daemon checks on each tailnet host every minute and records when
//! it last answered.
//!
//! What older daemons kept and this one drops as it starts, each with a
//! log line: a `provider` entry (a daemon resident in a sandbox, M4b,
//! removed in 0.67) and a `dial_out` entry (a host that dialed in with a
//! per-host token, M4c, removed in 0.68) in `hosts.json`, keeping the
//! other hosts; `provider-tokens.json`, `host-tokens.json` and the host
//! invites in `invites.json`, which were credentials for those.

use std::{
    path::PathBuf,
    sync::{Arc, Mutex},
    time::Duration,
};

use arugula_proto::flags;
use arugula_proto::hosts::{AddHost, Host, HostAnswer, HostFeatures, HostInfo, HostList, Transport};
use arugula_proto::op::ops::{HostAdd, HostGet, HostRemove, HostsList};
use axum::Router;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tracing::{info, warn};

use crate::{
    ops::OpRoutes,
    server::App,
    store::{now_ms, write_atomic},
};

const PROBE_EVERY: Duration = Duration::from_secs(60);

pub struct Hosts {
    /// This daemon's own name.
    name: String,
    path: PathBuf,
    inner: Mutex<Saved>,
    http: reqwest::Client,
}

#[derive(Default, Serialize, Deserialize)]
struct Saved {
    hosts: Vec<Host>,
}

impl Hosts {
    pub fn open(state_dir: &std::path::Path, name: String) -> Arc<Self> {
        let path = state_dir.join("hosts.json");
        let mut saved: Saved =
            std::fs::read(&path).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default();
        // Hosts of kinds since removed: an older daemon's entries go, with
        // a word, and the other hosts stay (a list that fails to parse would
        // be emptied whole, so the transport words stay readable).
        let gone = |t: Transport| match t {
            Transport::Provider => Some("daemons resident in a provider's sandbox were removed (0.67)"),
            Transport::DialOut => Some("hosts that dial in with a per-host token were removed (0.68)"),
            Transport::Tailnet | Transport::Ssh => None,
        };
        let old: Vec<(String, &str)> =
            saved.hosts.iter().filter_map(|h| Some((h.name.clone(), gone(h.transport)?))).collect();
        if !old.is_empty() {
            saved.hosts.retain(|h| gone(h.transport).is_none());
            for (host, why) in &old {
                warn!(host, "dropped from hosts.json: {why}");
            }
            let bytes = serde_json::to_vec_pretty(&saved).expect("serialize hosts");
            if let Err(e) = write_atomic(&path, &bytes) {
                warn!(error = %e, "can't save the host list");
            }
        }
        // Their credentials: spent, and nothing reads them.
        for (file, what) in [
            ("provider-tokens.json", "the tunnel tokens of resident daemons (removed in 0.67)"),
            ("host-tokens.json", "the hashes of dial-in host tokens (removed in 0.68)"),
        ] {
            let p = state_dir.join(file);
            if p.exists() {
                match std::fs::remove_file(&p) {
                    Ok(()) => warn!(file = %p.display(), "deleted: {what}"),
                    Err(e) => warn!(file = %p.display(), error = %e, "can't delete {what}"),
                }
            }
        }
        // `invites.json` in this list's own shape (`{"invites": [...]}`)
        // held host invites, removed in 0.68: spent tokens, deleted. A bare
        // list there is the waiting person invites a daemon before native
        // #677 kept in it, which `control.rs` moves into its own file as it
        // opens (before this list does): left alone until that file exists,
        // then deleted as the copy it is. Anything else is left alone.
        let invites = state_dir.join("invites.json");
        let moved_out = state_dir.join("person-invites.json").exists();
        let delete = match std::fs::read(&invites)
            .ok()
            .and_then(|b| serde_json::from_slice::<serde_json::Value>(&b).ok())
        {
            Some(serde_json::Value::Object(o)) if o.contains_key("invites") => Some("host invites (removed in 0.68)"),
            Some(serde_json::Value::Array(_)) if moved_out => {
                Some("waiting person invites that control moved into person-invites.json")
            }
            _ => None,
        };
        if let Some(what) = delete {
            match std::fs::remove_file(&invites) {
                Ok(()) => warn!(file = %invites.display(), "deleted: {what}"),
                Err(e) => warn!(file = %invites.display(), error = %e, "can't delete {what}"),
            }
        }
        let http =
            crate::roots::http().timeout(Duration::from_secs(5)).build().expect("an HTTP client with default settings");
        Arc::new(Self { name, path, inner: Mutex::new(saved), http })
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn list(&self) -> HostList {
        HostList { this: self.name.clone(), hosts: self.inner.lock().unwrap().hosts.clone() }
    }

    /// Add a host, or replace the one with the same name (keeping when it
    /// was first added and last seen, if its URLs are the same).
    pub fn add(&self, req: AddHost) -> Result<Host, String> {
        match req.transport {
            Transport::Provider => {
                return Err("daemons resident in a provider's sandbox were removed; add a tailnet or ssh host".into());
            }
            Transport::DialOut => {
                return Err("hosts that dial in were removed; add a tailnet or ssh host".into());
            }
            Transport::Tailnet | Transport::Ssh => {}
        }
        self.insert(validate(req, &self.name)?)
    }

    fn insert(&self, req: AddHost) -> Result<Host, String> {
        let mut inner = self.inner.lock().unwrap();
        let old = inner.hosts.iter().position(|h| h.name == req.name).map(|i| inner.hosts.remove(i));
        let host = Host {
            added_ms: old.as_ref().map_or_else(now_ms, |o| o.added_ms),
            last_seen_ms: old.as_ref().filter(|o| o.urls == req.urls).and_then(|o| o.last_seen_ms),
            name: req.name,
            urls: req.urls,
            transport: req.transport,
            ssh: req.ssh,
        };
        inner.hosts.push(host.clone());
        inner.hosts.sort_by(|a, b| a.name.cmp(&b.name));
        self.save(&inner);
        info!(name = host.name, urls = ?host.urls, "host added");
        Ok(host)
    }

    /// Remove a host.
    pub fn remove(&self, name: &str) -> bool {
        let mut inner = self.inner.lock().unwrap();
        let before = inner.hosts.len();
        inner.hosts.retain(|h| h.name != name);
        let gone = inner.hosts.len() != before;
        if gone {
            self.save(&inner);
            info!(name, "host removed");
        }
        gone
    }

    fn save(&self, inner: &Saved) {
        let bytes = serde_json::to_vec_pretty(inner).expect("serialize hosts");
        if let Err(e) = write_atomic(&self.path, &bytes) {
            warn!(error = %e, "can't save the host list");
        }
    }

    /// Ask every host who it is; note the ones that answer.
    pub async fn probe(&self) {
        let targets: Vec<(String, Vec<String>)> = self
            .inner
            .lock()
            .unwrap()
            .hosts
            .iter()
            .filter(|h| h.transport == Transport::Tailnet)
            .map(|h| (h.name.clone(), h.urls.clone()))
            .collect();
        self.probe_urls(targets).await;
    }

    async fn probe_urls(&self, targets: Vec<(String, Vec<String>)>) {
        for (name, urls) in targets {
            let mut seen = false;
            for url in &urls {
                // A daemon that wants a credential we don't show it (one on
                // this machine's loopback, which wants its local token) is
                // there all the same.
                let ok = self
                    .http
                    .get(format!("{url}/api/host"))
                    .send()
                    .await
                    .is_ok_and(|r| r.status().is_success() || r.status() == reqwest::StatusCode::UNAUTHORIZED);
                if ok {
                    seen = true;
                    break;
                }
            }
            if seen {
                let mut inner = self.inner.lock().unwrap();
                if let Some(h) = inner.hosts.iter_mut().find(|h| h.name == name && h.urls == urls) {
                    h.last_seen_ms = Some(now_ms());
                    self.save(&inner);
                }
            }
        }
    }

    pub fn spawn_probe(self: &Arc<Self>) {
        let me = Arc::downgrade(self);
        crate::task::spawn(async move {
            let mut tick = tokio::time::interval(PROBE_EVERY);
            loop {
                tick.tick().await;
                let Some(me) = me.upgrade() else { return };
                me.probe().await;
            }
        });
    }
}

/// Names are what `--host` and the client's switcher show; URLs must be
/// plain origins (a client appends `/ws` and `/api/...`).
fn validate(mut req: AddHost, this: &str) -> Result<AddHost, String> {
    let name = req.name.trim();
    // Names end up in paths (`--host NAME`, a remote block's config), so
    // not `.` or `..`.
    if name.is_empty()
        || name.len() > 63
        || name.starts_with('.')
        || !name.bytes().all(|b| b.is_ascii_alphanumeric() || b"-_.".contains(&b))
    {
        return Err(format!("bad host name {name:?}: letters, digits, '-', '_' and '.' (not first)"));
    }
    if name == this {
        return Err(format!("{name} is this daemon's own name"));
    }
    req.name = name.to_owned();
    match req.transport {
        Transport::Tailnet if req.urls.is_empty() => return Err("a host needs at least one URL".into()),
        // Each client runs ssh to it; we keep its destination and nothing
        // else, and it must never read as one of ssh's options.
        Transport::Ssh => {
            if !req.urls.is_empty() {
                return Err("an ssh host has no URLs".into());
            }
            let dest = req.ssh.as_deref().map(str::trim).unwrap_or_default();
            if dest.is_empty()
                || dest.starts_with('-')
                || !dest.bytes().all(|b| b.is_ascii_alphanumeric() || b"@._-:[]%+/".contains(&b))
            {
                return Err(format!("bad ssh destination {dest:?}: want user@host or a Host from ~/.ssh/config"));
            }
            req.ssh = Some(dest.to_owned());
        }
        _ => {}
    }
    for url in &mut req.urls {
        let parsed = reqwest::Url::parse(url.trim()).map_err(|e| format!("bad URL {url:?}: {e}"))?;
        if !matches!(parsed.scheme(), "http" | "https") || parsed.host_str().is_none() {
            return Err(format!("bad URL {url:?}: want http(s)://host[:port]"));
        }
        if parsed.path() != "/" || parsed.query().is_some() || !parsed.username().is_empty() {
            return Err(format!("bad URL {url:?}: just the origin, without a path"));
        }
        *url = parsed.origin().ascii_serialization();
    }
    Ok(req)
}

pub fn digest(token: &str) -> String {
    hex(&Sha256::digest(token.as_bytes()))
}

fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

// ---------------------------------------------------------------- routes

pub fn routes() -> Router<Arc<App>> {
    Router::new().op::<HostGet>().op::<HostsList>().op::<HostAdd>().op::<HostRemove>()
}

/// `GET /api/host`'s answer: [`HostInfo`], and for the owner this machine's
/// standing with control (#325), beside it.
pub(crate) fn answer(app: &App, owner: bool) -> HostAnswer {
    let joined = app.control.enrolled();
    let saved = joined.as_ref().map(|e| &e.saved);
    let info = HostInfo {
        name: app.hosts.name().to_owned(),
        version: arugula_core::version::get().to_owned(),
        protocol: Some(arugula_proto::PROTOCOL),
        tailnet_url: app.access.tailnet_url(),
        tailnet_seen: app.tailnet_seen.load(std::sync::atomic::Ordering::Relaxed),
        control: saved.map(|s| s.url.clone()),
        // The team's name once its roster is in, else its id.
        team: saved.and_then(|s| s.team()).map(|t| t.roster.as_ref().map_or(t.pin.team.clone(), |r| r.name.clone())),
        features: Some(features(app)),
    };
    HostAnswer { info, control_state: owner.then(|| crate::setup::control_state(app)) }
}

/// What this machine is set up for, so the menus offer only that (#180)
/// or say how to turn it on (#171). Cheap: nothing here asks anyone.
///
/// What a stranger doesn't get follows the machine's Labs flags: threads
/// are a flag, and studio and VMs need theirs as well as
/// their own setup.
pub(crate) fn features(app: &App) -> HostFeatures {
    let dir = app.control.state_dir();
    let on = crate::lab_flags::flags(dir);
    HostFeatures {
        labs: on.any(),
        dev: arugula_proto::flags::unlocked(dir),
        blocks: crate::sites::get().is_some_and(|s| s.listens()),
        block_channels: crate::sites::control_blocks(),
        vms: on.has(flags::SANDBOX_PROVIDERS) && app.mux.provider.is_some(),
        studio: on.has(flags::STUDIO) && crate::lab_hooks::labs().studio_here(),
        // A sharing setting, not a flag (native #205).
        guest_ssh: Some(crate::lab_flags::BUILT && arugula_proto::sharing::guest_ssh(dir)),
        flags: Some(on.names().map(str::to_owned).collect()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dir() -> PathBuf {
        static N: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
        let n = N.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let d = std::env::temp_dir().join(format!("ilg-hosts-{}-{n}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    fn req(name: &str, url: &str) -> AddHost {
        AddHost { name: name.into(), urls: vec![url.into()], transport: Transport::Tailnet, ssh: None }
    }

    #[test]
    fn add_replace_remove_and_persist() {
        let d = dir();
        let h = Hosts::open(&d, "geek".into());
        h.add(req("box", "https://box.example.ts.net/")).unwrap();
        h.add(req("alpha", "http://127.0.0.1:7691")).unwrap();
        let l = h.list();
        assert_eq!(l.this, "geek");
        assert_eq!(l.hosts.iter().map(|h| h.name.as_str()).collect::<Vec<_>>(), ["alpha", "box"]);
        assert_eq!(l.hosts[1].urls, ["https://box.example.ts.net"], "normalized to an origin");
        h.add(req("box", "https://box2.example.ts.net")).unwrap();
        assert_eq!(h.list().hosts.len(), 2);
        assert!(h.remove("alpha"));
        assert!(!h.remove("alpha"));
        drop(h);
        let again = Hosts::open(&d, "geek".into());
        assert_eq!(again.list().hosts.len(), 1);
        assert_eq!(again.list().hosts[0].urls, ["https://box2.example.ts.net"]);
        std::fs::remove_dir_all(d).unwrap();
    }

    #[test]
    fn bad_hosts_are_refused() {
        let d = dir();
        let h = Hosts::open(&d, "geek".into());
        assert!(h.add(req("geek", "https://x.example")).is_err(), "our own name");
        assert!(h.add(req("", "https://x.example")).is_err());
        assert!(h.add(req("a b", "https://x.example")).is_err());
        assert!(h.add(req("x", "ftp://x.example")).is_err());
        assert!(h.add(req("x", "https://x.example/path")).is_err());
        assert!(h.add(req("x", "https://user@x.example")).is_err());
        assert!(h.add(req("x", "javascript:alert(1)")).is_err());
        assert!(h.add(AddHost { name: "x".into(), urls: vec![], transport: Transport::Tailnet, ssh: None }).is_err());
        assert!(h.add(req("..", "https://x.example")).is_err(), "a directory name");
        assert!(h.add(req(".x", "https://x.example")).is_err());
        let dial = AddHost { name: "d".into(), urls: vec![], transport: Transport::DialOut, ssh: None };
        assert!(h.add(dial).unwrap_err().contains("removed"), "dial-in hosts were removed");
        assert!(h.list().hosts.is_empty());
        std::fs::remove_dir_all(d).unwrap();
    }

    /// An older daemon's `hosts.json` may hold a daemon made resident in a
    /// provider's sandbox (M4b, removed in 0.67) or a host that dialed in
    /// (M4c, removed in 0.68): those entries are dropped and the others
    /// kept, since a list that fails to parse is emptied whole; their
    /// credential files are deleted.
    #[test]
    fn old_provider_and_dial_out_hosts_are_dropped_and_the_other_hosts_kept() {
        let d = dir();
        std::fs::write(
            d.join("hosts.json"),
            serde_json::json!({ "hosts": [
                { "name": "box", "urls": ["https://box.example.ts.net"], "transport": "tailnet", "added_ms": 5, "last_seen_ms": 6 },
                { "name": "res", "urls": [], "transport": "provider",
                  "provider": { "provider": "wisp", "sandbox": "s1", "port": 7681 }, "status": "cold", "added_ms": 7 },
                { "name": "sbx", "urls": [], "transport": "dial_out", "added_ms": 9, "last_seen_ms": 10 },
                { "name": "bare", "urls": [], "transport": "ssh", "ssh": "illo@box-bare", "added_ms": 8 }
            ] })
            .to_string(),
        )
        .unwrap();
        std::fs::write(d.join("provider-tokens.json"), r#"{"res":"ilp_secret"}"#).unwrap();
        std::fs::write(d.join("host-tokens.json"), r#"{"tokens":[{"name":"sbx","digest":"ab","created_ms":1}]}"#)
            .unwrap();
        std::fs::write(d.join("invites.json"), r#"{"invites":[["abcd",9999999999999]]}"#).unwrap();
        let h = Hosts::open(&d, "geek".into());
        let l = h.list();
        assert_eq!(l.hosts.iter().map(|h| h.name.as_str()).collect::<Vec<_>>(), ["box", "bare"]);
        assert_eq!((l.hosts[0].transport, l.hosts[0].last_seen_ms), (Transport::Tailnet, Some(6)));
        assert_eq!((l.hosts[1].transport, l.hosts[1].ssh.as_deref()), (Transport::Ssh, Some("illo@box-bare")));
        for f in ["provider-tokens.json", "host-tokens.json", "invites.json"] {
            assert!(!d.join(f).exists(), "{f} is deleted: a spent credential");
        }
        // Written back without them, so the next start has nothing to drop.
        let saved = std::fs::read_to_string(d.join("hosts.json")).unwrap();
        assert!(!saved.contains("\"res\"") && !saved.contains("\"sbx\"") && !saved.contains("provider"), "{saved}");
        drop(h);
        let again = Hosts::open(&d, "geek".into());
        assert_eq!(again.list().hosts.len(), 2);
        // Nothing adds either now.
        for t in [Transport::Provider, Transport::DialOut] {
            let req = AddHost { name: "y".into(), urls: vec![], transport: t, ssh: None };
            assert!(again.add(req).unwrap_err().contains("removed"));
        }
        std::fs::remove_dir_all(d).unwrap();
    }

    /// Upgrading from a daemon before native #677, whose `invites.json`
    /// holds the waiting person invites as a bare list: control moves them
    /// into `person-invites.json` as it opens, and only once that file
    /// exists does this list delete the old copy. Until then it's left
    /// alone, whatever else is in the state dir.
    #[test]
    fn a_waiting_person_invite_list_in_invites_json_is_left_until_control_moved_it() {
        let d = dir();
        let list = r#"[{"session":1,"who":"x","role":"viewer"}]"#;
        std::fs::write(d.join("invites.json"), list).unwrap();
        let _ = Hosts::open(&d, "geek".into());
        assert_eq!(std::fs::read_to_string(d.join("invites.json")).unwrap(), list, "left for control");
        // Control moved it: the copy goes.
        std::fs::write(d.join("person-invites.json"), list).unwrap();
        let _ = Hosts::open(&d, "geek".into());
        assert!(!d.join("invites.json").exists(), "the moved-out copy is deleted");
        assert_eq!(std::fs::read_to_string(d.join("person-invites.json")).unwrap(), list, "the move is untouched");
        // The host list's own shape goes at once, with or without that file.
        std::fs::write(d.join("invites.json"), r#"{"invites":[["abcd",9999999999999]]}"#).unwrap();
        let _ = Hosts::open(&d, "geek".into());
        assert!(!d.join("invites.json").exists(), "spent host invites are deleted");
        // Anything else there is nobody's to delete.
        std::fs::write(d.join("invites.json"), r#"{"something":"else"}"#).unwrap();
        let _ = Hosts::open(&d, "geek".into());
        assert!(d.join("invites.json").exists());
        std::fs::remove_dir_all(d).unwrap();
    }

    #[test]
    fn ssh_hosts_keep_only_their_destination() {
        let d = dir();
        let h = Hosts::open(&d, "geek".into());
        let ssh = |urls: Vec<String>, dest: Option<&str>| AddHost {
            name: "box".into(),
            urls,
            transport: Transport::Ssh,
            ssh: dest.map(String::from),
        };
        let added = h.add(ssh(vec![], Some(" illo@box-bare "))).unwrap();
        assert_eq!(
            (added.transport, added.ssh.as_deref(), added.urls.len()),
            (Transport::Ssh, Some("illo@box-bare"), 0)
        );
        assert!(h.add(ssh(vec!["https://box".into()], Some("box"))).is_err(), "no URLs");
        assert!(h.add(ssh(vec![], None)).is_err(), "needs a destination");
        for bad in ["-oProxyCommand=sh", "box; id", "a b", "$(id)"] {
            assert!(h.add(ssh(vec![], Some(bad))).is_err(), "{bad}");
        }
        std::fs::remove_dir_all(d).unwrap();
    }
}
