//! What starts runs (M214, platform step 7): a project's run specs, their
//! schedules and their event rules, made by `0020_run_triggers`, and the
//! one way a trigger makes a run ([`start_in`]). Who may change them is
//! the routes' ([`crate::triggers`]).
//!
//! - **A spec** is a named operation a project runs on one of its hosted
//!   workspaces: a command (`argv`) or an instruction to an agent, and
//!   where. Its run's S37 operation (`arugula op v1`) is made from it when
//!   the run starts, naming the workspace's machine as it is then.
//! - **A schedule** fires its spec at each tick of a cron expression. Its
//!   job on `platform_jobs` (kind `schedule`, the schedule's id) does it
//!   ([`crate::triggers::schedule`]): under the job's lease it wins the
//!   tick (`next_at` still the tick it read) and makes the run in the same
//!   transaction ([`fire`]). A tick missed while the platform was down
//!   fires once, and the next is the first after now.
//! - **A rule** fires its spec when a linked repo's event matches it
//!   ([`crate::triggers::events`]).
//! - **Once:** a run a trigger makes carries a key (`trigger_key`, unique:
//!   `schedule:<id>:<tick>`, `rule:<id>:<event>`), so two machines, a
//!   restart or a redelivery make one run, and its `run` job is queued in
//!   the same transaction ([`super::runs::dispatch`]).
//! - **Untrusted text:** a run's `trigger_source` holds what an event's
//!   payload said (branches, a check's name, its author's standing), which
//!   whoever caused the event chose. It's for showing, quoted, and never
//!   reaches a run's environment, command or an agent's prompt unquoted.

use anyhow::Result;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use super::{
    Db,
    jobs::{self, JobKind},
    runs::{self, NewRun, Run, TriggerKind},
    sql::{Optional as _, Row, Sql, params},
    taken,
};
use crate::Deleted;

/// A new id: `prefix`, `_` and 32 hex characters.
pub fn new_id(prefix: &str) -> String {
    format!("{prefix}_{}", hex::encode(arugula_e2e::random::<16>()))
}

/// The most arguments a command has, and bytes in all of them.
pub const MAX_ARGS: usize = 256;
pub const MAX_ARGV_BYTES: usize = 16 * 1024;
/// The most bytes in an agent's instruction.
pub const MAX_INSTRUCTION: usize = 16 * 1024;

/// What a run does: a spec's operation, or one asked for by hand. A
/// command (`argv`) or an instruction to an agent (from recipe `agent`, or
/// the default), in `directory` or the workspace's home.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Operation {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub argv: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub instruction: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub directory: Option<String>,
}

fn plain(s: &str) -> bool {
    !s.contains(['\0', '\n', '\r'])
}

impl Operation {
    /// S37's kind: `run` for a command, `agent` for an instruction.
    pub fn kind(&self) -> &'static str {
        if self.instruction.is_some() { "agent" } else { "run" }
    }

    /// Why it can't run, if it can't.
    pub fn check(&self) -> Result<(), String> {
        match (&self.instruction, self.argv.is_empty()) {
            (None, true) => return Err("a command or an instruction".into()),
            (Some(_), false) => return Err("a command or an instruction, not both".into()),
            (Some(i), true) if i.trim().is_empty() => return Err("the instruction is empty".into()),
            (Some(i), true) if i.len() > MAX_INSTRUCTION => {
                return Err(format!("an instruction is at most {MAX_INSTRUCTION} bytes"));
            }
            _ => {}
        }
        if self.argv.len() > MAX_ARGS || self.argv.iter().map(String::len).sum::<usize>() > MAX_ARGV_BYTES {
            return Err(format!("a command is at most {MAX_ARGS} arguments and {MAX_ARGV_BYTES} bytes"));
        }
        if self.argv.first().is_some_and(|a| a.is_empty()) || self.argv.iter().any(|a| a.contains('\0')) {
            return Err("a command's program is named, and no argument has a NUL".into());
        }
        if let Some(a) = &self.agent {
            if self.instruction.is_none() {
                return Err("an agent recipe goes with an instruction".into());
            }
            if !crate::hosted::good_name(a) {
                return Err("an agent recipe's name: letters, digits, '-', '_' and '.'".into());
            }
        }
        if let Some(d) = &self.directory
            && (!d.starts_with('/') || d.len() > 1024 || !plain(d))
        {
            return Err("the directory is an absolute path".into());
        }
        Ok(())
    }

    /// The S37 operation (`spikes/s37-team-permissions/README.md`, q3) that
    /// runs it on `daemon`: what the run's hash, its grant and any
    /// approval name, and what the daemon runs.
    pub fn encode(&self, daemon: &str) -> String {
        arugula_e2e::run::Op {
            argv: self.argv.clone(),
            instruction: self.instruction.clone(),
            agent: self.agent.clone(),
            directory: self.directory.clone(),
            deploy: None,
        }
        .encode(daemon)
    }

    fn argv_json(&self) -> String {
        serde_json::to_string(&self.argv).unwrap_or_else(|_| "[]".into())
    }
}

