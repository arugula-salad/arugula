//! `arugula deploys`: a project's deploys on Arugula control (M252). `envs`
//! lists its environments, `plan` plans a deploy of a commit to one of
//! them (the platform makes the plan, from the environment's driver),
//! `request` asks its owners to approve it, and `show` reads one: its
//! plan's text, state and log. Through control, as this CLI's account
//! (`arugula login`): an owner or an editor plans and asks.
//!
//! **No approving here, and no secrets.** An owner approves or denies a
//! deploy on their phone or in a browser, with a passkey: a CLI's login
//! never counts (its key is a file an agent on this computer could read).
//! An environment's secrets are set on the platform page only, and never
//! read back. Making an environment is the page's too (Project settings,
//! Environments), where its driver's settings are asked for.
//!
//! An agent (CLAUDECODE or AI_AGENT set) reads deploys here, but doesn't
//! plan or ask: this CLI acts as your person, and an agent plans through
//! Arugula's MCP `plan_deploy`, under its machine's `deploys.plan` grant,
//! labelled as the agent's.

use anyhow::bail;
use arugula_control_wire::deploys::{DeployPlan, DeploySource, deploy_id, full_commit};
use serde_json::Value;

use crate::control::Enrolled;
use crate::http;
use crate::util::print_json;

#[derive(clap::Subcommand)]
pub enum DeploysCmd {
    /// A project's environments: their drivers, settings and secrets' names.
    ///
    /// Never a secret's value: those are set on the platform page and never
    /// read back.
    #[command(visible_alias = "environments")]
    Envs {
        #[arg(long)]
        project: Option<String>,
    },
    /// Plan a deploy of a commit to an environment, and print its plan.
    ///
    /// The platform makes the plan from the environment's driver. Needs
    /// owner or editor on the project. `request` then asks its owners to
    /// approve it.
    Plan {
        /// The environment: its name, or its id (`env_…`).
        #[arg(long)]
        env: String,
        /// The commit, by its full 40-hex id.
        #[arg(long)]
        commit: String,
        /// Where it is: `OWNER/NAME` on GitHub.
        #[arg(long)]
        repo: String,
        /// Back to a commit the environment ran before, with what it ran.
        #[arg(long)]
        roll_back: bool,
        #[arg(long)]
        project: Option<String>,
    },
    /// Ask the project's owners to approve a planned deploy.
    ///
    /// Each gets a card, and approves it on their phone or in a browser.
    Request {
        /// The deploy (`dep_…`).
        id: String,
    },
    /// A deploy: where it goes, its plan's text, its state and its log.
    Show {
        /// The deploy (`dep_…`).
        id: String,
    },
}

/// `--project`: an id, a name, or (without it) your personal one.
fn project(e: &Enrolled, p: Option<&str>) -> anyhow::Result<String> {
    match p {
        Some(p) => super::cloud::project_of(e, Some(p)),
        None => personal(&e.api("GET", "/api/projects", None)?),
    }
}

/// The account's own project, in `GET /api/projects`' answer.
fn personal(listed: &Value) -> anyhow::Result<String> {
    listed["projects"]
        .as_array()
        .into_iter()
        .flatten()
        .find(|p| p["team"].is_null())
        .and_then(|p| p["id"].as_str())
        .map(str::to_owned)
        .ok_or_else(|| anyhow::anyhow!("you have no personal project here: name one with --project"))
}

fn checked_id(id: &str) -> anyhow::Result<&str> {
    let id = id.trim();
    if !deploy_id(id) {
        bail!("a deploy is dep_ and 32 hex characters, not {id:?}");
    }
    Ok(id)
}

/// `OWNER/NAME`.
fn repo_ok(r: &str) -> bool {
    let part = |s: &str| {
        (1..=100).contains(&s.len())
            && !s.starts_with('.')
            && s.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.'))
    };
    r.split_once('/').is_some_and(|(o, n)| part(o) && part(n))
}

/// What `plan` sends: `Err` why not.
fn plan_body(env: &str, commit: &str, repo: &str, roll_back: bool) -> anyhow::Result<DeployPlan> {
    let commit = commit.trim().to_ascii_lowercase();
    if !full_commit(&commit) {
        bail!("--commit is a full 40-hex commit id (`git rev-parse HEAD`), not {commit:?}");
    }
    if !repo_ok(repo.trim()) {
        bail!("--repo is OWNER/NAME on GitHub, not {repo:?}");
    }
    Ok(DeployPlan {
        environment: env.trim().to_owned(),
        source: DeploySource { kind: "commit".into(), reference: commit, place: repo.trim().to_owned() },
        roll_back,
    })
}

