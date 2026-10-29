//! Deploys for agents (M252, platform step 7): `plan_deploy` and
//! `read_deploy`, over a project's deploys in Arugula control (M218).
//!
//! - **Plan and ask, never approve.** `plan_deploy` plans a deploy of a
//!   commit to one of the project's environments and asks its owners to
//!   approve it: the platform makes the plan (the environment's driver's),
//!   and pushes each owner a card. Only an owner's device approves, on
//!   their phone or in a browser, with a passkey. There's no tool here that
//!   approves or denies, and no machine route that would.
//! - **Through the owner's grant.** Each call is a machine-signed route on
//!   the platform (`/api/daemon/projects/{project}/deploys`,
//!   `/api/daemon/deploys/{id}` and its `request`,
//!   `crates/platform-deploys/src/deploys/agent.rs`), which asks for this machine's
//!   `deploys.plan` grant over the project every time, given by its owner
//!   (`arugula grants add <machine> deploys.plan --project <name>`, or the
//!   platform page). A plan is the owner's, labelled as their agent on this
//!   machine, never past their role (an owner or an editor plans).
//!
//! An agent a guest stands behind may not use them: they act as the owner.

use arugula_control_wire::deploys::{
    DeployPlan, DeploySource, agent_deploy, agent_deploy_request, agent_project_deploys, deploy_id, full_commit,
};
use reqwest::Method;
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::{Value, json};

use crate::mcp::tools::{Call, Out, done};
use crate::{control::CloudRefused, mux::Api};

pub(crate) const PLAN_TOOL: &str = "plan_deploy";
pub(crate) const READ_TOOL: &str = "read_deploy";

pub(crate) const PLAN_DESCRIPTION: &str = "Plan a deploy of a commit to one of a project's environments in Arugula control, and ask the project's owners to approve it. Agents plan and ask; they never approve: an owner approves (or denies) it on their phone or in a browser, with a passkey, and only then is it applied. The platform makes the plan from the environment's driver and returns its text, which is what the owner reads. Needs this machine's deploys.plan grant over the project, given by your person (an owner or editor there); without it the platform refuses and says how to give it. Returns the deploy (dep_…), waiting for approval; read_deploy follows it.";

pub(crate) const READ_DESCRIPTION: &str = "Read a deploy in Arugula control (with this machine's deploys.plan grant over its project): where it goes, what it deploys, its plan's text, its state (planned, pending: waiting for an owner's approval, approved, running, succeeded, failed, denied, stale) and its log. Reading never approves: an owner approves on their phone or in a browser.";

#[derive(Deserialize, JsonSchema)]
pub struct PlanDeployArgs {
    /// The project in Arugula control: its id (prj_…).
    pub project: String,
    /// The environment: its name in the project (production), or its id
    /// (env_…).
    pub environment: String,
    /// The commit to deploy, by its full 40-hex id.
    pub commit: String,
    /// The repository the commit is in: OWNER/NAME on GitHub.
    pub repo: String,
    /// Roll back: deploy a commit this environment ran before, with what it
    /// ran then.
    #[serde(default)]
    pub roll_back: bool,
}

#[derive(Deserialize, JsonSchema)]
pub struct ReadDeployArgs {
    /// The deploy (dep_…), from plan_deploy.
    pub id: String,
}

/// What a refusal says to an agent.
fn said(e: CloudRefused) -> String {
    match e {
        CloudRefused::NotJoined => "this machine hasn't joined Arugula control, where deploys live: ask your person to join it (Getting started on the web page, or `arugulad join`)".into(),
        CloudRefused::OldControl | CloudRefused::Said { status: 405, .. } => {
            "this Arugula control doesn't let agents plan deploys yet".into()
        }
        CloudRefused::Said { status: 403, said, reason } => {
            let why = match reason.as_deref() {
                Some("agent_ungranted") => " Your person gives this machine deploys.plan over the project to let you plan and ask; approving stays theirs.",
                _ => "",
            };
            format!("control refused: {said}.{why}")
        }
        e => e.to_string(),
    }
}

fn project_of(p: &str) -> Result<&str, String> {
    let p = p.trim();
    if p.strip_prefix("prj_").is_some_and(|h| !h.is_empty() && h.bytes().all(|b| b.is_ascii_hexdigit())) {
        Ok(p)
    } else {
        Err(format!("project: its id (prj_…), not {p:?}"))
    }
}

