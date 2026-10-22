//! Hosted machines (M178): control enrolls them into a project under the
//! project's delegation ("Control enrolls hosted machines under a project
//! delegation", aru-095; docs/control-e2e.md, "Hosted machines").
//!
//! - **Delegating.** An owner's device signs a [`Delegation`] naming
//!   control's hosted key ([`load_key`], published in `/control.json`),
//!   and sends it ([`delegate`]); any owner's device can revoke it
//!   ([`revoke`]). The hub checks both against the roster or the account
//!   as a courtesy: clients check them again with what they hold.
//! - **Enrolling.** The platform records a one-time enrollment as it
//!   makes a box (`HubPort::hosted_enrollment`, [`ask`]): a ticket the hub
//!   makes, or the box's sandbox id. The box enrolls once ([`enroll`]),
//!   and the hub signs its [`HostedCert`] with the hosted key, under the
//!   newest live delegation. Fail closed: a used enrollment is refused,
//!   and so is one whose delegation was revoked since.
//! - **Reaching.** A hosted machine is reached only by its project's
//!   people ([`may_reach`]), listed only to them ([`directory`]), and
//!   signs only its own routes and the relay (`auth::HostedAuth`).
//!   Nothing that reads a daemon's account sees it: it isn't in
//!   `daemons`.

use std::sync::Arc;

use arugula_control_wire::hosted::{
    DelegationPost, EnrollAsk, EnrollGiven, EnrollRequest, Enrolled, Enrollment, RevokePost, SANDBOX_MS, TICKET_MS,
};
use arugula_e2e::{
    Cert, Revocation, Trust,
    hosted::{Delegation, DelegationRevoke, HostedCert, HostedKey, Owner, Owners, enroll_proof_body, live},
    now_ms,
    team::{AccountCerts, Roster, TeamPin, TeamRole},
};
use arugula_platform::PlatformPort as _;
use axum::{
    Json,
    extract::{ConnectInfo, Path, State},
    http::{HeaderMap, StatusCode},
};
use serde_json::{Value, json};
use tracing::info;

use crate::{
    ApiError, App,
    auth::{HostedAuth, Session, hash, token},
    db::{
        Db,
        hosted::{DelegationRow, HostedRow, Recorded, Taken},
    },
    err,
};

type R = Result<Json<Value>, ApiError>;

/// How far a delegation's time may be from control's clock.
const SKEW_MS: u64 = 10 * 60 * 1000;

/// Control's hosted key, made on first start and kept in the database, as
/// the VAPID key is. Changing it ends every delegation (they name it):
/// owners would sign again.
pub fn load_key(db: &crate::db::Db) -> anyhow::Result<HostedKey> {
    if let Some(h) = db.setting("hosted_key")? {
        let seed: [u8; 32] =
            hex::decode(h)?.try_into().map_err(|_| anyhow::anyhow!("the hosted key isn't 32 bytes"))?;
        return Ok(HostedKey::from_seed(&seed));
    }
    let k = HostedKey::generate();
    db.set_setting("hosted_key", &hex::encode(k.seed()))?;
    Ok(k)
}

/// What control holds of a project owner's people, to check signatures
/// against: a team's pin, roster now and members' certificates, or an
/// account's root and certificates.
pub(crate) enum People {
    Team { pin: TeamPin, roster: Box<Roster>, certs: AccountCerts },
    Account { trust: Trust, certs: Vec<Cert>, revocations: Vec<Revocation> },
}

impl People {
    pub(crate) fn owners(&self) -> Owners<'_> {
        match self {
            People::Team { roster, certs, .. } => Owners::Team { roster, certs },
            People::Account { trust, certs, revocations } => Owners::Account { trust, certs, revocations },
        }
    }
}

