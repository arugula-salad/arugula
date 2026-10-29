//! redone's tools on the platform's MCP endpoint (M235): the shim for an
//! agent whose token the import carried over from redone
//! ([`crate::redone`]), so it keeps working with the prompts and
//! instructions it has, through the platform, until its token is revoked on
//! the date Jake announces. Only a carried-over token's agent gets these;
//! a token minted on the platform page gets the platform's names alone.
//!
//! - **An app is an environment** of the token's project, by its name:
//!   one an owner made (or the import made from redone's app). An agent
//!   never claims a name: a call naming an app the project has no
//!   environment of is refused, which is what fixes redone's claim hole
//!   (Arugula #55, a plan claimed any free name). Which project holds an
//!   app's name is the environments' (a shipped app's name is one
//!   project's).
//! - **Plan, approve, apply** is a deploy: `plan_deploy {app, upload_id}`
//!   plans a deploy of the uploaded source to the app's environment and
//!   asks the project's owners, who get the phone card; an owner approves
//!   it on their device; it applies itself. `apply_plan` applies nothing
//!   and approves nothing: it says where the deploy is. `get_plan` reads
//!   it, `plan_rollback` plans an earlier source again.
//! - **Uploads** go to `PUT /api/mcp/uploads` (a gzipped tar, by itself, so
//!   it never passes through the agent's context), kept as the project's
//!   artifact; its `upload_id` is the artifact's digest.
//! - **`set_secret {app, key, value}`** is the project's secret for that
//!   app (`app/<app>/<KEY>`, sealed with the project's key: the truth),
//!   projected into Infisical's `/<app>/` ([`crate::infisical`]).
//!   `list_secrets` says their names, never a value.
//! - **What isn't here:** an app's own output (`app_logs` read loam with a
//!   key the platform doesn't hold: it reads the deploy's run log instead),
//!   and `promote` (an owner plans it, to the app's promote environment).

use arugula_e2e::deploy::planter;
use serde::Deserialize;
use serde_json::{Value, json};

use super::{Agent, Done, Failure, Out, args, failed, prop, strings, tool};
use crate::{
    db::{
        deploys::{Environment, State as DeployState},
        projects::{DEPLOYS_PLAN, SECRETS_WRITE},
    },
    features::deploys,
    grants::{AgentScope, agent_may},
    infisical,
};

/// Where an app on loam answers: redone's apps' host.
pub const LOAM_DOMAIN: &str = "run.inevitable.fyi";

/// Where a project's secret for one of its apps is kept in
/// `project_secrets`: `app/<app>/<KEY>`.
pub fn app_secret(app: &str, key: &str) -> String {
    format!("app/{app}/{key}")
}

const APP: &str = "The app's name: an environment of the project, which its owner made. Its host is https://<app>.run.inevitable.fyi.";

