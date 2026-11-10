//! Runs under the project's standing grant (M211, "One standing grant per
//! project starts runs", aru-099): what a hosted machine checks before the
//! platform's start goes ahead, with every owner's device asleep.
//!
//! The platform sends the grant with each start (`runs.start`). The machine
//! checks it with what it pinned at enrollment and each refresh, not the
//! platform's word ([`check`]): an owner of its project signed it, for its
//! own project and the delegation it was enrolled under, that delegation
//! isn't revoked, it covers what's asked, it hasn't expired, and no owner
//! revoked it. A v2 grant (DK6, aru-126) is an owner's passkey assertion
//! over its bytes, checked against that owner's passkey log as the machine
//! pinned it from control (the platform's copy, carried with the start,
//! only takes keys away); a v1, from before, an owner's browser's device
//! signature (the team's roster and members' certificates it holds, or its
//! account's chain), until it expires.
//!
//! Revokes are S37's: an owner's (a passkey's since DK6, a device's
//! before), relayed by the platform (`runs.revoke`, and with every start).
//! Each takes back every grant of the project under its delegation made by
//! its time, v1 or v2. The machine checks a revoke's signature once, when
//! it first hears it, and **keeps** the newest
//! (`<state>/run-grant-revokes.json`): after that it counts without being
//! checked again, so the platform can't bring a grant back by leaving the
//! revoke out, or by dropping its signer's certificate or passkey from what
//! it sends. A machine that never heard a revoke honours the grant until it
//! expires.
//!
//! **Newest seen** (`<state>/run-grant-newest.json`). A grant that took
//! another's place doesn't undo the old one's signature, so the machine
//! keeps which is the newest it has seen check out, and refuses any older
//! ([`SUPERSEDED`]). The order is the `made` a grant signs, then its id. An
//! owner's clock can be off, so the platform is what makes that order the
//! order they were given: it takes a grant only if its `made` is past every
//! grant the project has had, and the page signs one past the last. A machine
//! that's only ever seen the old one can't know: it goes by it until it's
//! told otherwise, or the grant expires.
//!
//! The kept file fails closed: once a revoke was kept (a marker beside it
//! says so), a missing or unreadable file refuses every start. It's in
//! arugulad's state directory, which runs must never be able to write
//! (M212: run as another user, or outside it).
//!
//! The grant gates what the platform starts, and every turn of a thread's
//! agent session, whoever or whatever asks for it. People's other use of the
//! machine (its owners and editors, M180) doesn't go through it.
//!
//! A start that names a run (M212) has the machine pull its work at once
//! (`crate::work`); the run is checked again here ([`allows`]) before it
//! starts. A revoke that says `running stop` stops the runs going under
//! the grants it covers (`crate::work::stop_revoked`). A thread's agent
//! session is checked the same way for `runs.instruct`, before it's taken
//! and before each turn, and any revoke ends it ([`revoke_over`]).

use std::{path::Path, sync::Mutex};

use anyhow::Context as _;
use arugula_control_wire::{
    forge::OpAnswer,
    runs::{RunRevoke, RunStart},
};
use arugula_e2e::{
    cert::LegacyAppChildren,
    hosted::{Owner, Owners},
    now_ms,
    passkey::{Log, Origins},
    project_grant::{self, GrantRefusal, GrantRevoke, ProjectGrant, Revokes, Running, Signers},
};
use tracing::{info, warn};

use crate::control::Saved;

/// The revokes this machine checked and kept, in its state directory.
const FILE: &str = "run-grant-revokes.json";
/// Written before [`FILE`] first is: a revoke was kept, so [`FILE`] must be
/// there.
const MARKER: &str = "run-grant-revokes.kept";

/// The newest grant this machine has seen check out.
const NEWEST: &str = "run-grant-newest.json";

/// One writer at a time for [`FILE`] and [`NEWEST`].
static KEPT: Mutex<()> = Mutex::new(());

/// Why a start was refused: a code the platform shows, and what it means.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Refused {
    pub code: &'static str,
    pub says: &'static str,
}

impl From<GrantRefusal> for Refused {
    fn from(r: GrantRefusal) -> Self {
        Refused { code: r.code(), says: r.says() }
    }
}

const NOT_HOSTED: Refused = Refused { code: "not_hosted", says: "this machine isn't a hosted machine" };
const WRONG_WORKSPACE: Refused =
    Refused { code: "wrong_workspace", says: "this hosted machine is another workspace's" };
