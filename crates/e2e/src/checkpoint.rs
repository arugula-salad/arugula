//! What a machine keeps of what it already checked, when control sends
//! certificates and rosters again: removals only add up, and a team's
//! roster only moves forward from the version it holds.
//!
//! Control may leave things out of an answer (it can always withhold),
//! but what a machine learned stays learned: a removal it saw isn't
//! undone by control forgetting it, and a roster isn't taken back to an
//! older version or swapped for another history. A daemon keeps these in
//! `control.json`; the owner's page keeps its rosters in the browser.

use std::collections::HashSet;

use crate::{
    Cert, Revocation, Trust,
    team::{AccountCerts, Roster, TeamPin},
};

/// `new`, and every one of `old` it leaves out: what was held is added
/// to, never taken from.
pub fn kept<T: PartialEq + Clone>(mut new: Vec<T>, old: &[T]) -> Vec<T> {
    for o in old {
        if !new.contains(o) {
            new.push(o.clone());
        }
    }
    new
}

/// The devices `certs` and `revocations` hold removed, for `trust`'s
/// account: each named by a removal and not trusted, and each approved by
/// one of those that isn't trusted either (what it approved after it was
/// removed). A device trusted despite a removal naming it (one whose
/// signer doesn't check out) isn't among them.
pub fn removed(trust: &Trust, certs: &[Cert], revocations: &[Revocation]) -> HashSet<String> {
    let trusted = trust.evaluate(certs, revocations);
    let out_now = |id: &str| !trusted.devices.contains_key(id);
    let mut out: HashSet<String> = revocations
        .iter()
        .filter(|r| r.account == trust.account && out_now(&r.device))
        .map(|r| r.device.clone())
        .collect();
    loop {
        let before = out.len();
        for c in certs {
            if c.account == trust.account && out_now(&c.device) && out.contains(&c.approver) {
                out.insert(c.device.clone());
            }
        }
        if out.len() == before {
            return out;
        }
    }
}

/// An account's certificates and removals after a refresh: `new` as
/// control sends them now, over `old` as last held. Every removal held is
/// kept. And no device `old` held removed may count again: if `new`'s
/// certificates would let one back in (by leaving out the certificate a
/// removal rested on, say), they're refused and `old`'s stay, with every
/// removal. The flag says they were refused.
pub fn refresh_account(
    trust: &Trust,
    new: (Vec<Cert>, Vec<Revocation>),
    old: (&[Cert], &[Revocation]),
) -> (Vec<Cert>, Vec<Revocation>, bool) {
    let revocations = kept(new.1, old.1);
    let gone = removed(trust, old.0, old.1);
    if gone.is_empty() {
        return (new.0, revocations, false);
    }
    let now = trust.evaluate(&new.0, &revocations);
    if gone.iter().any(|id| now.devices.contains_key(id)) {
        return (old.0.to_vec(), revocations, true);
    }
    (new.0, revocations, false)
}