/// `OWNER/NAME`.
fn repo_ok(r: &str) -> bool {
    let part = |s: &str| {
        (1..=100).contains(&s.len())
            && s.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.'))
            && !s.starts_with('.')
    };
    r.split_once('/').is_some_and(|(o, n)| part(o) && part(n))
}

/// What the platform takes, from what the agent gave: `Err` why not.
fn plan_of(a: &PlanDeployArgs) -> Result<DeployPlan, String> {
    let commit = a.commit.trim().to_ascii_lowercase();
    if !full_commit(&commit) {
        return Err(format!("commit: its full 40-hex id (`git rev-parse HEAD`), not {:?}", a.commit));
    }
    let repo = a.repo.trim();
    if !repo_ok(repo) {
        return Err(format!("repo: OWNER/NAME on GitHub, not {repo:?}"));
    }
    let environment = a.environment.trim();
    if environment.is_empty() {
        return Err("environment: its name in the project, or its id (env_…)".into());
    }
    Ok(DeployPlan {
        environment: environment.to_owned(),
        source: DeploySource { kind: "commit".into(), reference: commit, place: repo.to_owned() },
        roll_back: a.roll_back,
    })
}

const STATES: &[(&str, &str)] = &[
    ("planned", "planned, not asked yet"),
    ("pending", "waiting for an owner's approval"),
    ("approved", "approved, about to be applied"),
    ("denied", "denied by an owner"),
    ("canceled", "canceled"),
    ("expired", "expired before it was applied"),
    ("running", "being applied"),
    ("succeeded", "applied"),
    ("failed", "failed"),
    ("stale", "stale: another deploy was applied after it was planned; plan it again"),
];

fn state_words(s: &str) -> &str {
    STATES.iter().find(|(k, _)| *k == s).map_or(s, |(_, w)| w)
}

/// `dep_… to production: commit 0123456789ab of acme/app [waiting for an
/// owner's approval]`.
fn line(d: &Value) -> String {
    let t = |v: &Value| v.as_str().unwrap_or("?").to_owned();
    let short: String = t(&d["source"]["ref"]).chars().take(12).collect();
    let back = if d["roll_back"].is_null() { "" } else { " (a roll back)" };
    format!(
        "{} to {}: {} {short} of {}{back} [{}]",
        t(&d["id"]),
        t(&d["environment"]["name"]),
        t(&d["source"]["kind"]),
        t(&d["source"]["where"]),
        state_words(d["state"].as_str().unwrap_or("?"))
    )
}

/// Its log, one line an event, and who approved or denied it.
fn history(d: &Value) -> String {
    let mut out: Vec<String> = d["events"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|e| format!("[{}] {} by {}", e["seq"], e["kind"].as_str().unwrap_or("?"), e["by"].as_str().unwrap_or("?")))
        .collect();
    for a in d["approvals"].as_array().into_iter().flatten() {
        let what = if a["decision"] == "deny" { "Denied" } else { "Approved" };
        out.push(format!(
            "{what} by {} on {}",
            a["name"].as_str().filter(|n| !n.is_empty()).unwrap_or("an owner"),
            a["device_name"].as_str().unwrap_or("a device")
        ));
    }
    out.join("\n")
}

fn plan_text(d: &Value) -> String {
    match d["plan_text"].as_str() {
        Some(t) => format!("Plan:\n{t}"),
        None => format!("Plan: digest {}", d["plan"].as_str().unwrap_or("?")),
    }
}

/// They act as this machine's owner: only the owner's own agents may.
async fn owners_deploys(c: &Call<'_>, what: &str) -> Result<(), String> {
    if let Some(me) = c.me()
        && c.app().mux.api(|r| Api::GuestBehind(me, r)).await.flatten().is_some()
    {
        return Err(format!("{what} acts as this machine's owner: only their own agents may"));
    }
    Ok(())
}

async fn deploys_call(c: &Call<'_>, method: Method, path: &str, body: Option<Value>) -> Result<Value, String> {
    c.app().control.cloud(method, path, body.as_ref()).await.map_err(said)
}