pub(crate) fn people(db: &Db, owner: &Owner) -> anyhow::Result<Option<People>> {
    Ok(match owner {
        Owner::Team(t) => {
            let (Some(team), Some(body)) = (db.team(t)?, db.latest_roster(t)?) else { return Ok(None) };
            let roster = crate::teams::parse(&body)?;
            let mut certs = AccountCerts::new();
            for m in &roster.members {
                // A deleted account's signing devices, kept to check the
                // history it signed (#173), as `teams::certs_for` does.
                let c = match db.retained_certs(&m.account)? {
                    Some(kept) if db.account(&m.account)?.is_none() => kept,
                    _ => (db.devices(&m.account)?.0, db.revocations(&m.account)?),
                };
                certs.insert(m.account.clone(), c);
            }
            Some(People::Team { pin: crate::teams::pin(&team), roster: Box::new(roster), certs })
        }
        Owner::Account(a) => {
            let Some(root) = db.account(a)?.and_then(|x| x.root) else { return Ok(None) };
            let (certs, revocations) = (db.devices(a)?.0, db.revocations(a)?);
            Some(People::Account { trust: Trust { account: a.clone(), root }, certs, revocations })
        }
    })
}

fn revokes(row: &DelegationRow) -> Vec<DelegationRevoke> {
    row.revoke.iter().cloned().collect()
}

/// `account`'s role in the project `owner` names, as control routes: a
/// locked team's machines are its owners' only, as for team machines.
fn role(app: &App, owner: &Owner, account: &str) -> anyhow::Result<Option<TeamRole>> {
    Ok(match owner {
        Owner::Account(a) => (a == account).then_some(TeamRole::Owner),
        Owner::Team(t) => {
            let (Some(team), Some(body)) = (app.db.team(t)?, app.db.latest_roster(t)?) else { return Ok(None) };
            crate::teams::role_in(&crate::teams::parse(&body)?, account)
                .filter(|r| !team.locked || *r == TeamRole::Owner)
        }
    })
}

/// The project owners `account` reaches hosted machines of: its own
/// account, and each team it's in (a locked one only as an owner).
fn owners_for(app: &App, account: &str) -> anyhow::Result<Vec<String>> {
    let mut out = vec![Owner::Account(account.to_owned()).as_string()];
    for body in app.db.teams_of(account)? {
        let t = Owner::Team(crate::teams::parse(&body)?.team);
        if role(app, &t, account)?.is_some() {
            out.push(t.as_string());
        }
    }
    Ok(out)
}

/// May `account` be routed to `id`? `None` when `id` isn't a hosted
/// machine (the caller goes on as for any daemon). Only its project's
/// people: never shared, never anyone's own, never a daemon's agents
/// beyond what their person may.
pub(crate) fn may_reach(app: &App, account: &str, id: &str) -> anyhow::Result<Option<bool>> {
    let Some(row) = app.db.hosted(id)? else { return Ok(None) };
    let Some(owner) = Owner::parse(&row.cert.owner) else { return Ok(Some(false)) };
    Ok(Some(role(app, &owner, account)?.is_some()))
}

/// The hosted machines `account` may reach, as the directory lists them:
/// with the certificate, the delegation and its revocations, which the
/// client checks against the roster or the account it holds itself.
pub(crate) fn directory(app: &App, account: &str) -> anyhow::Result<Vec<Value>> {
    let mut out = Vec::new();
    for h in app.db.hosted_of(&owners_for(app, account)?)? {
        let Some(d) = app.db.delegation(&h.cert.delegation)? else { continue };
        let team = match Owner::parse(&h.cert.owner) {
            Some(Owner::Team(t)) => Some(t),
            _ => None,
        };
        out.push(json!({
            "id": h.cert.device, "name": h.cert.name, "urls": h.urls, "last_seen": h.last_seen,
            "online": app.relay.online(&h.cert.device), "team": team,
            "hosted": { "cert": h.cert, "delegation": d.delegation, "revocations": revokes(&d) },
        }));
    }
    Ok(out)
}

/// The hosted machines of every project `account` is in (its own and its
/// teams'), to nudge when its devices change (M180): they let its devices
/// in, and look again at once, as when a team's roster changes.
pub(crate) fn of_account(app: &App, account: &str) -> Vec<String> {
    owners_for(app, account)
        .and_then(|o| app.db.hosted_of(&o))
        .map(|v| v.into_iter().map(|h| h.cert.device).collect())
        .unwrap_or_default()
}

