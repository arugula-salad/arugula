//! Environments and deploys (M217, platform step 7, native Arugula #13): a
//! project's environments, the deploys planned to them, and each
//! environment's event log. Who may plan or ask is decided by the routes;
//! the signed approval is M218's. This is storage, and the rules that
//! hold whoever calls it. The tables, with notes on their columns, are
//! made by `0017_deploys`.
//!
//! - **The log is the record.** Every change to a deploy is an event in its
//!   environment's log, written in the same transaction as the deploy's
//!   `state`. Events are never updated or deleted in place: triggers
//!   refuse an update, and a delete while the event's environment exists.
//!   Only a whole log goes, after its environment, with its project.
//! - **The log is a hash chain per environment.** Each event's `hash` is
//!   SHA-256 over its canonical lines ([`Event::canonical`]), the first of
//!   which is the hash of the event before it. Anyone with the rows can
//!   check it ([`verify_chain`]): a changed, removed or reordered event
//!   breaks the link at it. Events cut off the end of the log leave the
//!   deploys' rows saying more than their events do ([`verify_rows`]).
//!   The chain is per environment, not per deploy or per project, because
//!   that's what the stale rule reads: whether anything applied to this
//!   environment after a plan. It's tamper-evident, not write-once
//!   (aru-103): whoever runs the database could rewrite the log and the
//!   rows together, and only an earlier copy shows it.
//! - **The stale rule.** A deploy planned before another deploy applied to
//!   its environment can't be approved or started ([`Db::may_approve`]).
//!   "Applied" is any apply event of another deploy (`started`,
//!   `succeeded` or `failed`) with a higher `seq` than the deploy's own
//!   `planned` event, read from the log (never the row's copy): an apply
//!   that failed may still have changed the environment. The same events turn the environment's other open
//!   deploys (`planned`, `pending`, `approved`) `stale` at once, each with a
//!   `staled` event; the seq check holds even if a row's state didn't.
//! - **Who records what** ([`EventKind::allows`]): a person or an agent
//!   plans, asks, and cancels; only the run that started an apply finishes
//!   it; only the platform expires one.
//! - **Approvals.** An approval or a deny is recorded only from a
//!   [`VerifiedApproval`] ([`Db::record_approval`]), which only M218's
//!   verifier makes, after it checks the owner device's signature. It names
//!   the deploy, its plan and the hash of its `planned` event, and goes in
//!   the event whole, so anyone can check the signature from the log.
//!   **Agents never approve:** no [`Actor`] can record one.
//! - **States** are S37's (q4: `pending`, `approved`, `denied`,
//!   `canceled`, `expired`, `running`, `succeeded`, `failed`), with
//!   `planned` before them (a plan nobody has asked to apply yet) and
//!   `stale` beside them.
//! - **A deleted account** ([`erase_account`]): the environments, deploys
//!   and logs of projects that went with it go. Elsewhere its deploys stop
//!   naming it, and its events keep their rows as written, account id
//!   included, because rewriting one would break the chain: they're shown
//!   as "a deleted account" once the id names no one.

use anyhow::{Context as _, Result, bail};
use base64::Engine as _;
use serde_json::json;
use sha2::{Digest as _, Sha256};

use super::{
    Db,
    sql::{Optional as _, Param, Row, Sql, params},
    taken,
};
use crate::Deleted;

/// A new environment's id: `env_` and 32 hex characters.
fn new_environment_id() -> String {
    format!("env_{}", hex::encode(arugula_e2e::random::<16>()))
}

/// A new deploy's id: `dep_` and 32 hex characters.
fn new_deploy_id() -> String {
    format!("dep_{}", hex::encode(arugula_e2e::random::<16>()))
}

/// The drivers an environment may name (M219-M222, and planter's four,
/// M233), besides the fake one where it's allowed
/// (`crate::drivers::names`). arugula-release (M221) only in Arugula's own
/// project (`crate::drivers::Driver::allowed_in`).
pub const DRIVERS: &[&str] = &[
    "chant-fly",
    "apps-platform",
    "arugula-release",
    // planter's (M233).
    "planter-prod",
    "planter-go-live",
    "planter-ship",
    "planter-promote",
];

/// How long a project's shipped artifacts are kept at most (M233).
pub const ARTIFACT_DAYS: u64 = 30;

/// A project's kept artifacts would pass its quota (M233).
#[derive(Debug)]
pub struct OverQuota {
    pub kept: u64,
    pub quota: u64,
}

impl std::fmt::Display for OverQuota {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "the project keeps {} MiB of shipped sources waiting for their deploys, and at most {} MiB",
            self.kept >> 20,
            self.quota >> 20
        )
    }
}

impl std::error::Error for OverQuota {}

/// Drop `environment`'s project's artifacts no deploy that's waiting or
/// running names (M233): a deploy that ended, was denied, expired or went
/// stale no longer needs its source. In the transaction that moved it.
fn prune_artifacts(tx: &impl Sql, environment: &str) -> Result<()> {
    tx.execute(
        "DELETE FROM deploy_artifacts
         WHERE project_id = (SELECT project_id FROM environments WHERE id = ?1)
           AND digest NOT IN (
             SELECT d.source_ref FROM deploys d JOIN environments e ON e.id = d.environment_id
             WHERE e.project_id = deploy_artifacts.project_id AND d.source_kind = 'artifact'
               AND d.state IN ('planned', 'pending', 'approved', 'running'))",
        params![environment],
    )?;
    Ok(())
}

/// The first event's `prev_hash`.
pub const GENESIS: &str = "0000000000000000000000000000000000000000000000000000000000000000";

/// The most an event's detail holds, in bytes.
pub const MAX_DETAIL: usize = 4096;

/// The first line of an event's canonical form: its domain separator.
pub const EVENT_V1: &str = "arugula deploy event v1";

/// An environment's name: 1-40 of lowercase letters, digits and `-`,
/// starting with a letter or digit (`production`, `staging-eu`).
pub fn valid_environment_name(name: &str) -> bool {
    (1..=40).contains(&name.len())
        && name.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
        && !name.starts_with('-')
}

/// A secret's name, as an environment variable: 1-64 of `A-Z`, `0-9` and
/// `_`, not starting with a digit, and not one the driver's shell or its
/// environment uses (M219: `arugula_e2e::deploy::secret_name_ok`).
pub fn valid_secret_name(name: &str) -> bool {
    arugula_e2e::deploy::secret_name_ok(name)
}

/// An id or a reference that goes on a line of its own: printable, with
/// no space or newline.
fn valid_token(s: &str) -> bool {
    (1..=200).contains(&s.len()) && s.bytes().all(|b| b.is_ascii_graphic())
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Environment {
    pub id: String,
    pub project: String,
    pub name: String,
    pub driver: String,
    /// The names of the secrets a driver run gets, sorted. Their values
    /// are sealed per project (M209), never here.
    pub secret_names: Vec<String>,
    /// Its driver's settings (M220): what it needs that isn't secret, as
    /// `arugula_e2e::drivers::settings` checks them. The plan names them.
    pub settings: arugula_e2e::drivers::Settings,
    pub created_at: u64,
    pub updated_at: u64,
}

/// What a deploy deploys.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SourceKind {
    /// A commit, by its sha.
    Commit,
    /// A built artifact, by its digest.
    Artifact,
}