/// A named operation a project runs on one of its hosted workspaces.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RunSpec {
    pub id: String,
    pub project: String,
    pub name: String,
    /// The hosted workspace (`hw_…`) it runs on.
    pub workspace: String,
    pub kind: String,
    #[serde(flatten)]
    pub op: Operation,
    pub created_by: Option<String>,
    pub created_at: u64,
    pub updated_by: Option<String>,
    pub updated_at: u64,
}

const SPEC_COLS: [&str; 13] = [
    "id",
    "project_id",
    "name",
    "workspace_id",
    "kind",
    "argv",
    "instruction",
    "agent",
    "directory",
    "created_by",
    "created_at",
    "updated_by",
    "updated_at",
];

/// `run_specs`' columns, read by [`spec_row`] from `at`, each named under
/// `alias`.
fn spec_cols(alias: &str) -> String {
    SPEC_COLS.iter().map(|c| format!("{alias}.{c}")).collect::<Vec<_>>().join(", ")
}

fn spec_row(r: &Row, at: usize) -> Result<RunSpec> {
    let argv: String = r.get(at + 5)?;
    Ok(RunSpec {
        id: r.get(at)?,
        project: r.get(at + 1)?,
        name: r.get(at + 2)?,
        workspace: r.get(at + 3)?,
        kind: r.get(at + 4)?,
        op: Operation {
            argv: serde_json::from_str(&argv)?,
            instruction: r.get(at + 6)?,
            agent: r.get(at + 7)?,
            directory: r.get(at + 8)?,
        },
        created_by: r.get(at + 9)?,
        created_at: r.get(at + 10)?,
        updated_by: r.get(at + 11)?,
        updated_at: r.get(at + 12)?,
    })
}

/// A spec's schedule: a cron expression in a time zone.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Schedule {
    pub id: String,
    pub spec: String,
    pub cron: String,
    /// IANA's name for it.
    pub timezone: String,
    pub enabled: bool,
    /// Its next tick (ms).
    pub next_at: u64,
    /// The tick it last fired for.
    pub last_fired_at: Option<u64>,
    /// The run that made.
    pub last_run: Option<String>,
    /// Why its last tick made no run, or why it stopped.
    pub last_error: Option<String>,
    /// Its job ended `failed`: it fires no more until it's changed.
    pub failed: bool,
    pub created_by: Option<String>,
    pub created_at: u64,
    pub updated_at: u64,
}

/// A schedule's columns, and its job's state beside them: whether it ended
/// `failed`, and why.
const SCHEDULE_READ: &str = "SELECT s.id, s.spec_id, s.cron, s.timezone, s.enabled, s.next_at, s.last_fired_at,
         s.last_run_id, COALESCE(CASE WHEN j.state = 'failed' THEN j.error END, s.last_error),
         s.created_by, s.created_at, s.updated_at, CASE WHEN j.state = 'failed' THEN TRUE ELSE FALSE END
     FROM run_schedules s LEFT JOIN platform_jobs j ON j.id = s.id";

fn schedule_row(r: &Row) -> Result<Schedule> {
    Ok(Schedule {
        failed: r.get(12)?,
        id: r.get(0)?,
        spec: r.get(1)?,
        cron: r.get(2)?,
        timezone: r.get(3)?,
        enabled: r.get(4)?,
        next_at: r.get(5)?,
        last_fired_at: r.get(6)?,
        last_run: r.get(7)?,
        last_error: r.get(8)?,
        created_by: r.get(9)?,
        created_at: r.get(10)?,
        updated_at: r.get(11)?,
    })
}

