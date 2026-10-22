//! The standing project grant (M211, platform step 7): an owner's device
//! says, once per project, that the platform may start runs and instruct
//! them on the project's hosted workspaces ("One standing grant per project
//! starts runs", aru-099). It's S37's standing grant
//! (`spikes/s37-team-permissions/README.md`, q3) with a project as its
//! resource: the platform stores and relays it, but can't make or widen
//! it, and hosted `arugulad` checks it against the project's delegation
//! (M178, [`crate::hosted`]) before it starts anything ([`check`]).
//!
//! What's signed is a fixed line format, as for delegations. Its own first
//! line, so a signature over it never reads as S37's machine grant (`arugula
//! exec grant v1`) or a delegation:
//!
//! ```text
//! arugula project grant v1
//! id <32 hex>
//! project <prj_ id>
//! owner team:<id>|account:<id>
//! delegation <the project's delegation id>
//! resource hosted
//! to platform
//! caps <capability>        (once per capability, sorted)
//! made <ms>
//! expires <ms|never>
//! by <device id>
//! ```
//!
//! ```text
//! arugula project grant revoke v1
//! project <prj_ id>
//! grant <id>
//! running <finish|stop>
//! at <ms>
//! by <device id>
//! ```
//!
//! - **Who signs.** A browser or CLI of one of the project's owners, as for
//!   a delegation ([`Owners`]): never a machine, a recovery code, control's
//!   key or the platform.
//! - **What it covers.** `runs.start` and `runs.instruct` ([`CAPS`]) on the
//!   hosted workspaces of the project, under the delegation it names. A
//!   new delegation needs a new grant. It never covers a deploy (aru-101).
//! - **Revoking** is S37's: a revoke signed by any owner's device, relayed
//!   by the platform, and kept by each hosted machine that hears it, so the
//!   platform can't take it back by leaving it out later. It stops new
//!   runs; `running stop` also asks for the ones running to stop
//!   (decision 3: revoking and stopping are separate, so it says which).
//! - **Control can drop a revoke** (S37's threat model): it keeps the grant
//!   working on a machine that hasn't heard. A grant can carry an expiry
//!   to bound that.

use serde::{Deserialize, Serialize};

use crate::hosted::{Delegation, Owner, Owners};

/// What the grant gives, as signed: start runs, and instruct them.
pub const CAPS: &[&str] = &["runs.instruct", "runs.start"];
/// What the grant is over: the project's hosted workspaces.
pub const RESOURCE: &str = "hosted";
/// Who holds it: the platform, which starts runs under it.
pub const TO: &str = "platform";

fn token(s: &str) -> bool {
    !s.is_empty() && s.len() <= 64 && s.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}

fn hex_id(s: &str) -> bool {
    s.len() == 32 && s.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
}

/// A new grant's id: 32 hex digits from 16 random bytes.
pub fn new_id() -> String {
    hex::encode(crate::random::<16>())
}

/// An owner's device saying the platform may start and instruct runs on
/// `project`'s hosted workspaces, under `delegation`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProjectGrant {
    pub id: String,
    pub project: String,
    /// `team:<id>` or `account:<id>`, as the signing device saw it.
    pub owner: String,
    /// The project's delegation's [`Delegation::id`].
    pub delegation: String,
    /// Sorted, each once.
    pub caps: Vec<String>,
    pub made: u64,
    /// When it stops counting (ms); `None`: never.
    #[serde(default)]
    pub expires: Option<u64>,
    /// The signing device.
    pub by: String,
    #[serde(default)]
    pub sig: String,
}

impl ProjectGrant {
    /// Unsigned: `CAPS` over the project `d` delegates, made `made`.
    pub fn new(d: &Delegation, made: u64, expires: Option<u64>) -> Self {
        Self {
            id: new_id(),
            project: d.project.clone(),
            owner: d.owner.clone(),
            delegation: d.id(),
            caps: CAPS.iter().map(|c| (*c).to_owned()).collect(),
            made,
            expires,
            by: String::new(),
            sig: String::new(),
        }
    }

