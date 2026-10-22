//! Teams (M19): who is in one, signed by the team's owners, so control
//! can't add a member.
//!
//! A roster lists each member's account, the root device that account was
//! known by when added, and their role. It is versioned and signed by a
//! device of an owner: version 1 by the founder, every later one by an
//! owner in the version before it. A team daemon pins the founder when it
//! joins and accepts only a roster that follows from what it has.
//!
//! ```text
//! illogical team v1
//! team <id>
//! name <text>
//! version <n>
//! at <ms>
//! member <account> <root device> <owner|editor|viewer> <name>
//! by <device id>
//! ```
//!
//! A presigned invite lets the invitee write the next version themselves:
//! an owner's device signs an [`Invite`] naming a one-time key whose
//! private half lives only in the invite link, and the invitee's roster
//! (`v: 2`) adds them, carries that key's signature over who they are, and
//! marks the invite spent; the one-time key signs that version too. Control never holds the one-time key, so it
//! can't redeem an invite for an account of its own.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::{
    Cert, DeviceKeys, Revocation, Trust,
    cert::{Trusted, verify_hex},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TeamRole {
    Owner,
    Editor,
    Viewer,
}

impl TeamRole {
    pub fn as_str(self) -> &'static str {
        match self {
            TeamRole::Owner => "owner",
            TeamRole::Editor => "editor",
            TeamRole::Viewer => "viewer",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Member {
    pub account: String,
    /// The account's root device, as the owner who added them saw it.
    pub root: String,
    pub role: TeamRole,
    /// To show (a login); no spaces or control characters.
    pub name: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Roster {
    pub v: u32,
    pub team: String,
    pub name: String,
    pub version: u64,
    pub at: u64,
    pub members: Vec<Member>,
    /// Invites already redeemed (v2), so each works once.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub spent: Vec<Spent>,
    /// The presigned invite this version redeems (v2), when an invitee
    /// rather than an owner signs it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub redeem: Option<Redeem>,
    /// The signing device.
    pub by: String,
    pub sig: String,
}

/// An invite an owner's device signed ahead of time: whoever holds the
/// private half of `key` may add themselves, once, as `role`, until
/// `expires`.
///
/// ```text
/// illogical team invite v1
/// team <id>
/// role <editor|viewer>
/// expires <ms>
/// key <ed25519 public key, hex>
/// by <device id>
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Invite {
    pub team: String,
    pub role: TeamRole,
    pub expires: u64,
    /// The one-time key; it also names the invite.
    pub key: String,
    /// The owner's signing device.
    pub by: String,
    pub sig: String,
}

impl Invite {
    pub fn body(&self) -> String {
        // Frozen (#504): this and every signed body below; see `frozen.rs`.
        format!(
            "illogical team invite v1\nteam {}\nrole {}\nexpires {}\nkey {}\nby {}\n",
            self.team,
            self.role.as_str(),
            self.expires,
            self.key,
            self.by
        )
    }

    pub fn sign_with(&mut self, keys: &DeviceKeys) {
        self.by = keys.id();
        self.sig = hex::encode(keys.signature(self.body().as_bytes()));
    }

    /// What the one-time key signs: this member, at this version, so the
    /// proof can't be moved to another account or replayed later.
    ///
    /// ```text
    /// illogical team redeem v1
    /// team <id>
    /// version <n>
    /// member <account> <root device> <role> <name>
    /// key <one-time key>
    /// ```
    pub fn redeem_body(&self, version: u64, m: &Member) -> String {
        format!(
            "illogical team redeem v1\nteam {}\nversion {version}\nmember {} {} {} {}\nkey {}\n",
            self.team,
            m.account,
            m.root,
            m.role.as_str(),
            m.name,
            self.key
        )
    }
}

/// An *Ask me first* invite (#762): an owner's device signs it with a
/// one-time key whose private half lives only in the link, as a presigned
/// [`Invite`] does. Whoever uses the link proves they held it by signing
/// [`AskInvite::ask_body`] (their account and root) with that key, and an
/// owner then admits them by signing the next roster (a plain v1, so older
/// daemons follow it). Control never holds the key, so it can't put a
/// request of its own in front of an owner. Its own domain, so an owner
/// can't be made to have presigned one.
///
/// ```text
/// arugula team ask invite v1
/// team <id>
/// role <owner|editor|viewer>
/// made <ms>
/// expires <ms>
/// key <ed25519 public key, hex>
/// by <device id>
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AskInvite {
    pub team: String,
    pub role: TeamRole,
    /// When the owner made the link, as their device's clock said.
    pub made: u64,
    pub expires: u64,
    /// The one-time key; it also names the invite.
    pub key: String,
    /// The owner's signing device.
    pub by: String,
    pub sig: String,
}

impl AskInvite {
    pub fn body(&self) -> String {
        format!(
            "arugula team ask invite v1\nteam {}\nrole {}\nmade {}\nexpires {}\nkey {}\nby {}\n",
            self.team,
            self.role.as_str(),
            self.made,
            self.expires,
            self.key,
            self.by
        )
    }

    pub fn sign_with(&mut self, keys: &DeviceKeys) {
        self.by = keys.id();
        self.sig = hex::encode(keys.signature(self.body().as_bytes()));
    }

    /// Signed by `by`, a device that may approve. One for an owner or an
    /// editor also needs `by` to be a browser all the way back to its
    /// account's root ([`AskInvite::raises`], `Trust::browser_chained`),
    /// as the roster that admits them does.
    pub fn signed_by(&self, by: &Cert) -> bool {
        by.device == self.by && by.kind.approves() && verify_hex(&by.sign, self.body().as_bytes(), &self.sig)
    }

    /// Whether it makes someone an owner or an editor.
    pub fn raises(&self) -> bool {
        self.role != TeamRole::Viewer
    }

    /// What the one-time key signs for whoever used the link: their
    /// account and its root device, so the proof can't be moved to
    /// another account.
    ///
    /// ```text
    /// arugula team ask v1
    /// team <id>
    /// account <account>
    /// root <root device>
    /// key <one-time key>
    /// ```
    pub fn ask_body(&self, account: &str, root: &str) -> String {
        format!("arugula team ask v1\nteam {}\naccount {account}\nroot {root}\nkey {}\n", self.team, self.key)
    }

    /// Whether `proof` is the one-time key's signature for `account` and
    /// `root`.
    pub fn proves(&self, account: &str, root: &str, proof: &str) -> bool {
        verify_hex(&self.key, self.ask_body(account, root).as_bytes(), proof)
    }
}

/// An invite that's been used, kept until it would have expired anyway.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Spent {
    pub key: String,
    pub expires: u64,
}

/// A roster version written by an invitee: the invite, and the one-time
/// key's signature over [`Invite::redeem_body`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Redeem {
    pub invite: Invite,
    pub proof: String,
}

/// What a team daemon pins when it joins: the team and its founder.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TeamPin {
    pub team: String,
    pub founder: String,
    pub founder_root: String,
}

