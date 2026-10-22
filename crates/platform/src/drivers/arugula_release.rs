//! The arugula-release driver (M221, platform step 7, native Arugula #17):
//! Arugula's own release as a deploy, approved by an owner's device,
//! instead of `just release` from Jake's shell.
//!
//! - **Plan** ([`arugula_e2e::drivers::arugula_release::plan_lines`]): the
//!   commit, the bump and the lane (its settings), what `scripts/release
//!   bump` does with them, the token, and that it never rolls back. Only a
//!   commit of [`REPO`] is planned ([`Driver::refuses`]).
//! - **Only where the platform says so** ([`Driver::allowed_in`]): the one
//!   project `ARUGULA_PLATFORM_RELEASE_PROJECT` names (Arugula's own). No
//!   other project may make an environment for it, plan with it, or have
//!   its token minted, so no one else's approval can push to Arugula.
//! - **Apply** runs on the deploy's box (`arugulad _arugula-release`): it
//!   clones [`REPO`], checks the commit is on `main` and green, runs the
//!   commit's own `scripts/release bump`, which tags and pushes `main` and
//!   the tag atomically, revokes the token, and follows the tag's release
//!   workflow to its end.
//! - **Its token** is minted for the run: an installation token of the
//!   platform's GitHub App, `contents: write` on [`REPO`] alone
//!   ([`Mint::GithubWrite`], [`TOKEN`]). Nothing else: the release
//!   workflow's own secrets stay in GitHub Actions.
//! - **Verify:** the run exited 0, and it reported the tag it pushed (of
//!   the environment's lane) and that the release workflow published it.
//!   The log keeps the tag, the commits, the release's page and the
//!   workflow's run.
//! - **No roll back:** a bad release is fixed by a new patch release.

use arugula_e2e::drivers::{
    arugula_release::{self as ar, Lane, REPO, TOKEN},
    word,
};
use serde_json::{Map, Value};

use super::{Driver, Mint, Verdict, exited_0};
use crate::db::{
    deploys::{Deploy, Environment, Source},
    runs::Run,
};

/// Its name, as environments name it.
pub const NAME: &str = ar::DRIVER;

/// The platform's environment variable that names the project (or
/// projects, comma separated) whose environments may release Arugula.
pub const PROJECT_VAR: &str = "ARUGULA_PLATFORM_RELEASE_PROJECT";

pub struct ArugulaRelease;

/// Tests: projects allowed besides what [`PROJECT_VAR`] names (each test's
/// own, so tests don't share an environment variable).
#[cfg(test)]
pub static TEST_PROJECTS: std::sync::Mutex<Vec<String>> = std::sync::Mutex::new(Vec::new());

/// Whether `project` is one `named` (the variable's value) lists: none
/// when it's unset or empty.
pub fn project_named(named: Option<&str>, project: &str) -> bool {
    named.is_some_and(|n| n.split(',').map(str::trim).any(|p| !p.is_empty() && p == project))
}

/// A string in `v` at `k`, if it's one word.
fn at<'a>(v: &'a Value, k: &str) -> Option<&'a str> {
    v.get(k)?.as_str().filter(|s| word(s))
}

/// Whether `s` is a full commit id.
fn commit_ok(s: &str) -> bool {
    s.len() == 40 && s.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
}

/// The tag the box reported it pushed, if it's one of the environment's
/// lane.
fn tag<'a>(env: &Environment, reported: &'a Value) -> Option<&'a str> {
    at(reported, "tag").filter(|t| ar::tag_version(Lane::of(&env.settings), t).is_some())
}

impl Driver for ArugulaRelease {
    fn name(&self) -> &'static str {
        NAME
    }

    fn refuses(&self, source: &Source) -> Option<String> {
        ar::refuses(source.kind.as_str(), &source.reference, &source.place)
    }

    fn allowed_in(&self, project: &str) -> Result<(), String> {
        #[cfg(test)]
        if TEST_PROJECTS.lock().unwrap_or_else(|e| e.into_inner()).iter().any(|p| p == project) {
            return Ok(());
        }
        if project_named(std::env::var(PROJECT_VAR).ok().as_deref(), project) {
            Ok(())
        } else {
            Err(format!("the {NAME} driver releases Arugula itself: only Arugula's own project may use it"))
        }
    }

    fn mints(&self, _: &Environment) -> Vec<Mint> {
        vec![Mint::GithubWrite { name: TOKEN, repo: REPO }]
    }

    fn verify(&self, env: &Environment, _: &Deploy, run: &Run, reported: &Value) -> Verdict {
        if let Verdict::Unhealthy(why) = exited_0(run) {
            return Verdict::Unhealthy(why);
        }
        let Some(t) = tag(env, reported) else {
            return Verdict::Unhealthy("the apply didn't say which tag it pushed".into());
        };
        if !at(reported, "commit").is_some_and(commit_ok) {
            return Verdict::Unhealthy("the apply didn't say which commit it tagged".into());
        }
        if reported["published"] != Value::Bool(true) || at(reported, "release") != Some(ar::release_url(t).as_str()) {
            return Verdict::Unhealthy(format!("the release workflow didn't publish {t}"));
        }
        Verdict::Healthy
    }

    fn kept(&self, env: &Environment, _: &Deploy, reported: &Value) -> Map<String, Value> {
        let mut m = Map::new();
        let Some(t) = tag(env, reported) else { return m };
        m.insert("tag".into(), t.into());
        m.insert("release".into(), ar::release_url(t).into());
        for k in ["commit", "main"] {
            if let Some(c) = at(reported, k).filter(|c| commit_ok(c)) {
                m.insert(k.into(), c.into());
            }
        }
        let wf = &reported["workflow"];
        if let Some(u) = at(wf, "run").filter(|u| ar::run_url_ok(u)) {
            m.insert("workflow_run".into(), u.into());
        }
        if let Some(c) = at(wf, "conclusion").filter(|c| c.len() <= 40) {
            m.insert("workflow_conclusion".into(), c.into());
        }
        m.insert("published".into(), (reported["published"] == Value::Bool(true)).into());
        m
    }
}
