//! What a machine keeps of what it already checked, when control sends
//! certificates and rosters again: removals only add up, and a team's
//! roster only moves forward from the version it holds.
//!
//! Control may leave things out of an answer (it can always withhold),
//! but what a machine learned stays learned: a removal it saw isn't
//! undone by control forgetting it, and a roster isn't taken back to an
//! older version or swapped for another history. A daemon keeps these in
//! `control.json`; the owner's page keeps its own in the browser.
//!
//! Each certificate and removal is stamped with when this machine first
//! saw it ([`Seen`]): a certificate first seen after the removal of the
//! device that approved it is dropped, whatever date it carries, so a
//! removed device's key can't approve a new device by dating it before
//! its removal. A team's lock is held the same way: only an owner's newer
//! signed word changes it ([`take_lock`]).

use std::collections::{BTreeMap, HashSet};

use serde::{Deserialize, Serialize};

use crate::{
    Cert, Revocation, Trust,
    team::{AccountCerts, Roster, TeamLock, TeamPin},
};

/// In which round of looking this machine first saw each certificate and
/// removal, by its signature ([`seen_key`]). Rounds are counted here, one
/// per refresh that brings something new, so a clock set back can't
/// reorder what was seen. Round 0 is what was held before stamps were
/// kept.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Seen {
    /// The last round that brought something new.
    #[serde(default)]
    pub round: u64,
    #[serde(default)]
    pub at: BTreeMap<String, u64>,
}

impl Seen {
    /// The round a refresh stamps what's new with.
    pub fn next(&self) -> u64 {
        self.round + 1
    }

    /// After a refresh in `round`: count it if it stamped anything.
    pub fn settle(&mut self, round: u64) {
        if self.at.values().any(|&r| r == round) {
            self.round = self.round.max(round);
        }
    }

    pub fn get(&self, sig: &str) -> Option<u64> {
        self.at.get(&seen_key(sig)).copied()
    }
}

/// What a certificate or removal is stamped under: its signature, which
/// no other signed thing has.
pub fn seen_key(sig: &str) -> String {
    sig.to_owned()
}

/// Stamp each of `certs` and `revocations` not stamped yet with `round`.
pub fn stamp(seen: &mut Seen, certs: &[Cert], revocations: &[Revocation], round: u64) {
    for sig in certs.iter().map(|c| &c.sig).chain(revocations.iter().map(|r| &r.sig)) {
        seen.at.entry(seen_key(sig)).or_insert(round);
    }
}

/// `certs` without those first seen after the removal of the device that
/// approved them was first seen. A device approved before its approver
/// was removed was seen no later than the removal, by a machine that
/// held both; one dated before the removal but first seen after it was
/// made with a removed device's key. A removal never stamped counts as
/// seen first, and a certificate never stamped as seen last: a missing
/// stamp never lets one in.
pub fn drop_late(trust: &Trust, certs: Vec<Cert>, revocations: &[Revocation], seen: &Seen) -> Vec<Cert> {
    let gone = removed(trust, &certs, revocations);
    if gone.is_empty() {
        return certs;
    }
    let cert_at = |sig: &str| seen.get(sig).unwrap_or(u64::MAX);
    let removed_at = |device: &str| {
        revocations
            .iter()
            .filter(|r| r.account == trust.account && r.device == device)
            .map(|r| seen.get(&r.sig).unwrap_or(0))
            .min()
    };
    let at = cert_at;
    certs
        .into_iter()
        .filter(|c| {
            !(c.account == trust.account
                && gone.contains(&c.approver)
                && removed_at(&c.approver).is_some_and(|t| at(&c.sig) > t))
        })
        .collect()
}

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
/// kept, and everything is stamped in `seen` when first seen (in
/// `round`; what `old` held unstamped, from before stamps were kept, in
/// round 0); a certificate first seen after its approver's removal is
/// dropped ([`drop_late`]). Only what was held is let off: a certificate
/// first supplied in the refresh that first stamps is judged like any. And no device `old` held removed may count again: if
/// `new`'s certificates would let one back in (by leaving out the
/// certificate a removal rested on, say), they're refused and `old`'s
/// stay, with every removal. The flag says they were refused.
pub fn refresh_account(
    trust: &Trust,
    new: (Vec<Cert>, Vec<Revocation>),
    old: (&[Cert], &[Revocation]),
    seen: &mut Seen,
    round: u64,
) -> (Vec<Cert>, Vec<Revocation>, bool) {
    stamp(seen, old.0, old.1, 0);
    let revocations = kept(new.1, old.1);
    stamp(seen, &new.0, &revocations, round);
    let certs = drop_late(trust, new.0, &revocations, seen);
    let gone = removed(trust, old.0, old.1);
    if gone.is_empty() {
        return (certs, revocations, false);
    }
    let after = trust.evaluate(&certs, &revocations);
    if gone.iter().any(|id| after.devices.contains_key(id)) {
        return (drop_late(trust, old.0.to_vec(), &revocations, seen), revocations, true);
    }
    (certs, revocations, false)
}

