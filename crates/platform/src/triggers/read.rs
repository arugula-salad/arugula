//! Reading a project's runs (M216): their state, timeline and log, for the
//! CLI's `arugula runs list|show|logs` (a person, or a CLI device as its
//! account) and, through [`super::agent`], a machine's agents. Any member
//! reads a project's runs.
//!
//! | Route | Who |
//! |---|---|
//! | `GET /api/projects/{project}/runs?state=&limit=` | members |
//! | `GET /api/runs/{run}?since=&wait=` (with its timeline; `wait` holds the answer until it moves) | the run's project's members |
//! | `GET /api/runs/{run}/log?offset=&max=` (its durable log, [`crate::run_log`]) | the run's project's members |
//!
//! A run in a project someone isn't a member of is 404, as a run that
//! isn't there.
//!
//! **Waiting.** A read with `since` and `wait` holds its answer until the
//! run has a later event or has ended: it hears of each move this process
//! makes at once ([`runs::moves`]), and looks again every
//! [`LOOK_EVERY`] for those another platform machine made. At most
//! [`OPEN_WAITS`] are open at once per account and per machine (a 429
//! past that), and they're counted per hour apart from other reads
//! ([`WAITS`]), so a few long waits don't use up a machine's reads.

use std::{
    collections::HashMap,
    sync::{Arc, LazyLock, Mutex},
    time::Duration,
};

use arugula_control_common::{ApiError, err};
use arugula_control_wire::runs::{
    RUN_LOG_PAGE_MAX, RUN_WAIT_MAX, RunArtifactView, RunEventView, RunList, RunListQuery, RunLogPage, RunLogQuery,
    RunQuery, RunView,
};
use axum::{
    Json,
    extract::{Path, Query, State},
    http::StatusCode,
};

use crate::{
    Platform,
    auth::Person,
    db::runs::{self, Run},
    forge::links::membership,
};

type J<T> = Result<Json<T>, ApiError>;

/// How many runs a list answers without `limit`, and at most.
pub const LIST_DEFAULT: u32 = 20;
pub const LIST_MAX: u32 = 100;
/// How often a waiting read looks at the run again, for a move another
/// platform machine made (this one's wake it at once).
const LOOK_EVERY: Duration = Duration::from_secs(2);
/// The most waiting reads open at once, per account and per machine.
pub const OPEN_WAITS: usize = 8;
/// Waiting reads per hour, per account and per machine: about
/// [`OPEN_WAITS`] waits held all hour, [`RUN_WAIT_MAX`] seconds each.
pub(crate) const WAITS: (&str, usize) = ("run-wait", 2_000);

/// The waiting reads open now, by who holds them (`account:…`,
/// `daemon:…`).
static OPEN: LazyLock<Mutex<HashMap<String, usize>>> = LazyLock::new(Default::default);

/// Waiting reads held open ([`hold`]): let go when dropped.
pub(crate) struct Held(Vec<String>);

impl Drop for Held {
    fn drop(&mut self) {
        let mut open = OPEN.lock().unwrap();
        for k in &self.0 {
            if let Some(n) = open.get_mut(k) {
                *n -= 1;
                if *n == 0 {
                    open.remove(k);
                }
            }
        }
    }
}

/// One more waiting read for each of `who`, if none of them has
/// [`OPEN_WAITS`] open already.
pub(crate) fn hold(who: &[String]) -> Result<Held, ApiError> {
    let mut open = OPEN.lock().unwrap();
    if who.iter().any(|k| open.get(k).is_some_and(|n| *n >= OPEN_WAITS)) {
        return Err(err(
            StatusCode::TOO_MANY_REQUESTS,
            &format!("at most {OPEN_WAITS} waits on runs at once: try again when one ends"),
        ));
    }
    for k in who {
        *open.entry(k.clone()).or_default() += 1;
    }
    Ok(Held(who.to_vec()))
}

/// Whether `q` waits.
pub(crate) fn waits(q: &RunQuery) -> bool {
    q.since.is_some() && q.wait.is_some_and(|w| w > 0)
}

fn no_run() -> ApiError {
    err(StatusCode::NOT_FOUND, "no such run")
}