    pub fn body(&self) -> String {
        let mut s = format!(
            "arugula project grant v1\nid {}\nproject {}\nowner {}\ndelegation {}\nresource {RESOURCE}\nto {TO}\n",
            self.id, self.project, self.owner, self.delegation
        );
        for c in &self.caps {
            s.push_str(&format!("caps {c}\n"));
        }
        let expires = self.expires.map_or_else(|| "never".to_owned(), |e| e.to_string());
        s.push_str(&format!("made {}\nexpires {expires}\nby {}\n", self.made, self.by));
        s
    }

    /// The grant `body` is, with `sig`: what the platform stores is the
    /// bytes, and this reads them back. `None` when they aren't exactly a
    /// grant's (another body, extra lines, a field out of place).
    pub fn parse(body: &str, sig: &str) -> Option<Self> {
        let mut lines = body.strip_suffix('\n')?.split('\n');
        let mut field = |name: &str| lines.next()?.strip_prefix(name)?.strip_prefix(' ').map(str::to_owned);
        (field("arugula project grant")? == "v1").then_some(())?;
        let (id, project, owner, delegation) = (field("id")?, field("project")?, field("owner")?, field("delegation")?);
        (field("resource")? == RESOURCE && field("to")? == TO).then_some(())?;
        let rest: Vec<&str> = body.lines().skip(7).collect();
        let caps_n = rest.iter().take_while(|l| l.starts_with("caps ")).count();
        let caps = rest[..caps_n].iter().map(|l| l["caps ".len()..].to_owned()).collect();
        let tail = &rest[caps_n..];
        let [made, expires, by] = tail else { return None };
        let made = made.strip_prefix("made ")?.parse().ok()?;
        let expires = match expires.strip_prefix("expires ")? {
            "never" => None,
            e => Some(e.parse().ok()?),
        };
        let g = Self {
            id,
            project,
            owner,
            delegation,
            caps,
            made,
            expires,
            by: by.strip_prefix("by ")?.to_owned(),
            sig: sig.to_owned(),
        };
        (g.body() == body).then_some(g)
    }

    /// Well formed: its ids, its owner, and capabilities that are known,
    /// sorted and each once. Says nothing about the signature.
    pub fn well_formed(&self) -> bool {
        hex_id(&self.id)
            && token(&self.project)
            && Owner::parse(&self.owner).is_some()
            && token(&self.delegation)
            && token(&self.by)
            && !self.caps.is_empty()
            && self.caps.windows(2).all(|w| w[0] < w[1])
            && self.caps.iter().all(|c| CAPS.contains(&c.as_str()))
            && self.expires.is_none_or(|e| e > self.made)
    }

    pub fn sign_with(&mut self, keys: &crate::DeviceKeys) {
        self.by = keys.id();
        self.sig = hex::encode(keys.signature(self.body().as_bytes()));
    }

    /// Signed by a device that may sign for its project, as `owners` says.
    pub fn signed_for(&self, owners: &Owners) -> bool {
        self.well_formed() && owners.may_sign(&self.owner, &self.by, &self.body(), &self.sig)
    }
}

/// What happens to runs already going when a grant is revoked.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Running {
    /// They finish.
    Finish,
    /// They're stopped too.
    Stop,
}

impl Running {
    pub fn as_str(self) -> &'static str {
        match self {
            Running::Finish => "finish",
            Running::Stop => "stop",
        }
    }
}

/// A grant taken back, by any owner's device.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GrantRevoke {
    pub project: String,
    /// The grant's id.
    pub grant: String,
    pub running: Running,
    pub at: u64,
    pub by: String,
    #[serde(default)]
    pub sig: String,
}

impl GrantRevoke {
    pub fn new(g: &ProjectGrant, running: Running, at: u64, by: &crate::DeviceKeys) -> Self {
        let mut r =
            Self { project: g.project.clone(), grant: g.id.clone(), running, at, by: by.id(), sig: String::new() };
        r.sig = hex::encode(by.signature(r.body().as_bytes()));
        r
    }

    pub fn body(&self) -> String {
        format!(
            "arugula project grant revoke v1\nproject {}\ngrant {}\nrunning {}\nat {}\nby {}\n",
            self.project,
            self.grant,
            self.running.as_str(),
            self.at,
            self.by
        )
    }