impl TeamPin {
    /// What the approving device signs to put a joining daemon in this
    /// team (#100). The team is picked at approval, so a daemon pins only
    /// one a device of the approving account chose, not one control adds.
    ///
    /// ```text
    /// illogical team join v1
    /// daemon <device id>
    /// team <id>
    /// founder <account>
    /// founder_root <device id>
    /// ```
    pub fn join_body(&self, daemon: &str) -> String {
        format!(
            "illogical team join v1\ndaemon {daemon}\nteam {}\nfounder {}\nfounder_root {}\n",
            self.team, self.founder, self.founder_root
        )
    }

    /// `sig` is `approver`'s signature putting `daemon` in this team.
    pub fn join_signed_by(&self, daemon: &str, approver: &Cert, sig: &str) -> bool {
        approver.kind.approves() && verify_hex(&approver.sign, self.join_body(daemon).as_bytes(), sig)
    }
}

/// A joined machine moved into a team, between teams, or back to its
/// account, after it joined: a device of its own account signs it, and the
/// daemon applies only a move newer than the last it took, so control can
/// neither make one up nor replay an old one.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Move {
    /// The team it's in from now on; none for its own account.
    #[serde(default)]
    pub team: Option<TeamPin>,
    /// When it was made (ms); a daemon takes only newer ones.
    pub at: u64,
    /// The signing device.
    pub by: String,
    pub sig: String,
}

impl Move {
    /// ```text
    /// illogical machine move v1
    /// daemon <device id>
    /// team <id>|-
    /// founder <account>|-
    /// founder_root <device id>|-
    /// at <ms>
    /// ```
    pub fn body(daemon: &str, team: Option<&TeamPin>, at: u64) -> String {
        let (t, f, r) = team.map_or(("-", "-", "-"), |p| (&p.team, &p.founder, &p.founder_root));
        format!("illogical machine move v1\ndaemon {daemon}\nteam {t}\nfounder {f}\nfounder_root {r}\nat {at}\n")
    }

    /// Signed by `by` (a device that may approve) for `daemon`.
    pub fn signed_for(&self, daemon: &str, by: &Cert) -> bool {
        by.device == self.by
            && by.kind.approves()
            && verify_hex(&by.sign, Self::body(daemon, self.team.as_ref(), self.at).as_bytes(), &self.sig)
    }

