//! Environments and deploys (M217, platform step 7, native Arugula #13): a
//! project's environments, the deploys planned to them, and each
//! environment's event log. Who may plan or ask is decided by the routes;
//! the signed approval is M218's. This is storage, and the rules that
//! hold whoever calls it. The tables, with notes on their columns, are
//! made by `0021_deploys`.
//!
//! - **The log is the record.** Every change to a deploy is an event in its
//!   environment's log, written in the same transaction as the deploy's
//!   `state`. Events are never updated or deleted in place (a trigger
//!   refuses an update); only a whole log goes, with its project.
//! - **The log is a hash chain per environment.** Each event's `hash` is
//!   SHA-256 over its canonical lines ([`Event::canonical`]), the first of
//!   which is the hash of the event before it. Anyone with the rows can
//!   check it ([`verify_chain`]): a changed, removed or reordered event
//!   breaks the link at it. The chain is per environment, not per deploy
//!   or per project, because that's what the stale rule reads: whether
//!   anything applied to this environment after a plan.
//! - **The stale rule.** A deploy planned before another deploy applied to
//!   its environment can't be approved or started ([`Db::may_approve`]).
//!   "Applied" is any apply event of another deploy (`started`,
//!   `succeeded` or `failed`) with a higher `seq` than the deploy's
//!   `planned` event: an apply that failed may still have changed the
//!   environment. The same events turn the environment's other open
//!   deploys (`planned`, `pending`, `approved`) `stale` at once, each with a
//!   `staled` event; the seq check holds even if a row's state didn't.
//! - **Who records what** ([`EventKind::allows`]): a person or an agent
//!   plans, asks, and cancels; only a person approves or denies, **never an
//!   agent**; only a run starts, and finishes, its apply; only the
//!   platform expires one or fails a run that's gone.
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
    sql::{Optional as _, Row, Sql, params},
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

/// The drivers an environment may name (M219-M222). Planter's waits for
/// step 8.
pub const DRIVERS: &[&str] = &["chant-fly", "apps-platform"];

