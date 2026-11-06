//! A chant→Fly deploy's plan, run before anyone approves it (#1985,
//! aru-126): the person approves the plan chant made, not a description of
//! one.
//!
//! - **Asked for with its approval.** The first ask for a chant→Fly
//!   deploy's approval (not a roll back's, which goes through no gate)
//!   records nothing yet: it keeps who asked and queues this job
//!   ([`crate::db::Db::ask_deploy_plan`]).
//! - **A box of its own**, as an apply has (`deploys::apply`), named
//!   `<deploy>-plan`. Its run is the driver's plan of the deploy (`kind
//!   deploy`, `plan`), approved by the platform: no person has anything to
//!   approve yet, and the run gets none of the environment's secrets. Its
//!   credentials are two GitHub tokens minted for it ([`secrets`]): one
//!   that reads, to fetch the commit before anything of it runs, and one
//!   that writes, to push chant's ledger, which the box's daemon asks for
//!   only once nothing the plan ran is left and it checked what chant
//!   added, and uses itself: the run user never holds it. chant's op runs
//!   with no token at all: an op that needs Fly, or GitHub, before its gate
//!   fails here.
//! - **What it found.** The run ends with the gate chant's run stopped at
//!   and its pending fact (`chant_gate` in its result). Checked here
//!   ([`gate_of`]), it ends the plan, and the asker's `requested` event
//!   carries it (`{"chant": gate}`), in the environment's hash-chained log;
//!   then the owners are asked, as for any deploy. The approval they make
//!   is chant's statement for that gate (`arugula_e2e::deploy::chant`).
//!   A plan that didn't get there ends saying why, and the deploy stays
//!   planned: plan another.
//! - **The box goes after,** whatever happened, as an apply's does.

use std::sync::Arc;

use anyhow::{Context as _, Result};
use arugula_control_wire::work::{DeployPlanWork, PlanStep, Secret, SecretsGiven};
use arugula_e2e::{
    deploy::chant::Gate,
    drivers::{CHANT_GATES, CHANT_OP},
    run::{DeployApply, Op},
};
use futures_util::future::BoxFuture;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use tracing::{info, warn};

use super::apply::{BOX_LIFE_MS, BOX_MS, BOX_UP_MS, FOLLOW_MS, logs_pending_of, reported_result, why_it_ended};
use crate::{
    Platform,
    db::{
        Db,
        deploy_plans::{DeployPlan, NewPlanRun, box_name, deploy_of_job, plan_approval},
        deploys::{Actor, EventKind, State},
        jobs::{JobKind, LeasedJob},
        runs::{Move, State as RunState},
    },
    drivers::{self, Mint},
    jobs::{Clock, Handler, Step},
    work::HostedMachine,
};

/// The secret a plan run gets: a GitHub token, minted for it.
pub const GITHUB_TOKEN: &str = "GITHUB_TOKEN";

/// The `deploy_plan` kind's handler.
pub struct Plan {
    pub app: Arc<Platform>,
}

impl Handler for Plan {
    fn kind(&self) -> JobKind {
        JobKind::DeployPlan
    }

    fn step<'a>(&'a self, db: &'a Db, job: &'a mut LeasedJob, clock: &'a Clock) -> BoxFuture<'a, Result<Step>> {
        Box::pin(step(&self.app, db, job, clock.now()))
    }

    /// Never given up on, as a deploy's apply: its job is what deletes its
    /// box. The reaper is the backstop.
    fn max_attempts(&self) -> u32 {
        u32::MAX
    }
}

/// What a plan's job keeps: its box, once it's chosen.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Checkpoint {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub r#box: Option<String>,
    #[serde(default)]
    pub made_at: u64,
}

fn checkpoint_of(job: &LeasedJob) -> Checkpoint {
    job.checkpoint.as_ref().and_then(|c| serde_json::from_value(c.clone()).ok()).unwrap_or_default()
}