/// redone's tools as its agents know them, and `plan_deploy` and
/// `set_secret` taking both shapes.
pub fn tools() -> Vec<Value> {
    let mut out: Vec<Value> = super::tools()
        .into_iter()
        .map(|t| match t["name"].as_str() {
            Some("plan_deploy") => {
                let mut t = t;
                let p = t["inputSchema"]["properties"].as_object_mut().unwrap();
                p.insert("app".into(), prop("string", APP));
                p.insert("upload_id".into(), prop("string", "From PUT /api/mcp/uploads: what to ship to the app."));
                p.insert("port".into(), prop("integer", "The port the app listens on: its environment's, unchanged."));
                p.insert("health".into(), prop("string", "Its health path: its environment's, unchanged."));
                p.insert("port_env".into(), prop("string", "The variable it reads its port from: its environment's."));
                p.insert("secrets".into(), strings("Its secrets' names: its environment's."));
                t["inputSchema"]["required"] = json!([]);
                t["description"] = json!(format!(
                    "{} redone's way: app and upload_id (PUT the source to /api/mcp/uploads first) plan shipping \
                     it to that app's environment, and ask.",
                    t["description"].as_str().unwrap_or_default()
                ));
                t
            }
            Some("set_secret") => {
                let mut t = t;
                let p = t["inputSchema"]["properties"].as_object_mut().unwrap();
                p.insert("app".into(), prop("string", APP));
                p.insert("key".into(), prop("string", "redone's way, with app: the variable's name."));
                t["inputSchema"]["required"] = json!(["value"]);
                t["description"] = json!(format!(
                    "{} redone's way: app, key and value set the app's secret, which the app reads as that \
                     variable; it reaches the cluster through Infisical.",
                    t["description"].as_str().unwrap_or_default()
                ));
                t
            }
            _ => t,
        })
        .collect();
    let plan = json!({ "plan_id": prop("string", "The deploy (dep_…).") });
    out.extend([
        tool("whoami", "Whose agent this token is, its project, and how to upload source.", json!({}), &[], true),
        tool("list_apps", "The project's apps (its environments), with the deploy live on each.", json!({}), &[], true),
        tool(
            "plan_rollback",
            "Plan shipping an earlier deploy's source again: the one before the live one, or the deploy `to`. \
             Approved on an owner's device like any deploy.",
            json!({ "app": prop("string", APP), "to": prop("string", "An earlier deploy of the app (dep_…).") }),
            &["app"],
            false,
        ),
        tool(
            "get_plan",
            "A deploy and its state: pending (waits for an owner's device), approved, running, succeeded, failed, \
             denied, expired, stale.",
            plan.clone(),
            &["plan_id"],
            true,
        ),
        tool(
            "apply_plan",
            "Where a deploy is. It applies itself once an owner approves it on their phone: this never applies or \
             approves anything.",
            plan,
            &["plan_id"],
            true,
        ),
        tool(
            "app_status",
            "Where an app is: its live deploy, and the one being applied.",
            json!({ "app": prop("string", APP) }),
            &["app"],
            true,
        ),
        tool(
            "app_logs",
            "The log of the app's latest deploy's run (the app's own output isn't on the platform).",
            json!({ "app": prop("string", APP), "tail": prop("integer", "Bytes, newest last. Default 16000.") }),
            &["app"],
            true,
        ),
        tool(
            "list_secrets",
            "The names of an app's secrets (never their values).",
            json!({ "app": prop("string", APP) }),
            &["app"],
            true,
        ),
        tool(
            "promote",
            "Moving an app from loam into the cluster: its owner plans that on the platform page.",
            json!({ "app": prop("string", APP) }),
            &["app"],
            false,
        ),
    ]);
    out
}

/// The app's name an environment holds, if it's one of planter's ship or
/// promote environments: its `app` setting, which M233 claims for its
/// project across the platform. Only such an app's secrets are projected
/// into Infisical, into the folder of that name (never an environment's
/// own name, which any project may give any environment).
pub fn claimed_app(env: &Environment) -> Option<&str> {
    (env.driver == planter::SHIP || env.driver == planter::PROMOTE)
        .then(|| env.settings.get(planter::APP).map(String::as_str))
        .flatten()
}

/// What the app is called: its claimed name, or the environment's.
fn app_name(env: &Environment) -> &str {
    claimed_app(env).unwrap_or(&env.name)
}

/// What a carried-over token's agent may not do with the platform's own
/// tools (M235): plan to an environment the import didn't make from its
/// tenant's apps, or set a project-wide secret. `None` when it may.
pub(super) async fn refuses(agent: &Agent, name: &str, a: &Value) -> Option<Failure> {
    match name {
        "set_secret" => Some(failed("a redone token sets its apps' secrets only: set_secret with app, key and value")),
        "plan_deploy" => {
            let wanted = a.get("environment").and_then(Value::as_str).unwrap_or_default().trim().to_owned();
            let project = match agent.project(a.get("project").and_then(Value::as_str)).await {
                Ok(p) => p,
                Err(e) => return Some(e),
            };
            if let Err(e) =
                agent_may(&agent.app, &agent.me(), DEPLOYS_PLAN, &AgentScope::Project(project.clone())).await
            {
                return Some(e.into());
            }
            let allowed = match agent.app.db.redone_token_environments(&agent.token.id) {
                Ok(a) => a,
                Err(e) => return Some(e.into()),
            };
            let envs = match agent.app.db.environments(&project) {
                Ok(e) => e,
                Err(e) => return Some(e.into()),
            };
            let env = envs.into_iter().find(|e| e.id == wanted || e.name == wanted);
            match env {
                Some(e) if allowed.contains(&e.id) => None,
                _ => {
                    Some(Failure::Refused(format!("a redone token plans only to the apps redone had: not {wanted:?}")))
                }
            }
        }
        _ => None,
    }
}

