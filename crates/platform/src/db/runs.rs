//! Runs (M210, platform step 7): a project's runs, each one S37 operation
//! on a workspace, their history and what they left behind. Who may start
//! or move one is the routes' (M212, M214); this is storage and the one
//! place a run's state changes ([`Db::move_run`]). The tables, with notes
//! on their columns, are made by `0020_runs`.
//!
//! - **States** are S37's eight, and a run moves only as S37 allows
//!   ([`State::may_become`], in `arugula_e2e::run`, which the daemon
//!   shares): one transaction checks the move and writes the run's new
//!   state and its event, or refuses and writes nothing ([`Refused`]).
//! - **Events** are append-only: nothing here changes or deletes one but
//!   deleting the run's project, and the tables refuse an `UPDATE`. Each
//!   that a daemon reported carries its signature over `arugula run event
//!   v1` (`arugula_e2e::run::Event`). Checking that signature against the
//!   daemon's key is the route's job before it calls [`Db::move_run`]
//!   (M212); this stores it, and refuses a report that isn't next in the
//!   run's history, so a signed report can't be replayed.
//! - **Who reports what:** `running` and `succeeded` only a daemon (it's
//!   the one that knows); `pending`, `approved`, `denied` and `canceled`
//!   only control, as the signed objects behind them arrive; `expired` and
//!   `failed` either (control's clock, a dispatch that never reached the
//!   workspace).
//! - **The job** that carries a run to its workspace isn't here: M212 and
//!   M214 enqueue it on `platform_jobs` (M208) beside the run they make
//!   with [`Db::create_run`].

use anyhow::{Context as _, Result, bail};
pub use arugula_e2e::run::State;
use sha2::{Digest as _, Sha256};

use super::{
    Db,
    sql::{Optional as _, Row, Sql, params},
    taken,
};
use crate::Deleted;

/// A new run's id: `run_` and 32 hex characters, beside `prj_` for
/// projects.
fn new_id() -> String {
    format!("run_{}", hex::encode(arugula_e2e::random::<16>()))
}

/// The first line of an S37 operation.
const OP_V1: &str = "arugula op v1\n";

/// The most an event's detail may hold.
pub const MAX_DETAIL: usize = 4096;

/// What started a run.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TriggerKind {
    Schedule,
    Event,
    Manual,
}

impl TriggerKind {
    pub fn as_str(self) -> &'static str {
        match self {
            TriggerKind::Schedule => "schedule",
            TriggerKind::Event => "event",
            TriggerKind::Manual => "manual",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "schedule" => TriggerKind::Schedule,
            "event" => TriggerKind::Event,
            "manual" => TriggerKind::Manual,
            _ => return None,
        })
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Run {
    pub id: String,
    pub project: String,
    /// The workspace it runs on.
    pub workspace: Option<String>,
    pub trigger: TriggerKind,
    /// Which schedule or forge event, as a small JSON object.
    pub source: serde_json::Value,
    /// The S37 operation's bytes.
    pub op: String,
    /// Their SHA-256, hex: what an approval or a grant names.
    pub op_hash: String,
    pub state: State,
    /// The account that started it by hand (an agent's owner's, with the
    /// machine in `started_by_agent`), or the schedule's or event's id.
    /// Empty once that account is deleted.
    pub started_by: String,
    pub started_by_agent: Option<String>,
    /// The standing grant it may run under without an approval (M211).
    pub grant: Option<String>,
    /// The latest event's number.
    pub last_seq: u64,
    pub created_at: u64,
    pub started_at: Option<u64>,
    pub ended_at: Option<u64>,
    pub exit_code: Option<i64>,
    /// The chant ledger entry a release run wrote.
    pub chant_ledger: Option<String>,
}

/// One change of a run's state.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Event {
    pub run: String,
    pub seq: u64,
    /// `None` for the first: the run made, as `pending`.
    pub from: Option<State>,
    pub to: State,
    pub at: u64,
    /// A JSON object's text, as the reporter wrote it (and signed it): an
    /// exit code (`exit`), why it failed.
    pub detail: String,
    /// The daemon that reported it; `None` when control recorded it.
    pub daemon: Option<String>,
    /// The daemon's signature over [`Event::signed`], hex.
    pub sig: Option<String>,
}

