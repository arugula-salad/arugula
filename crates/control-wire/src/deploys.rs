//! Deploys for a machine's agents (M218's routes, called by arugulad's
//! `plan_deploy` and `read_deploy`, M252): plan one and ask for its
//! approval, and read it. Under the machine's `deploys.plan` grant over
//! the project, asked on every call. There's no route here that approves
//! or denies: only a project owner's device does, on their phone or in a
//! browser.

use serde::{Deserialize, Serialize};

/// `POST` [`DeployPlan`]: plan a deploy in a project, as the machine's
/// owner's agent. Answered with the deploy, `planned`.
pub const AGENT_PROJECT_DEPLOYS: &str = "/api/daemon/projects/{project}/deploys";
/// `GET`: a deploy, its plan's text, state and log.
pub const AGENT_DEPLOY: &str = "/api/daemon/deploys/{id}";
/// `POST`: ask the project's owners to approve it (`pending`).
pub const AGENT_DEPLOY_REQUEST: &str = "/api/daemon/deploys/{id}/request";

/// [`AGENT_PROJECT_DEPLOYS`] for `project`.
pub fn agent_project_deploys(project: &str) -> String {
    format!("/api/daemon/projects/{project}/deploys")
}

/// [`AGENT_DEPLOY`] for `id`.
pub fn agent_deploy(id: &str) -> String {
    format!("/api/daemon/deploys/{id}")
}

/// [`AGENT_DEPLOY_REQUEST`] for `id`.
pub fn agent_deploy_request(id: &str) -> String {
    format!("/api/daemon/deploys/{id}/request")
}

/// What a deploy deploys: a commit (by its full id) of a repository.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct DeploySource {
    /// `commit` or `artifact`.
    pub kind: String,
    #[serde(rename = "ref")]
    pub reference: String,
    /// `OWNER/NAME`.
    #[serde(rename = "where")]
    pub place: String,
}

/// A plan, as the platform's plan routes take it (the person's and the
/// agent's). The platform makes the plan's digest itself.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct DeployPlan {
    /// The environment's id (`env_…`), or its name in the project.
    pub environment: String,
    pub source: DeploySource,
    /// Back to a source the environment ran before, with what it ran.
    #[serde(default)]
    pub roll_back: bool,
}

/// Whether `sha` is a full commit id: 40 lowercase hex.
pub fn full_commit(sha: &str) -> bool {
    sha.len() == 40 && sha.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
}

/// Whether `id` is a deploy's: `dep_` and 32 hex.
pub fn deploy_id(id: &str) -> bool {
    id.strip_prefix("dep_").is_some_and(|h| h.len() == 32 && h.bytes().all(|b| b.is_ascii_hexdigit()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_plan_is_what_the_platform_takes() {
        let p = DeployPlan {
            environment: "production".into(),
            source: DeploySource { kind: "commit".into(), reference: "a".repeat(40), place: "acme/app".into() },
            roll_back: false,
        };
        let v = serde_json::to_value(&p).unwrap();
        assert_eq!(
            v,
            serde_json::json!({ "environment": "production", "roll_back": false,
                "source": { "kind": "commit", "ref": "a".repeat(40), "where": "acme/app" } })
        );
        assert_eq!(agent_project_deploys("prj_1"), AGENT_PROJECT_DEPLOYS.replace("{project}", "prj_1"));
        assert_eq!(agent_deploy("dep_1"), AGENT_DEPLOY.replace("{id}", "dep_1"));
        assert_eq!(agent_deploy_request("dep_1"), AGENT_DEPLOY_REQUEST.replace("{id}", "dep_1"));
        assert!(full_commit(&"0f".repeat(20)) && !full_commit("deadbeef") && !full_commit(&"A".repeat(40)));
        assert!(deploy_id(&format!("dep_{}", "a".repeat(32))) && !deploy_id("dep_1") && !deploy_id("run_1"));
    }
}