fn save(db: &Db, job: &mut LeasedJob, cp: &Checkpoint, now: u64) -> Result<()> {
    job.checkpoint = Some(serde_json::to_value(cp)?);
    db.checkpoint_job(job, now)
}

/// The plan ended without a gate: it says why, and the deploy stays
/// planned.
fn fail(db: &Db, deploy: &str, why: &str, now: u64) -> Result<()> {
    if db.end_deploy_plan(deploy, Err(why), now)? {
        info!(deploy, why, "a deploy's plan didn't plan it");
    }
    // Its run, if it's still going, ends too: nothing will take what it
    // finds.
    if let Some(run) = db.deploy_plan(deploy)?.and_then(|p| p.run)
        && let Some(r) = db.run(&run)?
        && !r.state.terminal()
    {
        let detail = json!({ "reason": format!("its deploy's plan ended: {why}") }).to_string();
        let _ = db.move_run(&r.id, &Move::Control { from: r.state, to: RunState::Failed, at: now, detail: &detail })?;
    }
    Ok(())
}

/// Its box goes, and the job follows the delete to its end (as an apply's).
fn teardown(app: &Platform, run: Option<&str>, cp: &Checkpoint, now: u64) -> Result<Step> {
    let Some(b) = &cp.r#box else { return Ok(Step::Done) };
    if let Some(at) = logs_pending_of(app, run, now)? {
        return Ok(Step::Again(at));
    }
    match app.db.deploy_box(b)? {
        None => Ok(Step::Done),
        Some(w) if w.state == "deleted" => Ok(Step::Done),
        Some(w) => {
            if w.asked.as_deref() != Some("delete") {
                crate::hosted::ask(app, &w.id, crate::hosted::Ask::Delete)?;
                info!(workspace = w.id, "a deploy's plan box is being deleted");
            }
            Ok(Step::Again(now + BOX_MS))
        }
    }
}

/// The gate a plan run's result names, checked: chant's pending fact for
/// the environment's op, at a gate the settings allow, from a chant that
/// signs a binding. `Err` why not.
pub fn gate_of(settings: &arugula_e2e::drivers::Settings, result: &Value) -> Result<Gate, String> {
    let g: Gate = serde_json::from_value(result.get("chant_gate").cloned().unwrap_or_default())
        .map_err(|_| "the plan didn't say which gate chant stopped at".to_owned())?;
    if !g.well_formed() {
        return Err(format!(
            "chant's gate as the plan read it isn't one Arugula signs (chant {} or newer, a plan digest, one word \
             each)",
            arugula_e2e::deploy::chant::FLOOR
        ));
    }
    if settings.get(CHANT_OP) != Some(&g.op) {
        return Err(format!("chant stopped at another op's gate ({} {})", g.op, g.gate));
    }
    if let Some(allowed) = settings.get(CHANT_GATES)
        && !allowed.split(',').any(|a| a == g.gate)
    {
        return Err(format!("chant stopped at gate {}, which the environment's chant_gates doesn't name", g.gate));
    }
    Ok(g)
}