pub fn run(cmd: DeploysCmd, json_out: bool) -> anyhow::Result<i32> {
    let Some(e) = Enrolled::load()? else {
        bail!("this CLI isn't logged in to Arugula control: `arugula login` first");
    };
    if matches!(cmd, DeploysCmd::Plan { .. } | DeploysCmd::Request { .. }) && http::agent() {
        bail!(
            "an agent doesn't plan or ask for deploys through this CLI, which acts as your person: it reads them \
             here (`arugula deploys envs|show`), and plans with Arugula's MCP plan_deploy, under its machine's \
             deploys.plan grant. Only an owner approves, on their phone or in a browser"
        );
    }
    let d = match cmd {
        DeploysCmd::Envs { project: p } => {
            let listed = e.api("GET", &format!("/api/projects/{}/environments", project(&e, p.as_deref())?), None)?;
            if json_out {
                print_json(&listed);
                return Ok(0);
            }
            let envs = listed["environments"].as_array().cloned().unwrap_or_default();
            if envs.is_empty() {
                println!("No environments: an owner makes one on the platform page, in the project's settings.");
            }
            for env in &envs {
                println!("{}", env_lines(env));
            }
            return Ok(0);
        }
        DeploysCmd::Plan { env, commit, repo, roll_back, project: p } => {
            let body = serde_json::to_value(plan_body(&env, &commit, &repo, roll_back)?)?;
            let d = e.api("POST", &format!("/api/projects/{}/deploys", project(&e, p.as_deref())?), Some(&body))?;
            if !json_out {
                print!("{}", deploy_text(&d));
                println!("`arugula deploys request {}` asks its owners to approve it.", t(&d, "id"));
                return Ok(0);
            }
            d
        }
        DeploysCmd::Request { id } => {
            let d = e.api("POST", &format!("/api/deploys/{}/request", checked_id(&id)?), None)?;
            if !json_out {
                println!("{}", deploy_line(&d));
                println!("Its owners got a card: one of them approves it on their phone or in a browser.");
                return Ok(0);
            }
            d
        }
        DeploysCmd::Show { id } => {
            let d = e.api("GET", &format!("/api/deploys/{}", checked_id(&id)?), None)?;
            if !json_out {
                print!("{}", deploy_text(&d));
                return Ok(0);
            }
            d
        }
    };
    print_json(&d);
    Ok(0)
}

fn t(v: &Value, k: &str) -> String {
    v[k].as_str().unwrap_or_default().to_owned()
}

/// `production  env_…  chant-fly` and its settings and secrets' names.
fn env_lines(env: &Value) -> String {
    let mut s = format!("{:<16} {}  {}", t(env, "name"), t(env, "id"), t(env, "driver"));
    if let Some(m) = env["settings"].as_object().filter(|m| !m.is_empty()) {
        let kv: Vec<String> = m.iter().map(|(k, v)| format!("{k}={}", v.as_str().unwrap_or_default())).collect();
        s += &format!("\n  settings {}", kv.join(" "));
    }
    let names: Vec<&str> = env["secret_names"].as_array().into_iter().flatten().filter_map(Value::as_str).collect();
    if !names.is_empty() {
        s += &format!("\n  secrets {}", names.join(" "));
    }
    s
}

/// `dep_…  pending  to production: commit 0123456789ab of acme/app`.
fn deploy_line(d: &Value) -> String {
    let short: String = t(&d["source"], "ref").chars().take(12).collect();
    let back = if d["roll_back"].is_null() { "" } else { " (roll back)" };
    format!(
        "{}  {}  to {}: {} {short} of {}{back}",
        t(d, "id"),
        t(d, "state"),
        t(&d["environment"], "name"),
        t(&d["source"], "kind"),
        t(&d["source"], "where")
    )
}