/// `plan_deploy`: plan, then ask.
pub(crate) async fn plan_deploy(c: &Call<'_>, a: PlanDeployArgs) -> Out {
    owners_deploys(c, PLAN_TOOL).await?;
    let project = project_of(&a.project)?;
    let body = serde_json::to_value(plan_of(&a)?).map_err(|e| e.to_string())?;
    let d = deploys_call(c, Method::POST, &agent_project_deploys(project), Some(body)).await?;
    let id = d["id"].as_str().unwrap_or_default().to_owned();
    let d = match deploys_call(c, Method::POST, &agent_deploy_request(&id), None).await {
        Ok(d) => d,
        Err(e) => {
            return Err(format!(
                "planned {id}, but asking for its approval was refused: {e}. read_deploy reads it; plan it again later to ask."
            ));
        }
    };
    let s = format!(
        "Planned and asked, as your person's agent on this machine: {}. Its owners got a card; one of them approves it on their phone or in a browser, and you can't. read_deploy {id} follows it.\n{}",
        line(&d),
        plan_text(&d)
    );
    done(s, json!({ "deploy": d }))
}

/// `read_deploy`.
pub(crate) async fn read_deploy(c: &Call<'_>, a: ReadDeployArgs) -> Out {
    owners_deploys(c, READ_TOOL).await?;
    let id = a.id.trim();
    if !deploy_id(id) {
        return Err(format!("id: a deploy is dep_ and 32 hex characters, not {id:?}"));
    }
    let d = deploys_call(c, Method::GET, &agent_deploy(id), None).await?;
    let s = format!("{}\n{}\nLog:\n{}", line(&d), plan_text(&d), history(&d));
    done(s, json!({ "deploy": d }))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(commit: &str, repo: &str) -> PlanDeployArgs {
        PlanDeployArgs {
            project: "prj_0a".into(),
            environment: " production ".into(),
            commit: commit.into(),
            repo: repo.into(),
            roll_back: false,
        }
    }

    #[test]
    fn a_plan_is_a_full_commit_of_a_repo() {
        let sha = "0A".repeat(20);
        let p = plan_of(&args(&sha, "acme/app")).unwrap();
        assert_eq!(p.environment, "production");
        assert_eq!((p.source.kind.as_str(), p.source.reference.as_str()), ("commit", "0a".repeat(20).as_str()));
        assert_eq!(p.source.place, "acme/app");
        assert!(plan_of(&args("deadbeef", "acme/app")).unwrap_err().contains("40-hex"));
        for bad in ["acme", "acme/", "/app", "acme/app/x", "acme app/x", "../x"] {
            assert!(plan_of(&args(&sha, bad)).unwrap_err().contains("OWNER/NAME"), "{bad}");
        }
        assert!(project_of("prj_0a").is_ok() && project_of("personal").is_err() && project_of("acme").is_err());
    }

    #[test]
    fn a_deploy_reads_as_a_line_its_plan_and_its_log() {
        let d = json!({ "id": "dep_1", "environment": { "name": "production" }, "state": "pending",
            "source": { "kind": "commit", "ref": "0123456789abcdef0123456789abcdef01234567", "where": "acme/app" },
            "roll_back": null, "plan": "ab", "plan_text": "deploy fake to production\n",
            "events": [{ "seq": 1, "kind": "planned", "by": "alice's agent on geek" },
                       { "seq": 2, "kind": "requested", "by": "alice's agent on geek" }],
            "approvals": [] });
        assert_eq!(line(&d), "dep_1 to production: commit 0123456789ab of acme/app [waiting for an owner's approval]");
        assert_eq!(plan_text(&d), "Plan:\ndeploy fake to production\n");
        assert_eq!(history(&d), "[1] planned by alice's agent on geek\n[2] requested by alice's agent on geek");
        let mut approved = d.clone();
        approved["approvals"] = json!([{ "decision": "once", "name": "alice", "device_name": "Pixel" }]);
        assert!(history(&approved).ends_with("Approved by alice on Pixel"));
        approved["plan_text"] = Value::Null;
        assert_eq!(plan_text(&approved), "Plan: digest ab");
    }

    #[test]
    fn a_refusal_says_who_approves() {
        let e = CloudRefused::Said {
            status: 403,
            said: "geek has no deploys.plan grant over that".into(),
            reason: Some("agent_ungranted".into()),
        };
        let s = said(e);
        assert!(s.contains("no deploys.plan grant") && s.contains("approving stays theirs"), "{s}");
        assert!(said(CloudRefused::NotJoined).contains("hasn't joined"));
    }
}