/// What redone's agents were told, for a carried-over token's.
pub fn instructions(upload: &str) -> String {
    format!(
        " This token came from redone, so redone's tools work too. How a deploy goes: 1. Upload the app's source, a \
         directory with a Dockerfile at its root, as a gzipped tar: tar -czf - -C <dir> . | curl -sS \
         --fail-with-body -X PUT --data-binary @- -H \"Authorization: Bearer $REDONE_TOKEN\" {upload} (it answers \
         upload_id). 2. plan_deploy with app and upload_id: the project's owners get a card on their phone. 3. An \
         owner approves it there; you can't, and must not try. 4. It applies itself: get_plan follows it. Secrets: \
         set_secret with app, key and value; nothing reads a value back. An app is one its owner added to the \
         project: you can't claim a new name."
    )
}

#[derive(Deserialize)]
struct AppArg {
    app: String,
}

#[derive(Deserialize)]
struct PlanArg {
    plan_id: String,
}

#[derive(Deserialize)]
struct Ship {
    app: String,
    upload_id: String,
    #[serde(default)]
    project: Option<String>,
    #[serde(default)]
    port: Option<u32>,
    #[serde(default)]
    health: Option<String>,
    #[serde(default)]
    port_env: Option<String>,
    #[serde(default)]
    secrets: Option<Vec<String>>,
}

#[derive(Deserialize)]
struct Rollback {
    app: String,
    #[serde(default)]
    to: Option<String>,
}

#[derive(Deserialize)]
struct AppSecret {
    app: String,
    key: String,
    value: String,
    #[serde(default)]
    project: Option<String>,
}

#[derive(Deserialize)]
struct Logs {
    app: String,
    #[serde(default)]
    tail: Option<u64>,
}

/// A comma-separated list of names, sorted and once each.
fn sorted(list: &str) -> String {
    let mut v: Vec<&str> = list.split(',').map(str::trim).filter(|n| !n.is_empty()).collect();
    v.sort_unstable();
    v.dedup();
    v.join(",")
}

/// redone's state names for a deploy's.
fn redone_state(s: &str) -> &str {
    match s {
        "succeeded" => "applied",
        "denied" | "canceled" => "rejected",
        other => other,
    }
}

/// A redone-shaped call: `None` when `name` with these arguments is the
/// platform's own (its `plan_deploy` and `set_secret` without `app`).
pub(super) async fn dispatch(agent: &Agent, name: &str, a: &Value) -> Option<Out> {
    let has_app = a.get("app").is_some();
    Some(match name {
        "plan_deploy" if has_app => ship(agent, a).await,
        "set_secret" if has_app => set_secret(agent, a).await,
        "whoami" => whoami(agent).await,
        "list_apps" => list_apps(agent).await,
        "plan_rollback" => rollback(agent, a).await,
        "get_plan" => get_plan(agent, a).await,
        "apply_plan" => apply_plan(agent, a).await,
        "app_status" => app_status(agent, a).await,
        "app_logs" => app_logs(agent, a).await,
        "list_secrets" => list_secrets(agent, a).await,
        "promote" => Err(failed(
            "promote: moving an app into the cluster is planned by its owner on the platform page (the app's \
             promote environment), and approved on their phone",
        )),
        _ => return None,
    })
}

/// The app's environment in project `p`, the caller having asked for the
/// token's grant there: one the import made from the token's tenant's
/// redone apps (a carried-over token reaches those only). A planter ship
/// or promote environment is found by the app's name it claims (ship's
/// first), any other by its own name. Never made here: an agent doesn't
/// claim names.
fn app_env(agent: &Agent, p: &str, app: &str) -> Result<Environment, Failure> {
    let app = app.trim();
    if !infisical::valid_app(app) {
        return Err(failed(format!(
            "app {app:?}: lowercase letters, digits and dashes, at most 32, and not one of the platform's names"
        )));
    }
    let allowed = agent.app.db.redone_token_environments(&agent.token.id)?;
    let mut envs: Vec<Environment> = agent
        .app
        .db
        .environments(p)?
        .into_iter()
        .filter(|e| match claimed_app(e) {
            Some(claimed) => claimed == app,
            None => !e.driver.starts_with("planter-") && e.name == app,
        })
        .collect();
    envs.sort_by_key(|e| e.driver != planter::SHIP);
    let none = || {
        failed(format!("this token has no app {app}: it reaches the apps redone had, and an agent can't claim a name"))
    };
    let env = envs.into_iter().next().ok_or_else(none)?;
    if !allowed.contains(&env.id) {
        return Err(none());
    }
    Ok(env)
}