/// Schedule `id` as it is now, in `c`.
fn read_schedule(c: &impl Sql, id: &str) -> Result<Option<Schedule>> {
    c.query_row(&format!("{SCHEDULE_READ} WHERE s.id = ?1"), params![id], schedule_row).optional()
}

/// The most runs a rule starts in an hour, unless it says otherwise.
pub const RULE_PER_HOUR: u32 = 20;

/// The events of a linked repo a rule can start a spec on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RuleEvent {
    /// A pull request opened.
    PrOpened,
    /// A check run or a commit status that failed.
    CheckFailed,
    /// A push to a branch.
    Push,
    /// A review, or a review's inline comment, on a PR (M260): no rule
    /// names it (it isn't in [`RuleEvent::ALL`], and isn't read from a
    /// request), it goes back to the PR's session
    /// (`sessions::feedback`).
    #[serde(skip)]
    Review,
}

impl RuleEvent {
    pub const ALL: [RuleEvent; 3] = [RuleEvent::PrOpened, RuleEvent::CheckFailed, RuleEvent::Push];

    pub fn as_str(self) -> &'static str {
        match self {
            RuleEvent::PrOpened => "pr_opened",
            RuleEvent::CheckFailed => "check_failed",
            RuleEvent::Push => "push",
            RuleEvent::Review => "review",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|e| e.as_str() == s)
    }
}

/// A spec's event rule.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Rule {
    pub id: String,
    pub spec: String,
    pub project: String,
    pub event: RuleEvent,
    /// `owner/name`; any repo linked to the project when `None`.
    pub repo: Option<String>,
    /// A branch name (`*` for any run of characters); any when `None`.
    pub branch: Option<String>,
    pub enabled: bool,
    /// It also fires on events from forks and from people who aren't the
    /// repo's owners, members or collaborators. Off, they're refused.
    pub outsiders: bool,
    /// The most runs it starts in an hour.
    pub max_per_hour: u32,
    pub created_by: Option<String>,
    pub created_at: u64,
    pub updated_at: u64,
}

const RULE_COLS: [&str; 12] = [
    "id",
    "spec_id",
    "project_id",
    "event",
    "repo",
    "branch",
    "enabled",
    "outsiders",
    "max_per_hour",
    "created_by",
    "created_at",
    "updated_at",
];

fn rule_cols(alias: &str) -> String {
    RULE_COLS.iter().map(|c| format!("{alias}.{c}")).collect::<Vec<_>>().join(", ")
}

fn rule_row(r: &Row) -> Result<Rule> {
    let event: String = r.get(3)?;
    Ok(Rule {
        id: r.get(0)?,
        spec: r.get(1)?,
        project: r.get(2)?,
        event: RuleEvent::parse(&event).ok_or_else(|| anyhow::anyhow!("a rule on {event:?}"))?,
        repo: r.get(4)?,
        branch: r.get(5)?,
        enabled: r.get(6)?,
        outsiders: r.get(7)?,
        max_per_hour: r.get(8)?,
        created_by: r.get(9)?,
        created_at: r.get(10)?,
        updated_at: r.get(11)?,
    })
}

/// A run a trigger asks for.
pub struct Start<'a> {
    pub project: &'a str,
    /// The hosted workspace (`hw_…`) it runs on.
    pub workspace: &'a str,
    pub op: &'a Operation,
    pub trigger: TriggerKind,
    /// Which spec, schedule, rule or event: a JSON object.
    pub source: &'a Value,
    /// The account that started it by hand, or the schedule's or rule's id.
    pub started_by: &'a str,
    /// The machine whose agent started it by hand (M216), for its owner
    /// (`started_by`).
    pub started_by_agent: Option<&'a str>,
    /// What the trigger made it for, so it makes one ([`super::runs::create_run_in`]).
    pub key: Option<&'a str>,
}