impl SourceKind {
    pub fn as_str(self) -> &'static str {
        match self {
            SourceKind::Commit => "commit",
            SourceKind::Artifact => "artifact",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "commit" => SourceKind::Commit,
            "artifact" => SourceKind::Artifact,
            _ => return None,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Source {
    pub kind: SourceKind,
    /// The commit's sha or the artifact's digest.
    pub reference: String,
    /// Where it is: a repo (`owner/name`), a registry, a bucket.
    pub place: String,
}

/// A deploy's state: S37's lifecycle (q4), with `planned` before it and
/// `stale` beside it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum State {
    /// Planned; nobody has asked to apply it yet.
    Planned,
    /// Approval asked for.
    Pending,
    Approved,
    Denied,
    Canceled,
    Expired,
    /// Being applied, by its run.
    Running,
    Succeeded,
    Failed,
    /// Something else applied to its environment after it was planned.
    Stale,
}

impl State {
    pub fn as_str(self) -> &'static str {
        match self {
            State::Planned => "planned",
            State::Pending => "pending",
            State::Approved => "approved",
            State::Denied => "denied",
            State::Canceled => "canceled",
            State::Expired => "expired",
            State::Running => "running",
            State::Succeeded => "succeeded",
            State::Failed => "failed",
            State::Stale => "stale",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "planned" => State::Planned,
            "pending" => State::Pending,
            "approved" => State::Approved,
            "denied" => State::Denied,
            "canceled" => State::Canceled,
            "expired" => State::Expired,
            "running" => State::Running,
            "succeeded" => State::Succeeded,
            "failed" => State::Failed,
            "stale" => State::Stale,
            _ => return None,
        })
    }

    /// Not applied yet, and still may be: what another apply makes stale.
    pub fn open(self) -> bool {
        matches!(self, State::Planned | State::Pending | State::Approved)
    }
}

/// What happened to a deploy: an event in its environment's log.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EventKind {
    Planned,
    /// Approval asked for.
    Requested,
    Approved,
    Denied,
    Canceled,
    Expired,
    /// Its run began applying it.
    Started,
    Succeeded,
    Failed,
    /// Another deploy applied to its environment after it was planned.
    Staled,
}

impl EventKind {
    pub fn as_str(self) -> &'static str {
        match self {
            EventKind::Planned => "planned",
            EventKind::Requested => "requested",
            EventKind::Approved => "approved",
            EventKind::Denied => "denied",
            EventKind::Canceled => "canceled",
            EventKind::Expired => "expired",
            EventKind::Started => "started",
            EventKind::Succeeded => "succeeded",
            EventKind::Failed => "failed",
            EventKind::Staled => "staled",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "planned" => EventKind::Planned,
            "requested" => EventKind::Requested,
            "approved" => EventKind::Approved,
            "denied" => EventKind::Denied,
            "canceled" => EventKind::Canceled,
            "expired" => EventKind::Expired,
            "started" => EventKind::Started,
            "succeeded" => EventKind::Succeeded,
            "failed" => EventKind::Failed,
            "staled" => EventKind::Staled,
            _ => return None,
        })
    }

    /// The state it leaves its deploy in.
    pub fn to(self) -> State {
        match self {
            EventKind::Planned => State::Planned,
            EventKind::Requested => State::Pending,
            EventKind::Approved => State::Approved,
            EventKind::Denied => State::Denied,
            EventKind::Canceled => State::Canceled,
            EventKind::Expired => State::Expired,
            EventKind::Started => State::Running,
            EventKind::Succeeded => State::Succeeded,
            EventKind::Failed => State::Failed,
            EventKind::Staled => State::Stale,
        }
    }

    /// The states it may move a deploy from.
    pub fn from(self) -> &'static [State] {
        match self {
            EventKind::Planned => &[],
            EventKind::Requested => &[State::Planned],
            EventKind::Approved | EventKind::Denied => &[State::Pending],
            EventKind::Canceled => &[State::Planned, State::Pending, State::Approved],
            EventKind::Expired => &[State::Pending, State::Approved],
            EventKind::Started => &[State::Approved],
            EventKind::Succeeded | EventKind::Failed => &[State::Running],
            EventKind::Staled => &[State::Planned, State::Pending, State::Approved],
        }
    }

    /// An apply to the environment: what makes the environment's other
    /// deploys stale.
    pub fn applies(self) -> bool {
        matches!(self, EventKind::Started | EventKind::Succeeded | EventKind::Failed)
    }

    /// Whether `by` may record it ([`Db::record_deploy`]): a person or an
    /// agent asks and cancels; only a run starts and finishes its apply
    /// (the run that started it: [`Refusal::NotItsRun`]); only the platform
    /// expires one. No actor approves or denies: that's
    /// [`Db::record_approval`]'s, from a [`VerifiedApproval`], and an agent
    /// asking is told [`Refusal::Agent`]. Planning is
    /// [`Db::plan_deploy`]'s and staling the platform's own, never asked
    /// for.
    ///
    /// The platform can't fail a running deploy: it would clear the busy
    /// check while the run may still be applying. TODO(M210/M223): let it,
    /// in the same transaction that reads the run as finished (`runs`), for
    /// a run whose lease ran out.
    pub fn allows(self, by: &Actor) -> std::result::Result<(), Refusal> {
        let ok = match self {
            EventKind::Requested => matches!(by, Actor::Person(_) | Actor::Agent { .. }),
            EventKind::Canceled => matches!(by, Actor::Person(_) | Actor::Agent { .. } | Actor::Platform),
            EventKind::Approved | EventKind::Denied => {
                return Err(match by {
                    Actor::Agent { .. } => Refusal::Agent,
                    _ => Refusal::Unverified,
                });
            }
            EventKind::Expired => matches!(by, Actor::Platform),
            EventKind::Started | EventKind::Succeeded | EventKind::Failed => matches!(by, Actor::Run(_)),
            EventKind::Planned | EventKind::Staled => false,
        };
        if ok { Ok(()) } else { Err(Refusal::Actor) }
    }
}

/// Who an event is by.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Actor {
    /// A person, by account.
    Person(String),
    /// A person's agent: its owner's account, and its machine.
    Agent { account: String, machine: String },
    /// A run (M210), by id: a driver applying a deploy.
    Run(String),
    /// The platform itself: staling, expiring, failing a run that's gone.
    Platform,
}

impl Actor {
    pub fn person(account: &str) -> Self {
        Actor::Person(account.to_owned())
    }

    pub fn agent(account: &str, machine: &str) -> Self {
        Actor::Agent { account: account.to_owned(), machine: machine.to_owned() }
    }

    pub fn run(id: &str) -> Self {
        Actor::Run(id.to_owned())
    }