async fn step(app: &Arc<Platform>, db: &Db, job: &mut LeasedJob, now: u64) -> Result<Step> {
    let mut cp = checkpoint_of(job);
    let Some(deploy) = deploy_of_job(&job.id).map(str::to_owned) else { return Ok(Step::Done) };
    let Some(p) = db.deploy_plan(&deploy)? else { return teardown(app, None, &cp, now) };
    let run = p.run.clone();
    let Some(d) = db.deploy(&deploy)? else { return teardown(app, run.as_deref(), &cp, now) };
    let Some(env) = db.environment(&d.environment)? else { return teardown(app, run.as_deref(), &cp, now) };
    let project = env.project.clone();
    if job.cancelled() {
        fail(db, &deploy, "its plan was canceled", now)?;
        return teardown(app, run.as_deref(), &cp, now);
    }
    if !p.planning() {
        return teardown(app, run.as_deref(), &cp, now);
    }
    if d.state != State::Planned {
        fail(db, &deploy, &format!("the deploy is {} now, not waiting to be asked for", d.state.as_str()), now)?;
        return teardown(app, run.as_deref(), &cp, now);
    }
    // Its run is made: follow it until it ends, then take what it found.
    if let Some(id) = &p.run {
        let run = db.run(id)?.context("a deploy plan's run is gone")?;
        if run.state == RunState::Pending {
            let _ = db.move_run(
                &run.id,
                &Move::Approved { from: RunState::Pending, at: now, approval: &plan_approval(&run, &deploy) },
            )?;
        }
        if !run.state.terminal() && now > cp.made_at + BOX_UP_MS + BOX_LIFE_MS {
            let detail = json!({ "reason": "its deploy plan's box ran out of time" }).to_string();
            let _ = db.move_run(
                &run.id,
                &Move::Control { from: run.state, to: RunState::Failed, at: now, detail: &detail },
            )?;
        }
        let run = db.run(id)?.context("a deploy plan's run is gone")?;
        if !run.state.terminal() {
            return Ok(Step::Again(now + FOLLOW_MS));
        }
        drivers::revoke_run(app, &run.id).await;
        let result = reported_result(db, &run)?;
        let gate = match run.state {
            RunState::Succeeded => gate_of(&env.settings, &result),
            _ => Err(format!("its plan run ended: {}", why_it_ended(db, &run)?)),
        };
        match gate {
            Ok(g) => planned(app, &p, &g, now).await?,
            Err(why) => fail(db, &deploy, &why, now)?,
        }
        return teardown(app, Some(id), &cp, now);
    }
    let Some(driver) = drivers::driver(&env.driver) else {
        fail(db, &deploy, &format!("there's no driver {} here", env.driver), now)?;
        return teardown(app, None, &cp, now);
    };
    if app.compute.is_none() {
        fail(db, &deploy, "this platform makes no hosted boxes to plan deploys on", now)?;
        return Ok(Step::Done);
    }
    let asker = Actor::parse(&p.asked_by);
    let account = asker.as_ref().and_then(|a| a.account()).unwrap_or_default().to_owned();
    // A box of its own: named in the checkpoint first, then made, then
    // waited for until its daemon enrolled.
    let b = match cp.r#box.clone() {
        Some(b) => b,
        None => {
            let b = crate::hosted::new_id();
            cp.r#box = Some(b.clone());
            cp.made_at = now;
            save(db, job, &cp, now)?;
            b
        }
    };
    let name = box_name(&deploy);
    let w = match db.hosted_any(&project, &b)? {
        Some(w) => w,
        None => {
            if !db.add_deploy_box(&b, &project, &name, &account, now)? {
                let Some(w) = db.deploy_box_named(&project, &name)? else {
                    fail(db, &deploy, "its plan box couldn't be made (its name is taken)", now)?;
                    return Ok(Step::Done);
                };
                if cp.r#box.as_deref() != Some(w.id.as_str()) {
                    cp.r#box = Some(w.id.clone());
                    save(db, job, &cp, now)?;
                }
                return Ok(Step::Again(now + BOX_MS));
            }
            db.set_plan_box(&deploy, &b)?;
            info!(deploy, workspace = b, "a deploy's plan box is being made");
            return Ok(Step::Again(now + BOX_MS));
        }
    };
    if !(w.state == "ready" && w.machine.is_some()) {
        let why = match w.state.as_str() {
            "failed" | "deleting" | "deleted" => {
                Some(format!("its plan box didn't come up: {}", w.error.as_deref().unwrap_or("it failed")))
            }
            _ if now > cp.made_at + BOX_UP_MS => Some("its plan box didn't come up in time".to_owned()),
            _ => None,
        };
        if let Some(why) = why {
            fail(db, &deploy, &why, now)?;
            return teardown(app, None, &cp, now);
        }
        return Ok(Step::Again(now + BOX_MS));
    }
    let Some((_, operation)) = db.deploy_operation(&deploy)? else {
        fail(db, &deploy, "the deploy has no operation", now)?;
        return teardown(app, None, &cp, now);
    };
    let machine = w.machine.clone().unwrap_or_default();
    let op = Op {
        deploy: Some(DeployApply {
            deploy: deploy.clone(),
            driver: driver.name().into(),
            op: operation.hash(),
            plan: true,
        }),
        ..Op::default()
    }
    .encode(&machine);
    let new = NewPlanRun { project: &project, deploy: &deploy, account: &account, workspace: &w.id, op: &op };
    let run = db.make_plan_run(&new, now)?.context("a deploy's project is gone")?;
    let _ = db.move_run(
        &run.id,
        &Move::Approved { from: RunState::Pending, at: now, approval: &plan_approval(&run, &deploy) },
    )?;
    info!(deploy, run = run.id, workspace = w.id, "a deploy's plan run is made on its box");
    Ok(Step::Again(now + FOLLOW_MS))
}