    /// One of `roster`'s owners taking `daemon` out of that team, back to
    /// its own account (#332): a member puts their own machine in, and the
    /// team's owners can take it out. Never into a team.
    pub fn owner_takes_out(&self, daemon: &str, roster: &Roster, certs: &AccountCerts) -> bool {
        self.team.is_none()
            && roster
                .members
                .iter()
                .filter(|m| m.role == TeamRole::Owner)
                .any(|m| roster.devices(&m.account, certs).get(&self.by).is_some_and(|by| self.signed_for(daemon, by)))
    }
}

/// Every member account's certificates and revocations, as control hands
/// them out.
pub type AccountCerts = HashMap<String, (Vec<Cert>, Vec<Revocation>)>;

fn word(s: &str) -> bool {
    !s.is_empty() && s.len() <= 120 && !s.chars().any(|c| c.is_whitespace() || c.is_control())
}

impl Roster {
    pub fn body(&self) -> String {
        let mut b = format!(
            "illogical team v{}\nteam {}\nname {}\nversion {}\nat {}\n",
            self.v, self.team, self.name, self.version, self.at
        );
        for m in &self.members {
            b.push_str(&format!("member {} {} {} {}\n", m.account, m.root, m.role.as_str(), m.name));
        }
        // v1 bodies stay as they were, so older daemons check them.
        for s in &self.spent {
            b.push_str(&format!("spent {} {}\n", s.key, s.expires));
        }
        if let Some(r) = &self.redeem {
            b.push_str(&format!("redeem {} {}\n", r.invite.key, r.proof));
        }
        b.push_str(&format!("by {}\n", self.by));
        b
    }

    pub fn well_formed(&self) -> bool {
        let shape = match self.v {
            1 => self.spent.is_empty() && self.redeem.is_none(),
            2 => self.spent.iter().all(|s| word(&s.key)),
            _ => false,
        };
        shape
            && word(&self.team)
            && !self.name.is_empty()
            && self.name.len() <= 80
            && !self.name.chars().any(char::is_control)
            && self.members.iter().all(|m| word(&m.account) && word(&m.root) && word(&m.name))
            && self.members.iter().any(|m| m.role == TeamRole::Owner)
    }

    pub fn sign_with(&mut self, keys: &DeviceKeys) {
        self.by = keys.id();
        self.sig = hex::encode(keys.signature(self.body().as_bytes()));
    }

    /// Sign a version that redeems a presigned invite, with the invite's
    /// one-time key `k`.
    pub fn sign_redeem(&mut self, k: &DeviceKeys) {
        self.by = hex::encode(k.sign_public());
        self.sig = hex::encode(k.signature(self.body().as_bytes()));
    }

    pub fn member(&self, account: &str) -> Option<&Member> {
        self.members.iter().find(|m| m.account == account)
    }

    /// The devices an account in this roster trusts now, from its root
    /// as listed here.
    pub fn devices(&self, account: &str, certs: &AccountCerts) -> Trusted {
        let Some(m) = self.member(account) else { return Trusted::default() };
        let (c, r) = certs.get(account).map(|(c, r)| (c.as_slice(), r.as_slice())).unwrap_or((&[], &[]));
        Trust { account: account.to_owned(), root: m.root.clone() }.evaluate(c, r)
    }

    /// Whether this version adds or promotes anyone but `signer` to owner
    /// or editor since `prev` (from nothing, for a first version): a member
    /// whose account and root weren't there with at least that role; or
    /// moves `signer`'s own entry to another root. Only a browser all the
    /// way back to its root signs that.
    pub fn raises(&self, prev: Option<&Roster>, signer: &str) -> bool {
        let rank = |r: TeamRole| match r {
            TeamRole::Owner => 2,
            TeamRole::Editor => 1,
            TeamRole::Viewer => 0,
        };
        self.members.iter().any(|m| {
            let was = prev.and_then(|p| p.member(&m.account));
            if m.account == signer {
                // The signer's own entry: a raise if it now names another
                // root (another identity for that account).
                return was.is_some_and(|w| w.root != m.root);
            }
            m.role != TeamRole::Viewer && !was.is_some_and(|w| w.root == m.root && rank(w.role) >= rank(m.role))
        })
    }