/// What came of a [`Start`].
#[derive(Debug, Clone, PartialEq)]
pub enum Started {
    /// Made, `pending`, and its `run` job queued.
    Made(Run),
    /// Its key's run was there already: this one.
    Already(Run),
    /// The workspace isn't the project's, or is deleted.
    NoWorkspace,
    /// The workspace has no machine yet (its box hasn't enrolled).
    NoMachine,
    NoProject,
    /// An event from a fork, or from someone who isn't the repo's owner,
    /// member or collaborator, on a rule that doesn't take them.
    Outsider,
    /// The rule started its most runs this hour.
    Capped,
    /// The trigger's maker can't start runs on the project now (not one of
    /// its owners or editors, or not on the hosted beta's list).
    Refused(&'static str),
}

impl Started {
    /// Why no run, when there's none.
    pub fn why_not(&self) -> Option<&'static str> {
        match self {
            Started::Made(_) | Started::Already(_) => None,
            Started::NoWorkspace => Some("its cloud workspace is gone"),
            Started::NoMachine => Some("its cloud workspace has no machine yet"),
            Started::NoProject => Some("its project is gone"),
            Started::Outsider => Some("the event came from a fork or from outside the repo"),
            Started::Capped => Some("its rule started its most runs this hour"),
            Started::Refused(why) => Some(why),
        }
    }
}

/// Make the run `s` asks for, `pending`, and queue its `run` job, in the
/// caller's transaction. The operation names the workspace's machine as
/// it is now. A key's run that's there already is answered, and its job
/// asked for again (which queues nothing new).
///
/// It runs under the project's live standing grant (M211) when there is
/// one, named in `grant_id`, so it may start with every device asleep;
/// without one it stays `pending` for an approval.
pub fn start_in(c: &impl Sql, s: &Start, now: u64) -> Result<Started> {
    let machine = c
        .query_row(
            "SELECT machine FROM hosted_workspaces
             WHERE id = ?1 AND project_id = ?2 AND deleted_at IS NULL AND purpose = 'workspace'",
            params![s.workspace, s.project],
            |r| r.get::<Option<String>>(0),
        )
        .optional()?;
    let Some(machine) = machine else { return Ok(Started::NoWorkspace) };
    let Some(machine) = machine.filter(|m| !m.is_empty()) else { return Ok(Started::NoMachine) };
    let op = s.op.encode(&machine);
    let grant = super::run_grants::live_in(c, s.project, now)?.map(|g| g.grant.id);
    let new = NewRun {
        project: s.project,
        workspace: Some(s.workspace),
        trigger: s.trigger,
        source: s.source,
        op: &op,
        started_by: s.started_by,
        started_by_agent: s.started_by_agent,
        grant: grant.as_deref(),
    };
    let Some((run, made)) = runs::create_run_in(c, &new, s.key, now)? else { return Ok(Started::NoProject) };
    runs::dispatch(c, &run.id, now)?;
    Ok(if made { Started::Made(run) } else { Started::Already(run) })
}