/// The hosted machines of a team's project, to nudge when its roster
/// changes.
pub(crate) fn of_team(app: &App, team: &str) -> Vec<String> {
    app.db
        .hosted_of(&[Owner::Team(team.to_owned()).as_string()])
        .map(|v| v.into_iter().map(|h| h.cert.device).collect())
        .unwrap_or_default()
}

/// The newest delegation control may enroll under for `project`, owned by
/// `owner`: live now, and naming control's key.
fn live_delegation(db: &Db, key: &str, project: &str, owner: &Owner) -> anyhow::Result<Option<Delegation>> {
    let Some(p) = people(db, owner)? else { return Ok(None) };
    Ok(db.delegations_of(&[owner.as_string()])?.into_iter().find_map(|row| {
        (row.delegation.project == project
            && row.delegation.key == key
            && live(&row.delegation, &revokes(&row), &p.owners()))
        .then_some(row.delegation)
    }))
}

// ---------------------------------------------------------------- the platform's side

/// `HubPort::hosted_enrollment`: record a one-time enrollment for a box
/// the platform is making, under a live delegation naming `key`.
pub(crate) fn ask(db: &Db, key: &str, a: &EnrollAsk) -> anyhow::Result<EnrollGiven> {
    let owner = Owner::parse(&a.owner).ok_or_else(|| anyhow::anyhow!("bad owner {:?}", a.owner))?;
    anyhow::ensure!(!a.workspace.is_empty() && !a.name.is_empty(), "a workspace and a name");
    let Some(d) = live_delegation(db, key, &a.project, &owner)? else { return Ok(EnrollGiven::NoDelegation) };
    let now = now_ms();
    let (credential, via, ticket, expires) = match &a.sandbox {
        Some(s) => (s.clone(), "sandbox", None, now + SANDBOX_MS),
        None => {
            let t = token();
            (t.clone(), "ticket", Some(t), now + TICKET_MS)
        }
    };
    let name: String = a.name.chars().filter(|c| !c.is_control()).take(64).collect();
    match db.add_enrollment(
        &hash(&credential),
        via,
        &a.project,
        &a.owner,
        &a.workspace,
        &name,
        &d.id(),
        now,
        expires,
    )? {
        Recorded::New => {
            info!(project = a.project, workspace = a.workspace, via, "a hosted enrollment recorded");
            Ok(EnrollGiven::Ready { ticket, expires })
        }
        // Asked again for the same box (a lost answer): the same one.
        Recorded::Again(expires) => Ok(EnrollGiven::Ready { ticket: None, expires }),
        Recorded::Taken => Ok(EnrollGiven::Taken),
    }
}

/// `HubPort::hosted_enrolled`: which of these workspaces' boxes enrolled,
/// as which machine, and whose enrollment someone else tried too.
pub(crate) fn enrolled(app: &App, workspaces: &[String]) -> anyhow::Result<Vec<Enrolled>> {
    Ok(app
        .db
        .hosted_enrolled(workspaces)?
        .into_iter()
        .map(|(workspace, machine, taken)| Enrolled { workspace, machine: machine.unwrap_or_default(), taken })
        .collect())
}

/// `HubPort::hosted_gone`: these workspaces' boxes are gone (deleted or
/// made again): their machines leave the project and are hung up on, and
/// enrollments nobody used are dropped.
pub(crate) fn gone(app: &App, workspaces: &[String]) -> anyhow::Result<()> {
    for id in app.db.hosted_gone(workspaces, now_ms())? {
        app.relay.drop_daemon(&id);
    }
    Ok(())
}

// ---------------------------------------------------------------- the box's side