    /// The revoke `body` is, with `sig`, as [`ProjectGrant::parse`].
    pub fn parse(body: &str, sig: &str) -> Option<Self> {
        let mut lines = body.strip_suffix('\n')?.split('\n');
        let mut field = |name: &str| lines.next()?.strip_prefix(name)?.strip_prefix(' ').map(str::to_owned);
        (field("arugula project grant revoke")? == "v1").then_some(())?;
        let (project, grant) = (field("project")?, field("grant")?);
        let running = match field("running")?.as_str() {
            "finish" => Running::Finish,
            "stop" => Running::Stop,
            _ => return None,
        };
        let at = field("at")?.parse().ok()?;
        let by = field("by")?;
        let r = Self { project, grant, running, at, by, sig: sig.to_owned() };
        (r.body() == body).then_some(r)
    }

    /// It takes back `g`, signed by a device that may sign for `g`'s
    /// project (any of its owners, not only the one who granted).
    pub fn revokes(&self, g: &ProjectGrant, owners: &Owners) -> bool {
        self.project == g.project && self.grant == g.id && owners.may_sign(&g.owner, &self.by, &self.body(), &self.sig)
    }
}

/// Why a hosted machine won't act under a grant.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GrantRefusal {
    /// It isn't well formed.
    Form,
    /// It's for another project, or another owner, than the machine's.
    WrongProject,
    /// It names a delegation other than the one the machine was enrolled
    /// under.
    WrongDelegation,
    /// No owner's device signed it (or that device isn't trusted now).
    Unsigned,
    /// It doesn't give what's asked.
    NotCovered,
    /// Its expiry has passed.
    Expired,
    /// An owner's device revoked it.
    Revoked,
}

impl GrantRefusal {
    pub fn code(self) -> &'static str {
        match self {
            GrantRefusal::Form => "grant_form",
            GrantRefusal::WrongProject => "grant_wrong_project",
            GrantRefusal::WrongDelegation => "grant_wrong_delegation",
            GrantRefusal::Unsigned => "grant_unsigned",
            GrantRefusal::NotCovered => "grant_not_covered",
            GrantRefusal::Expired => "grant_expired",
            GrantRefusal::Revoked => "grant_revoked",
        }
    }

    pub fn says(self) -> &'static str {
        match self {
            GrantRefusal::Form => "the project's standing grant isn't well formed",
            GrantRefusal::WrongProject => "the standing grant is for another project",
            GrantRefusal::WrongDelegation => {
                "the standing grant names another delegation than this machine's: an owner signs it again"
            }
            GrantRefusal::Unsigned => "no owner's device signed the standing grant",
            GrantRefusal::NotCovered => "the standing grant doesn't cover this",
            GrantRefusal::Expired => "the standing grant has expired: an owner signs it again",
            GrantRefusal::Revoked => "the standing grant was revoked",
        }
    }
}