/// Where a team's roster stands after `history` (oldest first, as control
/// sends it), from `held`: the version this machine last checked, or none
/// for a first look (from version 1, which the founder `pin` names
/// signs). Versions older than `held` are passed over; one at `held`'s
/// version must be `held`; each newer one must be the very next and
/// follow the one before it (control numbers them that way). The first
/// that doesn't stops the walk, and its version comes back with where it
/// stopped: a control that leaves versions out, or sends older ones or
/// another history, leaves `held` as it is.
pub fn advance(
    held: Option<&Roster>,
    history: impl IntoIterator<Item = Roster>,
    pin: &TeamPin,
    certs: &AccountCerts,
) -> (Option<Roster>, Option<u64>) {
    let mut cur = held.cloned();
    for r in history {
        if let Some(c) = &cur {
            if r.version < c.version {
                continue;
            }
            if r.version == c.version {
                if r == *c {
                    continue;
                }
                return (cur, Some(r.version));
            }
        }
        let next = cur.as_ref().map_or(1, |c| c.version + 1);
        if r.version != next || !r.follows(cur.as_ref(), pin, certs) {
            return (cur, Some(r.version));
        }
        cur = Some(r);
    }
    (cur, None)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        DeviceKeys, Kind,
        team::{Member, TeamRole},
    };

    fn root(account: &str) -> (DeviceKeys, Cert) {
        let k = DeviceKeys::generate();
        let mut c = Cert::new(&k, account, Kind::Browser, "laptop");
        c.sign_with(&k);
        (k, c)
    }

    fn approved(account: &str, by: &DeviceKeys, name: &str) -> (DeviceKeys, Cert) {
        let k = DeviceKeys::generate();
        let mut c = Cert::new(&k, account, Kind::Browser, name);
        c.sign_with(by);
        (k, c)
    }

    /// Root approves a laptop and a phone; the laptop removes the phone.
    /// Control then leaves the removal out, or the laptop's certificate
    /// (what the removal rests on): the phone stays out either way.
    #[test]
    fn a_removal_stays_when_control_leaves_it_or_its_signer_out() {
        let (rk, rc) = root("a");
        let (lk, lc) = approved("a", &rk, "laptop2");
        let (pk, pc) = approved("a", &rk, "phone");
        let trust = Trust { account: "a".into(), root: rc.device.clone() };
        let certs = vec![rc.clone(), lc.clone(), pc.clone()];
        let rev = Revocation::new("a", &pc.device, &lk);
        let held = (certs.clone(), vec![rev.clone()]);
        assert!(trust.evaluate(&held.0, &held.1).get(&pc.device).is_none());

        // The removal left out.
        let (c, r, refused) = refresh_account(&trust, (certs.clone(), vec![]), (&held.0, &held.1));
        assert!(!refused);
        assert_eq!(r, vec![rev.clone()]);
        assert!(trust.evaluate(&c, &r).get(&pc.device).is_none(), "the phone counts again");

        // The laptop's certificate left out: the removal wouldn't count.
        let without_signer = vec![rc.clone(), pc.clone()];
        let (c, r, refused) = refresh_account(&trust, (without_signer, vec![rev.clone()]), (&held.0, &held.1));
        assert!(refused);
        assert!(trust.evaluate(&c, &r).get(&pc.device).is_none(), "the phone counts again");
        assert!(trust.evaluate(&c, &r).get(&lc.device).is_some());

        // What the phone approves after it's removed stays out with it.
        std::thread::sleep(std::time::Duration::from_millis(5));
        let (_, tc) = approved("a", &pk, "tablet");
        let mut later = held.0.clone();
        later.push(tc.clone());
        let held2 = (later.clone(), held.1.clone());
        let (c, r, refused) =
            refresh_account(&trust, (vec![rc.clone(), pc.clone(), tc.clone()], vec![]), (&held2.0, &held2.1));
        assert!(refused);
        assert!(trust.evaluate(&c, &r).get(&tc.device).is_none());

        // A new device control adds, with nothing brought back, is taken.
        let (_, nc) = approved("a", &rk, "new");
        let mut more = certs.clone();
        more.push(nc.clone());
        let (c, r, refused) = refresh_account(&trust, (more, vec![rev]), (&held.0, &held.1));
        assert!(!refused);
        assert!(trust.evaluate(&c, &r).get(&nc.device).is_some());
    }

    fn roster(version: u64, members: Vec<Member>, by: &DeviceKeys) -> Roster {
        let mut r = Roster {
            v: 1,
            team: "t1".into(),
            name: "Acme".into(),
            version,
            at: version,
            members,
            spent: vec![],
            redeem: None,
            by: String::new(),
            sig: String::new(),
        };
        r.sign_with(by);
        r
    }

    /// From the version held, a newer one that follows is taken; an older
    /// history, another version at the held one, or a gap leaves it.
    #[test]
    fn a_roster_only_moves_forward_from_the_one_held() {
        let ((ok, oc), (_, ec)) = (root("o"), root("e"));
        let m = |a: &str, c: &Cert, role| Member { account: a.into(), root: c.device.clone(), role, name: a.into() };
        let pin = TeamPin { team: "t1".into(), founder: "o".into(), founder_root: oc.device.clone() };
        let certs: AccountCerts =
            [("o".to_owned(), (vec![oc.clone()], vec![])), ("e".to_owned(), (vec![ec.clone()], vec![]))].into();
        let v1 = roster(1, vec![m("o", &oc, TeamRole::Owner)], &ok);
        let v2 = roster(2, vec![m("o", &oc, TeamRole::Owner), m("e", &ec, TeamRole::Editor)], &ok);
        let v3 = roster(3, vec![m("o", &oc, TeamRole::Owner)], &ok);

        // A first look checks the whole chain from the founder.
        assert_eq!(advance(None, [v1.clone(), v2.clone()], &pin, &certs), (Some(v2.clone()), None));
        // From v3, the history up to v2 is old: v3 stays.
        assert_eq!(advance(Some(&v3), [v1.clone(), v2.clone()], &pin, &certs), (Some(v3.clone()), None));
        // The whole history again, through v3: v3.
        assert_eq!(advance(Some(&v3), [v1.clone(), v2.clone(), v3.clone()], &pin, &certs), (Some(v3.clone()), None));
        // Another v3 (signed by an owner, but not the one held): refused.
        let other3 = roster(3, vec![m("o", &oc, TeamRole::Owner), m("e", &ec, TeamRole::Owner)], &ok);
        assert_eq!(advance(Some(&v3), [v1.clone(), v2.clone(), other3], &pin, &certs), (Some(v3.clone()), Some(3)));
        // A v4 that follows v3 is taken; one that skips v3's owners isn't.
        let v4 = roster(4, vec![m("o", &oc, TeamRole::Owner), m("e", &ec, TeamRole::Viewer)], &ok);
        assert_eq!(advance(Some(&v3), [v4.clone()], &pin, &certs), (Some(v4.clone()), None));
        // A version left out in between: the walk stops before the gap.
        assert_eq!(advance(Some(&v1), [v2.clone(), v4.clone()], &pin, &certs), (Some(v2.clone()), Some(4)));
        assert_eq!(advance(None, [v2.clone()], &pin, &certs), (None, Some(2)), "a first look starts at 1");
        let ek = DeviceKeys::generate();
        let bad4 = roster(4, vec![m("o", &oc, TeamRole::Owner), m("e", &ec, TeamRole::Owner)], &ek);
        assert_eq!(advance(Some(&v3), [bad4], &pin, &certs), (Some(v3), Some(4)));
    }
}