/// What a hosted machine pins: its certificate and delegation, and who's
/// in its project now.
fn enrollment(app: &App, h: &HostedRow) -> anyhow::Result<Enrollment> {
    let row = app.db.delegation(&h.cert.delegation)?.ok_or_else(|| anyhow::anyhow!("its delegation is gone"))?;
    let mut e = Enrollment {
        url: app.cfg.public_url.clone(),
        cert: h.cert.clone(),
        delegation: row.delegation.clone(),
        revocations: revokes(&row),
        team: None,
        roster: None,
        certs: Default::default(),
        trust: None,
        account_certs: vec![],
        account_revocations: vec![],
    };
    match Owner::parse(&h.cert.owner).and_then(|o| people(&app.db, &o).transpose()).transpose()? {
        Some(People::Team { pin, roster, certs }) => {
            e.team = Some(pin);
            e.roster = Some(*roster);
            e.certs = certs;
        }
        Some(People::Account { trust, certs, revocations }) => {
            e.trust = Some(trust);
            e.account_certs = certs;
            e.account_revocations = revocations;
        }
        None => anyhow::bail!("its project's owner is gone"),
    }
    Ok(e)
}

/// `POST /api/hosted/enroll`: a box enrolls, once, with its ticket or its
/// sandbox id and its new keys. Neither credential is ever logged.
pub async fn enroll(
    State(app): State<Arc<App>>,
    ConnectInfo(peer): ConnectInfo<std::net::SocketAddr>,
    headers: HeaderMap,
    Json(b): Json<EnrollRequest>,
) -> R {
    app.limits.check(crate::limit::JOINS, app.limits.client_ip(peer, &headers))?;
    let credential = match (&b.ticket, &b.sandbox) {
        (Some(c), None) | (None, Some(c)) if !c.is_empty() && c.len() <= 256 => c,
        _ => return Err(err(StatusCode::BAD_REQUEST, "enroll with a ticket or a sandbox id: one of them")),
    };
    let credential = hash(credential);
    if now_ms().abs_diff(b.at) > SKEW_MS
        || !arugula_e2e::cert::verify_hex(
            &b.sign,
            enroll_proof_body(&credential, &b.noise, &b.sign, b.at).as_bytes(),
            &b.proof,
        )
    {
        return Err(err(StatusCode::UNAUTHORIZED, "the proof isn't this key's (or this machine's clock is off)"));
    }
    let now = now_ms();
    let (project, owner, workspace, name, delegation) = match app.db.take_enrollment(&credential, now)? {
        Taken::Fresh { project, owner, workspace, name, delegation } => (project, owner, workspace, name, delegation),
        // Fail closed: the box that finds its enrollment taken is deleted
        // by the platform.
        Taken::Used(machine) => {
            if let Some(m) = machine {
                app.relay.drop_daemon(&m);
            }
            return Err(err(StatusCode::CONFLICT, "this enrollment was used already"));
        }
        Taken::Missing => return Err(err(StatusCode::NOT_FOUND, "no such enrollment, or it ran out")),
    };
    // The delegation may have been revoked since the enrollment was made:
    // revoking stops the next enrollment.
    let row = app.db.delegation(&delegation)?;
    let people = Owner::parse(&owner).map(|o| people(&app.db, &o)).transpose()?.flatten();
    let d = match (row, people) {
        (Some(row), Some(p))
            if row.delegation.key == app.hosted_key.public() && live(&row.delegation, &revokes(&row), &p.owners()) =>
        {
            row.delegation
        }
        _ => {
            return Err(err(
                StatusCode::FORBIDDEN,
                "the project's delegation was revoked: no more machines enroll under it",
            ));
        }
    };
    let mut cert = HostedCert::new(&d, &workspace, &name, &b.noise, &b.sign, now);
    cert.check_form().map_err(|e| err(StatusCode::BAD_REQUEST, &e.to_string()))?;
    // A key is one machine's, ever.
    if app.db.daemon_cert(&cert.device)?.is_some()
        || app.db.hosted(&cert.device)?.is_some()
        || app.db.hosted_removed(&cert.device)?
    {
        return Err(err(StatusCode::CONFLICT, "that key is enrolled already: make a new one"));
    }
    app.hosted_key.sign(&mut cert);
    for old in app.db.add_hosted(&cert, now)? {
        app.relay.drop_daemon(&old);
    }
    app.db.enrolled_as(&credential, &cert.device)?;
    info!(project, workspace, device = cert.device, "a hosted machine enrolled");
    let row = HostedRow { cert, urls: vec![], last_seen: None };
    crate::reply(&enrollment(&app, &row)?)
}