impl Event {
    /// What its daemon signed (`None` when control recorded it).
    pub fn signed(&self) -> Option<arugula_e2e::run::Event<'_>> {
        Some(arugula_e2e::run::Event {
            run: &self.run,
            seq: self.seq,
            from: self.from,
            to: self.to,
            at: self.at,
            detail: &self.detail,
            daemon: self.daemon.as_deref()?,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Artifact {
    pub run: String,
    pub name: String,
    pub size: u64,
    /// `sha256:` and hex.
    pub digest: String,
    /// Where it is in the run bucket (M213).
    pub bucket_key: String,
    pub created_at: u64,
}

pub struct NewRun<'a> {
    pub project: &'a str,
    pub workspace: Option<&'a str>,
    pub trigger: TriggerKind,
    /// A JSON object.
    pub source: &'a serde_json::Value,
    /// An S37 operation (`arugula op v1`).
    pub op: &'a str,
    pub started_by: &'a str,
    pub started_by_agent: Option<&'a str>,
    pub grant: Option<&'a str>,
}

/// Who reports a move.
#[derive(Debug, Clone, Copy)]
pub enum Reporter<'a> {
    /// Control, as a signed object arrives or by its clock.
    Control,
    /// The daemon running it, with the event's number it signed and its
    /// signature (hex), which the route has checked.
    Daemon { id: &'a str, seq: u64, sig: &'a str },
}

/// A change of state asked for.
#[derive(Debug, Clone, Copy)]
pub struct Move<'a> {
    /// The state the reporter saw it in: a move from any other is refused,
    /// so two reporters can't both move it.
    pub from: State,
    pub to: State,
    pub at: u64,
    /// A JSON object's text, at most [`MAX_DETAIL`] bytes. `exit` is the
    /// process's exit code: 0 for `succeeded`, never 0 for `failed`, and
    /// only on those two.
    pub detail: &'a str,
    pub by: Reporter<'a>,
}

/// Why a move was refused. Nothing was written.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Refused {
    /// There's no such run.
    NoRun,
    /// The run isn't in the state the move is from (another move won).
    Moved { now: State },
    /// S37 doesn't let a run move so.
    Disallowed { from: State, to: State },
    /// Only a daemon may report it, or only control.
    Reporter,
    /// Straight to `running` from `pending` needs a standing grant.
    NoGrant,
    /// A daemon's report that isn't the run's next event.
    Seq { next: u64 },
    /// The detail isn't a JSON object of at most [`MAX_DETAIL`] bytes, or
    /// its exit code doesn't fit the state.
    Detail(&'static str),
}

impl std::fmt::Display for Refused {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Refused::NoRun => f.write_str("no such run"),
            Refused::Moved { now } => write!(f, "the run is {now} now"),
            Refused::Disallowed { from, to } => write!(f, "a run can't go from {from} to {to}"),
            Refused::Reporter => f.write_str("not a move this reporter may make"),
            Refused::NoGrant => f.write_str("the run has no standing grant: it needs an approval"),
            Refused::Seq { next } => write!(f, "the run's next event is {next}"),
            Refused::Detail(why) => f.write_str(why),
        }
    }
}

impl std::error::Error for Refused {}

const COLS: &str = "id, project_id, workspace_id, trigger_kind, trigger_source, op, op_hash, state, started_by, \
                    started_by_agent, grant_id, last_seq, created_at, started_at, ended_at, exit_code, chant_ledger";
const EVENT_COLS: &str = "run_id, seq, from_state, to_state, at, detail, daemon, sig";
const ARTIFACT_COLS: &str = "run_id, name, size, digest, bucket_key, created_at";

fn state(s: &str) -> Result<State> {
    State::parse(s).with_context(|| format!("a run in state {s:?}"))
}

fn run_row(r: &Row) -> Result<Run> {
    let trigger: String = r.get(3)?;
    Ok(Run {
        id: r.get(0)?,
        project: r.get(1)?,
        workspace: r.get(2)?,
        trigger: TriggerKind::parse(&trigger).with_context(|| format!("a run triggered by {trigger:?}"))?,
        source: serde_json::from_str(&r.get::<String>(4)?)?,
        op: r.get(5)?,
        op_hash: r.get(6)?,
        state: state(&r.get::<String>(7)?)?,
        started_by: r.get(8)?,
        started_by_agent: r.get(9)?,
        grant: r.get(10)?,
        last_seq: r.get(11)?,
        created_at: r.get(12)?,
        started_at: r.get(13)?,
        ended_at: r.get(14)?,
        exit_code: r.get(15)?,
        chant_ledger: r.get(16)?,
    })
}

fn event_row(r: &Row) -> Result<Event> {
    Ok(Event {
        run: r.get(0)?,
        seq: r.get(1)?,
        from: r.get::<Option<String>>(2)?.as_deref().map(state).transpose()?,
        to: state(&r.get::<String>(3)?)?,
        at: r.get(4)?,
        detail: r.get(5)?,
        daemon: r.get(6)?,
        sig: r.get(7)?,
    })
}

fn artifact_row(r: &Row) -> Result<Artifact> {
    Ok(Artifact {
        run: r.get(0)?,
        name: r.get(1)?,
        size: r.get(2)?,
        digest: r.get(3)?,
        bucket_key: r.get(4)?,
        created_at: r.get(5)?,
    })
}