/// What a schedule's tick came to ([`fire`]).
#[derive(Debug, Clone, PartialEq)]
pub enum Fired {
    /// It made this run (or found the one its tick made).
    Run(Box<Run>),
    /// The schedule changed since it was read (another step fired it, or
    /// someone changed or stopped it): nothing done.
    Moved,
    /// The tick passed with no run, for this reason (kept as its
    /// `last_error`).
    Skipped(&'static str),
}

/// Fire `s` for its tick (`s.next_at`), its next tick being `next_at`, in
/// the caller's transaction (a step's, under its job's lease): win the
/// tick, then make its run and queue its job. A tick is won only while the
/// schedule is enabled and still due at the tick read, so a tick fires
/// once however many steps read it. `refused`: its maker may no longer
/// start runs, so the tick passes with no run, saying why.
pub fn fire(c: &impl Sql, s: &Schedule, next_at: u64, refused: Option<&'static str>, now: u64) -> Result<Fired> {
    let won = c.execute(
        "UPDATE run_schedules SET next_at = ?3, last_fired_at = ?2, updated_at = ?4
         WHERE id = ?1 AND next_at = ?2 AND enabled",
        params![s.id, s.next_at, next_at, now],
    )?;
    if won != 1 {
        return Ok(Fired::Moved);
    }
    if let Some(why) = refused {
        c.execute("UPDATE run_schedules SET last_error = ?2 WHERE id = ?1", params![s.id, why])?;
        return Ok(Fired::Skipped(why));
    }
    let spec = c
        .query_row(&format!("SELECT {} FROM run_specs s WHERE s.id = ?1", spec_cols("s")), params![s.spec], |r| {
            spec_row(r, 0)
        })
        .optional()?;
    let started = match &spec {
        None => Started::NoProject,
        Some(spec) => {
            let key = format!("schedule:{}:{}", s.id, s.next_at);
            let source = json!({ "schedule": s.id, "spec": spec.id, "tick": s.next_at });
            let start = Start {
                project: &spec.project,
                workspace: &spec.workspace,
                op: &spec.op,
                trigger: TriggerKind::Schedule,
                source: &source,
                started_by: &s.id,
                started_by_agent: None,
                key: Some(&key),
            };
            start_in(c, &start, now)?
        }
    };
    match started {
        Started::Made(run) | Started::Already(run) => {
            c.execute(
                "UPDATE run_schedules SET last_run_id = ?2, last_error = NULL WHERE id = ?1",
                params![s.id, run.id],
            )?;
            Ok(Fired::Run(Box::new(run)))
        }
        other => {
            let why = other.why_not().unwrap_or_default();
            c.execute("UPDATE run_schedules SET last_error = ?2 WHERE id = ?1", params![s.id, why])?;
            Ok(Fired::Skipped(why))
        }
    }
}

/// What a schedule's job carries.
fn schedule_job(id: &str) -> Value {
    json!({ "schedule": id })
}

impl Db {
    /// [`start_in`] in a transaction of its own.
    pub fn start_run(&self, s: &Start, now: u64) -> Result<Started> {
        let mut c = self.c();
        let tx = c.transaction()?;
        let started = start_in(&tx, s, now)?;
        tx.commit()?;
        Ok(started)
    }

    /// Keep a new spec. [`Taken`](super::Taken): the project has one by that name.
    pub fn add_run_spec(
        &self,
        project: &str,
        name: &str,
        workspace: &str,
        op: &Operation,
        by: &str,
        now: u64,
    ) -> Result<RunSpec> {
        let cols = SPEC_COLS.join(", ");
        self.c()
            .query_row(
                &format!(
                    "INSERT INTO run_specs ({cols}) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?10, ?11)
                     RETURNING {}",
                    spec_cols("run_specs")
                ),
                params![
                    new_id("rsp"),
                    project,
                    name,
                    workspace,
                    op.kind(),
                    op.argv_json(),
                    op.instruction,
                    op.agent,
                    op.directory,
                    by,
                    now
                ],
                |r| spec_row(r, 0),
            )
            .map_err(taken)
    }

    /// A project's specs, by name.
    pub fn run_specs(&self, project: &str) -> Result<Vec<RunSpec>> {
        self.c().query_map(
            &format!("SELECT {} FROM run_specs s WHERE s.project_id = ?1 ORDER BY s.name, s.id", spec_cols("s")),
            params![project],
            |r| spec_row(r, 0),
        )
    }

    /// A spec by its id, in whichever project.
    pub fn run_spec_by_id(&self, id: &str) -> Result<Option<RunSpec>> {
        self.c()
            .query_row(&format!("SELECT {} FROM run_specs s WHERE s.id = ?1", spec_cols("s")), params![id], |r| {
                spec_row(r, 0)
            })
            .optional()
    }

    /// A project's spec, by its id or its name.
    pub fn run_spec(&self, project: &str, id_or_name: &str) -> Result<Option<RunSpec>> {
        self.c()
            .query_row(
                &format!(
                    "SELECT {} FROM run_specs s WHERE s.project_id = ?1 AND (s.id = ?2 OR s.name = ?2)
                     ORDER BY CASE WHEN s.id = ?2 THEN 0 ELSE 1 END LIMIT 1",
                    spec_cols("s")
                ),
                params![project, id_or_name],
                |r| spec_row(r, 0),
            )
            .optional()
    }