/// `GET /api/hosted/trust`: the machine's project as it is now.
pub async fn trust(State(app): State<Arc<App>>, h: HostedAuth) -> R {
    crate::reply(&enrollment(&app, &h.0)?)
}

/// `POST /api/hosted/turns` (M185): the machine's box starts an agent
/// turn. Whose inference key pays is the platform's to say
/// (`PlatformPort::hosted_turn`), for the workspace and machine its
/// certificate names.
pub async fn turn(State(app): State<Arc<App>>, h: HostedAuth, Json(b): Json<Value>) -> R {
    let agent = b["agent"].as_str().unwrap_or_default();
    match app.platform.hosted_turn(&h.0.cert.workspace, &h.0.cert.device, agent).await? {
        arugula_platform::TurnAnswer::Given { turn, pays } => Ok(Json(json!({ "turn": turn, "pays": pays }))),
        arugula_platform::TurnAnswer::Refused { status, reason, error } => {
            let status = StatusCode::from_u16(status).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
            Err(match reason.as_deref() {
                Some("no_key") => crate::refusal(status, "no_key", &error),
                Some("cap") => crate::refusal(status, "cap", &error),
                _ => err(status, &error),
            })
        }
    }
}

/// `POST /api/hosted/turns/{turn}/end` (M185): the turn ended.
pub async fn turn_end(State(app): State<Arc<App>>, h: HostedAuth, Path(turn): Path<String>) -> R {
    let ended = app.platform.hosted_turn_end(&h.0.cert.workspace, &h.0.cert.device, &turn).await?;
    Ok(Json(json!({ "ended": ended })))
}

// ---------------------------------------------------------------- people's side

/// The session's own device that signed `by`, or a refusal.
fn own_device(app: &App, s: &Session, by: &str) -> Result<(), ApiError> {
    match app.db.device(&s.account, by)? {
        Some((c, true)) if matches!(c.kind, arugula_e2e::Kind::Browser | arugula_e2e::Kind::Cli) => Ok(()),
        _ => Err(err(StatusCode::FORBIDDEN, "sign it with one of your own devices (a browser or the CLI)")),
    }
}

fn must_own(app: &App, s: &Session, owner: &Owner) -> Result<(), ApiError> {
    match role(app, owner, &s.account)? {
        Some(TeamRole::Owner) => Ok(()),
        _ => {
            Err(err(StatusCode::FORBIDDEN, "only the project's owners delegate it, revoke it, or remove its machines"))
        }
    }
}

/// `GET /api/hosted/delegations`: control's hosted key, and the
/// delegations of the projects the session is in.
pub async fn delegations(State(app): State<Arc<App>>, s: Session) -> R {
    let rows = app.db.delegations_of(&owners_for(&app, &s.account)?)?;
    let list: Vec<Value> = rows
        .iter()
        .map(|r| json!({ "id": r.delegation.id(), "delegation": r.delegation, "revoke": r.revoke, "by_account": r.by_account }))
        .collect();
    Ok(Json(json!({ "key": app.hosted_key.public(), "delegations": list })))
}

/// `POST /api/hosted/delegations`: an owner's device delegates a project.
pub async fn delegate(State(app): State<Arc<App>>, s: Session, Json(b): Json<DelegationPost>) -> R {
    let d = b.delegation;
    let owner =
        d.owner().filter(|_| d.well_formed()).ok_or_else(|| err(StatusCode::BAD_REQUEST, "not a delegation"))?;
    if d.key != app.hosted_key.public() {
        return Err(err(StatusCode::CONFLICT, "that isn't control's hosted key: sign again with /control.json's"));
    }
    if now_ms().abs_diff(d.at) > SKEW_MS {
        return Err(err(StatusCode::BAD_REQUEST, "its time is too far from control's: check this device's clock"));
    }
    must_own(&app, &s, &owner)?;
    own_device(&app, &s, &d.by)?;
    let p = people(&app.db, &owner)?.ok_or_else(|| err(StatusCode::NOT_FOUND, "no such project owner"))?;
    if !d.signed_for(&p.owners()) {
        return Err(err(StatusCode::FORBIDDEN, "it isn't signed by a device of one of the project's owners"));
    }
    app.db.add_delegation(&d, &s.account, now_ms())?;
    info!(project = d.project, owner = d.owner, "a project delegated to control");
    Ok(Json(json!({ "id": d.id() })))
}