async fn whoami(agent: &Agent) -> Out {
    let p = agent.project(None).await?;
    let name = agent.app.db.project_owner(&p)?.map(|o| o.name).unwrap_or_default();
    let upload = agent.upload_url();
    let value = json!({
        "tenant": name,
        "project": p,
        "agent": agent.token.name,
        "token_id": agent.token.id,
        "owner": agent.owner,
        "upload": format!(
            "tar -czf - -C <dir> . | curl -sS --fail-with-body -X PUT --data-binary @- -H \"Authorization: Bearer $REDONE_TOKEN\" {upload}"
        ),
        "updates_need_approval": true,
        "approves": false,
    });
    let text = format!(
        "You're {}'s agent \"{}\", in project {name} ({p}). Every deploy waits for an owner's device.",
        agent.owner, agent.token.name
    );
    Ok(Done { text, value, project: Some(p), note: "whoami".into() })
}

/// The deploys of `env`, newest first.
fn deploys_of(agent: &Agent, env: &Environment) -> Result<Vec<crate::db::deploys::Deploy>, Failure> {
    Ok(agent.app.db.deploys(&env.id, 200)?)
}

async fn list_apps(agent: &Agent) -> Out {
    let p = agent.project(None).await?;
    agent_may(&agent.app, &agent.me(), DEPLOYS_PLAN, &AgentScope::Project(p.clone())).await?;
    let mut apps = vec![];
    let allowed = agent.app.db.redone_token_environments(&agent.token.id)?;
    for e in agent.app.db.environments(&p)? {
        if !allowed.contains(&e.id) {
            continue;
        }
        let ds = deploys_of(agent, &e)?;
        let live = ds.iter().find(|d| d.state == DeployState::Succeeded).map(|d| d.id.clone());
        apps.push(json!({
            "app": app_name(&e),
            "url": format!("https://{}.{LOAM_DOMAIN}", app_name(&e)),
            "driver": e.driver,
            "current_plan": live,
            "deploys": ds.iter().filter(|d| d.state == DeployState::Succeeded).count(),
        }));
    }
    let text = format!("{} app(s).\n{}", apps.len(), Value::from(apps.clone()));
    Ok(Done { text, value: json!(apps), project: Some(p), note: format!("listed {} app(s)", apps.len()) })
}

/// A deploy as redone's `get_plan` answered: the platform's deploy, with
/// redone's names beside.
fn view(d: &Value, app: &str) -> Value {
    let state = d["state"].as_str().unwrap_or_default();
    json!({
        "plan_id": d["id"],
        "app": app,
        "kind": if d["roll_back"].as_bool().unwrap_or(false) { "rollback" } else { "deploy" },
        "state": redone_state(state),
        "summary": d["plan_text"],
        "waits_for": (state == "pending").then_some("an owner's approval, on their phone"),
        "deploy": d,
    })
}

async fn ship(agent: &Agent, a: &Value) -> Out {
    let s: Ship = args(a)?;
    let p = agent.project(s.project.as_deref()).await?;
    // The grant first: nothing of a project is read for a token that
    // doesn't reach it.
    agent_may(&agent.app, &agent.me(), DEPLOYS_PLAN, &AgentScope::Project(p.clone())).await?;
    let env = app_env(agent, &p, &s.app)?;
    // What the app is (its port, health path, secrets) is its
    // environment's, which its owner set: a plan doesn't change it.
    for (k, given) in [
        ("port", s.port.map(|v| v.to_string())),
        ("health", s.health.clone()),
        ("port_env", s.port_env.clone()),
        ("secrets", s.secrets.as_ref().map(|v| sorted(&v.join(",")))),
    ] {
        let has = env.settings.get(k).map(|h| if k == "secrets" { sorted(h) } else { h.clone() });
        if let (Some(given), Some(has)) = (given.as_ref(), has)
            && *given != has
        {
            return Err(failed(format!(
                "{} {k} is {has}, not {given}: its owner changes that on the platform page",
                app_name(&env)
            )));
        }
    }
    let digest = s.upload_id.trim();
    if !planter::artifact_ok(digest) {
        return Err(failed(format!("upload_id: what PUT {} answered (sha256:…)", agent.upload_url())));
    }
    let Some(bytes) = agent.app.db.deploy_artifact(&p, digest)? else {
        return Err(failed(format!(
            "no upload {digest} in this project: PUT the source to {} first",
            agent.upload_url()
        )));
    };
    agent.app.limits.check_account(super::ACCOUNT_DEPLOY_ASKS, &agent.token.account)?;
    let source = json!({ "kind": "artifact", "ref": digest, "where": planter::ARTIFACT_WHERE });
    // Kept again with its deploy (M233's rule), so it stays while that waits.
    let done = agent.plan_and_ask(p.clone(), &env.id, source, false, Some(&bytes)).await?;
    let v = view(&done.value["deploy"], app_name(&env));
    let text = format!(
        "{}\nIt waits for an owner's approval on their phone; you can't approve it. get_plan {} follows it.",
        done.text,
        v["plan_id"].as_str().unwrap_or_default()
    );
    Ok(Done { text, value: v, project: Some(p), note: done.note })
}