    /// Change a spec: its name, workspace or operation, each when given.
    /// `None`: there's no such spec. [`Taken`](super::Taken): the project has another by
    /// that name.
    pub fn update_run_spec(
        &self,
        id: &str,
        name: Option<&str>,
        workspace: Option<&str>,
        op: Option<&Operation>,
        by: &str,
        now: u64,
    ) -> Result<Option<RunSpec>> {
        self.c()
            .query_row(
                &format!(
                    "UPDATE run_specs SET name = COALESCE(?2, name), workspace_id = COALESCE(?3, workspace_id),
                         kind = CASE WHEN ?4 THEN ?5 ELSE kind END,
                         argv = CASE WHEN ?4 THEN ?6 ELSE argv END,
                         instruction = CASE WHEN ?4 THEN ?7 ELSE instruction END,
                         agent = CASE WHEN ?4 THEN ?8 ELSE agent END,
                         directory = CASE WHEN ?4 THEN ?9 ELSE directory END,
                         updated_by = ?10, updated_at = ?11
                     WHERE id = ?1 RETURNING {}",
                    spec_cols("run_specs")
                ),
                params![
                    id,
                    name,
                    workspace,
                    op.is_some(),
                    op.map(Operation::kind),
                    op.map(Operation::argv_json),
                    op.and_then(|o| o.instruction.clone()),
                    op.and_then(|o| o.agent.clone()),
                    op.and_then(|o| o.directory.clone()),
                    by,
                    now
                ],
                |r| spec_row(r, 0),
            )
            .optional()
            .map_err(taken)
    }

    /// Delete a spec, with its schedules (and their jobs) and its rules.
    pub fn delete_run_spec(&self, id: &str) -> Result<bool> {
        let mut c = self.c();
        let tx = c.transaction()?;
        tx.execute(
            "DELETE FROM platform_jobs WHERE kind = 'schedule' AND id IN (SELECT id FROM run_schedules WHERE spec_id = ?1)",
            params![id],
        )?;
        tx.execute("DELETE FROM run_schedules WHERE spec_id = ?1", params![id])?;
        tx.execute("DELETE FROM run_rules WHERE spec_id = ?1", params![id])?;
        let gone = tx.execute("DELETE FROM run_specs WHERE id = ?1", params![id])? == 1;
        tx.commit()?;
        Ok(gone)
    }

    /// Keep a new schedule for `spec`, first due at `next_at`, and queue
    /// its job.
    pub fn add_run_schedule(
        &self,
        spec: &RunSpec,
        cron: &str,
        timezone: &str,
        next_at: u64,
        by: &str,
        now: u64,
    ) -> Result<Schedule> {
        let id = new_id("rsc");
        let mut c = self.c();
        let tx = c.transaction()?;
        tx.execute(
            "INSERT INTO run_schedules (id, spec_id, cron, timezone, enabled, next_at, created_by, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, TRUE, ?5, ?6, ?7, ?7)",
            params![id, spec.id, cron, timezone, next_at, by, now],
        )?;
        jobs::requeue(&tx, &id, JobKind::Schedule, &schedule_job(&id), next_at, now)?;
        let s = read_schedule(&tx, &id)?.ok_or_else(|| anyhow::anyhow!("the schedule just kept"))?;
        tx.commit()?;
        Ok(s)
    }

    pub fn run_schedule(&self, id: &str) -> Result<Option<Schedule>> {
        read_schedule(&self.c(), id)
    }

    /// A spec's schedules, oldest first.
    pub fn spec_schedules(&self, spec: &str) -> Result<Vec<Schedule>> {
        self.c().query_map(
            &format!("{SCHEDULE_READ} WHERE s.spec_id = ?1 ORDER BY s.created_at, s.id"),
            params![spec],
            schedule_row,
        )
    }