/// `POST /api/hosted/delegations/revoke`: an owner's device takes a
/// delegation back. Nothing more enrolls under it; its machines stay
/// until they're removed.
pub async fn revoke(State(app): State<Arc<App>>, s: Session, Json(b): Json<RevokePost>) -> R {
    let r = b.revoke;
    let row = app.db.delegation(&r.delegation)?.ok_or_else(|| err(StatusCode::NOT_FOUND, "no such delegation"))?;
    let owner = row.delegation.owner().ok_or_else(|| err(StatusCode::NOT_FOUND, "no such delegation"))?;
    must_own(&app, &s, &owner)?;
    own_device(&app, &s, &r.by)?;
    let p = people(&app.db, &owner)?.ok_or_else(|| err(StatusCode::NOT_FOUND, "no such project owner"))?;
    if !r.revokes(&row.delegation, &p.owners()) {
        return Err(err(StatusCode::FORBIDDEN, "it isn't signed by a device of one of the project's owners"));
    }
    if now_ms().abs_diff(r.at) > SKEW_MS {
        return Err(err(StatusCode::BAD_REQUEST, "its time is too far from control's: check this device's clock"));
    }
    app.db.revoke_delegation(&r, now_ms())?;
    info!(project = row.delegation.project, "a project delegation revoked");
    // Its machines pick the revocation up now.
    let ids: Vec<String> = app.db.hosted_of(&[owner.as_string()])?.into_iter().map(|h| h.cert.device).collect();
    app.relay.nudge(&ids);
    Ok(Json(json!({})))
}

/// `DELETE /api/hosted/machines/{id}`: a project owner removes a hosted
/// machine, as a device is removed: it's hung up on, and its key signs
/// nothing here again.
pub async fn remove(State(app): State<Arc<App>>, s: Session, Path(id): Path<String>) -> R {
    let h = app.db.hosted(&id)?.ok_or_else(|| err(StatusCode::NOT_FOUND, "no such machine"))?;
    let owner = Owner::parse(&h.cert.owner).ok_or_else(|| err(StatusCode::NOT_FOUND, "no such machine"))?;
    must_own(&app, &s, &owner)?;
    app.db.remove_hosted(&id, now_ms())?;
    app.relay.drop_daemon(&id);
    Ok(Json(json!({})))
}

// ---------------------------------------------------------------- by hand

#[derive(clap::Parser)]
#[command(about = "Record a one-time hosted enrollment, as the platform does when it makes a box (M178)")]
struct Args {
    /// The hub's database.
    #[arg(long)]
    db: std::path::PathBuf,
    #[arg(long)]
    project: String,
    /// `team:<id>` or `account:<id>`.
    #[arg(long)]
    owner: String,
    /// The hosted workspace (`hw_…`).
    #[arg(long)]
    workspace: String,
    #[arg(long, default_value = "hosted")]
    name: String,
    /// Enroll by this sandbox id rather than a ticket.
    #[arg(long)]
    sandbox: Option<String>,
}

/// `arugula-control hosted-enrollment`: what `HubPort::hosted_enrollment`
/// does, from a shell, for testnet and an operator by hand (the platform
/// calls the hub for it). Prints the answer as JSON; a ticket is the one
/// thing printed that's secret.
pub fn run(args: impl Iterator<Item = String>) -> anyhow::Result<()> {
    use clap::Parser as _;
    let a = Args::parse_from(std::iter::once("arugula-control hosted-enrollment".to_owned()).chain(args));
    let db = Db::open(&a.db)?;
    let key = load_key(&db)?;
    let ask =
        EnrollAsk { project: a.project, owner: a.owner, workspace: a.workspace, name: a.name, sandbox: a.sandbox };
    println!("{}", serde_json::to_string(&self::ask(&db, &key.public(), &ask)?)?);
    Ok(())
}