async fn rollback(agent: &Agent, a: &Value) -> Out {
    let r: Rollback = args(a)?;
    let p = agent.project(None).await?;
    agent_may(&agent.app, &agent.me(), DEPLOYS_PLAN, &AgentScope::Project(p.clone())).await?;
    let env = app_env(agent, &p, &r.app)?;
    let done: Vec<_> = deploys_of(agent, &env)?.into_iter().filter(|d| d.state == DeployState::Succeeded).collect();
    let target = match &r.to {
        Some(to) => done.iter().find(|d| &d.id == to),
        None => done.get(1),
    };
    let Some(target) = target else {
        return Err(failed(match &r.to {
            Some(to) => format!("{to} was never applied to {}", app_name(&env)),
            None => format!("{} has no earlier deploy to roll back to", app_name(&env)),
        }));
    };
    agent.app.limits.check_account(super::ACCOUNT_DEPLOY_ASKS, &agent.token.account)?;
    let source =
        json!({ "kind": target.source.kind.as_str(), "ref": target.source.reference, "where": target.source.place });
    let kept = match target.source.kind.as_str() {
        "artifact" => Some(
            agent
                .app
                .db
                .deploy_artifact(&p, &target.source.reference)?
                .ok_or_else(|| failed(format!("the source of {} isn't kept any more: ship it again", target.id)))?,
        ),
        _ => None,
    };
    let out = agent.plan_and_ask(p.clone(), &env.id, source, false, kept.as_deref()).await?;
    let mut v = view(&out.value["deploy"], app_name(&env));
    v["kind"] = json!("rollback");
    v["rollback_to"] = json!(target.id);
    Ok(Done { text: out.text, value: v, project: Some(p), note: format!("planned a roll back to {}", target.id) })
}

/// The deploy, as the agent may read it, and its app's name.
async fn read(agent: &Agent, id: &str) -> Result<(Value, String), Failure> {
    let d = deploys().agent_get(agent.app.clone(), agent.me(), id.trim().to_owned()).await?;
    let env = d["environment"]["name"].as_str().unwrap_or_default().to_owned();
    Ok((d, env))
}

async fn get_plan(agent: &Agent, a: &Value) -> Out {
    let q: PlanArg = args(a)?;
    let (d, app) = read(agent, &q.plan_id).await?;
    let v = view(&d, &app);
    let project = d["project"].as_str().or(d["environment"]["project"].as_str()).map(str::to_owned);
    Ok(Done { text: format!("{v}"), value: v, project, note: format!("read {}", q.plan_id.trim()) })
}

async fn apply_plan(agent: &Agent, a: &Value) -> Out {
    let q: PlanArg = args(a)?;
    let (d, app) = read(agent, &q.plan_id).await?;
    let v = view(&d, &app);
    let state = d["state"].as_str().unwrap_or_default();
    let text = match state {
        "planned" | "pending" => format!(
            "{} waits for an owner to approve it on their phone; you can't. It applies itself once they do: get_plan \
             follows it.",
            q.plan_id.trim()
        ),
        "approved" | "running" => format!("{} is approved and applies itself: get_plan follows it.", q.plan_id.trim()),
        "succeeded" => format!("{} is applied.", q.plan_id.trim()),
        other => format!("{} is {}: make a new plan.", q.plan_id.trim(), redone_state(other)),
    };
    let project = d["project"].as_str().or(d["environment"]["project"].as_str()).map(str::to_owned);
    Ok(Done { text, value: v, project, note: format!("asked about {}", q.plan_id.trim()) })
}

