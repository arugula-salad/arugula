//! Who called from where (#663): the devices seen under each tailnet
//! login, and tagged devices that were refused.
//!
//! Grants go to a tailnet login, so everyone signed in as that login gets
//! its role, on every device: two people sharing one Tailscale account
//! can't be told apart. That's allowed (lex00, 2026-10-08), but said: the
//! access list (`GET /api/acl`, `arugula access`, the share dialog) and
//! `arugula status` name a login seen from more than one device. A tagged
//! device has no login at all, so nothing can be granted to it and it's
//! refused (`access.rs`); those say which tagged devices tried.
//!
//! Devices come from tailscaled's WhoIs: of the connection itself, or, for a
//! request `tailscale serve` passed on, of the address in its
//! `X-Forwarded-For`. Kept in memory: a restart starts the list again.

use std::{
    collections::{BTreeMap, HashMap},
    sync::Mutex,
};

use serde::Serialize;

/// A device tailscaled named, with its tags (none for a person's).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Device {
    pub name: String,
    pub tags: Vec<String>,
}

/// One device seen, and when last (ms).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Seen {
    pub device: String,
    pub at: u64,
}

/// A login seen from more than one device.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SharedLogin {
    pub login: String,
    pub devices: Vec<Seen>,
}

/// A tagged device that called and was refused.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TaggedCaller {
    pub device: String,
    pub tags: Vec<String>,
    pub at: u64,
}

/// What `GET /api/acl` says of callers.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct Report {
    pub shared: Vec<SharedLogin>,
    pub tagged: Vec<TaggedCaller>,
}

#[derive(Default)]
struct Inner {
    /// Login (lowercase) → device → last seen.
    logins: HashMap<String, BTreeMap<String, u64>>,
    /// Device → its tags and when last seen.
    tagged: BTreeMap<String, (Vec<String>, u64)>,
}

#[derive(Default)]
pub struct Callers {
    inner: Mutex<Inner>,
}

/// More devices than this under one login aren't kept (the oldest goes).
const DEVICES: usize = 32;

impl Callers {
    /// A call from `device`, signed in as `login` (`None`: tagged).
    pub fn saw(&self, login: Option<&str>, device: &Device, at: u64) {
        let mut i = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        match login {
            Some(login) => {
                let devices = i.logins.entry(login.to_ascii_lowercase()).or_default();
                devices.insert(device.name.clone(), at);
                if devices.len() > DEVICES {
                    let oldest = devices.iter().min_by_key(|(_, t)| **t).map(|(d, _)| d.clone());
                    if let Some(d) = oldest {
                        devices.remove(&d);
                    }
                }
            }
            None => {
                i.tagged.insert(device.name.clone(), (device.tags.clone(), at));
                if i.tagged.len() > DEVICES {
                    let oldest = i.tagged.iter().min_by_key(|(_, (_, t))| *t).map(|(d, _)| d.clone());
                    if let Some(d) = oldest {
                        i.tagged.remove(&d);
                    }
                }
            }
        }
    }

    /// The logins in `granted` seen from more than one device, and the
    /// tagged devices that called. The owner's own devices aren't
    /// listed: they're all the owner's.
    pub fn report(&self, granted: &[String]) -> Report {
        let i = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        let mut shared: Vec<SharedLogin> = granted
            .iter()
            .filter_map(|login| {
                let devices = i.logins.get(&login.to_ascii_lowercase())?;
                (devices.len() > 1).then(|| SharedLogin {
                    login: login.clone(),
                    devices: devices.iter().map(|(d, at)| Seen { device: d.clone(), at: *at }).collect(),
                })
            })
            .collect();
        shared.sort_by(|a, b| a.login.cmp(&b.login));
        shared.dedup_by(|a, b| a.login == b.login);
        let tagged = i
            .tagged
            .iter()
            .map(|(d, (tags, at))| TaggedCaller { device: d.clone(), tags: tags.clone(), at: *at })
            .collect();
        Report { shared, tagged }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dev(name: &str) -> Device {
        Device { name: name.into(), tags: vec![] }
    }

    #[test]
    fn a_granted_login_from_two_devices_is_shared() {
        let c = Callers::default();
        c.saw(Some("Bob@x.com"), &dev("laptop"), 1);
        c.saw(Some("bob@x.com"), &dev("ipad"), 2);
        c.saw(Some("bob@x.com"), &dev("laptop"), 3);
        c.saw(Some("ann@x.com"), &dev("phone"), 4);
        let r = c.report(&["bob@x.com".into(), "ann@x.com".into()]);
        assert_eq!(r.shared.len(), 1, "ann used one device");
        assert_eq!(r.shared[0].login, "bob@x.com");
        assert_eq!(
            r.shared[0].devices,
            [Seen { device: "ipad".into(), at: 2 }, Seen { device: "laptop".into(), at: 3 }]
        );
        // Not granted (the owner, or nobody yet): not listed.
        assert!(c.report(&["ann@x.com".into()]).shared.is_empty());
    }

    #[test]
    fn tagged_devices_are_listed_with_their_tags() {
        let c = Callers::default();
        c.saw(None, &Device { name: "ci-1".into(), tags: vec!["tag:ci".into()] }, 5);
        let r = c.report(&[]);
        assert_eq!(r.tagged, [TaggedCaller { device: "ci-1".into(), tags: vec!["tag:ci".into()], at: 5 }]);
    }

    #[test]
    fn keeps_a_bounded_number_of_devices() {
        let c = Callers::default();
        for n in 0..(DEVICES as u64 + 5) {
            c.saw(Some("bob@x.com"), &dev(&format!("d{n}")), n);
        }
        let r = c.report(&["bob@x.com".into()]);
        assert_eq!(r.shared[0].devices.len(), DEVICES);
        assert!(!r.shared[0].devices.iter().any(|s| s.device == "d0"), "the oldest went");
    }
}