/// Whether a hosted machine enrolled under `d` may do `cap` under `g` at
/// `now`: the grant is well formed, for `d`'s project and owner, names
/// `d`, is signed by one of the project's owners as `owners` says (which
/// the machine got from its own checks, not the platform's word), covers
/// `cap`, hasn't expired, and no revoke among `revokes` that checks out
/// takes it back. Revokes that don't check out are ignored: the platform
/// may send junk, and only a refusal comes of it.
pub fn check(
    g: &ProjectGrant,
    d: &Delegation,
    revokes: &[GrantRevoke],
    owners: &Owners,
    cap: &str,
    now: u64,
) -> Result<(), GrantRefusal> {
    if !g.well_formed() {
        return Err(GrantRefusal::Form);
    }
    if g.project != d.project || g.owner != d.owner || Owner::parse(&g.owner) != Some(owners.owner()) {
        return Err(GrantRefusal::WrongProject);
    }
    if g.delegation != d.id() {
        return Err(GrantRefusal::WrongDelegation);
    }
    if !g.signed_for(owners) {
        return Err(GrantRefusal::Unsigned);
    }
    if !g.caps.iter().any(|c| c == cap) {
        return Err(GrantRefusal::NotCovered);
    }
    if g.expires.is_some_and(|e| e <= now) {
        return Err(GrantRefusal::Expired);
    }
    if revokes.iter().any(|r| r.revokes(g, owners)) {
        return Err(GrantRefusal::Revoked);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        Cert, DeviceKeys, Kind, Revocation, Trust,
        hosted::HostedKey,
        team::{AccountCerts, Member, Roster, TeamRole},
    };

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

    fn member(p: &Person, role: TeamRole) -> Member {
        Member { account: p.account.clone(), root: p.cert.device.clone(), role, name: p.account.clone() }
    }

    fn roster(members: Vec<Member>, by: &Person) -> Roster {
        let mut r = Roster {
            v: 1,
            team: "t1".into(),
            name: "Acme".into(),
            version: 1,
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

    fn certs(people: &[&Person]) -> AccountCerts {
        people.iter().map(|p| (p.account.clone(), (vec![p.cert.clone()], vec![]))).collect()
    }

    fn delegation(owner: &str, by: &DeviceKeys, at: u64) -> Delegation {
        let mut d = Delegation {
            project: "prj_1".into(),
            owner: owner.into(),
            key: HostedKey::from_seed(&[1; 32]).public(),
            at,
            by: String::new(),
            sig: String::new(),
        };
        d.sign_with(by);
        d
    }

    fn grant(d: &Delegation, by: &DeviceKeys) -> ProjectGrant {
        let mut g = ProjectGrant::new(d, 100, None);
        g.sign_with(by);
        g
    }

    /// A team with an owner, a second owner, an editor and a viewer.
    struct World {
        owner: Person,
        other_owner: Person,
        editor: Person,
        roster: Roster,
        certs: AccountCerts,
    }

    fn world() -> World {
        let (owner, other_owner, editor, viewer) = (person("own"), person("own2"), person("ed"), person("view"));
        let roster = roster(
            vec![
                member(&owner, TeamRole::Owner),
                member(&other_owner, TeamRole::Owner),
                member(&editor, TeamRole::Editor),
                member(&viewer, TeamRole::Viewer),
            ],
            &owner,
        );
        let certs = certs(&[&owner, &other_owner, &editor, &viewer]);
        World { owner, other_owner, editor, roster, certs }
    }

    #[test]
    fn an_owners_grant_starts_and_instructs() {
        let w = world();
        let owners = Owners::Team { roster: &w.roster, certs: &w.certs };
        let d = delegation("team:t1", &w.owner.keys, 10);
        let g = grant(&d, &w.owner.keys);
        assert_eq!(g.caps, ["runs.instruct", "runs.start"]);
        for cap in CAPS {
            assert_eq!(check(&g, &d, &[], &owners, cap, 200), Ok(()));
        }
        // Not a deploy, or anything else.
        assert_eq!(check(&g, &d, &[], &owners, "deploys.apply", 200), Err(GrantRefusal::NotCovered));
        // Another owner's device's grant counts the same.
        let g2 = grant(&d, &w.other_owner.keys);
        assert_eq!(check(&g2, &d, &[], &owners, "runs.start", 200), Ok(()));
    }

    #[test]
    fn only_owners_devices_grant() {
        let w = world();
        let owners = Owners::Team { roster: &w.roster, certs: &w.certs };
        let d = delegation("team:t1", &w.owner.keys, 10);
        // An editor's device, a stranger's, control's own key: none.
        for by in [&w.editor.keys, &DeviceKeys::generate()] {
            assert_eq!(check(&grant(&d, by), &d, &[], &owners, "runs.start", 200), Err(GrantRefusal::Unsigned));
        }
        // An owner's machine or recovery code doesn't either.
        for kind in [Kind::Daemon, Kind::Recovery] {
            let k = DeviceKeys::generate();
            let mut kc = Cert::new(&k, &w.owner.account, kind, "x");
            kc.sign_with(&w.owner.keys);
            let mut certs = w.certs.clone();
            certs.get_mut(&w.owner.account).unwrap().0.push(kc);
            let owners = Owners::Team { roster: &w.roster, certs: &certs };
            assert_eq!(check(&grant(&d, &k), &d, &[], &owners, "runs.start", 200), Err(GrantRefusal::Unsigned));
        }
        // An owner who's an editor now.
        let later = roster(vec![member(&w.owner, TeamRole::Editor), member(&w.editor, TeamRole::Owner)], &w.owner);
        let owners = Owners::Team { roster: &later, certs: &w.certs };
        assert_eq!(check(&grant(&d, &w.owner.keys), &d, &[], &owners, "runs.start", 200), Err(GrantRefusal::Unsigned));
    }

    /// The platform holds the grant: whatever it changes, the signature no
    /// longer checks out, so it can't widen one or move it.
    #[test]
    fn the_platform_cant_widen_or_move_it() {
        let w = world();
        let owners = Owners::Team { roster: &w.roster, certs: &w.certs };
        let d = delegation("team:t1", &w.owner.keys, 10);
        let g = grant(&d, &w.owner.keys);
        let mut narrow = ProjectGrant { caps: vec!["runs.start".into()], ..g.clone() };
        narrow.sign_with(&w.owner.keys);
        assert_eq!(check(&narrow, &d, &[], &owners, "runs.instruct", 200), Err(GrantRefusal::NotCovered));
        let wider = ProjectGrant { caps: g.caps.clone(), ..narrow.clone() };
        assert_eq!(check(&wider, &d, &[], &owners, "runs.instruct", 200), Err(GrantRefusal::Unsigned));
        let mut short = ProjectGrant { expires: Some(150), ..g.clone() };
        short.sign_with(&w.owner.keys);
        assert_eq!(check(&short, &d, &[], &owners, "runs.start", 200), Err(GrantRefusal::Expired));
        assert_eq!(check(&short, &d, &[], &owners, "runs.start", 149), Ok(()));
        let longer = ProjectGrant { expires: None, ..short.clone() };
        assert_eq!(check(&longer, &d, &[], &owners, "runs.start", 200), Err(GrantRefusal::Unsigned));
        // An unknown power, signed or not, isn't well formed.
        let mut odd = ProjectGrant { caps: vec!["deploys.apply".into(), "runs.start".into()], ..g.clone() };
        odd.sign_with(&w.owner.keys);
        assert_eq!(check(&odd, &d, &[], &owners, "deploys.apply", 200), Err(GrantRefusal::Form));
        // Another project, or another delegation of this one.
        let mut other = ProjectGrant { project: "prj_2".into(), ..g.clone() };
        other.sign_with(&w.owner.keys);
        assert_eq!(check(&other, &d, &[], &owners, "runs.start", 200), Err(GrantRefusal::WrongProject));
        let d2 = delegation("team:t1", &w.owner.keys, 11);
        assert_eq!(check(&g, &d2, &[], &owners, "runs.start", 200), Err(GrantRefusal::WrongDelegation));
        // Another team's project, signed by this team's owner.
        let mut theirs = ProjectGrant { owner: "team:t2".into(), ..g.clone() };
        theirs.sign_with(&w.owner.keys);
        let dt = Delegation { owner: "team:t2".into(), ..d.clone() };
        assert_eq!(check(&theirs, &dt, &[], &owners, "runs.start", 200), Err(GrantRefusal::WrongProject));
    }

    #[test]
    fn any_owner_revokes_it_and_nobody_else() {
        let w = world();
        let owners = Owners::Team { roster: &w.roster, certs: &w.certs };
        let d = delegation("team:t1", &w.owner.keys, 10);
        let g = grant(&d, &w.owner.keys);
        let spite = GrantRevoke::new(&g, Running::Stop, 150, &w.editor.keys);
        assert_eq!(check(&g, &d, std::slice::from_ref(&spite), &owners, "runs.start", 200), Ok(()));
        let r = GrantRevoke::new(&g, Running::Finish, 150, &w.other_owner.keys);
        assert_eq!(check(&g, &d, &[spite, r.clone()], &owners, "runs.start", 200), Err(GrantRefusal::Revoked));
        // The platform can't move a revoke to another grant, or turn a
        // finish into a stop.
        let g2 = grant(&d, &w.owner.keys);
        let moved = GrantRevoke { grant: g2.id.clone(), ..r.clone() };
        assert_eq!(check(&g2, &d, &[moved], &owners, "runs.start", 200), Ok(()));
        let harsher = GrantRevoke { running: Running::Stop, ..r.clone() };
        assert!(!harsher.revokes(&g, &owners));
    }

    #[test]
    fn a_personal_project() {
        let me = person("me");
        let d = delegation("account:me", &me.keys, 10);
        let g = grant(&d, &me.keys);
        let t = Trust { account: "me".into(), root: me.cert.device.clone() };
        let owners = Owners::Account { trust: &t, certs: std::slice::from_ref(&me.cert), revocations: &[] };
        assert_eq!(check(&g, &d, &[], &owners, "runs.start", 200), Ok(()));
        // A phone of mine, removed since: its grant stops counting.
        let pk = DeviceKeys::generate();
        let mut pc = Cert::new(&pk, "me", Kind::Browser, "phone");
        pc.sign_with(&me.keys);
        let g = grant(&d, &pk);
        let both = [me.cert.clone(), pc.clone()];
        let owners = Owners::Account { trust: &t, certs: &both, revocations: &[] };
        assert_eq!(check(&g, &d, &[], &owners, "runs.start", 200), Ok(()));
        let gone = [Revocation::new("me", &pc.device, &me.keys)];
        let owners = Owners::Account { trust: &t, certs: &both, revocations: &gone };
        assert_eq!(check(&g, &d, &[], &owners, "runs.start", 200), Err(GrantRefusal::Unsigned));
    }

    #[test]
    fn the_bytes_read_back() {
        let me = person("me");
        let d = delegation("account:me", &me.keys, 10);
        for expires in [None, Some(500)] {
            let mut g = ProjectGrant::new(&d, 100, expires);
            g.sign_with(&me.keys);
            assert_eq!(ProjectGrant::parse(&g.body(), &g.sig), Some(g.clone()));
            for running in [Running::Finish, Running::Stop] {
                let r = GrantRevoke::new(&g, running, 300, &me.keys);
                assert_eq!(GrantRevoke::parse(&r.body(), &r.sig), Some(r));
            }
        }
        let g = grant(&d, &me.keys);
        let body = g.body();
        for bad in [
            body.replace("resource hosted", "resource machine"),
            body.replace("to platform", "to agent"),
            body.replace("arugula project grant v1", "arugula exec grant v1"),
            format!("{body}extra\n"),
            body.trim_end().to_owned(),
        ] {
            assert_eq!(ProjectGrant::parse(&bad, &g.sig), None, "{bad}");
        }
    }
}

#[cfg(test)]
mod fixed {
    use super::*;
    use crate::{Cert, DeviceKeys, Kind, Trust, hosted::HostedKey};
    use ed25519_dalek::SigningKey;

    fn keys(sign: u8, noise: u8) -> DeviceKeys {
        DeviceKeys { noise_private: [0; 32], noise_public: [noise; 32], sign: SigningKey::from_bytes(&[sign; 32]) }
    }

    /// The page signs the same bytes (`web/scripts/run-grant.fixture.json`,
    /// checked by `web/scripts/run-grant.test.mjs`): the same bodies and
    /// signatures, from the same keys.
    #[test]
    fn the_pages_fixture_is_these_bytes() {
        let owner = keys(2, 3);
        let mut root = Cert::new(&owner, "acct1", Kind::Browser, "laptop");
        root.created = 1000;
        root.sign_with(&owner);
        let mut d = Delegation {
            project: "prj_1".into(),
            owner: "account:acct1".into(),
            key: HostedKey::from_seed(&[1; 32]).public(),
            at: 2000,
            by: String::new(),
            sig: String::new(),
        };
        d.sign_with(&owner);
        let mut g = ProjectGrant { id: "0123456789abcdef0123456789abcdef".into(), ..ProjectGrant::new(&d, 3000, None) };
        g.sign_with(&owner);
        let mut e =
            ProjectGrant { id: "fedcba9876543210fedcba9876543210".into(), ..ProjectGrant::new(&d, 3000, Some(9000)) };
        e.sign_with(&owner);
        let r = GrantRevoke::new(&g, Running::Stop, 4000, &owner);
        let got = serde_json::json!({
            "root": root, "delegation": d, "grant": g, "body": g.body(), "expiring": e, "revoke": r, "revoke_body": r.body(),
        });
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../web/scripts/run-grant.fixture.json");
        if std::env::var_os("ARUGULA_WRITE_FIXTURE").is_some() {
            std::fs::write(&path, serde_json::to_string_pretty(&got).unwrap() + "\n").unwrap();
        }
        let want: serde_json::Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        assert_eq!(got, want, "ARUGULA_WRITE_FIXTURE=1 writes it again, if the formats changed on purpose");
        let t = Trust { account: "acct1".into(), root: root.device.clone() };
        let owners = Owners::Account { trust: &t, certs: std::slice::from_ref(&root), revocations: &[] };
        assert_eq!(check(&g, &d, &[], &owners, "runs.start", 3500), Ok(()));
        assert_eq!(check(&g, &d, &[r], &owners, "runs.start", 3500), Err(GrantRefusal::Revoked));
    }
}