/// chant planned it: the plan ends with its gate, the asker's `requested`
/// event carries it, and the owners are asked. Only while the asker may
/// still ask, as when they asked: else the plan ends saying why, and
/// nobody is asked.
async fn planned(app: &Platform, p: &DeployPlan, g: &Gate, now: u64) -> Result<()> {
    let db = &app.db;
    let Some(by) = Actor::parse(&p.asked_by) else {
        fail(db, &p.deploy, "whoever asked for it isn't one who asks", now)?;
        return Ok(());
    };
    if let Err(why) = may_still_ask(app, &by, &p.deploy).await? {
        fail(db, &p.deploy, &why, now)?;
        return Ok(());
    }
    if !db.end_deploy_plan(&p.deploy, Ok(g), now)? {
        return Ok(());
    }
    match db.record_deploy(&p.deploy, EventKind::Requested, &by, now, json!({ "chant": g }))? {
        Ok(r) => {
            info!(deploy = p.deploy, gate = g.gate, plan = g.plan, "chant planned a deploy: its owners are asked");
            let env = db.environment(&r.deploy.environment)?.context("a deploy's environment is gone")?;
            let project = db.issue_project(&env.project)?.context("a deploy's project is gone")?;
            super::push_owners(app, &by, &r.deploy, &env, &project).await;
        }
        Err(why) => warn!(deploy = p.deploy, %why, "chant planned a deploy that can't be asked for now"),
    }
    Ok(())
}

/// Whether `by` may still ask for `deploy`'s approval, as `request` checked
/// when they asked (#1985 review): a person an owner or an editor of its
/// project now, and an agent's owner too, with its machine's `deploys.plan`
/// grant over the project, live. `Err` why not; a server's error (the
/// hub's) is the job's, which tries again.
async fn may_still_ask(app: &Platform, by: &Actor, deploy: &str) -> Result<std::result::Result<(), String>> {
    let Some(account) = by.account() else { return Ok(Err("whoever asked for it isn't one who asks".into())) };
    let project = match super::deploy_as(app, account, deploy, true).await {
        Ok((_, env, _, _)) => env.project,
        Err(e) if e.0.is_server_error() => anyhow::bail!("whether its asker may still ask: {}", e.1),
        Err(e) => return Ok(Err(format!("whoever asked for it can't now: {}", e.1))),
    };
    if let Actor::Agent { account, machine } = by {
        let m = crate::auth::Machine { account: account.clone(), daemon: machine.clone() };
        match crate::grants::agent_may(
            app,
            &m,
            crate::db::projects::DEPLOYS_PLAN,
            &crate::grants::AgentScope::Project(project),
        )
        .await
        {
            Ok(_) => {}
            Err(e) if e.0.is_server_error() => anyhow::bail!("whether its asker may still ask: {}", e.1),
            Err(e) => return Ok(Err(format!("the agent that asked for it can't now: {}", e.1))),
        }
    }
    Ok(Ok(()))
}