    /// As the `actor` column and the canonical line hold it:
    /// `person <account>`, `agent <account> <machine>`, `run <id>` or
    /// `platform`.
    pub fn encode(&self) -> String {
        match self {
            Actor::Person(a) => format!("person {a}"),
            Actor::Agent { account, machine } => format!("agent {account} {machine}"),
            Actor::Run(r) => format!("run {r}"),
            Actor::Platform => "platform".to_owned(),
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        let mut words = s.split(' ');
        let actor = match (words.next()?, words.next(), words.next()) {
            ("person", Some(a), None) => Actor::person(a),
            ("agent", Some(a), Some(m)) => Actor::agent(a, m),
            ("run", Some(r), None) => Actor::run(r),
            ("platform", None, None) => Actor::Platform,
            _ => return None,
        };
        (words.next().is_none() && actor.well_formed()).then_some(actor)
    }

    /// Each id it holds is one printable word.
    fn well_formed(&self) -> bool {
        match self {
            Actor::Person(a) | Actor::Run(a) => valid_token(a),
            Actor::Agent { account, machine } => valid_token(account) && valid_token(machine),
            Actor::Platform => true,
        }
    }

    /// The account it acts for: a person's own, or an agent's owner's.
    pub fn account(&self) -> Option<&str> {
        match self {
            Actor::Person(a) | Actor::Agent { account: a, .. } => Some(a),
            Actor::Run(_) | Actor::Platform => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Deploy {
    pub id: String,
    pub environment: String,
    pub source: Source,
    /// SHA-256 of the plan the driver made, hex.
    pub plan_digest: String,
    pub state: State,
    /// The `seq` of its `planned` event in its environment's log: a copy,
    /// for ordering. The stale rule reads the event itself.
    pub planned_seq: u64,
    /// The run that applied it, once it started.
    pub run: Option<String>,
    /// The account that planned it: a person's, or an agent's owner's.
    /// Empty once that account is deleted.
    pub planned_by: String,
    /// The machine whose agent planned it.
    pub planned_by_agent: Option<String>,
    pub created_at: u64,
    pub updated_at: u64,
}

/// One event in an environment's log, as stored. `actor` and `detail` are
/// the exact text that was hashed: [`Actor::parse`] and
/// [`Event::detail_json`] read them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Event {
    pub environment: String,
    /// From 1, in its environment.
    pub seq: u64,
    pub deploy: String,
    /// As stored; [`EventKind::parse`] reads it.
    pub kind: String,
    pub actor: String,
    pub at: u64,
    /// A small JSON object:
    /// - `planned`: `source` (`kind`, `ref`, `where`) and `plan` (the
    ///   plan's digest), and for a roll back (M220) `roll_back` (the deploy
    ///   it goes back to, and what that ran);
    /// - `started`: `run`;
    /// - `staled`: `after` (the deploy that applied) and `seq` (its event);
    /// - `approved`, `denied`: the approval ([`VerifiedApproval::detail`]);
    /// - the others: what their caller gives.
    ///
    /// At most [`MAX_DETAIL`] bytes.
    pub detail: String,
    /// The hash of the event before it in the environment ([`GENESIS`] for
    /// the first).
    pub prev_hash: String,
    /// SHA-256 of [`Event::canonical`], hex.
    pub hash: String,
}

impl Event {
    /// The bytes its hash is over: fixed lines in S37's style, the first a
    /// domain separator, `detail` in base64 (standard, padded) so it holds
    /// no newline. `None` when another field holds a newline, which no
    /// event written here does: such a row can't be read one way only.
    ///
    /// ```text
    /// arugula deploy event v1
    /// prev <hex>
    /// environment <id>
    /// seq <n>
    /// deploy <id>
    /// kind <kind>
    /// actor <actor>
    /// at <ms>
    /// detail <base64>
    /// ```
    pub fn canonical(&self) -> Option<String> {
        let fields = [&self.prev_hash, &self.environment, &self.deploy, &self.kind, &self.actor];
        if fields.iter().any(|f| f.contains(['\n', '\r'])) {
            return None;
        }
        Some(format!(
            "{EVENT_V1}\nprev {}\nenvironment {}\nseq {}\ndeploy {}\nkind {}\nactor {}\nat {}\ndetail {}\n",
            self.prev_hash,
            self.environment,
            self.seq,
            self.deploy,
            self.kind,
            self.actor,
            self.at,
            base64::engine::general_purpose::STANDARD.encode(self.detail.as_bytes()),
        ))
    }

    /// What its hash should be: SHA-256 of [`Event::canonical`], hex.
    pub fn digest(&self) -> Option<String> {
        self.canonical().map(|c| hex::encode(Sha256::digest(c.as_bytes())))
    }

    pub fn detail_json(&self) -> Result<serde_json::Value> {
        Ok(serde_json::from_str(&self.detail)?)
    }
}

/// Where a log stops verifying.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BadLink {
    /// The seq the bad event should have had (its place in the log, from 1).
    pub seq: u64,
    pub why: Broken,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Broken {
    /// Its seq isn't the next one: an event before it was removed, or the
    /// log was reordered.
    Seq { found: u64 },
    /// Another environment's event, in this one's log.
    Environment,
    /// Its `prev_hash` isn't the hash of the event before it.
    Prev,
    /// Its hash isn't SHA-256 over what it holds: it was changed.
    Hash,
    /// A field holds a newline, so it has no one canonical form.
    Unreadable,
    /// A deploy's row disagrees with its own events, or an event names a
    /// deploy with no row: what events cut off the end of the log, or a
    /// row changed behind them, leave.
    Deploy { deploy: String, field: DeployField },
}

/// What a deploy's row says that its events don't ([`Broken::Deploy`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeployField {
    /// It has no `planned` event, or its `planned_seq` isn't that event's.
    Planned,
    /// Its state isn't what its last event left it in.
    State,
    /// Its run isn't the one its `started` event names.
    Run,
    /// An event names it, and it has no row.
    Missing,
}

/// Check an environment's log, given in seq order from its first event: each
/// seq the next, each `prev_hash` the hash before it, each hash right. How
/// many events verified, or the first bad link. Pure, so anyone holding the
/// rows (an export) can run it.
pub fn verify_chain(environment: &str, events: &[Event]) -> std::result::Result<u64, BadLink> {
    let mut prev = GENESIS.to_owned();
    for (i, e) in events.iter().enumerate() {
        let seq = i as u64 + 1;
        let bad = |why| Err(BadLink { seq, why });
        if e.seq != seq {
            return bad(Broken::Seq { found: e.seq });
        }
        if e.environment != environment {
            return bad(Broken::Environment);
        }
        if e.prev_hash != prev {
            return bad(Broken::Prev);
        }
        match e.digest() {
            None => return bad(Broken::Unreadable),
            Some(h) if h != e.hash => return bad(Broken::Hash),
            Some(_) => {}
        }
        prev = e.hash.clone();
    }
    Ok(events.len() as u64)
}

/// Check an environment's deploys against its log (given in seq order, as
/// [`verify_chain`] takes it): each deploy has its `planned` event at its
/// `planned_seq`, is in the state its last event left it in, with the run
/// its `started` event names, and every event's deploy has a row. The
/// first bad link (the lowest seq where they disagree), if any. Pure, as
/// [`verify_chain`] is.
pub fn verify_rows(events: &[Event], deploys: &[Deploy]) -> std::result::Result<(), BadLink> {
    let mut bad: Vec<BadLink> = Vec::new();
    let link = |seq, deploy: &str, field| BadLink { seq, why: Broken::Deploy { deploy: deploy.to_owned(), field } };
    for e in events {
        if !deploys.iter().any(|d| d.id == e.deploy) {
            bad.push(link(e.seq, &e.deploy, DeployField::Missing));
        }
    }
    for d in deploys {
        let own: Vec<&Event> = events.iter().filter(|e| e.deploy == d.id).collect();
        let planned = own.iter().find(|e| e.kind == EventKind::Planned.as_str());
        let Some(planned) = planned.filter(|p| p.seq == d.planned_seq) else {
            bad.push(link(d.planned_seq, &d.id, DeployField::Planned));
            continue;
        };
        let last = own.last().unwrap_or(planned);
        if EventKind::parse(&last.kind).map(EventKind::to) != Some(d.state) {
            bad.push(link(last.seq, &d.id, DeployField::State));
        }
        let started = own.iter().find(|e| e.kind == EventKind::Started.as_str());
        // A `started` by anything but a run names no run a row could have.
        let run = started.map(|e| match Actor::parse(&e.actor) {
            Some(Actor::Run(r)) => r,
            _ => String::new(),
        });
        if run != d.run {
            bad.push(link(started.map_or(last.seq, |e| e.seq), &d.id, DeployField::Run));
        }
    }
    match bad.into_iter().min_by_key(|b| b.seq) {
        Some(b) => Err(b),
        None => Ok(()),
    }
}

/// Why a change to a deploy wasn't made.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Refusal {
    /// There's no such deploy.
    Gone,
    /// It's in a state this change doesn't move it from.
    State(State),
    /// The stale rule: another deploy applied to its environment after it
    /// was planned, at `seq` in the log.
    Stale { seq: u64, deploy: String },
    /// An agent tried to approve or deny. Agents never do.
    Agent,
    /// An approval or a deny asked for without a [`VerifiedApproval`]
    /// ([`Db::record_approval`]).
    Unverified,
    /// A verified approval that isn't for this deploy as it is: another
    /// plan, or another `planned` event (another log).
    Mismatch(Mismatch),
    /// Not this actor's to record ([`EventKind::allows`]).
    Actor,
    /// Another deploy of its environment is being applied.
    Busy { deploy: String },
    /// A run reporting on an apply that isn't its own, or starting a
    /// deploy that isn't the one it was made for (M219).
    NotItsRun,
    /// A start after the approval's `expires` (M219).
    ApprovalExpired,
    /// A start with an approval a start used up already (M219).
    ApprovalConsumed,
}

impl std::fmt::Display for Refusal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Refusal::Gone => f.write_str("there's no such deploy"),
            Refusal::State(s) => write!(f, "the deploy is {}", s.as_str()),
            Refusal::Stale { deploy, .. } => {
                write!(f, "the deploy is stale: {deploy} applied to its environment after it was planned")
            }
            Refusal::Agent => f.write_str("an agent can't approve or deny a deploy"),
            Refusal::Unverified => f.write_str("a deploy is approved or denied only by an owner device's signature"),
            Refusal::Mismatch(Mismatch::Plan) => f.write_str("the approval is for another plan"),
            Refusal::Mismatch(Mismatch::Planned) => f.write_str("the approval is for another log"),
            Refusal::Actor => f.write_str("that's not this actor's to record"),
            Refusal::Busy { deploy } => write!(f, "{deploy} is being applied to the environment"),
            Refusal::NotItsRun => f.write_str("that run isn't applying this deploy"),
            Refusal::ApprovalExpired => f.write_str("the deploy's approval expired before its apply started"),
            Refusal::ApprovalConsumed => f.write_str("the deploy's approval was used up by a start already"),
        }
    }
}