    /// Change a schedule: its expression and zone, whether it's on, and so
    /// its next tick, `next_at`. On, its job is due then (queued again if
    /// it had ended, its error cleared); a step holding the job as this
    /// lands hands it back due at once and reads the change
    /// ([`jobs::requeue`]). A tick a step is firing as this lands doesn't
    /// fire.
    pub fn set_run_schedule(
        &self,
        id: &str,
        cron: &str,
        timezone: &str,
        enabled: bool,
        next_at: u64,
        now: u64,
    ) -> Result<Option<Schedule>> {
        let mut c = self.c();
        let tx = c.transaction()?;
        let changed = tx.execute(
            "UPDATE run_schedules SET cron = ?2, timezone = ?3, enabled = ?4, next_at = ?5, updated_at = ?6,
                 last_error = CASE WHEN ?4 THEN NULL ELSE last_error END
             WHERE id = ?1",
            params![id, cron, timezone, enabled, next_at, now],
        )?;
        if changed != 1 {
            return Ok(None);
        }
        if enabled {
            jobs::requeue(&tx, id, JobKind::Schedule, &schedule_job(id), next_at, now)?;
        }
        let s = read_schedule(&tx, id)?;
        tx.commit()?;
        Ok(s)
    }

    /// Why a schedule's job ended `failed`, kept on the schedule.
    pub fn schedule_stopped(&self, id: &str, why: &str) -> Result<()> {
        self.c().execute("UPDATE run_schedules SET last_error = ?2 WHERE id = ?1", params![id, why])?;
        Ok(())
    }

    /// Delete a schedule and its job.
    pub fn delete_run_schedule(&self, id: &str) -> Result<bool> {
        let mut c = self.c();
        let tx = c.transaction()?;
        tx.execute("DELETE FROM platform_jobs WHERE kind = 'schedule' AND id = ?1", params![id])?;
        let gone = tx.execute("DELETE FROM run_schedules WHERE id = ?1", params![id])? == 1;
        tx.commit()?;
        Ok(gone)
    }

    /// Keep a new rule for `spec`: on `event`, from `repo` and on `branch`
    /// (any when `None`), taking events from outside the repo when
    /// `outsiders`, at most `max_per_hour` runs an hour.
    #[allow(clippy::too_many_arguments)]
    pub fn add_run_rule(
        &self,
        spec: &RunSpec,
        event: RuleEvent,
        repo: Option<&str>,
        branch: Option<&str>,
        outsiders: bool,
        max_per_hour: u32,
        by: &str,
        now: u64,
    ) -> Result<Rule> {
        self.c().query_row(
            &format!(
                "INSERT INTO run_rules ({}) VALUES (?1, ?2, ?3, ?4, ?5, ?6, TRUE, ?7, ?8, ?9, ?10, ?10) RETURNING {}",
                RULE_COLS.join(", "),
                RULE_COLS.join(", ")
            ),
            params![
                new_id("rru"),
                spec.id,
                spec.project,
                event.as_str(),
                repo,
                branch,
                outsiders,
                max_per_hour,
                by,
                now
            ],
            rule_row,
        )
    }

    pub fn run_rule(&self, id: &str) -> Result<Option<Rule>> {
        self.c()
            .query_row(&format!("SELECT {} FROM run_rules r WHERE r.id = ?1", rule_cols("r")), params![id], rule_row)
            .optional()
    }

    /// A spec's rules, oldest first.
    pub fn spec_rules(&self, spec: &str) -> Result<Vec<Rule>> {
        self.c().query_map(
            &format!("SELECT {} FROM run_rules r WHERE r.spec_id = ?1 ORDER BY r.created_at, r.id", rule_cols("r")),
            params![spec],
            rule_row,
        )
    }

    /// Change a rule: whether it's on, takes outsiders' events, and its
    /// hourly cap, each when given.
    pub fn set_run_rule(
        &self,
        id: &str,
        enabled: Option<bool>,
        outsiders: Option<bool>,
        max_per_hour: Option<u32>,
        now: u64,
    ) -> Result<Option<Rule>> {
        self.c()
            .query_row(
                &format!(
                    "UPDATE run_rules SET enabled = COALESCE(?2, enabled), outsiders = COALESCE(?3, outsiders),
                         max_per_hour = COALESCE(?4, max_per_hour), updated_at = ?5
                     WHERE id = ?1 RETURNING {}",
                    RULE_COLS.join(", ")
                ),
                params![id, enabled, outsiders, max_per_hour, now],
                rule_row,
            )
            .optional()
    }