    /// Whether this roster may follow `prev` (or, with none, start the team
    /// as `pin` says): newer, well formed, and signed by a device of an
    /// owner of the previous version (the founder for the first).
    pub fn follows(&self, prev: Option<&Roster>, pin: &TeamPin, certs: &AccountCerts) -> bool {
        if !self.well_formed() || self.team != pin.team {
            return false;
        }
        // Who may sign: the previous version's owners, or the founder.
        let founder;
        let signers: Vec<&Member> = match prev {
            Some(p) => {
                if self.version <= p.version {
                    return false;
                }
                p.members.iter().filter(|m| m.role == TeamRole::Owner).collect()
            }
            None => {
                founder = Member {
                    account: pin.founder.clone(),
                    root: pin.founder_root.clone(),
                    role: TeamRole::Owner,
                    name: String::new(),
                };
                vec![&founder]
            }
        };
        // Whether `device` of `account` (from `root`) signed `body`, and
        // with `strong`, is a browser all the way back to that root: what
        // adds or promotes an owner or an editor. A CLI's or the app's key
        // is a program's on its computer as much as its person's.
        let signed_by = |account: &str, root: &str, device: &str, body: &str, sig: &str, strong: bool| {
            let (c, r) = certs.get(account).map(|(c, r)| (c.as_slice(), r.as_slice())).unwrap_or((&[], &[]));
            let trust = Trust { account: account.to_owned(), root: root.to_owned() };
            trust
                .evaluate(c, r)
                .get(device)
                .is_some_and(|d| d.kind.approves() && verify_hex(&d.sign, body.as_bytes(), sig))
                && (!strong || trust.browser_chained(c, device))
        };
        let body = self.body();
        if signers.iter().any(|m| {
            let strong = self.raises(prev, &m.account);
            signed_by(&m.account, &m.root, &self.by, &body, &self.sig, strong)
        }) {
            // Owners write the roster outright; an invite is only for
            // someone who isn't one.
            return self.redeem.is_none();
        }
        let (Some(prev), Some(r)) = (prev, &self.redeem) else { return false };
        let inv = &r.invite;
        let n = prev.members.len();
        // Exactly the version before, plus the invitee at the end, with
        // the invite marked spent; nothing else changes.
        let Some(new) = self.members.get(n) else { return false };
        let mut spent = prev.spent.clone();
        spent.push(Spent { key: inv.key.clone(), expires: inv.expires });
        self.version == prev.version + 1
            && self.name == prev.name
            && self.members.len() == n + 1
            && self.members[..n] == prev.members[..]
            && prev.member(&new.account).is_none()
            && self.spent == spent
            && !prev.spent.iter().any(|s| s.key == inv.key)
            // The invite: this team, not an owner's role, still good, and
            // signed by a device of someone who was an owner.
            && inv.team == self.team
            && inv.role != TeamRole::Owner
            && new.role == inv.role
            && self.at <= inv.expires
            // Time only moves forward, so an expired invite can't be
            // redeemed with a backdated version once owners drop it.
            && self.at >= prev.at
            && prev
                .members
                .iter()
                .filter(|m| m.role == TeamRole::Owner)
                .any(|m| signed_by(&m.account, &m.root, &inv.by, &inv.body(), &inv.sig, inv.role != TeamRole::Viewer))
            // The one-time key vouches for exactly this member here, and
            // signs the version itself. Not the invitee's device: devices
            // are judged as of now, so revoking that one later would stop
            // every later version from checking, removals too.
            && verify_hex(&inv.key, inv.redeem_body(self.version, new).as_bytes(), &r.proof)
            && self.by == inv.key
            && verify_hex(&inv.key, body.as_bytes(), &self.sig)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Kind;

    struct Person {
        account: String,
        keys: DeviceKeys,
        cert: Cert,
    }

    fn person(account: &str) -> Person {
        let keys = DeviceKeys::generate();
        let mut cert = Cert::new(&keys, account, Kind::Browser, "laptop");
        cert.sign_with(&keys);
        Person { account: account.into(), keys, cert }
    }

    fn certs(people: &[&Person]) -> AccountCerts {
        people.iter().map(|p| (p.account.clone(), (vec![p.cert.clone()], vec![]))).collect()
    }

    fn member(p: &Person, role: TeamRole) -> Member {
        Member { account: p.account.clone(), root: p.cert.device.clone(), role, name: format!("{}-login", p.account) }
    }

    fn roster(version: u64, members: Vec<Member>, by: &Person) -> Roster {
        let mut r = Roster {
            v: 1,
            team: "t1".into(),
            name: "Acme".into(),
            version,
            at: 1,
            members,
            spent: vec![],
            redeem: None,
            by: String::new(),
            sig: String::new(),
        };
        r.sign_with(&by.keys);
        r
    }

    /// An invite `owner` signs, and its one-time key.
    fn invite(owner: &Person, role: TeamRole, expires: u64) -> (Invite, DeviceKeys) {
        let k = DeviceKeys::generate();
        let mut inv = Invite {
            team: "t1".into(),
            role,
            expires,
            key: hex::encode(k.sign_public()),
            by: String::new(),
            sig: String::new(),
        };
        inv.sign_with(&owner.keys);
        (inv, k)
    }

    /// `who` redeems `inv` with `k`, writing the version after `prev`.
    fn redeem(prev: &Roster, inv: &Invite, k: &DeviceKeys, who: &Person) -> Roster {
        let new = member(who, inv.role);
        let mut members = prev.members.clone();
        members.push(new.clone());
        let mut spent = prev.spent.clone();
        spent.push(Spent { key: inv.key.clone(), expires: inv.expires });
        let version = prev.version + 1;
        let proof = hex::encode(k.signature(inv.redeem_body(version, &new).as_bytes()));
        let mut r = Roster {
            v: 2,
            version,
            at: 1,
            members,
            spent,
            redeem: Some(Redeem { invite: inv.clone(), proof }),
            ..prev.clone()
        };
        r.sign_redeem(k);
        r
    }

    /// A CLI's or the app's key may be a program's: it can't make anyone an
    /// owner or an editor, by a roster or by a presigned invite. It still
    /// adds viewers and takes people out; a browser all the way to its root
    /// does everything.
    #[test]
    fn only_a_browser_makes_an_owner_or_an_editor() {
        let (alice, bob, mallory) = (person("alice"), person("bob"), person("mallory"));
        let pin = TeamPin { team: "t1".into(), founder: "alice".into(), founder_root: alice.cert.device.clone() };
        let device = |kind: Kind, by: &DeviceKeys| {
            let k = DeviceKeys::generate();
            let mut c = Cert::new(&k, "alice", kind, "x");
            c.sign_with(by);
            (k, c)
        };
        let (cli_k, cli) = device(Kind::Cli, &alice.keys);
        let (app_k, app) = device(Kind::App, &alice.keys);
        // A browser a CLI made (no control would keep one now) isn't one.
        let (laundered_k, laundered) = {
            let k = DeviceKeys::generate();
            let mut c = Cert::new(&k, "alice", Kind::Browser, "laundered");
            c.sign_with(&cli_k);
            (k, c)
        };
        let mut all = certs(&[&alice, &bob, &mallory]);
        all.get_mut("alice").unwrap().0.extend([cli, app, laundered]);
        let v1 = roster(1, vec![member(&alice, TeamRole::Owner), member(&bob, TeamRole::Viewer)], &alice);
        assert!(v1.follows(None, &pin, &all));
        let signed = |members: Vec<Member>, k: &DeviceKeys| {
            let mut r = roster(2, members, &alice);
            r.sign_with(k);
            r
        };
        let alice_m = member(&alice, TeamRole::Owner);
        for (role, who) in [(TeamRole::Owner, &mallory), (TeamRole::Editor, &mallory), (TeamRole::Editor, &bob)] {
            let members = vec![alice_m.clone(), member(who, role)];
            for k in [&cli_k, &app_k] {
                assert!(
                    !signed(members.clone(), k).follows(Some(&v1), &pin, &all),
                    "{role:?} {} by a CLI or app",
                    who.account
                );
            }
            assert!(!signed(members.clone(), &laundered_k).follows(Some(&v1), &pin, &all), "by a browser a CLI made");
            assert!(signed(members, &alice.keys).follows(Some(&v1), &pin, &all), "by the browser");
        }
        // Bob's account swapped for another root, as an editor: raising too.
        let swapped = Member { root: mallory.cert.device.clone(), ..member(&bob, TeamRole::Editor) };
        assert!(signed(vec![alice_m.clone(), member(&bob, TeamRole::Viewer)], &cli_k).follows(Some(&v1), &pin, &all));
        let mut v1e = v1.clone();
        v1e.members[1].role = TeamRole::Editor;
        v1e.sign_with(&alice.keys);
        assert!(!signed(vec![alice_m.clone(), swapped], &cli_k).follows(Some(&v1e), &pin, &all));
        // The CLI's owner moving her own entry to another root: a raise.
        let elsewhere = Member { root: mallory.cert.device.clone(), ..alice_m.clone() };
        assert!(!signed(vec![elsewhere.clone(), member(&bob, TeamRole::Viewer)], &cli_k).follows(
            Some(&v1),
            &pin,
            &all
        ));
        assert!(signed(vec![elsewhere, member(&bob, TeamRole::Viewer)], &alice.keys).raises(Some(&v1), "alice"));
        // A viewer, and a removal: the CLI may.
        assert!(
            signed(vec![alice_m.clone(), member(&bob, TeamRole::Viewer), member(&mallory, TeamRole::Viewer)], &cli_k)
                .follows(Some(&v1), &pin, &all)
        );
        assert!(signed(vec![alice_m.clone()], &app_k).follows(Some(&v1), &pin, &all));
        // Presigned invites: an editor's only from a browser; a viewer's from the CLI too.
        let by = |role, k: &DeviceKeys| {
            let one = DeviceKeys::generate();
            let mut inv = Invite {
                team: "t1".into(),
                role,
                expires: 10,
                key: hex::encode(one.sign_public()),
                by: String::new(),
                sig: String::new(),
            };
            inv.sign_with(k);
            (inv, one)
        };
        let (inv, k) = by(TeamRole::Editor, &cli_k);
        assert!(!redeem(&v1, &inv, &k, &mallory).follows(Some(&v1), &pin, &all));
        let (inv, k) = by(TeamRole::Viewer, &cli_k);
        assert!(redeem(&v1, &inv, &k, &mallory).follows(Some(&v1), &pin, &all));
        let (inv, k) = by(TeamRole::Editor, &alice.keys);
        assert!(redeem(&v1, &inv, &k, &mallory).follows(Some(&v1), &pin, &all));
        assert!(
            AskInvite {
                team: "t1".into(),
                role: TeamRole::Owner,
                made: 0,
                expires: 1,
                key: String::new(),
                by: String::new(),
                sig: String::new()
            }
            .raises()
        );
    }

    #[test]
    fn a_presigned_invite_admits_its_holder_once() {
        let (alice, bob, carol) = (person("alice"), person("bob"), person("carol"));
        let pin = TeamPin { team: "t1".into(), founder: "alice".into(), founder_root: alice.cert.device.clone() };
        let all = certs(&[&alice, &bob, &carol]);
        let v1 = roster(1, vec![member(&alice, TeamRole::Owner)], &alice);
        let (inv, k) = invite(&alice, TeamRole::Editor, 10);
        // Bob adds himself with the invite: no owner signs this version.
        let v2 = redeem(&v1, &inv, &k, &bob);
        assert!(v2.follows(Some(&v1), &pin, &all));
        assert_eq!(v2.member("bob").map(|m| m.role), Some(TeamRole::Editor));
        // Once: Carol can't use the same invite after him.
        let again = redeem(&v2, &inv, &k, &carol);
        assert!(!again.follows(Some(&v2), &pin, &all));
        // Owners carry what's spent on; their versions still check.
        let mut v3 = v2.clone();
        v3.version = 3;
        v3.redeem = None;
        v3.sign_with(&alice.keys);
        assert!(v3.follows(Some(&v2), &pin, &all));
        // A second, separate invite works the same way.
        let (inv2, k2) = invite(&alice, TeamRole::Viewer, 10);
        assert!(redeem(&v3, &inv2, &k2, &carol).follows(Some(&v3), &pin, &all));
    }

    #[test]
    fn a_redeemed_version_still_checks_after_the_invitee_revokes_their_device() {
        let (alice, bob) = (person("alice"), person("bob"));
        let pin = TeamPin { team: "t1".into(), founder: "alice".into(), founder_root: alice.cert.device.clone() };
        let v1 = roster(1, vec![member(&alice, TeamRole::Owner)], &alice);
        let (inv, k) = invite(&alice, TeamRole::Editor, 10);
        // Bob joins from his phone.
        let v2 = redeem(&v1, &inv, &k, &bob);
        let phone_keys = DeviceKeys::generate();
        let mut phone = Cert::new(&phone_keys, "bob", Kind::Browser, "phone");
        phone.sign_with(&bob.keys);
        // He's removed, then takes the phone off his account: a daemon
        // checking the team from the start still gets to the removal.
        let mut v3 = roster(3, vec![member(&alice, TeamRole::Owner)], &alice);
        v3.v = 2;
        v3.spent = v2.spent.clone();
        v3.sign_with(&alice.keys);
        let mut all = certs(&[&alice, &bob]);
        let (c, r) = all.get_mut("bob").unwrap();
        c.push(phone.clone());
        r.push(Revocation::new("bob", &phone.device, &bob.keys));
        assert!(v2.devices("bob", &all).get(&phone.device).is_none());
        assert!(v1.follows(None, &pin, &all));
        assert!(v2.follows(Some(&v1), &pin, &all));
        assert!(v3.follows(Some(&v2), &pin, &all));
    }

    #[test]
    fn an_ask_invites_proof_names_its_holder() {
        let (alice, bob, mallory) = (person("alice"), person("bob"), person("mallory"));
        let k = DeviceKeys::generate();
        let mut inv = AskInvite {
            team: "t1".into(),
            role: TeamRole::Owner,
            made: 1,
            expires: 2,
            key: hex::encode(k.sign_public()),
            by: String::new(),
            sig: String::new(),
        };
        inv.sign_with(&alice.keys);
        assert!(inv.signed_by(&alice.cert));
        assert!(!inv.signed_by(&bob.cert), "another device's");
        let proof = hex::encode(k.signature(inv.ask_body("bob", &bob.cert.device).as_bytes()));
        assert!(inv.proves("bob", &bob.cert.device, &proof));
        // Control can't move Bob's proof to an account (or a root) of its own.
        assert!(!inv.proves("mallory", &mallory.cert.device, &proof));
        assert!(!inv.proves("bob", &mallory.cert.device, &proof));
        // Nor sign one without the link's key.
        let forged = hex::encode(mallory.keys.signature(inv.ask_body("mallory", &mallory.cert.device).as_bytes()));
        assert!(!inv.proves("mallory", &mallory.cert.device, &forged));
        // A changed role or expiry no longer carries the owner's signature.
        let mut bent = inv.clone();
        bent.expires = 99;
        assert!(!bent.signed_by(&alice.cert));
        // Nor does it read as a presigned invite.
        let as_presigned = Invite {
            team: inv.team.clone(),
            role: TeamRole::Editor,
            expires: inv.expires,
            key: inv.key.clone(),
            by: inv.by.clone(),
            sig: inv.sig.clone(),
        };
        assert!(!verify_hex(&alice.cert.sign, as_presigned.body().as_bytes(), &as_presigned.sig));
    }

    #[test]
    fn a_presigned_invite_cant_be_bent() {
        let (alice, bob, mallory) = (person("alice"), person("bob"), person("mallory"));
        let pin = TeamPin { team: "t1".into(), founder: "alice".into(), founder_root: alice.cert.device.clone() };
        let all = certs(&[&alice, &bob, &mallory]);
        let v1 = roster(1, vec![member(&alice, TeamRole::Owner)], &alice);
        let (inv, k) = invite(&alice, TeamRole::Viewer, 10);
        let good = redeem(&v1, &inv, &k, &bob);
        assert!(good.follows(Some(&v1), &pin, &all));

        // Control saw Bob's proof but can't move it to Mallory: the proof
        // names Bob, and Mallory's device can't sign as Bob.
        let mut stolen = good.clone();
        stolen.members[1] = member(&mallory, TeamRole::Viewer);
        stolen.sign_with(&mallory.keys);
        assert!(!stolen.follows(Some(&v1), &pin, &all));
        // Without the one-time key, no proof.
        let forged = redeem(&v1, &inv, &DeviceKeys::generate(), &mallory);
        assert!(!forged.follows(Some(&v1), &pin, &all));
        // A higher role than the invite's.
        let mut raised = good.clone();
        raised.members[1].role = TeamRole::Editor;
        raised.sign_redeem(&k);
        assert!(!raised.follows(Some(&v1), &pin, &all));
        // Anything else changed on the way in: another member's role.
        let v2 = roster(2, vec![member(&alice, TeamRole::Owner), member(&mallory, TeamRole::Viewer)], &alice);
        let mut demoted = redeem(&v2, &inv, &k, &bob);
        demoted.members[1].role = TeamRole::Editor;
        demoted.sign_redeem(&k);
        assert!(!demoted.follows(Some(&v2), &pin, &all));
        // Past its expiry.
        let mut late = good.clone();
        late.at = 11;
        late.sign_redeem(&k);
        assert!(!late.follows(Some(&v1), &pin, &all));
        // Backdated: before the version it follows.
        let mut v1_later = v1.clone();
        v1_later.at = 5;
        v1_later.sign_with(&alice.keys);
        let mut early = redeem(&v1_later, &inv, &k, &bob);
        early.at = 4;
        early.sign_redeem(&k);
        assert!(!early.follows(Some(&v1_later), &pin, &all));
        // Signed by the invitee's device rather than the one-time key.
        let mut by_device = good.clone();
        by_device.sign_with(&bob.keys);
        assert!(!by_device.follows(Some(&v1), &pin, &all));
        // An invite signed by someone who isn't an owner.
        let (by_bob, kb) = invite(&bob, TeamRole::Viewer, 10);
        assert!(!redeem(&v1, &by_bob, &kb, &mallory).follows(Some(&v1), &pin, &all));
        // An invite to be an owner is never presigned.
        let (own, ko) = invite(&alice, TeamRole::Owner, 10);
        assert!(!redeem(&v1, &own, &ko, &bob).follows(Some(&v1), &pin, &all));
        // Not marking it spent.
        let mut unspent = good.clone();
        unspent.spent.clear();
        unspent.sign_redeem(&k);
        assert!(!unspent.follows(Some(&v1), &pin, &all));
        // An owner's version can't carry a redeem.
        let mut owner_redeem = good.clone();
        owner_redeem.sign_with(&alice.keys);
        assert!(!owner_redeem.follows(Some(&v1), &pin, &all));
        // A v1 roster can't carry v2's fields.
        let mut old = good.clone();
        old.v = 1;
        old.sign_redeem(&k);
        assert!(!old.follows(Some(&v1), &pin, &all));
    }

    #[test]
    fn owners_sign_each_version() {
        let (alice, bob, mallory) = (person("alice"), person("bob"), person("mallory"));
        let pin = TeamPin { team: "t1".into(), founder: "alice".into(), founder_root: alice.cert.device.clone() };
        let all = certs(&[&alice, &bob, &mallory]);
        let v1 = roster(1, vec![member(&alice, TeamRole::Owner)], &alice);
        assert!(v1.follows(None, &pin, &all));
        let v2 = roster(2, vec![member(&alice, TeamRole::Owner), member(&bob, TeamRole::Editor)], &alice);
        assert!(v2.follows(Some(&v1), &pin, &all));
        // An editor can't sign the next version, nor can an outsider.
        let by_bob = roster(3, vec![member(&alice, TeamRole::Owner), member(&bob, TeamRole::Owner)], &bob);
        assert!(!by_bob.follows(Some(&v2), &pin, &all));
        let by_mallory = roster(3, vec![member(&mallory, TeamRole::Owner)], &mallory);
        assert!(!by_mallory.follows(Some(&v2), &pin, &all));
        // Nor can control start the team over, or go back a version.
        assert!(!by_mallory.follows(None, &pin, &all));
        assert!(!v1.follows(Some(&v2), &pin, &all));
        // Ownership passes on: Alice makes Bob an owner, then Bob signs.
        let v3 = roster(3, vec![member(&alice, TeamRole::Editor), member(&bob, TeamRole::Owner)], &alice);
        assert!(v3.follows(Some(&v2), &pin, &all));
        let v4 = roster(4, vec![member(&bob, TeamRole::Owner)], &bob);
        assert!(v4.follows(Some(&v3), &pin, &all));
    }

    #[test]
    fn tampering_fails() {
        let (alice, bob) = (person("alice"), person("bob"));
        let pin = TeamPin { team: "t1".into(), founder: "alice".into(), founder_root: alice.cert.device.clone() };
        let all = certs(&[&alice, &bob]);
        let v1 = roster(1, vec![member(&alice, TeamRole::Owner), member(&bob, TeamRole::Viewer)], &alice);
        let mut promoted = v1.clone();
        promoted.members[1].role = TeamRole::Owner;
        assert!(!promoted.follows(None, &pin, &all));
        // A member's devices come from the root the roster lists.
        assert_eq!(v1.devices("bob", &all).devices.len(), 1);
        let mut moved = all.clone();
        let other = person("bob");
        moved.insert("bob".into(), (vec![other.cert], vec![]));
        assert_eq!(v1.devices("bob", &moved).devices.len(), 0);
    }

    #[test]
    fn a_team_join_is_signed_by_the_approver() {
        let (alice, mallory) = (person("alice"), person("mallory"));
        let pin = TeamPin { team: "t1".into(), founder: "alice".into(), founder_root: alice.cert.device.clone() };
        let sig = hex::encode(alice.keys.signature(pin.join_body("box").as_bytes()));
        assert!(pin.join_signed_by("box", &alice.cert, &sig));
        // Not for another daemon, another founder, or by someone else.
        assert!(!pin.join_signed_by("other", &alice.cert, &sig));
        let swapped = TeamPin { founder_root: mallory.cert.device.clone(), ..pin.clone() };
        assert!(!swapped.join_signed_by("box", &alice.cert, &sig));
        assert!(!pin.join_signed_by("box", &mallory.cert, &sig));
    }

    #[test]
    fn a_move_is_signed_for_its_daemon_and_time() {
        let (alice, mallory) = (person("alice"), person("mallory"));
        let pin = TeamPin { team: "t1".into(), founder: "alice".into(), founder_root: alice.cert.device.clone() };
        let sign = |who: &Person, team: Option<&TeamPin>, at| Move {
            team: team.cloned(),
            at,
            by: who.cert.device.clone(),
            sig: hex::encode(who.keys.signature(Move::body("box", team, at).as_bytes())),
        };
        let into = sign(&alice, Some(&pin), 5);
        assert!(into.signed_for("box", &alice.cert));
        assert!(!into.signed_for("other", &alice.cert));
        assert!(!into.signed_for("box", &mallory.cert));
        // Control can't change the team, drop it, or change the time.
        assert!(!Move { team: None, ..into.clone() }.signed_for("box", &alice.cert));
        assert!(!Move { at: 6, ..into.clone() }.signed_for("box", &alice.cert));
        let out = sign(&alice, None, 7);
        assert!(out.signed_for("box", &alice.cert));
        assert!(!sign(&mallory, None, 7).signed_for("box", &alice.cert));
    }
}