/// What a verified approval doesn't match ([`Refusal::Mismatch`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mismatch {
    /// Its plan digest isn't the deploy's.
    Plan,
    /// Its `planned` event's hash isn't the one in the log.
    Planned,
}

/// An owner device's decision on a deploy.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Decision {
    /// Approve it, once.
    Once,
    Deny,
}

impl Decision {
    pub fn as_str(self) -> &'static str {
        match self {
            Decision::Once => "once",
            Decision::Deny => "deny",
        }
    }
}

/// An approval or a deny whose signature has been checked: the only way
/// one is recorded ([`Db::record_approval`]). Its fields are private, so
/// storage can't be handed one that wasn't verified: outside tests, it's
/// made only by M218's verifier, which checks the owner device's signature
/// over the deploy's operation before it makes one.
///
/// It names the deploy, its plan's digest and the hash of its `planned`
/// event, so the device signs the log up to the plan; recording it checks
/// them against the deploy's row and its log. The whole of it goes in the
/// event's detail ([`VerifiedApproval::detail`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerifiedApproval {
    deploy: String,
    plan_digest: String,
    planned_hash: String,
    /// The operation's hash the device signed over (M218's `arugula deploy
    /// op v1`), hex.
    op: String,
    /// The approver: a project owner's account.
    account: String,
    /// The approving device.
    device: String,
    decision: Decision,
    /// By when the apply must start, ms.
    expires: u64,
    /// The device's signature, hex.
    signature: String,
    /// The passkey assertion the hub checked over the approval's bytes
    /// (M218): `None` only for tests' unchecked ones.
    passkey: Option<PasskeyProof>,
}

/// A passkey's assertion, as the hub checked it (M218): each base64url.
/// With the passkey's public key (the hub's), anyone can check it again
/// from the log.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PasskeyProof {
    pub id: String,
    pub client_data_json: String,
    pub authenticator_data: String,
    pub signature: String,
}

impl VerifiedApproval {
    /// M218's verifier (`arugula_platform_deploys::deploys::verify`) makes
    /// one this way, and nothing else does: once the approving device is one
    /// of the account's that counts (a browser's, chained to its root, not
    /// revoked), its signature checks out over `approval`'s bytes, and `op`
    /// (the deploy's operation as the log has it now) hashes to what it
    /// signed. `pub` only because the verifier is a crate of its own
    /// (#314): no other caller may make one.
    pub fn from_checked(
        approval: &arugula_e2e::deploy::Approval,
        op: &arugula_e2e::deploy::Operation,
        passkey: PasskeyProof,
    ) -> Self {
        Self {
            deploy: approval.deploy.clone(),
            plan_digest: op.plan.clone(),
            planned_hash: op.planned.clone(),
            op: approval.op.clone(),
            account: approval.account.clone(),
            device: approval.by.clone(),
            decision: match approval.decision {
                arugula_e2e::deploy::Decision::Once => Decision::Once,
                arugula_e2e::deploy::Decision::Deny => Decision::Deny,
            },
            expires: approval.expires,
            signature: approval.sig.clone(),
            passkey: Some(passkey),
        }
    }

    pub fn passkey(&self) -> Option<&PasskeyProof> {
        self.passkey.as_ref()
    }

    pub fn op(&self) -> &str {
        &self.op
    }

    pub fn signature(&self) -> &str {
        &self.signature
    }

    /// One made without checking anything: tests only. M218's verifier is
    /// the only other constructor.
    #[cfg(any(test, feature = "testing"))]
    #[allow(clippy::too_many_arguments)]
    pub fn unchecked_for_tests(
        deploy: &str,
        plan_digest: &str,
        planned_hash: &str,
        op: &str,
        account: &str,
        device: &str,
        decision: Decision,
        expires: u64,
        signature: &str,
    ) -> Self {
        Self {
            deploy: deploy.into(),
            plan_digest: plan_digest.into(),
            planned_hash: planned_hash.into(),
            op: op.into(),
            account: account.into(),
            device: device.into(),
            decision,
            expires,
            signature: signature.into(),
            passkey: None,
        }
    }

    pub fn deploy(&self) -> &str {
        &self.deploy
    }

    pub fn account(&self) -> &str {
        &self.account
    }

    pub fn device(&self) -> &str {
        &self.device
    }

    pub fn decision(&self) -> Decision {
        self.decision
    }

    pub fn expires(&self) -> u64 {
        self.expires
    }

    /// The event it's recorded as.
    pub fn kind(&self) -> EventKind {
        match self.decision {
            Decision::Once => EventKind::Approved,
            Decision::Deny => EventKind::Denied,
        }
    }

    /// What its event's detail holds: enough, with the event's deploy and
    /// actor, to check the signature from the log.
    pub fn detail(&self) -> serde_json::Value {
        let mut d = json!({
            "decision": self.decision.as_str(),
            "plan": self.plan_digest,
            "planned": self.planned_hash,
            "op": self.op,
            "device": self.device,
            "expires": self.expires,
            "signature": self.signature,
        });
        // M218: the passkey's assertion the hub checked over the
        // approval's bytes.
        if let Some(p) = &self.passkey {
            d["passkey"] = json!({
                "id": p.id,
                "client_data_json": p.client_data_json,
                "authenticator_data": p.authenticator_data,
                "signature": p.signature,
            });
        }
        d
    }
}

/// A change made: the deploy as it is now, and the events written (its
/// own first, then any deploys it made stale).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Recorded {
    pub deploy: Deploy,
    pub events: Vec<Event>,
}

pub struct NewDeploy<'a> {
    pub environment: &'a str,
    pub source: &'a Source,
    pub plan_digest: &'a str,
    /// A roll back (M220): the earlier deploy it goes back to and what that
    /// ran, kept in the `planned` event (so the approval, over that event's
    /// hash, covers it too).
    pub roll_back: Option<&'a arugula_e2e::drivers::RollBack>,
}

const ENV_COLS: &str = "id, project_id, name, driver, secret_names, created_at, updated_at, settings";
pub(super) const DEPLOY_COLS: &str = "id, environment_id, source_kind, source_ref, source_where, plan_digest, state, planned_seq, \
                           run_id, planned_by, planned_by_agent, created_at, updated_at";
pub(super) const EVENT_COLS: &str = "environment_id, seq, deploy_id, kind, actor, at, detail, prev_hash, hash";