/// The pane a run runs in on its machine: what its latest `running` event
/// says.
pub(crate) fn pane_of(events: &[runs::Event]) -> Option<u64> {
    events
        .iter()
        .rev()
        .filter(|e| e.to == runs::State::Running)
        .find_map(|e| serde_json::from_str::<serde_json::Value>(&e.detail).ok().and_then(|d| d["pane"].as_u64()))
}

/// `run` as it's read: in a list (no timeline), or in full.
pub(crate) fn view(app: &Platform, run: &Run, full: bool) -> anyhow::Result<RunView> {
    let events = app.db.run_events(&run.id)?;
    let pane = pane_of(&events);
    let (events, artifacts) = if full {
        let events = events
            .into_iter()
            .map(|e| RunEventView {
                seq: e.seq,
                from: e.from.map(|s| s.as_str().to_owned()),
                to: e.to.as_str().to_owned(),
                at: e.at,
                detail: serde_json::from_str(&e.detail).unwrap_or_default(),
                daemon: e.daemon,
            })
            .collect();
        let artifacts = app
            .db
            .run_artifacts(&run.id)?
            .into_iter()
            .map(|a| RunArtifactView { name: a.name, size: a.size, digest: a.digest, created_at: a.created_at })
            .collect();
        (events, artifacts)
    } else {
        (vec![], vec![])
    };
    Ok(RunView {
        id: run.id.clone(),
        project: run.project.clone(),
        workspace: run.workspace.clone(),
        trigger: run.trigger.as_str().to_owned(),
        source: run.source.clone(),
        op: run.op.clone(),
        op_hash: run.op_hash.clone(),
        daemon: run.daemon.clone(),
        state: run.state.as_str().to_owned(),
        started_by: run.started_by.clone(),
        started_by_agent: run.started_by_agent.clone(),
        grant: run.grant.clone(),
        last_seq: run.last_seq,
        created_at: run.created_at,
        started_at: run.started_at,
        ended_at: run.ended_at,
        exit_code: run.exit_code,
        chant_ledger: run.chant_ledger.clone(),
        pane,
        events,
        artifacts,
    })
}

/// A project's runs, newest first, as [`PROJECT_RUNS`] answers them.
///
/// [`PROJECT_RUNS`]: arugula_control_wire::runs::PROJECT_RUNS
pub(crate) fn list_of(app: &Platform, project: &str, q: &RunListQuery) -> Result<RunList, ApiError> {
    let state = match q.state.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
        Some(s) => Some(runs::State::parse(s).ok_or_else(|| err(StatusCode::BAD_REQUEST, "no such run state"))?),
        None => None,
    };
    let trigger = match q.trigger.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
        Some(t) => Some(runs::TriggerKind::parse(t).ok_or_else(|| err(StatusCode::BAD_REQUEST, "no such trigger"))?),
        None => None,
    };
    let spec = q.spec.as_deref().map(str::trim).filter(|s| !s.is_empty());
    let limit = q.limit.unwrap_or(LIST_DEFAULT).clamp(1, LIST_MAX);
    let f = runs::RunFilter { state, trigger, spec };
    let runs = app.db.runs_of(project, &f, limit)?;
    let runs = runs.iter().map(|r| view(app, r, false)).collect::<anyhow::Result<_>>()?;
    Ok(RunList { project: project.to_owned(), runs })
}

/// `id`, once it has an event after `since` or has ended, waiting up to
/// `wait` seconds (at most [`RUN_WAIT_MAX`]) for that; as it is at the
/// end of the wait if neither. Without `since`, at once. The caller holds
/// the wait ([`hold`]).
pub(crate) async fn settled(app: &Platform, id: &str, q: &RunQuery) -> Result<Run, ApiError> {
    let wait = Duration::from_secs(q.wait.unwrap_or(0).min(RUN_WAIT_MAX));
    let until = tokio::time::Instant::now() + wait;
    let mut moves = runs::moves();
    loop {
        let run = app.db.run(id)?.ok_or_else(no_run)?;
        let moved = q.since.is_none_or(|s| run.last_seq > s) || run.state.terminal();
        if moved || tokio::time::Instant::now() >= until {
            return Ok(run);
        }
        let heard = async {
            loop {
                match moves.recv().await {
                    Ok(moved) if moved == id => return,
                    Ok(_) => {}
                    // Missed some: look.
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => return,
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => std::future::pending().await,
                }
            }
        };
        tokio::select! {
            () = heard => {}
            () = tokio::time::sleep(LOOK_EVERY) => {}
            () = tokio::time::sleep_until(until) => {}
        }
    }
}