fn add_event(c: &impl Sql, e: &Event) -> Result<()> {
    c.execute(
        &format!("INSERT INTO run_events ({EVENT_COLS}) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)"),
        params![e.run, e.seq, e.from.map(State::as_str), e.to.as_str(), e.at, e.detail, e.daemon, e.sig],
    )?;
    Ok(())
}

/// The exit code `detail` carries, if it's a JSON object that fits `to`.
fn exit_code(detail: &str, to: State) -> Result<Option<i64>, Refused> {
    if detail.len() > MAX_DETAIL {
        return Err(Refused::Detail("the detail is too long"));
    }
    let Ok(serde_json::Value::Object(d)) = serde_json::from_str(detail) else {
        return Err(Refused::Detail("the detail isn't a JSON object"));
    };
    let exit = match d.get("exit") {
        None => None,
        Some(v) => Some(v.as_i64().ok_or(Refused::Detail("the exit code isn't a number"))?),
    };
    match (to, exit) {
        (State::Succeeded, Some(0)) | (State::Failed, None) => Ok(exit),
        (State::Failed, Some(n)) if n != 0 => Ok(exit),
        (State::Succeeded, _) => Err(Refused::Detail("a run that succeeded exited 0")),
        (State::Failed, _) => Err(Refused::Detail("a run that failed didn't exit 0")),
        (_, None) => Ok(None),
        (_, Some(_)) => Err(Refused::Detail("only an ended run has an exit code")),
    }
}