/// A session's work listed no grant.
pub const NO_GRANT: Refused = Refused {
    code: "no_grant",
    says: "this project has no standing grant for agent sessions: an owner of the project allows them with a passkey",
};
/// What's refused for what this machine pinned of who may sign, which a
/// refresh brings up to date.
pub const PIN_CAN_CHANGE: &[&str] = &[
    GrantRefusal::Unsigned.code(),
    GrantRefusal::NotAnOwner.code(),
    GrantRefusal::PasskeyLog.code(),
    GrantRefusal::Passkey.code(),
];
/// A newer grant took this one's place, as this machine has seen.
pub const SUPERSEDED: Refused = Refused {
    code: "grant_superseded",
    says: "a newer standing grant of this project has taken this one's place: this machine has seen it",
};
const UNREADABLE: Refused = Refused {
    code: "revokes_unreadable",
    says: "this hosted machine can't read the revokes it kept, so it starts nothing: rebuild the workspace",
};

/// Who may sign for the machine's project, from what it pinned.
pub fn owners(saved: &Saved) -> Option<Owners<'_>> {
    let pin = saved.hosted()?;
    match pin.delegation.owner()? {
        Owner::Team(_) => {
            let team = pin.team.as_ref()?;
            Some(Owners::Team { roster: team.roster.as_ref()?, certs: &team.certs })
        }
        Owner::Account(_) => {
            Some(Owners::Account { trust: &saved.trust, certs: &saved.certs, revocations: &saved.revocations })
        }
    }
}

/// The browsers and phones the desktop app's old windows approved, as this
/// machine pinned them with its owners' certificates (#1394): an app window
/// counts just above one of them and nowhere else. None pinned (a machine
/// enrolled before #1394, until its first refresh): no app window counts.
pub(crate) fn legacy_app_children(saved: &Saved) -> &LegacyAppChildren {
    const NONE: &LegacyAppChildren = &LegacyAppChildren::new();
    saved.hosted().and_then(|p| p.legacy_app_children.as_ref()).unwrap_or(NONE)
}

/// No passkey logs pinned.
static NO_LOGS: std::collections::BTreeMap<String, Log> = std::collections::BTreeMap::new();
/// No origins pinned: no passkey counts.
static NO_ORIGINS: Origins = Origins::new();

/// Who may sign for the machine's project and what it checks them by
/// (DK6): `owners`, with the passkey logs and origins it pinned from
/// control (as a deploy's box does, DK4: each log only ever extended, the
/// origins only shrunk; none pinned, no passkey counts), and for a v1 the
/// browsers the desktop app's old windows approved.
pub fn signers<'a>(saved: &'a Saved, owners: &'a Owners<'a>) -> Signers<'a> {
    let pin = saved.hosted();
    Signers {
        pinned: arugula_e2e::deploy::Pinned {
            owners,
            passkey_logs: pin.map_or(&NO_LOGS, |p| &p.passkey_logs),
            origins: pin.and_then(|p| p.passkey_origins.as_ref()).unwrap_or(&NO_ORIGINS),
        },
        children: legacy_app_children(saved),
    }
}

/// Whether the platform may do `s.cap` on this machine under `s.grant` at
/// `now`, with the revokes it sent (checked now) and the ones `kept` here
/// (checked when they were kept).
pub fn check(saved: &Saved, s: &RunStart, kept: &[GrantRevoke], now: u64) -> Result<(), Refused> {
    let Some(pin) = saved.hosted() else { return Err(NOT_HOSTED) };
    if s.workspace != pin.cert.workspace {
        return Err(WRONG_WORKSPACE);
    }
    if s.project != pin.cert.project {
        return Err(GrantRefusal::WrongProject.into());
    }
    let owners = owners(saved).ok_or(GrantRefusal::Unsigned)?;
    let revokes = Revokes { delegation: &pin.revocations, closed: &pin.closed, sent: &s.revokes, kept };
    let signers = signers(saved, &owners);
    project_grant::check(&s.grant, &pin.delegation, revokes, &signers, s.passkey_log.as_ref(), &s.cap, now)?;
    Ok(())
}

/// The revokes kept here. Fails closed: a file that isn't there though a
/// revoke was kept (the marker), or that doesn't read, is an error, never
/// "none".
fn read_kept(dir: &Path) -> anyhow::Result<Vec<GrantRevoke>> {
    match std::fs::read(dir.join(FILE)) {
        Ok(b) => serde_json::from_slice(&b).context("the kept revokes don't read"),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound && !dir.join(MARKER).exists() => Ok(vec![]),
        Err(e) => Err(anyhow::Error::new(e).context("the kept revokes are missing")),
    }
}