async fn app_status(agent: &Agent, a: &Value) -> Out {
    let q: AppArg = args(a)?;
    let p = agent.project(None).await?;
    agent_may(&agent.app, &agent.me(), DEPLOYS_PLAN, &AgentScope::Project(p.clone())).await?;
    let env = app_env(agent, &p, &q.app)?;
    let ds = deploys_of(agent, &env)?;
    let live = ds.iter().find(|d| d.state == DeployState::Succeeded);
    let moving = ds.iter().find(|d| matches!(d.state, DeployState::Approved | DeployState::Running));
    let state = match (moving, live) {
        (Some(_), _) => "deploying",
        (None, Some(_)) => "serving",
        (None, None) => "not shipped",
    };
    let v = json!({
        "app": app_name(&env),
        "state": state,
        "url": format!("https://{}.{LOAM_DOMAIN}", app_name(&env)),
        "current_plan": live.map(|d| d.id.clone()),
        "applying": moving.map(|d| d.id.clone()),
    });
    Ok(Done { text: format!("{v}"), value: v, project: Some(p), note: format!("status of {}", app_name(&env)) })
}

async fn app_logs(agent: &Agent, a: &Value) -> Out {
    let q: Logs = args(a)?;
    let p = agent.project(None).await?;
    agent_may(&agent.app, &agent.me(), DEPLOYS_PLAN, &AgentScope::Project(p.clone())).await?;
    let env = app_env(agent, &p, &q.app)?;
    let Some(run) = deploys_of(agent, &env)?.into_iter().find_map(|d| d.run) else {
        let text =
            format!("{} has no deploy that ran yet (the app's own output isn't on the platform)", app_name(&env));
        return Ok(Done {
            text,
            value: json!({ "app": app_name(&env), "log": null }),
            project: Some(p),
            note: "no run".into(),
        });
    };
    let a = json!({ "run": run, "max_chars": q.tail.unwrap_or(16_000) });
    let mut out = agent.read_run(args(&a)?).await?;
    out.text = format!(
        "The log of {}'s latest deploy's run (the app's own output isn't on the platform):\n{}",
        app_name(&env),
        out.text
    );
    Ok(out)
}

async fn set_secret(agent: &Agent, a: &Value) -> Out {
    // Never serde's words: they can quote the value.
    let s: AppSecret = serde_json::from_value(a.clone())
        .map_err(|_| failed("set_secret takes app, key and value (strings), redone's way"))?;
    let p = agent.project(s.project.as_deref()).await?;
    // The grant first: nothing of a project is read for a token that
    // doesn't reach it.
    agent_may(&agent.app, &agent.me(), SECRETS_WRITE, &AgentScope::Project(p.clone())).await?;
    let env = app_env(agent, &p, &s.app)?;
    let key = s.key.trim();
    if !infisical::valid_key(key) {
        return Err(failed("key: an environment variable's name, letters, digits and '_', not starting with a digit"));
    }
    let name = app_name(&env).to_owned();
    agent.seal(&p, &app_secret(&name, key), &s.value).await?;
    let note = format!("set {name}'s secret {key}");
    let Some(claimed) = claimed_app(&env) else {
        let v = json!({ "project": p, "app": name, "key": key, "set": true, "projected": null });
        let text = format!(
            "{name}: {key} is set on the platform. It isn't a shipped app of the project's, so it isn't put in \
             Infisical."
        );
        return Ok(Done { text, value: v, project: Some(p), note });
    };
    let projected = infisical::project(
        &agent.app,
        &infisical::Ask {
            project: &p,
            app: claimed,
            name: key,
            value: &s.value,
            by_account: Some(&agent.token.account),
            by_agent: Some(&agent.token.id),
        },
    )
    .await?;
    let v = json!({
        "project": p,
        "app": name,
        "key": key,
        "set": true,
        "projected": projected.as_ref().map(|r| r.outcome.clone()),
    });
    let text = match &projected {
        Some(r) if r.outcome == "written" => format!("{name}: {key} is set, and in the cluster's secrets."),
        Some(r) => format!("{name}: {key} is set on the platform; Infisical: {} ({}).", r.outcome, r.detail),
        None => format!("{name}: {key} is set on the platform."),
    };
    Ok(Done { text, value: v, project: Some(p), note })
}

async fn list_secrets(agent: &Agent, a: &Value) -> Out {
    let q: AppArg = args(a)?;
    let p = agent.project(None).await?;
    agent_may(&agent.app, &agent.me(), SECRETS_WRITE, &AgentScope::Project(p.clone())).await?;
    let env = app_env(agent, &p, &q.app)?;
    let prefix = app_secret(app_name(&env), "");
    let keys: Vec<String> = agent
        .app
        .db
        .project_secret_names(&p, &prefix)?
        .into_iter()
        .map(|n| n.trim_start_matches(&prefix).to_owned())
        .collect();
    let v = json!({ "app": app_name(&env), "keys": keys });
    Ok(Done { text: format!("{v}"), value: v, project: Some(p), note: format!("listed {}'s secrets", env.name) })
}