fn environment_row(r: &Row) -> Result<Environment> {
    Ok(Environment {
        id: r.get(0)?,
        project: r.get(1)?,
        name: r.get(2)?,
        driver: r.get(3)?,
        secret_names: serde_json::from_str(&r.get::<String>(4)?)?,
        created_at: r.get(5)?,
        updated_at: r.get(6)?,
        settings: serde_json::from_str(&r.get::<String>(7)?)?,
    })
}

pub(super) fn deploy_row(r: &Row) -> Result<Deploy> {
    let kind: String = r.get(2)?;
    let state: String = r.get(6)?;
    Ok(Deploy {
        id: r.get(0)?,
        environment: r.get(1)?,
        source: Source {
            kind: SourceKind::parse(&kind).with_context(|| format!("a deploy of a {kind:?}"))?,
            reference: r.get(3)?,
            place: r.get(4)?,
        },
        plan_digest: r.get(5)?,
        state: State::parse(&state).with_context(|| format!("a deploy in state {state:?}"))?,
        planned_seq: r.get(7)?,
        run: r.get(8)?,
        planned_by: r.get(9)?,
        planned_by_agent: r.get(10)?,
        created_at: r.get(11)?,
        updated_at: r.get(12)?,
    })
}

pub(super) fn event_row(r: &Row) -> Result<Event> {
    Ok(Event {
        environment: r.get(0)?,
        seq: r.get(1)?,
        deploy: r.get(2)?,
        kind: r.get(3)?,
        actor: r.get(4)?,
        at: r.get(5)?,
        detail: r.get(6)?,
        prev_hash: r.get(7)?,
        hash: r.get(8)?,
    })
}

fn sorted_names(names: &[String], driver: &str) -> Result<Vec<String>> {
    if let Some(bad) = names.iter().find(|n| !valid_secret_name(n)) {
        bail!(
            "{bad:?} isn't a secret's name: A-Z, 0-9 and _, not starting with a digit, and not one a driver's shell \
             or tools use (PATH, IFS, BASH_ENV, LD_*, NODE_*, GIT_*, *_PROXY, DEPLOY_* and the like)"
        );
    }
    // Each driver takes only the secrets it names (M220's review): a name
    // it doesn't read could only change how it behaves.
    let takes = arugula_e2e::deploy::driver_secrets(driver).unwrap_or_default();
    if let Some(bad) = names.iter().find(|n| !takes.contains(&n.as_str())) {
        bail!("the {driver} driver doesn't take a secret called {bad:?}: it takes {}", takes.join(", "));
    }
    let mut names = names.to_vec();
    names.sort();
    names.dedup();
    Ok(names)
}

/// Lock `environment`'s row until the transaction ends, so its events are
/// written one at a time, in seq order (on SQLite the write lock already
/// is). `false`: there's no such environment.
fn lock_environment(c: &impl Sql, environment: &str) -> Result<bool> {
    Ok(c.query_row(
        &format!("SELECT id FROM environments WHERE id = ?1{}", c.for_update()),
        params![environment],
        |r| r.get::<String>(0),
    )
    .optional()?
    .is_some())
}