/// A page of `run`'s durable log ([`crate::run_log::page`]), or why there
/// isn't one.
pub(crate) async fn log_of(app: &Platform, run: &Run, q: &RunLogQuery) -> Result<RunLogPage, ApiError> {
    let max = q.max.unwrap_or(RUN_LOG_PAGE_MAX).clamp(1, RUN_LOG_PAGE_MAX);
    let from = match (q.tail, q.offset) {
        (Some(true), Some(_)) => return Err(err(StatusCode::BAD_REQUEST, "an offset, or the tail: not both")),
        (Some(true), None) => crate::run_log::From::Tail,
        (_, offset) => crate::run_log::From::Offset(offset.unwrap_or(0)),
    };
    let ended = run.state.terminal();
    Ok(match crate::run_log::page(app, run, from, max).await? {
        Some(p) => RunLogPage {
            run: run.id.clone(),
            kept: true,
            end: ended && p.at_end,
            text: p.text,
            offset: p.offset,
            next_offset: p.next_offset,
            note: None,
        },
        None => {
            let pane = pane_of(&app.db.run_events(&run.id)?);
            let note = match pane {
                Some(p) => format!(
                    "the platform keeps no log of this run yet: its output is in pane %{p} on its machine ({}), \
                     which the project's people watch end to end",
                    run.daemon
                ),
                None => "the platform keeps no log of this run yet, and it hasn't started in a pane".to_owned(),
            };
            let offset = match from {
                crate::run_log::From::Offset(o) => o,
                crate::run_log::From::Tail => 0,
            };
            RunLogPage {
                run: run.id.clone(),
                kept: false,
                text: String::new(),
                offset,
                next_offset: offset,
                end: ended,
                note: Some(note),
            }
        }
    })
}

/// A run `account` reads: one of a project it's a member of.
async fn readable(app: &Platform, account: &str, id: &str) -> Result<Run, ApiError> {
    let run = app.db.run(id)?.ok_or_else(no_run)?;
    match membership(app, account, &run.project, false).await {
        Ok(_) => Ok(run),
        Err(e) if e.0 == StatusCode::NOT_FOUND => Err(no_run()),
        Err(e) => Err(e),
    }
}

/// `GET /api/projects/{project}/runs`.
pub async fn list(
    State(app): State<Arc<Platform>>,
    Person { account }: Person,
    Path(project): Path<String>,
    Query(q): Query<RunListQuery>,
) -> J<RunList> {
    let (project, _) = membership(&app, &account, &project, false).await?;
    Ok(Json(list_of(&app, &project, &q)?))
}

/// `GET /api/runs/{run}`.
pub async fn show(
    State(app): State<Arc<Platform>>,
    Person { account }: Person,
    Path(id): Path<String>,
    Query(q): Query<RunQuery>,
) -> J<RunView> {
    readable(&app, &account, &id).await?;
    let _held = if waits(&q) {
        app.limits.check_account(WAITS, &account)?;
        Some(hold(&[format!("account:{account}")])?)
    } else {
        None
    };
    let run = settled(&app, &id, &q).await?;
    Ok(Json(view(&app, &run, true)?))
}

/// The most pages of runs' logs a person reads in a minute (M213: each may
/// read the run-log bucket).
pub const LOG_READS: (&str, usize) = ("run-log-reads", 120);

/// `GET /api/runs/{run}/log`.
pub async fn log(
    State(app): State<Arc<Platform>>,
    Person { account }: Person,
    Path(id): Path<String>,
    Query(q): Query<RunLogQuery>,
) -> J<RunLogPage> {
    // A log followed a page every 2 s is 30 a minute.
    app.limits.check_account_per_minute(LOG_READS, &account)?;
    let run = readable(&app, &account, &id).await?;
    Ok(Json(log_of(&app, &run, &q).await?))
}