/// A deploy as `show` prints it: its line, who planned it, its plan's text,
/// its approvals and its log.
fn deploy_text(d: &Value) -> String {
    let mut s = format!("{}\n", deploy_line(d));
    let who = Some(t(d, "planned_by_name")).filter(|n| !n.is_empty()).unwrap_or_else(|| "a deleted account".into());
    match d["agent"]["machine_name"].as_str() {
        Some(m) => s += &format!("  planned by {who}'s agent on {m}\n"),
        None => s += &format!("  planned by {who}\n"),
    }
    match d["plan_text"].as_str() {
        Some(p) => {
            s += "  plan:\n";
            for l in p.lines() {
                s += &format!("    {l}\n");
            }
        }
        None => s += &format!("  plan digest {}\n", t(d, "plan")),
    }
    for a in d["approvals"].as_array().into_iter().flatten() {
        let what = if a["decision"] == "deny" { "denied" } else { "approved" };
        s += &format!("  {what} by {} on {}\n", t(a, "name"), t(a, "device_name"));
    }
    for ev in d["events"].as_array().into_iter().flatten() {
        s += &format!("  [{}] {} by {}\n", ev["seq"], t(ev, "kind"), t(ev, "by"));
    }
    if let Some(at) = d["log"]["broken_at"].as_u64() {
        s += &format!("  the environment's log doesn't verify: it breaks at {at}\n");
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;
    use serde_json::json;

    #[test]
    fn commands_parse() {
        let sha = "0123456789abcdef0123456789abcdef01234567";
        for argv in [
            &["arugula", "deploys", "envs"][..],
            &["arugula", "deploys", "environments", "--project", "Acme"],
            &["arugula", "deploys", "plan", "--env", "production", "--commit", sha, "--repo", "acme/app"],
            &["arugula", "deploys", "plan", "--env", "env_1", "--commit", sha, "--repo", "acme/app", "--roll-back"],
            &["arugula", "deploys", "plan", "--project", "prj_1", "--env", "x", "--commit", sha, "--repo", "a/b"],
            &["arugula", "deploys", "request", "dep_1"],
            &["arugula", "deploys", "show", "dep_1"],
        ] {
            assert!(crate::Cli::try_parse_from(argv).is_ok(), "{argv:?}");
        }
        for argv in [
            // No approving or denying from a CLI, and no secrets.
            &["arugula", "deploys", "approve", "dep_1"][..],
            &["arugula", "deploys", "deny", "dep_1"],
            &["arugula", "deploys", "secret", "env_1", "FLY_API_TOKEN"],
            &["arugula", "deploys", "plan", "--env", "production", "--commit", sha],
            &["arugula", "deploys", "plan", "--commit", sha, "--repo", "acme/app"],
        ] {
            assert!(crate::Cli::try_parse_from(argv).is_err(), "{argv:?}");
        }
    }

    #[test]
    fn a_plan_is_a_full_commit_of_a_repo() {
        let sha = "0123456789ABCDEF0123456789ABCDEF01234567";
        let p = plan_body(" production ", sha, "acme/app", true).unwrap();
        assert_eq!(p.environment, "production");
        assert_eq!(p.source.reference, sha.to_ascii_lowercase());
        assert!(p.roll_back);
        assert!(plan_body("p", "deadbeef", "acme/app", false).unwrap_err().to_string().contains("40-hex"));
        assert!(plan_body("p", sha, "acme", false).unwrap_err().to_string().contains("OWNER/NAME"));
        assert!(checked_id("dep_1").is_err() && checked_id(&format!(" dep_{} ", "a".repeat(32))).is_ok());
        let listed =
            json!({ "projects": [{ "id": "prj_t", "team": { "id": "t" } }, { "id": "prj_me", "team": null }] });
        assert_eq!(personal(&listed).unwrap(), "prj_me");
        assert!(personal(&json!({ "projects": [] })).is_err());
    }

    #[test]
    fn a_deploy_reads_as_its_plan_and_log() {
        let d = json!({ "id": "dep_1", "state": "pending", "environment": { "name": "production" },
            "source": { "kind": "commit", "ref": "0123456789abcdef0123456789abcdef01234567", "where": "acme/app" },
            "roll_back": null, "planned_by_name": "alice", "agent": { "machine": "m", "machine_name": "geek" },
            "plan": "ab", "plan_text": "deploy fake to production\nsetting x y\n",
            "approvals": [{ "decision": "deny", "name": "bob", "device_name": "Pixel" }],
            "events": [{ "seq": 1, "kind": "planned", "by": "alice's agent on geek" }], "log": { "verified": 1 } });
        assert_eq!(
            deploy_text(&d),
            "dep_1  pending  to production: commit 0123456789ab of acme/app\n  planned by alice's agent on geek\n  \
             plan:\n    deploy fake to production\n    setting x y\n  denied by bob on Pixel\n  [1] planned by \
             alice's agent on geek\n"
        );
        let env = json!({ "name": "production", "id": "env_1", "driver": "chant-fly",
            "settings": { "fly_app": "acme" }, "secret_names": ["FLY_API_TOKEN"] });
        assert_eq!(
            env_lines(&env),
            "production       env_1  chant-fly\n  settings fly_app=acme\n  secrets FLY_API_TOKEN"
        );
    }
}