/// Append an event to `environment`'s log, chained to the one before it.
/// The environment's row must be locked ([`lock_environment`]).
fn append(
    c: &impl Sql,
    environment: &str,
    deploy: &str,
    kind: EventKind,
    by: &Actor,
    at: u64,
    detail: &serde_json::Value,
) -> Result<Event> {
    let last: Option<(u64, String)> = c
        .query_row(
            "SELECT seq, hash FROM deploy_events WHERE environment_id = ?1 ORDER BY seq DESC LIMIT 1",
            params![environment],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()?;
    let (seq, prev_hash) = last.map_or((1, GENESIS.to_owned()), |(s, h)| (s + 1, h));
    let mut e = Event {
        environment: environment.to_owned(),
        seq,
        deploy: deploy.to_owned(),
        kind: kind.as_str().to_owned(),
        actor: by.encode(),
        at,
        detail: detail.to_string(),
        prev_hash,
        hash: String::new(),
    };
    if e.detail.len() > MAX_DETAIL {
        bail!("an event's detail is at most {MAX_DETAIL} bytes, and this one is {}", e.detail.len());
    }
    e.hash = e.digest().context("an event with a newline in it")?;
    c.execute(
        "INSERT INTO deploy_events (environment_id, seq, deploy_id, kind, actor, at, detail, prev_hash, hash)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
        params![e.environment, e.seq, e.deploy, e.kind, e.actor, e.at, e.detail, e.prev_hash, e.hash],
    )?;
    Ok(e)
}

/// The environment's latest apply by a deploy other than `deploy`: its
/// event's seq and that deploy.
fn last_apply(c: &impl Sql, environment: &str, deploy: &str) -> Result<Option<(u64, String)>> {
    c.query_row(
        "SELECT seq, deploy_id FROM deploy_events
         WHERE environment_id = ?1 AND deploy_id != ?2 AND kind IN ('started', 'succeeded', 'failed')
         ORDER BY seq DESC LIMIT 1",
        params![environment, deploy],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )
    .optional()
}

/// A deploy's `planned` event: where the stale rule starts from, never the
/// row's copy of its seq.
pub(super) fn planned_event(c: &impl Sql, deploy: &str) -> Result<Option<Event>> {
    c.query_row(
        &format!(
            "SELECT {EVENT_COLS} FROM deploy_events WHERE deploy_id = ?1 AND kind = 'planned' ORDER BY seq LIMIT 1"
        ),
        params![deploy],
        event_row,
    )
    .optional()
}

/// A deploy's row and its `planned` event, which must agree on its
/// environment. `None`: there's no such deploy.
pub(super) fn deploy_and_plan(c: &impl Sql, deploy: &str) -> Result<Option<(Deploy, Event)>> {
    let Some(d) = c
        .query_row(&format!("SELECT {DEPLOY_COLS} FROM deploys WHERE id = ?1"), params![deploy], deploy_row)
        .optional()?
    else {
        return Ok(None);
    };
    let Some(planned) = planned_event(c, deploy)? else {
        bail!("deploy {deploy} has no planned event: its environment's log is broken");
    };
    if planned.environment != d.environment {
        bail!("deploy {deploy}'s row and its planned event name different environments");
    }
    Ok(Some((d, planned)))
}

/// By when the deploy's approval says its apply must start: the
/// `expires` of its latest `approved` event, from the log.
fn approval_expires(c: &impl Sql, deploy: &str) -> Result<Option<u64>> {
    let detail: Option<String> = c
        .query_row(
            "SELECT detail FROM deploy_events WHERE deploy_id = ?1 AND kind = 'approved' ORDER BY seq DESC LIMIT 1",
            params![deploy],
            |r| r.get(0),
        )
        .optional()?;
    Ok(detail.and_then(|d| serde_json::from_str::<serde_json::Value>(&d).ok()).and_then(|d| d["expires"].as_u64()))
}

/// The stale rule, on what's been read: `d`, planned at `planned_seq` in
/// the log, may not be approved or started once another deploy applied
/// after it.
fn stale(planned_seq: u64, last_apply: Option<(u64, String)>) -> std::result::Result<(), Refusal> {
    match last_apply {
        Some((seq, deploy)) if seq > planned_seq => Err(Refusal::Stale { seq, deploy }),
        _ => Ok(()),
    }
}

/// Whether `d`'s state lets `kind` move it, and for an approval or a start
/// the stale rule (from its `planned` event's seq), and for a start that
/// nothing else is being applied.
fn ready(c: &impl Sql, d: &Deploy, planned_seq: u64, kind: EventKind) -> Result<std::result::Result<(), Refusal>> {
    let gate = matches!(kind, EventKind::Approved | EventKind::Started);
    if gate && (d.state == State::Stale || kind.from().contains(&d.state)) {
        // The rule itself, whatever the row's state says: and for a stale
        // one, why.
        if let Err(r) = stale(planned_seq, last_apply(c, &d.environment, &d.id)?) {
            return Ok(Err(r));
        }
    }
    if !kind.from().contains(&d.state) {
        return Ok(Err(Refusal::State(d.state)));
    }
    if kind == EventKind::Started
        && let Some(busy) = c
            .query_row(
                "SELECT id FROM deploys WHERE environment_id = ?1 AND state = 'running' AND id != ?2 LIMIT 1",
                params![d.environment, d.id],
                |r| r.get::<String>(0),
            )
            .optional()?
    {
        return Ok(Err(Refusal::Busy { deploy: busy }));
    }
    Ok(Ok(()))
}

/// [`Db::record_deploy`] inside a transaction the caller holds and commits
/// (M218 writes its own rows in the same one).
pub(super) fn record_in(
    tx: &impl Sql,
    deploy: &str,
    kind: EventKind,
    by: &Actor,
    at: u64,
    detail: serde_json::Value,
) -> Result<std::result::Result<Recorded, Refusal>> {
    if !detail.is_object() {
        bail!("an event's detail is a JSON object");
    }
    if !by.well_formed() {
        bail!("an actor's ids are each one word");
    }
    if let Err(r) = kind.allows(by) {
        return Ok(Err(r));
    }
    write(tx, deploy, kind, by, at, detail, None)
}

/// [`Db::record_approval`] inside a transaction the caller holds and
/// commits.
pub(super) fn record_approval_in(
    tx: &impl Sql,
    approval: &VerifiedApproval,
    at: u64,
) -> Result<std::result::Result<Recorded, Refusal>> {
    let by = Actor::Person(approval.account.clone());
    if !by.well_formed() {
        bail!("an approver's account is one word");
    }
    write(tx, &approval.deploy, approval.kind(), &by, at, approval.detail(), Some(approval))
}

/// Record `kind` on `deploy`, once the actor may: the environment locked,
/// then the deploy's own checks, then its event, its row, and the deploys
/// an apply makes stale.
fn write(
    tx: &impl Sql,
    deploy: &str,
    kind: EventKind,
    by: &Actor,
    at: u64,
    mut detail: serde_json::Value,
    approval: Option<&VerifiedApproval>,
) -> Result<std::result::Result<Recorded, Refusal>> {
    let Some(environment) = tx
        .query_row("SELECT environment_id FROM deploys WHERE id = ?1", params![deploy], |r| r.get::<String>(0))
        .optional()?
    else {
        return Ok(Err(Refusal::Gone));
    };
    lock_environment(tx, &environment)?;
    let Some((d, planned)) = deploy_and_plan(tx, deploy)? else {
        return Ok(Err(Refusal::Gone));
    };
    if matches!(kind, EventKind::Succeeded | EventKind::Failed)
        && let Actor::Run(r) = by
        && d.state == State::Running
        && d.run.as_deref() != Some(r)
    {
        return Ok(Err(Refusal::NotItsRun));
    }
    if let Some(a) = approval {
        if a.plan_digest != d.plan_digest {
            return Ok(Err(Refusal::Mismatch(Mismatch::Plan)));
        }
        if a.planned_hash != planned.hash {
            return Ok(Err(Refusal::Mismatch(Mismatch::Planned)));
        }
    }
    if let Err(r) = ready(tx, &d, planned.seq, kind)? {
        return Ok(Err(r));
    }
    if kind == EventKind::Started
        && let Actor::Run(r) = by
    {
        // Only the run made for this deploy's operation (M219) starts it,
        // and only while its approval is good: by `at`, the run's own
        // signed time.
        if !super::deploy_runs::is_its_run(tx, deploy, r)? {
            return Ok(Err(Refusal::NotItsRun));
        }
        if approval_expires(tx, deploy)?.is_none_or(|e| at > e) {
            return Ok(Err(Refusal::ApprovalExpired));
        }
        // Its approval is used up as it starts: once, by this run. One
        // used up (by this run or another) never starts it again.
        if !super::deploy_approvals::consume(tx, deploy, r, at)? {
            return Ok(Err(Refusal::ApprovalConsumed));
        }
    }
    let run = match (kind, by) {
        // The deploy's own run (checked above).
        (EventKind::Started, Actor::Run(r)) => {
            detail["run"] = r.clone().into();
            Some(r.clone())
        }
        _ => d.run.clone(),
    };
    let own = append(tx, &environment, deploy, kind, by, at, &detail)?;
    let d = tx.query_row(
        &format!("UPDATE deploys SET state = ?2, run_id = ?3, updated_at = ?4 WHERE id = ?1 RETURNING {DEPLOY_COLS}"),
        params![deploy, kind.to().as_str(), run, at],
        deploy_row,
    )?;
    let mut events = vec![own.clone()];
    if kind.applies() {
        let open: Vec<String> = tx.query_map(
            "SELECT id FROM deploys WHERE environment_id = ?1 AND id != ?2
                 AND state IN ('planned', 'pending', 'approved')
             ORDER BY planned_seq",
            params![environment, deploy],
            |r| r.get(0),
        )?;
        for other in open {
            let why = json!({ "after": deploy, "seq": own.seq });
            events.push(append(tx, &environment, &other, EventKind::Staled, &Actor::Platform, at, &why)?);
            tx.execute("UPDATE deploys SET state = 'stale', updated_at = ?2 WHERE id = ?1", params![other, at])?;
        }
    }
    prune_artifacts(tx, &environment)?;
    Ok(Ok(Recorded { deploy: d, events }))
}

impl Db {
    /// Make an environment in `project`. A name the project already has is
    /// [`Taken`](super::Taken).
    pub fn create_environment(
        &self,
        project: &str,
        name: &str,
        driver: &str,
        secret_names: &[String],
        at: u64,
    ) -> Result<Environment> {
        self.create_environment_with(project, name, driver, secret_names, &Default::default(), at)
    }

    /// [`Db::create_environment`] with its driver's settings (M220), as
    /// given: the route checks them first (`arugula_e2e::drivers::settings`).
    pub fn create_environment_with(
        &self,
        project: &str,
        name: &str,
        driver: &str,
        secret_names: &[String],
        settings: &arugula_e2e::drivers::Settings,
        at: u64,
    ) -> Result<Environment> {
        if !valid_environment_name(name) {
            bail!("{name:?} isn't an environment's name: a-z, 0-9 and -, up to 40");
        }
        let drivers = crate::drivers::names();
        if !drivers.contains(&driver) {
            bail!("{driver:?} isn't a driver: {}", drivers.join(", "));
        }
        let names = sorted_names(secret_names, driver)?;
        self.c()
            .query_row(
                &format!(
                    "INSERT INTO environments (id, project_id, name, driver, secret_names, created_at, updated_at,
                         settings)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?6, ?7) RETURNING {ENV_COLS}"
                ),
                params![
                    new_environment_id(),
                    project,
                    name,
                    driver,
                    serde_json::to_string(&names)?,
                    at,
                    serde_json::to_string(settings)?
                ],
                environment_row,
            )
            .map_err(taken)
    }

    pub fn environment(&self, id: &str) -> Result<Option<Environment>> {
        self.c()
            .query_row(&format!("SELECT {ENV_COLS} FROM environments WHERE id = ?1"), params![id], environment_row)
            .optional()
    }

    /// A project's environments, by name.
    pub fn environments(&self, project: &str) -> Result<Vec<Environment>> {
        self.c().query_map(
            &format!("SELECT {ENV_COLS} FROM environments WHERE project_id = ?1 ORDER BY name"),
            params![project],
            environment_row,
        )
    }

    /// Keep `bytes`, an artifact shipped to `project` (M233: planter's
    /// Ship), under `digest` (`sha256:<hex>` of them), once a deploy of it
    /// is planned: the same bytes again are the same row. Refused
    /// ([`OverQuota`]) when the project's kept artifacts would pass
    /// `quota` bytes. Each goes once no deploy that names it is waiting or
    /// running ([`prune_artifacts`]), or after [`ARTIFACT_DAYS`] anyway.
    pub fn put_deploy_artifact(
        &self,
        project: &str,
        digest: &str,
        bytes: &[u8],
        by: &str,
        at: u64,
        quota: u64,
    ) -> Result<()> {
        let mut c = self.c();
        let tx = c.immediate()?;
        tx.execute(
            "DELETE FROM deploy_artifacts WHERE project_id = ?1 AND created_at < ?2",
            params![project, at.saturating_sub(ARTIFACT_DAYS * 24 * 3600 * 1000)],
        )?;
        let kept = self.deploy_artifacts_size_in(&tx, project, digest)?;
        if kept + bytes.len() as u64 > quota {
            return Err(OverQuota { kept, quota }.into());
        }
        tx.execute(
            "INSERT INTO deploy_artifacts (project_id, digest, bytes, size, created_by, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6) ON CONFLICT (project_id, digest) DO NOTHING",
            params![project, digest, bytes.to_vec(), bytes.len() as u64, by, at],
        )?;
        tx.commit()?;
        Ok(())
    }

    /// The bytes `project`'s kept artifacts add up to, but `digest`'s.
    fn deploy_artifacts_size_in(&self, tx: &impl Sql, project: &str, digest: &str) -> Result<u64> {
        let n: i64 = tx.query_row(
            "SELECT CAST(COALESCE(SUM(size), 0) AS BIGINT) FROM deploy_artifacts WHERE project_id = ?1 AND digest != ?2",
            params![project, digest],
            |r| r.get(0),
        )?;
        Ok(n.max(0) as u64)
    }

    /// The bytes `project`'s kept artifacts add up to.
    pub fn deploy_artifacts_size(&self, project: &str) -> Result<u64> {
        let c = self.c();
        self.deploy_artifacts_size_in(&c, project, "")
    }

    /// Claim the shipped app name `app` for `project` (M233), atomically:
    /// whether it's `project`'s now (claimed now, or by it before).
    pub fn claim_planter_app(&self, app: &str, project: &str, at: u64) -> Result<bool> {
        let c = self.c();
        c.execute(
            "INSERT INTO planter_app_claims (app, project_id, created_at) VALUES (?1, ?2, ?3)
             ON CONFLICT (app) DO NOTHING",
            params![app, project, at],
        )?;
        let owner: String =
            c.query_row("SELECT project_id FROM planter_app_claims WHERE app = ?1", params![app], |r| r.get(0))?;
        Ok(owner == project)
    }

    /// `project`'s artifact `digest`, if it keeps one.
    pub fn deploy_artifact(&self, project: &str, digest: &str) -> Result<Option<Vec<u8>>> {
        self.c()
            .query_row(
                "SELECT bytes FROM deploy_artifacts WHERE project_id = ?1 AND digest = ?2",
                params![project, digest],
                |r| r.get::<Vec<u8>>(0),
            )
            .optional()
    }

    /// Every project's environments that `driver` drives, oldest first
    /// (M233: who has a shipped app's name).
    pub fn environments_driven_by(&self, driver: &str) -> Result<Vec<Environment>> {
        self.c().query_map(
            &format!("SELECT {ENV_COLS} FROM environments WHERE driver = ?1 ORDER BY created_at, id"),
            params![driver],
            environment_row,
        )
    }

    /// Set the names of the secrets an environment's driver runs get.
    /// `None`: there's no such environment.
    pub fn set_environment_secrets(&self, id: &str, names: &[String], at: u64) -> Result<Option<Environment>> {
        let Some(env) = self.environment(id)? else { return Ok(None) };
        let names = sorted_names(names, &env.driver)?;
        self.c()
            .query_row(
                &format!(
                    "UPDATE environments SET secret_names = ?2, updated_at = ?3 WHERE id = ?1 RETURNING {ENV_COLS}"
                ),
                params![id, serde_json::to_string(&names)?, at],
                environment_row,
            )
            .optional()
    }

    /// Plan a deploy to an environment as `by` (a person, or an agent:
    /// agents may plan and ask). Its log gets a `planned` event naming the
    /// source and the plan. `None`: there's no such environment.
    pub fn plan_deploy(&self, new: &NewDeploy, by: &Actor, at: u64) -> Result<Option<Deploy>> {
        let Some(account) = by.account().filter(|_| by.well_formed()) else {
            bail!("a deploy is planned by a person or an agent");
        };
        let s = new.source;
        for (what, v) in [("source", &s.reference), ("place", &s.place)] {
            if !valid_token(v) {
                bail!("a deploy's {what} is one word: {v:?}");
            }
        }
        if new.plan_digest.len() != 64 || !new.plan_digest.bytes().all(|b| b.is_ascii_hexdigit()) {
            bail!("a plan's digest is SHA-256, in hex");
        }
        let agent = match by {
            Actor::Agent { machine, .. } => Some(machine.as_str()),
            _ => None,
        };
        let mut c = self.c();
        let tx = c.immediate()?;
        if !lock_environment(&tx, new.environment)? {
            return Ok(None);
        }
        let id = new_deploy_id();
        let mut detail = json!({
            "source": { "kind": s.kind.as_str(), "ref": s.reference, "where": s.place },
            "plan": new.plan_digest,
        });
        if let Some(r) = new.roll_back {
            detail["roll_back"] = serde_json::to_value(r)?;
        }
        let e = append(&tx, new.environment, &id, EventKind::Planned, by, at, &detail)?;
        let d = tx.query_row(
            &format!(
                "INSERT INTO deploys (id, environment_id, source_kind, source_ref, source_where, plan_digest, state,
                     planned_seq, planned_by, planned_by_agent, created_at, updated_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, 'planned', ?7, ?8, ?9, ?10, ?10) RETURNING {DEPLOY_COLS}"
            ),
            params![
                id,
                new.environment,
                s.kind.as_str(),
                s.reference,
                s.place,
                new.plan_digest,
                e.seq,
                account,
                agent,
                at
            ],
            deploy_row,
        )?;
        tx.commit()?;
        Ok(Some(d))
    }

    pub fn deploy(&self, id: &str) -> Result<Option<Deploy>> {
        self.c()
            .query_row(&format!("SELECT {DEPLOY_COLS} FROM deploys WHERE id = ?1"), params![id], deploy_row)
            .optional()
    }

    /// An environment's deploys, newest first.
    pub fn deploys(&self, environment: &str, limit: u32) -> Result<Vec<Deploy>> {
        self.c().query_map(
            &format!(
                "SELECT {DEPLOY_COLS} FROM deploys WHERE environment_id = ?1 ORDER BY planned_seq DESC LIMIT {limit}"
            ),
            params![environment],
            deploy_row,
        )
    }

    /// A project's deploys, newest first (by when they were planned, then
    /// by id), at most `limit`: in `environment` only when it's given, and
    /// only those after `before` (a deploy's `created_at` and id) when
    /// that is. Each with its events, oldest first: what a project's
    /// history lists.
    pub fn project_deploys(
        &self,
        project: &str,
        environment: Option<&str>,
        before: Option<(u64, &str)>,
        limit: u32,
    ) -> Result<Vec<(Deploy, Vec<Event>)>> {
        let c = self.c();
        let mut args: Vec<&dyn Param> = vec![&project];
        let mut sql = format!(
            "SELECT {DEPLOY_COLS} FROM deploys WHERE environment_id IN (SELECT id FROM environments WHERE project_id = ?)"
        );
        if let Some(env) = &environment {
            sql += " AND environment_id = ?";
            args.push(env);
        }
        if let Some((at, id)) = &before {
            sql += " AND (created_at < ? OR (created_at = ? AND id < ?))";
            args.extend([at as &dyn Param, at, id]);
        }
        sql += &format!(" ORDER BY created_at DESC, id DESC LIMIT {limit}");
        let deploys = c.query_map(&sql, &args, deploy_row)?;
        if deploys.is_empty() {
            return Ok(vec![]);
        }
        let ids: Vec<&str> = deploys.iter().map(|d| d.id.as_str()).collect();
        let marks = vec!["?"; ids.len()].join(", ");
        let events = c.query_map(
            &format!("SELECT {EVENT_COLS} FROM deploy_events WHERE deploy_id IN ({marks}) ORDER BY seq"),
            &super::sql::list(&ids),
            event_row,
        )?;
        let mut of: std::collections::HashMap<String, Vec<Event>> = std::collections::HashMap::new();
        for e in events {
            of.entry(e.deploy.clone()).or_default().push(e);
        }
        Ok(deploys
            .into_iter()
            .map(|d| {
                let events = of.remove(&d.id).unwrap_or_default();
                (d, events)
            })
            .collect())
    }

    /// Whether `deploy` may be approved now: it's waiting for approval and
    /// nothing else applied to its environment since its `planned` event
    /// (the stale rule). What M218 asks before it takes an approval;
    /// recording one ([`Db::record_approval`]) asks again in its own
    /// transaction.
    pub fn may_approve(&self, deploy: &str) -> Result<std::result::Result<(), Refusal>> {
        let c = self.c();
        let Some((d, planned)) = deploy_and_plan(&c, deploy)? else {
            return Ok(Err(Refusal::Gone));
        };
        ready(&c, &d, planned.seq, EventKind::Approved)
    }

    /// Move a deploy on by recording `kind` as `by`, with `detail` (a JSON
    /// object, at most [`MAX_DETAIL`] bytes) in its event: the deploy's
    /// state and its log change in one transaction, or neither does. A
    /// `started` sets the run that applies it. An apply (`started`,
    /// `succeeded`, `failed`) makes every other open deploy of the
    /// environment stale, each with its own event. Refused when the
    /// deploy's state, the actor or the stale rule don't allow it
    /// ([`Refusal`]); an approval or a deny always is
    /// ([`Db::record_approval`]).
    pub fn record_deploy(
        &self,
        deploy: &str,
        kind: EventKind,
        by: &Actor,
        at: u64,
        detail: serde_json::Value,
    ) -> Result<std::result::Result<Recorded, Refusal>> {
        let mut c = self.c();
        let tx = c.immediate()?;
        let r = record_in(&tx, deploy, kind, by, at, detail)?;
        if r.is_ok() {
            tx.commit()?;
        }
        Ok(r)
    }

    /// Record an owner device's verified approval or deny on its deploy,
    /// as its account: refused unless it names the deploy's plan and its
    /// `planned` event as they are, the deploy is waiting for approval, and
    /// (for an approval) the stale rule allows it.
    pub fn record_approval(
        &self,
        approval: &VerifiedApproval,
        at: u64,
    ) -> Result<std::result::Result<Recorded, Refusal>> {
        let mut c = self.c();
        let tx = c.immediate()?;
        let r = record_approval_in(&tx, approval, at)?;
        if r.is_ok() {
            tx.commit()?;
        }
        Ok(r)
    }

    /// An environment's log, oldest first.
    pub fn deploy_events(&self, environment: &str) -> Result<Vec<Event>> {
        self.c().query_map(
            &format!("SELECT {EVENT_COLS} FROM deploy_events WHERE environment_id = ?1 ORDER BY seq"),
            params![environment],
            event_row,
        )
    }

    /// The roll back a deploy was planned as (M220), from its `planned`
    /// event: `None` for a deploy forward.
    pub fn deploy_roll_back(&self, deploy: &str) -> Result<Option<arugula_e2e::drivers::RollBack>> {
        let planned = self.deploy_history(deploy)?.into_iter().find(|e| e.kind == EventKind::Planned.as_str());
        let Some(e) = planned else { return Ok(None) };
        Ok(match e.detail_json()?.get("roll_back") {
            Some(r) => Some(serde_json::from_value(r.clone())?),
            None => None,
        })
    }

    /// What the newest `succeeded` event of a deploy of `source` in
    /// `environment` kept (M220), with that deploy's id: what a roll back
    /// to `source` goes back to.
    pub fn last_succeeded_of(&self, environment: &str, source: &Source) -> Result<Option<(String, serde_json::Value)>> {
        let events = self.deploy_events(environment)?;
        for e in events.iter().rev().filter(|e| e.kind == EventKind::Succeeded.as_str()) {
            let Some(d) = self.deploy(&e.deploy)? else { continue };
            if d.source == *source {
                return Ok(Some((d.id, e.detail_json()?)));
            }
        }
        Ok(None)
    }

    /// One deploy's events, oldest first.
    pub fn deploy_history(&self, deploy: &str) -> Result<Vec<Event>> {
        self.c().query_map(
            &format!("SELECT {EVENT_COLS} FROM deploy_events WHERE deploy_id = ?1 ORDER BY seq"),
            params![deploy],
            event_row,
        )
    }

    /// Check an environment's log ([`verify_chain`]) and its deploys'
    /// rows against it ([`verify_rows`]), read in one snapshot: how many
    /// events verified, or the first bad link.
    pub fn verify_deploy_log(&self, environment: &str) -> Result<std::result::Result<u64, BadLink>> {
        let mut c = self.c();
        let tx = c.transaction()?;
        if tx.dialect() == super::migrate::Dialect::Postgres {
            tx.execute_batch("SET TRANSACTION ISOLATION LEVEL REPEATABLE READ")?;
        }
        let events = tx.query_map(
            &format!("SELECT {EVENT_COLS} FROM deploy_events WHERE environment_id = ?1 ORDER BY seq"),
            params![environment],
            event_row,
        )?;
        let deploys = tx.query_map(
            &format!("SELECT {DEPLOY_COLS} FROM deploys WHERE environment_id = ?1 ORDER BY planned_seq"),
            params![environment],
            deploy_row,
        )?;
        tx.commit()?;
        Ok(verify_chain(environment, &events).and_then(|n| verify_rows(&events, &deploys).map(|()| n)))
    }
}