/// A grant's place in the order (module docs).
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, serde::Serialize, serde::Deserialize)]
struct Newest {
    made: u64,
    id: String,
}

impl Newest {
    fn of(g: &ProjectGrant) -> Self {
        Newest { made: g.made, id: g.id.clone() }
    }
}

/// The newest grant seen here. As [`read_kept`], one that doesn't read is an
/// error, never "none".
fn read_newest(dir: &Path) -> anyhow::Result<Option<Newest>> {
    match std::fs::read(dir.join(NEWEST)) {
        Ok(b) => Ok(Some(serde_json::from_slice(&b).context("the newest grant seen doesn't read")?)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(anyhow::Error::new(e).context("the newest grant seen can't be read")),
    }
}

/// `g`, which checked out, against the newest seen here: refused if it's
/// older, else it's the newest now.
fn fresh(dir: &Path, g: &ProjectGrant) -> Result<(), Refused> {
    let _held = KEPT.lock().unwrap_or_else(|e| e.into_inner());
    let newest = read_newest(dir).map_err(|e| {
        warn!(error = %format!("{e:#}"), "hosted: refusing starts: the newest grant seen can't be read");
        UNREADABLE
    })?;
    let this = Newest::of(g);
    if newest.as_ref().is_some_and(|n| *n > this) {
        return Err(SUPERSEDED);
    }
    if newest.as_ref() != Some(&this) {
        let wrote = serde_json::to_vec(&this)
            .map_err(anyhow::Error::new)
            .and_then(|b| crate::store::write_atomic(&dir.join(NEWEST), &b).map_err(anyhow::Error::new));
        if let Err(e) = wrote {
            warn!(error = %format!("{e:#}"), "hosted: can't keep the newest grant seen");
        }
    }
    Ok(())
}

/// Have seen `g`, the project's grant as the platform lists it, whatever it
/// gives: a narrower one takes an older one's place too. Nothing if it
/// doesn't check out.
pub fn notice(app: &crate::server::App, project: &str, workspace: &str, g: &ProjectGrant, carried: Option<&Log>) {
    let dir = app.control.state_dir();
    if read_newest(dir).ok().flatten().is_some_and(|n| n >= Newest::of(g)) {
        return;
    }
    let Some(cap) = g.caps.first().cloned() else { return };
    let start = RunStart {
        project: project.to_owned(),
        workspace: workspace.to_owned(),
        run: None,
        cap,
        grant: g.clone(),
        revokes: vec![],
        passkey_log: carried.cloned(),
    };
    let _ = allows(app, &start);
}

/// Keep the revokes among `revokes` that are about this machine (its
/// project and delegation) and that one of its project's owners signed,
/// checked at `now`: the newest one, as it covers what any older one did.
/// Whether it kept one.
fn keep(dir: &Path, saved: &Saved, revokes: &[GrantRevoke], now: u64) -> anyhow::Result<bool> {
    let (Some(pin), Some(owners)) = (saved.hosted(), owners(saved)) else { return Ok(false) };
    let delegation = pin.delegation.id();
    let signers = signers(saved, &owners);
    let Some(newest) = revokes
        .iter()
        .filter(|r| {
            r.project == pin.cert.project
                && r.delegation == delegation
                && r.signed_for(&pin.delegation.owner, &signers, now)
        })
        .max_by_key(|r| r.at)
    else {
        return Ok(false);
    };
    let _held = KEPT.lock().unwrap_or_else(|e| e.into_inner());
    let mut kept = read_kept(dir)?;
    if kept.iter().any(|k| k.project == newest.project && k.delegation == newest.delegation && k.at >= newest.at) {
        return Ok(false);
    }
    kept.retain(|k| !(k.project == newest.project && k.delegation == newest.delegation));
    kept.push(newest.clone());
    crate::store::write_atomic(&dir.join(MARKER), b"")?;
    crate::store::write_atomic(&dir.join(FILE), &serde_json::to_vec_pretty(&kept)?)?;
    Ok(true)
}

fn saved(app: &crate::server::App) -> Option<Saved> {
    app.control.enrolled().map(|e| e.saved.clone())
}

/// Whether the platform may do `s.cap` here under `s.grant` now: the
/// revokes it carries kept first, then [`check`]ed with every revoke kept
/// here. What a start asks, and what a run checks before it starts (M212).
pub fn allows(app: &crate::server::App, s: &RunStart) -> Result<(), Refused> {
    let Some(saved) = saved(app) else { return Err(NOT_HOSTED) };
    let dir = app.control.state_dir();
    let now = now_ms();
    if let Err(e) = keep(dir, &saved, &s.revokes, now) {
        warn!(error = %format!("{e:#}"), "hosted: can't keep a standing grant's revoke");
    }
    let kept = match read_kept(dir) {
        Ok(k) => k,
        Err(e) => {
            warn!(error = %format!("{e:#}"), "hosted: refusing starts: the kept revokes can't be read");
            return Err(UNREADABLE);
        }
    };
    check(&saved, s, &kept, now)?;
    fresh(dir, &s.grant)
}

/// Keep what's about this machine among `revokes` (they come with each
/// pull, M212), checked as a relayed revoke is.
pub fn keep_revokes(app: &crate::server::App, revokes: &[GrantRevoke]) {
    let Some(saved) = saved(app) else { return };
    if let Err(e) = keep(app.control.state_dir(), &saved, revokes, now_ms()) {
        warn!(error = %format!("{e:#}"), "hosted: can't keep a standing grant's revoke");
    }
}

/// Whether the runs going under `g` stop (M212): a revoke kept here covers
/// it and says `running stop`; or the kept revokes can't be read, which
/// fails closed.
pub fn stops(app: &crate::server::App, g: &ProjectGrant) -> bool {
    match read_kept(app.control.state_dir()) {
        Ok(kept) => kept.iter().any(|r| r.running == Running::Stop && r.covers(g)),
        Err(_) => true,
    }
}

/// The kept revoke's word on a session here, under `grant` (the last one
/// that checked out for it), taken at `taken_at`: `None` when none takes it
/// back, else whether a turn under way finishes first. A session from
/// before sessions held a grant has none to compare, so a revoke made since
/// it was taken covers it. Revokes that can't be read fail closed.
pub fn revoke_over(app: &crate::server::App, grant: Option<&ProjectGrant>, taken_at: u64) -> Option<Running> {
    match read_kept(app.control.state_dir()) {
        Ok(kept) => over(&kept, grant, taken_at),
        Err(_) => Some(Running::Stop),
    }
}

/// [`revoke_over`], of the revokes `kept`.
fn over(kept: &[GrantRevoke], grant: Option<&ProjectGrant>, taken_at: u64) -> Option<Running> {
    let covering = |r: &&GrantRevoke| grant.map_or(r.at >= taken_at, |g| r.covers(g));
    let mut covers = kept.iter().filter(covering).peekable();
    covers.peek()?;
    Some(if covers.any(|r| r.running == Running::Stop) { Running::Stop } else { Running::Finish })
}

/// `runs.start`: check the grant, and answer. A start that names a run
/// (M212) has the machine ask for its work now: the run itself comes that
/// way, and is checked again before it starts.
pub fn start(app: &crate::server::App, s: &RunStart) -> OpAnswer {
    match allows(app, s) {
        Ok(()) => {
            info!(
                grant = s.grant.id,
                cap = s.cap,
                run = s.run.as_deref().unwrap_or("-"),
                "hosted: a start under the project's standing grant checks out"
            );
            if s.run.is_some() {
                crate::work::nudge();
            }
            OpAnswer::Done { url: None, routed: None }
        }
        Err(r) => {
            info!(grant = s.grant.id, code = r.code, "hosted: refused a start under the project's standing grant");
            refused(r)
        }
    }
}

/// `runs.revoke`: keep it, if it's about this machine and an owner of its
/// project signed it.
pub fn revoke(app: &crate::server::App, r: &RunRevoke) -> OpAnswer {
    let Some(saved) = saved(app) else { return refused(NOT_HOSTED) };
    let stop = r.revoke.running == Running::Stop;
    let now = now_ms();
    match keep(app.control.state_dir(), &saved, std::slice::from_ref(&r.revoke), now) {
        Ok(true) => {
            info!(
                delegation = r.revoke.delegation,
                at = r.revoke.at,
                stop,
                "hosted: the project's standing grants were revoked"
            );
            // What it stops goes now: nothing here waits on control. A
            // session it ends with its turn is ended then, by what follows it.
            crate::work::stop_revoked(app);
            crate::work::nudge();
            OpAnswer::Done { url: None, routed: None }
        }
        // Not this machine's, not an owner's, or an older one than it has.
        Ok(false) => {
            let signed = saved
                .hosted()
                .zip(owners(&saved))
                .is_some_and(|(p, o)| r.revoke.signed_for(&p.delegation.owner, &signers(&saved, &o), now));
            if signed { OpAnswer::Done { url: None, routed: None } } else { refused(GrantRefusal::Unsigned.into()) }
        }
        Err(e) => OpAnswer::Failed { error: format!("{e:#}") },
    }
}

fn refused(r: Refused) -> OpAnswer {
    OpAnswer::Cant { why: format!("{}: {}", r.code, r.says) }
}

#[cfg(test)]
pub(crate) mod tests {
    use arugula_control_wire::hosted;
    use arugula_e2e::{
        Cert, DeviceKeys, Kind, Trust,
        hosted::{Delegation, DelegationRevoke, HostedCert, HostedKey},
        project_grant::ProjectGrant,
    };

    use super::*;

    /// Control's hosted key in [`machine`].
    pub(crate) const MACHINE_KEY: [u8; 32] = [7; 32];

    /// A hosted machine of an account's project, enrolled under a
    /// delegation from the account's root device; and the account's phone,
    /// another of its devices.
    pub(crate) fn machine() -> (Saved, Delegation, DeviceKeys, DeviceKeys) {
        let root = DeviceKeys::generate();
        let mut root_cert = Cert::new(&root, "acct1", Kind::Browser, "laptop");
        root_cert.sign_with(&root);
        let phone = DeviceKeys::generate();
        let mut phone_cert = Cert::new(&phone, "acct1", Kind::Browser, "phone");
        phone_cert.sign_with(&root);
        let key = HostedKey::from_seed(&MACHINE_KEY);
        let mut d = Delegation {
            project: "prj_1".into(),
            owner: "account:acct1".into(),
            key: key.public(),
            at: 1,
            by: String::new(),
            sig: String::new(),
        };
        d.sign_with(&root);
        let keys = DeviceKeys::generate();
        let (noise, sign) = (hex::encode(keys.noise_public), hex::encode(keys.sign_public()));
        let mut cert = HostedCert::new(&d, "hw_1", "box", &noise, &sign, 2);
        key.sign(&mut cert);
        let got = hosted::Enrollment {
            url: "http://control".into(),
            cert,
            delegation: d.clone(),
            revocations: vec![],
            closed: vec![],
            team: None,
            roster: None,
            certs: Default::default(),
            trust: Some(Trust { account: "acct1".into(), root: root.id() }),
            account_certs: vec![root_cert, phone_cert],
            account_revocations: vec![],
            locked: false,
            lock: None,
            legacy_app_children: None,
            passkey_logs: None,
            passkey_origins: None,
            purpose: None,
        };
        (super::super::trust::saved_from(got, &keys, None).unwrap(), d, root, phone)
    }

    fn grant(d: &Delegation, by: &DeviceKeys, made: u64) -> ProjectGrant {
        let mut g = ProjectGrant::new(d, made, Some(made + 1_000_000));
        g.sign_with(by);
        g
    }

    fn start(g: &ProjectGrant) -> RunStart {
        RunStart {
            project: "prj_1".into(),
            workspace: "hw_1".into(),
            run: None,
            cap: "runs.start".into(),
            grant: g.clone(),
            revokes: vec![],
            passkey_log: None,
        }
    }

    #[test]
    fn a_start_checks_the_grant_against_the_enrollment() {
        let (saved, d, root, _) = machine();
        let g = grant(&d, &root, 10);
        assert_eq!(check(&saved, &start(&g), &[], 20), Ok(()));
        // Another workspace's start, another project's, a grant signed by
        // someone else, one for another delegation.
        let s = RunStart { workspace: "hw_2".into(), ..start(&g) };
        assert_eq!(check(&saved, &s, &[], 20), Err(WRONG_WORKSPACE));
        let s = RunStart { project: "prj_2".into(), ..start(&g) };
        assert_eq!(check(&saved, &s, &[], 20).unwrap_err().code, "grant_wrong_project");
        let forged = grant(&d, &DeviceKeys::generate(), 10);
        assert_eq!(check(&saved, &start(&forged), &[], 20).unwrap_err().code, "grant_unsigned");
        let other = Delegation { at: 5, ..d.clone() };
        let moved = grant(&other, &root, 10);
        assert_eq!(check(&saved, &start(&moved), &[], 20).unwrap_err().code, "grant_wrong_delegation");
        // A machine that isn't hosted.
        let plain = Saved { kind: crate::control::MachineKind::Personal, ..saved.clone() };
        assert_eq!(check(&plain, &start(&g), &[], 20), Err(NOT_HOSTED));
    }

    /// The newest grant seen here: one older refused, by `made` then by id,
    /// whichever clock signed it; the same one, and a newer one, taken; and a
    /// file that doesn't read refuses.
    #[test]
    fn a_grant_older_than_the_newest_seen_is_refused() {
        let (_, d, root, _) = machine();
        let dir = super::super::scratch("run-grant-newest");
        let (old, new) = (grant(&d, &root, 10), grant(&d, &root, 20));
        assert_eq!(fresh(&dir, &old), Ok(()));
        assert_eq!(fresh(&dir, &old), Ok(()), "the same one again");
        assert_eq!(fresh(&dir, &new), Ok(()));
        assert_eq!(fresh(&dir, &old), Err(SUPERSEDED));
        assert_eq!(fresh(&dir, &new), Ok(()));
        let later = grant(&d, &root, 30);
        assert_eq!(fresh(&dir, &later), Ok(()));
        assert_eq!(fresh(&dir, &new), Err(SUPERSEDED));
        std::fs::write(dir.join(NEWEST), b"not json").unwrap();
        assert_eq!(fresh(&dir, &later), Err(UNREADABLE));
    }

    /// Which revoke takes a session back: one covering the grant it holds,
    /// however its clock reads; one made since it was taken, for a session
    /// from before sessions held a grant; `stop` over `finish`.
    #[test]
    fn a_session_is_taken_back_by_a_revoke_covering_its_grant() {
        let (_, d, root, _) = machine();
        let g = grant(&d, &root, 10);
        let r = |running, at| GrantRevoke::new(&g.project, &g.delegation, running, at, &root);
        // A grant made after the revoke is a new one.
        assert_eq!(over(&[r(Running::Stop, 9)], Some(&g), 1000), None);
        assert_eq!(over(&[r(Running::Stop, 10)], Some(&g), 1000), Some(Running::Stop));
        assert_eq!(over(&[r(Running::Finish, 30)], Some(&g), 1000), Some(Running::Finish));
        assert_eq!(over(&[], Some(&g), 1000), None);
        // No grant held: by when it was taken.
        assert_eq!(over(&[r(Running::Finish, 30)], None, 40), None);
        assert_eq!(over(&[r(Running::Finish, 40)], None, 40), Some(Running::Finish));
        let (a, b) = (r(Running::Finish, 50), r(Running::Stop, 50));
        assert_eq!(over(&[a, b], None, 40), Some(Running::Stop));
    }

    #[test]
    fn a_revoke_heard_once_is_kept() {
        let (saved, d, root, _) = machine();
        let dir = super::super::scratch("run-grant");
        let g = grant(&d, &root, 10);
        let r = GrantRevoke::new(&g.project, &g.delegation, Running::Finish, 30, &root);
        // A revoke nobody in the project signed isn't kept, nor one for
        // another delegation.
        let spite = GrantRevoke::new(&g.project, &g.delegation, Running::Stop, 30, &DeviceKeys::generate());
        assert!(!keep(&dir, &saved, &[spite], 30).unwrap());
        let elsewhere = GrantRevoke::new(&g.project, "another", Running::Stop, 30, &root);
        assert!(!keep(&dir, &saved, &[elsewhere], 30).unwrap());
        assert!(read_kept(&dir).unwrap().is_empty());
        assert!(keep(&dir, &saved, std::slice::from_ref(&r), 30).unwrap());
        assert!(!keep(&dir, &saved, std::slice::from_ref(&r), 30).unwrap(), "once");
        // The platform leaves it out of the next start: still revoked.
        assert_eq!(check(&saved, &start(&g), &read_kept(&dir).unwrap(), 40).unwrap_err().code, "grant_revoked");
        // A grant made after it isn't.
        let g2 = grant(&d, &root, 50);
        assert_eq!(check(&saved, &start(&g2), &read_kept(&dir).unwrap(), 60), Ok(()));
        // A newer revoke takes its place; an older one doesn't.
        let r2 = GrantRevoke::new(&g.project, &g.delegation, Running::Finish, 55, &root);
        assert!(keep(&dir, &saved, &[r.clone(), r2.clone()], 30).unwrap());
        assert!(!keep(&dir, &saved, std::slice::from_ref(&r), 30).unwrap());
        assert_eq!(read_kept(&dir).unwrap(), [r2]);
        assert_eq!(check(&saved, &start(&g2), &read_kept(&dir).unwrap(), 60).unwrap_err().code, "grant_revoked");
    }

    /// Signed by device A, revoked by owner device B; then a refresh from
    /// the platform without B's certificate: the start is still refused.
    #[test]
    fn a_kept_revoke_survives_a_refresh_without_its_signer() {
        let (saved, d, root, phone) = machine();
        let dir = super::super::scratch("run-grant-refresh");
        let g = grant(&d, &root, 10);
        let r = GrantRevoke::new(&g.project, &g.delegation, Running::Finish, 30, &phone);
        assert!(keep(&dir, &saved, std::slice::from_ref(&r), 30).unwrap());
        let mut refreshed = saved.clone();
        refreshed.certs.retain(|c| c.device != phone.id());
        // Sent again with the start, it no longer checks out; kept, it counts.
        let s = RunStart { revokes: vec![r.clone()], ..start(&g) };
        assert_eq!(check(&refreshed, &s, &[], 40), Ok(()));
        assert_eq!(check(&refreshed, &s, &read_kept(&dir).unwrap(), 40).unwrap_err().code, "grant_revoked");
    }

    #[test]
    fn the_kept_revokes_fail_closed() {
        let (saved, d, root, _) = machine();
        let dir = super::super::scratch("run-grant-closed");
        assert!(read_kept(&dir).unwrap().is_empty(), "never had one");
        let g = grant(&d, &root, 10);
        let r = GrantRevoke::new(&g.project, &g.delegation, Running::Finish, 30, &root);
        assert!(keep(&dir, &saved, &[r], 30).unwrap());
        std::fs::write(dir.join(FILE), "not json").unwrap();
        assert!(read_kept(&dir).is_err(), "corrupt");
        std::fs::remove_file(dir.join(FILE)).unwrap();
        assert!(read_kept(&dir).is_err(), "missing, though one was kept");
    }

    /// A browser the desktop app's old window approved signs a grant only
    /// once the machine pinned it as such (M253, #1394); the window never.
    #[test]
    fn an_app_window_counts_only_by_the_pinned_set() {
        let (mut saved, d, root, _) = machine();
        let mk = |name: &str, by: &DeviceKeys| {
            let k = DeviceKeys::generate();
            let mut c = Cert::new(&k, "acct1", Kind::Browser, name);
            c.sign_with(by);
            (k, c)
        };
        let (window, window_cert) = mk("Arugula app on jake-air", &root);
        let (firefox, firefox_cert) = mk("Firefox on Mac", &window);
        let (edge, edge_cert) = mk("Edge", &window);
        saved.certs.extend([window_cert, firefox_cert, edge_cert]);
        let code =
            |saved: &Saved, by: &DeviceKeys| check(saved, &start(&grant(&d, by, 10)), &[], 20).err().map(|r| r.code);
        // Enrolled before #1394: none pinned, until its first refresh.
        assert_eq!(code(&saved, &firefox), Some("grant_unsigned"));
        saved.hosted_mut().unwrap().legacy_app_children = Some([("acct1".to_owned(), vec![firefox.id()])].into());
        assert_eq!(code(&saved, &firefox), None);
        assert_eq!(code(&saved, &edge), Some("grant_unsigned"), "not one it approved back then");
        assert_eq!(code(&saved, &window), Some("grant_unsigned"), "the window itself");
        assert_eq!(code(&saved, &root), None);
    }

    #[test]
    fn a_revoked_delegation_stops_its_grants() {
        let (mut saved, d, root, _) = machine();
        let g = grant(&d, &root, 10);
        saved.hosted_mut().unwrap().revocations.push(DelegationRevoke::new(&d, 30, &root));
        assert_eq!(check(&saved, &start(&g), &[], 40).unwrap_err().code, "delegation_revoked");
    }

    /// A box that holds control's word stays stopped, by a revoke from a
    /// device now off the account too.
    #[test]
    fn a_revoked_delegation_stays_revoked_when_its_signer_goes() {
        let (mut saved, d, root, phone) = machine();
        let g = grant(&d, &root, 10);
        saved.hosted_mut().unwrap().revocations.push(DelegationRevoke::new(&d, 30, &phone));
        assert_eq!(check(&saved, &start(&g), &[], 40).unwrap_err().code, "delegation_revoked");
        saved.revocations.push(arugula_e2e::Revocation::new("acct1", &phone.id(), &root));
        assert!(check(&saved, &start(&g), &[], 40).is_ok());
        saved.hosted_mut().unwrap().closed.push(HostedKey::from_seed(&MACHINE_KEY).close(&d, 30));
        assert_eq!(check(&saved, &start(&g), &[], 40).unwrap_err().code, "delegation_revoked");
    }

    /// DK6: a v2 grant counts by the passkey log the machine pinned from
    /// control (none pinned: refused), the v1 beside it still counts, and a
    /// passkey revoke heard once is kept: it takes back both, and holds
    /// after its passkey is removed.
    #[test]
    fn a_passkey_grant_and_its_revoke() {
        use arugula_e2e::passkey::{Assertion, Change, Draft, EDDSA, NO_AAGUID, origins_of_sites};
        use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD as B64};
        use sha2::{Digest as _, Sha256};
        let (mut saved, d, root, _) = machine();
        // A passkey here is an Ed25519 key like a device's.
        let pk = DeviceKeys::generate();
        let sign = |challenge: &str| {
            let cd = serde_json::json!({ "type": "webauthn.get", "challenge": challenge, "origin": "https://c.test" })
                .to_string();
            let mut data = Sha256::digest(b"c.test").to_vec();
            data.extend_from_slice(&[0x05, 0, 0, 0, 0]);
            let mut msg = data.clone();
            msg.extend_from_slice(&Sha256::digest(cd.as_bytes()));
            Assertion {
                id: "pk".into(),
                client_data_json: B64.encode(cd.as_bytes()),
                authenticator_data: B64.encode(&data),
                signature: B64.encode(pk.signature(&msg)),
            }
        };
        let mut g = ProjectGrant::v2(&d, "acct1", 10, 1_000);
        g.passkey = Some(sign(&g.passkey_challenge()));
        assert_eq!(check(&saved, &start(&g), &[], 20).unwrap_err().code, "grant_passkey_log", "none pinned");
        let added = Draft {
            account: "acct1".into(),
            event: Change::Added,
            credential: "pk".into(),
            rp: "c.test".into(),
            alg: EDDSA,
            key: pk.sign_public().to_vec(),
            aaguid: NO_AAGUID.into(),
            at: 1,
            by: "signup".into(),
        }
        .after(None);
        let log = Log { account: "acct1".into(), events: vec![added], began: None };
        let pin = saved.hosted_mut().unwrap();
        pin.passkey_logs = [("acct1".to_owned(), log.clone())].into();
        pin.passkey_origins = Some(origins_of_sites(["https://c.test"]));
        let v1 = grant(&d, &root, 10);
        assert_eq!(check(&saved, &start(&g), &[], 20), Ok(()));
        assert_eq!(check(&saved, &start(&v1), &[], 20), Ok(()));
        let mut r = GrantRevoke::v3(&g.project, &g.delegation, Running::Finish, 30, "acct1");
        r.passkey = Some(sign(&r.passkey_challenge()));
        let dir = super::super::scratch("run-grant-passkey");
        let forged = GrantRevoke { passkey: Some(sign("other")), ..r.clone() };
        assert!(!keep(&dir, &saved, &[forged], 30).unwrap());
        assert!(keep(&dir, &saved, std::slice::from_ref(&r), 30).unwrap());
        for g in [&g, &v1] {
            assert_eq!(check(&saved, &start(g), &read_kept(&dir).unwrap(), 40).unwrap_err().code, "grant_revoked");
        }
        // The passkey removed since: the v1 is still revoked by what was kept.
        let mut gone = log.clone();
        let e = gone.events[0].removal(35, "reset").placed_after(gone.events.last());
        gone.events.push(e);
        saved.hosted_mut().unwrap().passkey_logs = [("acct1".to_owned(), gone)].into();
        assert_eq!(check(&saved, &start(&v1), &read_kept(&dir).unwrap(), 40).unwrap_err().code, "grant_revoked");
        // A paused box that pulls the revoke only now, after its passkey
        // was removed, still keeps it: it's checked at its own time (30).
        let paused = super::super::scratch("run-grant-passkey-paused");
        assert!(keep(&paused, &saved, std::slice::from_ref(&r), 40).unwrap());
        assert_eq!(check(&saved, &start(&v1), &read_kept(&paused).unwrap(), 40).unwrap_err().code, "grant_revoked");
        // One dated after the removal isn't.
        let mut late = GrantRevoke::v3(&g.project, &g.delegation, Running::Finish, 36, "acct1");
        late.passkey = Some(sign(&late.passkey_challenge()));
        assert!(!late.signed_for(&d.owner, &signers(&saved, &owners(&saved).unwrap()), 40));
        assert_eq!(check(&saved, &start(&g), &[], 40).unwrap_err().code, "grant_passkey", "its passkey removed");
    }
}