/// Whether a team is locked after control's answer: `got` (control's
/// signed record) over `held` (the record this machine holds, and whether
/// it holds the team locked). A record counts only if an owner in
/// `roster` (the roster held) signed it ([`TeamLock::signed_in`]) and
/// it's newer than the one held. Without one, the team stays as held,
/// locked if control's plain word (`plain`, from before records) says so:
/// control may lock a team (it can refuse service anyway), never unlock
/// it.
pub fn take_lock(
    held: (bool, Option<&TeamLock>),
    got: Option<TeamLock>,
    plain: bool,
    roster: Option<&Roster>,
    certs: &AccountCerts,
) -> (bool, Option<TeamLock>) {
    if let (Some(l), Some(r)) = (got, roster)
        && l.signed_in(r, certs)
        && held.1.is_none_or(|h| l.at > h.at)
    {
        return (l.locked, Some(l));
    }
    (held.0 || plain, held.1.cloned())
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
        let (c, r, refused) =
            refresh_account(&trust, (certs.clone(), vec![]), (&held.0, &held.1), &mut Seen::default(), 1);
        assert!(!refused);
        assert_eq!(r, vec![rev.clone()]);
        assert!(trust.evaluate(&c, &r).get(&pc.device).is_none(), "the phone counts again");

        // The laptop's certificate left out: the removal wouldn't count.
        let without_signer = vec![rc.clone(), pc.clone()];
        let (c, r, refused) =
            refresh_account(&trust, (without_signer, vec![rev.clone()]), (&held.0, &held.1), &mut Seen::default(), 1);
        assert!(refused);
        assert!(trust.evaluate(&c, &r).get(&pc.device).is_none(), "the phone counts again");
        assert!(trust.evaluate(&c, &r).get(&lc.device).is_some());

        // What the phone approves after it's removed stays out with it.
        std::thread::sleep(std::time::Duration::from_millis(5));
        let (_, tc) = approved("a", &pk, "tablet");
        let mut later = held.0.clone();
        later.push(tc.clone());
        let held2 = (later.clone(), held.1.clone());
        let (c, r, refused) = refresh_account(
            &trust,
            (vec![rc.clone(), pc.clone(), tc.clone()], vec![]),
            (&held2.0, &held2.1),
            &mut Seen::default(),
            1,
        );
        assert!(refused);
        assert!(trust.evaluate(&c, &r).get(&tc.device).is_none());

        // A new device control adds, with nothing brought back, is taken.
        let (_, nc) = approved("a", &rk, "new");
        let mut more = certs.clone();
        more.push(nc.clone());
        let (c, r, refused) = refresh_account(&trust, (more, vec![rev]), (&held.0, &held.1), &mut Seen::default(), 1);
        assert!(!refused);
        assert!(trust.evaluate(&c, &r).get(&nc.device).is_some());
    }

    /// A certificate a removed device's key signs, dated before the
    /// removal, is dropped when this machine saw the removal first; one
    /// that comes with the removal (what the device approved before it
    /// was removed) stays. A store from before stamps (none held) stamps
    /// what it holds when it first refreshes, and drops nothing for it.
    #[test]
    fn a_certificate_first_seen_after_its_approvers_removal_is_dropped() {
        let (rk, rc) = root("a");
        let (lk, lc) = approved("a", &rk, "laptop");
        let trust = Trust { account: "a".into(), root: rc.device.clone() };
        let rev = Revocation::new("a", &lc.device, &rk);
        let backdated = |name: &str| {
            let k = DeviceKeys::generate();
            let mut c = Cert::new(&k, "a", Kind::Browser, name);
            c.created = rev.at.saturating_sub(1000);
            c.sign_with(&lk);
            c
        };
        let early = backdated("tablet");
        let late = backdated("stranger");
        let trusted = |c: &[Cert], r: &[Revocation], id: &str| trust.evaluate(c, r).get(id).is_some();
        // Its date alone lets it in.
        assert!(trusted(&[rc.clone(), lc.clone(), late.clone()], std::slice::from_ref(&rev), &late.device));

        let mut seen = Seen::default();
        // Refresh 1: the laptop, the tablet it approved, and its removal.
        let held = vec![rc.clone(), lc.clone(), early.clone()];
        let round = seen.next();
        let (c1, r1, _) = refresh_account(&trust, (held.clone(), vec![rev.clone()]), (&[], &[]), &mut seen, round);
        seen.settle(round);
        assert!(trusted(&c1, &r1, &early.device), "what it approved before, seen with the removal");
        // A refresh that brings nothing new doesn't count as a round.
        let round = seen.next();
        refresh_account(&trust, (held.clone(), vec![rev.clone()]), (&c1, &r1), &mut seen, round);
        seen.settle(round);
        assert_eq!(seen.round, 1);
        // Refresh 2: a certificate from the removed laptop, dated before.
        // Rounds are counted, not read off a clock: whatever the clock
        // says, it's a later round.
        let mut more = held.clone();
        more.push(late.clone());
        let round = seen.next();
        let (c2, r2, _) = refresh_account(&trust, (more.clone(), vec![]), (&c1, &r1), &mut seen, round);
        assert!(!trusted(&c2, &r2, &late.device), "a removed device's late certificate counts");
        assert!(trusted(&c2, &r2, &early.device));

        // From before stamps: what was held (the laptop, the tablet, the
        // removal) is let off in round 0; a certificate first supplied in
        // that refresh is judged like any, and dropped.
        let mut fresh = Seen::default();
        let round = fresh.next();
        let (c3, r3, _) = refresh_account(&trust, (more.clone(), vec![]), (&held, &r1), &mut fresh, round);
        assert!(trusted(&c3, &r3, &early.device), "what was held from before");
        assert!(!trusted(&c3, &r3, &late.device), "a certificate first supplied as stamps begin counts");
        assert_eq!((fresh.get(&early.sig), fresh.get(&rev.sig), fresh.get(&late.sig)), (Some(0), Some(0), Some(1)));
        // Held from before, the late one too: let off.
        let mut fresh = Seen::default();
        let (c4, r4, _) = refresh_account(&trust, (more.clone(), vec![]), (&more, &r1), &mut fresh, 1);
        assert!(trusted(&c4, &r4, &late.device));
        // A removal with no stamp at all counts as seen first: it never
        // lets a stamped certificate in.
        let mut partial = Seen::default();
        partial.at.insert(seen_key(&late.sig), 5);
        let kept_certs = drop_late(&trust, more.clone(), std::slice::from_ref(&rev), &partial);
        assert!(!kept_certs.iter().any(|c| c.sig == late.sig));
    }

    /// A lock counts only from an owner in the roster held, newer than the
    /// one held; without one the team stays as held, and control's plain
    /// word can lock it but never unlock it.
    #[test]
    fn a_teams_lock_changes_only_by_an_owners_newer_word() {
        let ((ok, oc), (ek, ec)) = (root("o"), root("e"));
        let m = |a: &str, c: &Cert, role| Member { account: a.into(), root: c.device.clone(), role, name: a.into() };
        let r = roster(2, vec![m("o", &oc, TeamRole::Owner), m("e", &ec, TeamRole::Editor)], &ok);
        let certs: AccountCerts =
            [("o".to_owned(), (vec![oc.clone()], vec![])), ("e".to_owned(), (vec![ec.clone()], vec![]))].into();
        let lock = |locked, at, by: &DeviceKeys| {
            let mut l = TeamLock { team: "t1".into(), locked, version: 2, at, by: String::new(), sig: String::new() };
            l.sign_with(by);
            l
        };
        let locked = lock(true, 10, &ok);
        assert_eq!(
            take_lock((false, None), Some(locked.clone()), false, Some(&r), &certs),
            (true, Some(locked.clone()))
        );
        // Control unlocking by its plain word, or with no record: locked.
        assert_eq!(take_lock((true, Some(&locked)), None, false, Some(&r), &certs), (true, Some(locked.clone())));
        // An unlock an editor signed, or one older than the lock held.
        assert!(take_lock((true, Some(&locked)), Some(lock(false, 20, &ek)), false, Some(&r), &certs).0);
        assert!(take_lock((true, Some(&locked)), Some(lock(false, 5, &ok)), false, Some(&r), &certs).0);
        // Bent after signing, or against a version the machine doesn't have.
        let mut bent = lock(false, 20, &ok);
        bent.at = 21;
        assert!(take_lock((true, Some(&locked)), Some(bent), false, Some(&r), &certs).0);
        let mut ahead = TeamLock { version: 3, ..lock(false, 20, &ok) };
        ahead.sign_with(&ok);
        assert!(take_lock((true, Some(&locked)), Some(ahead), false, Some(&r), &certs).0);
        // An owner's newer unlock: unlocked.
        let open = lock(false, 20, &ok);
        assert_eq!(
            take_lock((true, Some(&locked)), Some(open.clone()), false, Some(&r), &certs),
            (false, Some(open.clone()))
        );
        // Control's plain word locks (from before records), never unlocks.
        assert!(take_lock((false, Some(&open)), None, true, Some(&r), &certs).0);
        assert!(!take_lock((false, Some(&open)), None, false, Some(&r), &certs).0);
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