impl Db {
    /// Make a run in `project`, `pending`, with its first event (control's).
    /// `None`: there's no such project. Its job is the caller's (M212,
    /// M214).
    pub fn create_run(&self, new: &NewRun, at: u64) -> Result<Option<Run>> {
        if !new.op.starts_with(OP_V1) {
            bail!("a run's spec is an S37 operation (`arugula op v1`)");
        }
        if !new.source.is_object() {
            bail!("a run's trigger source is a JSON object");
        }
        let op_hash = hex::encode(Sha256::digest(new.op.as_bytes()));
        let mut c = self.c();
        let tx = c.transaction()?;
        let Some(run) = tx
            .query_row(
                &format!(
                    "INSERT INTO runs (id, project_id, workspace_id, trigger_kind, trigger_source, op, op_hash,
                         state, started_by, started_by_agent, grant_id, last_seq, created_at)
                     SELECT ?1, id, ?2, ?3, ?4, ?5, ?6, 'pending', ?7, ?8, ?9, 1, ?10 FROM projects WHERE id = ?11
                     RETURNING {COLS}"
                ),
                params![
                    new_id(),
                    new.workspace,
                    new.trigger.as_str(),
                    new.source.to_string(),
                    new.op,
                    op_hash,
                    new.started_by,
                    new.started_by_agent,
                    new.grant,
                    at,
                    new.project
                ],
                run_row,
            )
            .optional()?
        else {
            return Ok(None);
        };
        let first = Event {
            run: run.id.clone(),
            seq: 1,
            from: None,
            to: State::Pending,
            at,
            detail: "{}".into(),
            daemon: None,
            sig: None,
        };
        add_event(&tx, &first)?;
        tx.commit()?;
        Ok(Some(run))
    }

    pub fn run(&self, id: &str) -> Result<Option<Run>> {
        self.c().query_row(&format!("SELECT {COLS} FROM runs WHERE id = ?1"), params![id], run_row).optional()
    }

    /// A project's latest runs, newest first.
    pub fn project_runs(&self, project: &str, limit: u32) -> Result<Vec<Run>> {
        self.c().query_map(
            &format!("SELECT {COLS} FROM runs WHERE project_id = ?1 ORDER BY created_at DESC, id DESC LIMIT ?2"),
            params![project, limit],
            run_row,
        )
    }

    /// A run's history, oldest first.
    pub fn run_events(&self, run: &str) -> Result<Vec<Event>> {
        self.c().query_map(
            &format!("SELECT {EVENT_COLS} FROM run_events WHERE run_id = ?1 ORDER BY seq"),
            params![run],
            event_row,
        )
    }

    /// Move a run, as S37 allows: its new state and the event saying so,
    /// in one transaction. `Err(Refused)` writes nothing.
    pub fn move_run(&self, id: &str, m: &Move) -> Result<Result<(Run, Event), Refused>> {
        let exit = match exit_code(m.detail, m.to) {
            Ok(e) => e,
            Err(r) => return Ok(Err(r)),
        };
        let by_daemon = matches!(m.by, Reporter::Daemon { .. });
        if (by_daemon && m.to.control_only()) || (!by_daemon && m.to.daemon_only()) {
            return Ok(Err(Refused::Reporter));
        }
        let mut c = self.c();
        let tx = c.immediate()?;
        let Some(run) = tx
            .query_row(&format!("SELECT {COLS} FROM runs WHERE id = ?1{}", tx.for_update()), params![id], run_row)
            .optional()?
        else {
            return Ok(Err(Refused::NoRun));
        };
        if run.state != m.from {
            return Ok(Err(Refused::Moved { now: run.state }));
        }
        if !m.from.may_become(m.to) {
            return Ok(Err(Refused::Disallowed { from: m.from, to: m.to }));
        }
        if (m.from, m.to) == (State::Pending, State::Running) && run.grant.is_none() {
            return Ok(Err(Refused::NoGrant));
        }
        let seq = run.last_seq + 1;
        let (daemon, sig) = match m.by {
            Reporter::Control => (None, None),
            Reporter::Daemon { id, seq: signed, sig } => {
                if signed != seq {
                    return Ok(Err(Refused::Seq { next: seq }));
                }
                (Some(id.to_owned()), Some(sig.to_owned()))
            }
        };
        let event = Event {
            run: run.id.clone(),
            seq,
            from: Some(m.from),
            to: m.to,
            at: m.at,
            detail: m.detail.into(),
            daemon,
            sig,
        };
        if event.signed().is_some_and(|s| !s.well_formed()) {
            return Ok(Err(Refused::Reporter));
        }
        let run = tx.query_row(
            &format!(
                "UPDATE runs SET state = ?2, last_seq = ?3,
                     started_at = CASE WHEN ?2 = 'running' THEN ?4 ELSE started_at END,
                     ended_at = CASE WHEN ?5 THEN ?4 ELSE ended_at END,
                     exit_code = ?6
                 WHERE id = ?1 RETURNING {COLS}"
            ),
            params![run.id, m.to.as_str(), seq, m.at, m.to.terminal(), exit],
            run_row,
        )?;
        add_event(&tx, &event)?;
        tx.commit()?;
        Ok(Ok((run, event)))
    }

    /// Link the chant ledger entry a release run wrote, once: false when
    /// there's no such run or it has one already.
    pub fn link_chant_ledger(&self, run: &str, entry: &str) -> Result<bool> {
        Ok(self
            .c()
            .execute("UPDATE runs SET chant_ledger = ?2 WHERE id = ?1 AND chant_ledger IS NULL", params![run, entry])?
            == 1)
    }

    /// Record what a run left behind. `None`: there's no such run; a name
    /// the run already has is [`super::Taken`].
    pub fn add_run_artifact(
        &self,
        run: &str,
        name: &str,
        size: u64,
        digest: &str,
        bucket_key: &str,
        at: u64,
    ) -> Result<Option<Artifact>> {
        self.c()
            .query_row(
                &format!(
                    "INSERT INTO run_artifacts ({ARTIFACT_COLS})
                     SELECT id, ?2, ?3, ?4, ?5, ?6 FROM runs WHERE id = ?1 RETURNING {ARTIFACT_COLS}"
                ),
                params![run, name, size, digest, bucket_key, at],
                artifact_row,
            )
            .optional()
            .map_err(taken)
    }

    /// What a run left behind, by name.
    pub fn run_artifacts(&self, run: &str) -> Result<Vec<Artifact>> {
        self.c().query_map(
            &format!("SELECT {ARTIFACT_COLS} FROM run_artifacts WHERE run_id = ?1 ORDER BY name"),
            params![run],
            artifact_row,
        )
    }
}

/// What deleting an account takes from here, inside the platform's
/// `account_deleted` transaction, after [`super::projects::erase_account`]:
/// the runs of projects that went, with their events and artifacts' rows
/// (their files expire from the bucket, M213), and the account's name on
/// the runs that stay. Events keep the daemon that signed them: a
/// signature names its signer.
pub(crate) fn erase_account(c: &impl Sql, d: &Deleted) -> Result<()> {
    let gone = "SELECT id FROM runs WHERE project_id NOT IN (SELECT id FROM projects)";
    c.execute(&format!("DELETE FROM run_events WHERE run_id IN ({gone})"), params![])?;
    c.execute(&format!("DELETE FROM run_artifacts WHERE run_id IN ({gone})"), params![])?;
    c.execute("DELETE FROM runs WHERE project_id NOT IN (SELECT id FROM projects)", params![])?;
    c.execute(
        "UPDATE runs SET started_by = '', started_by_agent = NULL WHERE trigger_kind = 'manual' AND started_by = ?1",
        params![d.account],
    )?;
    Ok(())
}

#[cfg(test)]
#[path = "runs_tests.rs"]
mod tests;