/// What a deploy's plan run carries to its box: the deploy's operation,
/// its driver and settings, and the secret it gets (by name: a token per
/// step). `None` when its plan ended.
pub fn plan_work(db: &Db, p: &DeployPlan) -> Result<Option<DeployPlanWork>> {
    if !p.planning() {
        return Ok(None);
    }
    let Some((_, operation)) = db.deploy_operation(&p.deploy)? else { return Ok(None) };
    let env = db.environment(&operation.environment)?.context("a deploy's environment is gone")?;
    Ok(Some(DeployPlanWork {
        deploy: p.deploy.clone(),
        operation,
        driver: env.driver,
        secrets: vec![GITHUB_TOKEN.into()],
        settings: env.settings,
    }))
}

/// A plan run's secrets (`/api/hosted/work/secrets` for a plan run), one
/// GitHub token per step, each minted for it, once, while it runs and its
/// deploy waits to be asked for (#1985): for `fetch`, one that reads the
/// deploy's repository, before anything of the commit's runs; for `push`,
/// after it, one that writes the repository's contents, which the box asks
/// for only once everything the plan ran is stopped and it checked chant's
/// ledger, and which goes only to this daemon's own push, never the run
/// user; and only while the asker may still ask. Never the environment's
/// secrets, its own GitHub token among them.
pub async fn secrets(
    app: &Platform,
    m: &HostedMachine,
    run: &crate::db::runs::Run,
    p: &DeployPlan,
    step: Option<PlanStep>,
) -> Result<SecretsGiven> {
    let refused = |why: &str| {
        info!(run = run.id, machine = m.machine, why, "refused a deploy plan run's secrets");
        Ok(SecretsGiven::Refused { why: why.into() })
    };
    let Some(step) = step else {
        return refused("a deploy plan run asks for each of its tokens by its step (fetch, then push)");
    };
    let db = &app.db;
    let Some(d) = db.deploy(&p.deploy)? else { return refused("no such deploy") };
    if d.state != State::Planned {
        return refused("its deploy isn't waiting to be asked for");
    }
    let env = db.environment(&d.environment)?.context("a deploy's environment is gone")?;
    if let Some(driver) = drivers::driver(&env.driver)
        && let Err(why) = driver.allowed_in(&env.project)
    {
        return refused(&why);
    }
    // The push's token writes the repository: only while whoever asked may
    // still ask, as `planned` checks again before anyone is asked.
    if step == PlanStep::Push {
        let Some(by) = Actor::parse(&p.asked_by) else { return refused("whoever asked for it isn't one who asks") };
        if let Err(why) = may_still_ask(app, &by, &p.deploy).await? {
            return refused(&why);
        }
    }
    if !db.release_plan_secrets(&run.id, step, arugula_e2e::now_ms())? {
        return refused(match step {
            PlanStep::Fetch => "its fetch token was given already (once per run), or its plan ended",
            PlanStep::Push => "its push token comes once per run, after its fetch token, while its plan hasn't ended",
        });
    }
    let mint = match step {
        PlanStep::Fetch => Mint::GithubRead { name: GITHUB_TOKEN },
        PlanStep::Push => Mint::GithubWriteSource { name: GITHUB_TOKEN },
    };
    let s: Secret = match drivers::mint(app, mint, &d, &env.project, &run.id).await {
        Ok(s) => s,
        Err(e) => return refused(&format!("minting {GITHUB_TOKEN}: {e:#}")),
    };
    info!(run = run.id, deploy = d.id, ?step, "gave a deploy plan run a token, once");
    Ok(SecretsGiven::Given { secrets: vec![s] })
}