    /// Start what `rule` asks for (`s`, keyed), within its hourly cap: in
    /// one transaction that holds the rule's row, so two machines can't
    /// both pass the cap. A key whose run is there already is answered as
    /// it is, cap or not; a rule turned off since it was read starts
    /// nothing.
    pub fn fire_rule(&self, rule: &Rule, s: &Start, now: u64) -> Result<Started> {
        let mut c = self.c();
        let tx = c.immediate()?;
        let held = tx
            .query_row(
                &format!("SELECT enabled, max_per_hour FROM run_rules WHERE id = ?1{}", tx.for_update()),
                params![rule.id],
                |r| Ok((r.get::<bool>(0)?, r.get::<u64>(1)?)),
            )
            .optional()?;
        let Some((true, cap)) = held else { return Ok(Started::Refused("the rule is off or gone")) };
        let there = match s.key {
            Some(k) => tx
                .query_row("SELECT id FROM runs WHERE trigger_key = ?1", params![k], |r| r.get::<String>(0))
                .optional()?
                .is_some(),
            None => false,
        };
        if !there {
            let hour: u64 = tx.query_row(
                "SELECT count(*) FROM runs WHERE started_by = ?1 AND trigger_kind = 'event' AND created_at > ?2",
                params![rule.id, now.saturating_sub(3_600_000)],
                |r| r.get(0),
            )?;
            if hour >= cap {
                return Ok(Started::Capped);
            }
        }
        let started = start_in(&tx, s, now)?;
        tx.commit()?;
        Ok(started)
    }

    pub fn delete_run_rule(&self, id: &str) -> Result<bool> {
        Ok(self.c().execute("DELETE FROM run_rules WHERE id = ?1", params![id])? == 1)
    }

    /// The rules on `event` that a repo's event may fire, with their specs:
    /// on, in the project the repo (GitHub's id) is linked to, and naming
    /// it or no repo. Their branches are the caller's to match.
    pub fn rules_for(&self, repo_id: u64, event: RuleEvent) -> Result<Vec<(Rule, RunSpec)>> {
        self.c().query_map(
            &format!(
                "SELECT {}, {} FROM run_rules r
                 JOIN run_specs s ON s.id = r.spec_id
                 JOIN linked_repos l ON l.project_id = r.project_id AND l.repo_id = ?1 AND l.unlinked_at IS NULL
                 WHERE r.event = ?2 AND r.enabled AND (r.repo IS NULL OR lower(r.repo) = lower(l.full_name))
                 ORDER BY r.created_at, r.id",
                rule_cols("r"),
                spec_cols("s")
            ),
            params![repo_id, event.as_str()],
            |r| Ok((rule_row(r)?, spec_row(r, RULE_COLS.len())?)),
        )
    }
}

/// What deleting an account takes from here, inside the platform's
/// `account_deleted` transaction, after [`super::projects::erase_account`]:
/// the specs of projects that went, with their schedules (and their jobs)
/// and rules, the `run` jobs of their runs, and the account's name on what
/// stays.
pub(crate) fn erase_account(c: &impl Sql, d: &Deleted) -> Result<()> {
    c.execute("DELETE FROM run_specs WHERE project_id NOT IN (SELECT id FROM projects)", params![])?;
    c.execute("DELETE FROM run_schedules WHERE spec_id NOT IN (SELECT id FROM run_specs)", params![])?;
    c.execute("DELETE FROM run_rules WHERE spec_id NOT IN (SELECT id FROM run_specs)", params![])?;
    c.execute(
        "DELETE FROM platform_jobs WHERE kind = 'schedule' AND id NOT IN (SELECT id FROM run_schedules)",
        params![],
    )?;
    // The `run` jobs of runs that went (`super::runs::erase_account`, first).
    c.execute("DELETE FROM platform_jobs WHERE kind = 'run' AND id NOT IN (SELECT id FROM runs)", params![])?;
    let a = &d.account;
    c.execute("UPDATE run_specs SET created_by = NULL WHERE created_by = ?1", params![a])?;
    c.execute("UPDATE run_specs SET updated_by = NULL WHERE updated_by = ?1", params![a])?;
    c.execute("UPDATE run_schedules SET created_by = NULL WHERE created_by = ?1", params![a])?;
    c.execute("UPDATE run_rules SET created_by = NULL WHERE created_by = ?1", params![a])?;
    Ok(())
}

#[cfg(test)]
#[path = "triggers_tests.rs"]
pub(crate) mod tests;