/// What deleting an account takes from deploys, inside the platform's
/// `account_deleted` transaction, after [`super::projects::erase_account`]:
/// - the environments of the projects that went with it, then their logs
///   (which a trigger keeps while their environment exists) and their
///   deploys, whole;
/// - elsewhere its deploys keep their rows, naming no planner (and no
///   agent);
/// - its events keep their rows as written, its account id in `actor`
///   included: an event is never rewritten, and rewriting one would break
///   its environment's chain. The id names no one once the account is
///   gone, so they're shown as "a deleted account".
pub(crate) fn erase_account(c: &impl Sql, d: &Deleted) -> Result<()> {
    c.execute("DELETE FROM environments WHERE project_id NOT IN (SELECT id FROM projects)", params![])?;
    c.execute("DELETE FROM deploy_events WHERE environment_id NOT IN (SELECT id FROM environments)", params![])?;
    c.execute("DELETE FROM deploys WHERE environment_id NOT IN (SELECT id FROM environments)", params![])?;
    c.execute("DELETE FROM deploy_artifacts WHERE project_id NOT IN (SELECT id FROM projects)", params![])?;
    c.execute("DELETE FROM planter_app_claims WHERE project_id NOT IN (SELECT id FROM projects)", params![])?;
    super::deploy_approvals::erase_account(c)?;
    super::deploy_runs::erase_account(c)?;
    c.execute("UPDATE deploys SET planned_by = '', planned_by_agent = NULL WHERE planned_by = ?1", params![d.account])?;
    Ok(())
}

#[cfg(test)]
#[path = "deploys_tests.rs"]
mod tests;