/// The first event's `prev_hash`.
pub const GENESIS: &str = "0000000000000000000000000000000000000000000000000000000000000000";

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
/// `_`, not starting with a digit.
pub fn valid_secret_name(name: &str) -> bool {
    (1..=64).contains(&name.len())
        && name.bytes().all(|b| b.is_ascii_uppercase() || b.is_ascii_digit() || b == b'_')
        && !name.as_bytes()[0].is_ascii_digit()
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
    /// agent asks and cancels; only a person approves or denies, never an
    /// agent (M218 adds the owner device's signature); only a run starts
    /// and finishes its apply, or the platform fails one whose run is gone;
    /// only the platform expires one. Planning is [`Db::plan_deploy`]'s and
    /// staling the platform's own, never asked for.
    pub fn allows(self, by: &Actor) -> std::result::Result<(), Refusal> {
        let ok = match self {
            EventKind::Requested => matches!(by, Actor::Person(_) | Actor::Agent { .. }),
            EventKind::Canceled => matches!(by, Actor::Person(_) | Actor::Agent { .. } | Actor::Platform),
            EventKind::Approved | EventKind::Denied => match by {
                Actor::Agent { .. } => return Err(Refusal::Agent),
                a => matches!(a, Actor::Person(_)),
            },
            EventKind::Expired => matches!(by, Actor::Platform),
            EventKind::Started | EventKind::Succeeded => matches!(by, Actor::Run(_)),
            EventKind::Failed => matches!(by, Actor::Run(_) | Actor::Platform),
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
    /// The `seq` of its `planned` event in its environment's log.
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
    ///   plan's digest);
    /// - `started`: `run`;
    /// - `staled`: `after` (the deploy that applied) and `seq` (its event);
    /// - the others: what their caller gives (M218: the approval).
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
    /// Not this actor's to record ([`EventKind::allows`]).
    Actor,
    /// Another deploy of its environment is being applied.
    Busy { deploy: String },
    /// A run reporting on an apply that isn't its own.
    NotItsRun,
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
            Refusal::Actor => f.write_str("that's not this actor's to record"),
            Refusal::Busy { deploy } => write!(f, "{deploy} is being applied to the environment"),
            Refusal::NotItsRun => f.write_str("that run isn't applying this deploy"),
        }
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
}

const ENV_COLS: &str = "id, project_id, name, driver, secret_names, created_at, updated_at";
const DEPLOY_COLS: &str = "id, environment_id, source_kind, source_ref, source_where, plan_digest, state, planned_seq, \
                           run_id, planned_by, planned_by_agent, created_at, updated_at";
const EVENT_COLS: &str = "environment_id, seq, deploy_id, kind, actor, at, detail, prev_hash, hash";

fn environment_row(r: &Row) -> Result<Environment> {
    Ok(Environment {
        id: r.get(0)?,
        project: r.get(1)?,
        name: r.get(2)?,
        driver: r.get(3)?,
        secret_names: serde_json::from_str(&r.get::<String>(4)?)?,
        created_at: r.get(5)?,
        updated_at: r.get(6)?,
    })
}

fn deploy_row(r: &Row) -> Result<Deploy> {
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

fn event_row(r: &Row) -> Result<Event> {
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

fn sorted_names(names: &[String]) -> Result<Vec<String>> {
    if let Some(bad) = names.iter().find(|n| !valid_secret_name(n)) {
        bail!("{bad:?} isn't a secret's name: A-Z, 0-9 and _, not starting with a digit");
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

/// The stale rule, on what's been read: `d` may not be approved or started
/// once another deploy applied after its plan.
fn stale(d: &Deploy, last_apply: Option<(u64, String)>) -> std::result::Result<(), Refusal> {
    match last_apply {
        Some((seq, deploy)) if seq > d.planned_seq => Err(Refusal::Stale { seq, deploy }),
        _ => Ok(()),
    }
}

/// Whether `kind` may be recorded on `d` by `by`, inside the transaction
/// that holds `d`'s environment: the actor ([`EventKind::allows`]), then
/// [`ready`].
fn check(c: &impl Sql, d: &Deploy, kind: EventKind, by: &Actor) -> Result<std::result::Result<(), Refusal>> {
    if let Err(r) = kind.allows(by) {
        return Ok(Err(r));
    }
    if matches!(kind, EventKind::Succeeded | EventKind::Failed)
        && let Actor::Run(r) = by
        && d.state == State::Running
        && d.run.as_deref() != Some(r)
    {
        return Ok(Err(Refusal::NotItsRun));
    }
    ready(c, d, kind)
}

/// Whether `d`'s state lets `kind` move it, and for an approval or a start
/// the stale rule, and for a start that nothing else is being applied.
fn ready(c: &impl Sql, d: &Deploy, kind: EventKind) -> Result<std::result::Result<(), Refusal>> {
    let gate = matches!(kind, EventKind::Approved | EventKind::Started);
    if gate && (d.state == State::Stale || kind.from().contains(&d.state)) {
        // The rule itself, whatever the row's state says: and for a stale
        // one, why.
        if let Err(r) = stale(d, last_apply(c, &d.environment, &d.id)?) {
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
        if !valid_environment_name(name) {
            bail!("{name:?} isn't an environment's name: a-z, 0-9 and -, up to 40");
        }
        if !DRIVERS.contains(&driver) {
            bail!("{driver:?} isn't a driver: {}", DRIVERS.join(", "));
        }
        let names = sorted_names(secret_names)?;
        self.c()
            .query_row(
                &format!(
                    "INSERT INTO environments (id, project_id, name, driver, secret_names, created_at, updated_at)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?6) RETURNING {ENV_COLS}"
                ),
                params![new_environment_id(), project, name, driver, serde_json::to_string(&names)?, at],
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

    /// Set the names of the secrets an environment's driver runs get.
    /// `None`: there's no such environment.
    pub fn set_environment_secrets(&self, id: &str, names: &[String], at: u64) -> Result<Option<Environment>> {
        let names = sorted_names(names)?;
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
        let detail = json!({
            "source": { "kind": s.kind.as_str(), "ref": s.reference, "where": s.place },
            "plan": new.plan_digest,
        });
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

    /// Whether `deploy` may be approved now: it's waiting for approval and
    /// nothing else applied to its environment since it was planned (the
    /// stale rule). What M218 asks before it takes an approval; recording
    /// one ([`Db::record_deploy`]) asks again in its own transaction.
    pub fn may_approve(&self, deploy: &str) -> Result<std::result::Result<(), Refusal>> {
        let c = self.c();
        let Some(d) = c
            .query_row(&format!("SELECT {DEPLOY_COLS} FROM deploys WHERE id = ?1"), params![deploy], deploy_row)
            .optional()?
        else {
            return Ok(Err(Refusal::Gone));
        };
        ready(&c, &d, EventKind::Approved)
    }

    /// Move a deploy on by recording `kind` as `by`, with `detail` (a JSON
    /// object) in its event: the deploy's state and its log change in one
    /// transaction, or neither does. A `started` sets the run that applies
    /// it. An apply (`started`, `succeeded`, `failed`) makes every other
    /// open deploy of the environment stale, each with its own event.
    /// Refused when the deploy's state, the actor or the stale rule don't
    /// allow it ([`Refusal`]).
    pub fn record_deploy(
        &self,
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
        let mut c = self.c();
        let tx = c.immediate()?;
        let Some(environment) = tx
            .query_row("SELECT environment_id FROM deploys WHERE id = ?1", params![deploy], |r| r.get::<String>(0))
            .optional()?
        else {
            return Ok(Err(Refusal::Gone));
        };
        lock_environment(&tx, &environment)?;
        let d =
            tx.query_row(&format!("SELECT {DEPLOY_COLS} FROM deploys WHERE id = ?1"), params![deploy], deploy_row)?;
        if let Err(r) = check(&tx, &d, kind, by)? {
            return Ok(Err(r));
        }
        let mut detail = detail;
        let run = match (kind, by) {
            (EventKind::Started, Actor::Run(r)) => {
                detail["run"] = r.clone().into();
                Some(r.clone())
            }
            _ => d.run.clone(),
        };
        let own = append(&tx, &environment, deploy, kind, by, at, &detail)?;
        let d = tx.query_row(
            &format!(
                "UPDATE deploys SET state = ?2, run_id = ?3, updated_at = ?4 WHERE id = ?1 RETURNING {DEPLOY_COLS}"
            ),
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
                events.push(append(&tx, &environment, &other, EventKind::Staled, &Actor::Platform, at, &why)?);
                tx.execute("UPDATE deploys SET state = 'stale', updated_at = ?2 WHERE id = ?1", params![other, at])?;
            }
        }
        tx.commit()?;
        Ok(Ok(Recorded { deploy: d, events }))
    }

    /// An environment's log, oldest first.
    pub fn deploy_events(&self, environment: &str) -> Result<Vec<Event>> {
        self.c().query_map(
            &format!("SELECT {EVENT_COLS} FROM deploy_events WHERE environment_id = ?1 ORDER BY seq"),
            params![environment],
            event_row,
        )
    }

    /// One deploy's events, oldest first.
    pub fn deploy_history(&self, deploy: &str) -> Result<Vec<Event>> {
        self.c().query_map(
            &format!("SELECT {EVENT_COLS} FROM deploy_events WHERE deploy_id = ?1 ORDER BY seq"),
            params![deploy],
            event_row,
        )
    }

    /// Check an environment's log ([`verify_chain`]): how many events
    /// verified, or the first bad link.
    pub fn verify_deploy_log(&self, environment: &str) -> Result<std::result::Result<u64, BadLink>> {
        Ok(verify_chain(environment, &self.deploy_events(environment)?))
    }
}

/// What deleting an account takes from deploys, inside the platform's
/// `account_deleted` transaction, after [`super::projects::erase_account`]:
/// - the environments of the projects that went with it, their deploys and
///   their logs, whole;
/// - elsewhere its deploys keep their rows, naming no planner (and no
///   agent);
/// - its events keep their rows as written, its account id in `actor`
///   included: an event is never rewritten, and rewriting one would break
///   its environment's chain. The id names no one once the account is
///   gone, so they're shown as "a deleted account".
pub(crate) fn erase_account(c: &impl Sql, d: &Deleted) -> Result<()> {
    c.execute(
        "DELETE FROM deploy_events WHERE environment_id IN
             (SELECT id FROM environments WHERE project_id NOT IN (SELECT id FROM projects))",
        params![],
    )?;
    c.execute(
        "DELETE FROM deploys WHERE environment_id IN
             (SELECT id FROM environments WHERE project_id NOT IN (SELECT id FROM projects))",
        params![],
    )?;
    c.execute("DELETE FROM environments WHERE project_id NOT IN (SELECT id FROM projects)", params![])?;
    c.execute("UPDATE deploys SET planned_by = '', planned_by_agent = NULL WHERE planned_by = ?1", params![d.account])?;
    Ok(())
}

#[cfg(test)]
#[path = "deploys_tests.rs"]
mod tests;
