//! planter's drivers (M233, platform step 8, native Arugula #31; aru-104,
//! aru-108): planter's Prod, Go live, ship and promote, each a driver an
//! environment names, deploying to home-cloud's app platform through the
//! same GitOps repositories planter wrote (`arugula-salad/planter-apps`,
//! `arugula-salad/shipped`). What each does and its plan are
//! [`arugula_e2e::deploy::planter`]; the box's half is arugulad's
//! `_planter` (`work_planter.rs`), with planter's own renderer, vendored.
//!
//! - **Credentials:** installation tokens of planter's GitHub App, the
//!   deploy App ([`Mint::Planter`]), one repository each, minted for the
//!   run and revoked when it ends. The platform's own App is never asked
//!   (aru-108). Nothing is the environment's own: they hold no secrets.
//! - **Approval:** as every deploy's, an owner's device over the plan's
//!   bytes (aru-101). An agent's ship (M230's agent tokens with
//!   `deploys.plan`) plans and asks; it never approves.
//! - **Verify:** the run exited 0 (the box waited for the app to answer, or
//!   opened its pull request), and for the drivers that commit or open a
//!   pull request, what it reported on fd 3 is of the shape it must be: the
//!   commit it pushed, the pull request it opened, in the repository the
//!   plan names. The log keeps them ([`Driver::kept`]).
//! - **Roll back:** Go live only, as apps-platform's: a revert commit of
//!   the pointer. A ship goes back by shipping the old source again; a
//!   promotion by closing its pull request.
//! - **A shipped app's name is one project's** ([`Driver::claims`]): the
//!   first project to make a Ship or Promote environment for it has it;
//!   another is refused, when it makes one and when it plans. The box
//!   checks the same in `shipped`'s history: it won't ship over an app
//!   another project (or planter, before the platform) shipped last.

use arugula_e2e::deploy::planter::{self as p, POINTERS_REPO, SHIPPED_REPO};
use serde_json::{Map, Value};

use super::{Driver, Mint, Verdict, exited_0, mint::PlanterRepo};
use crate::{
    Platform,
    db::{
        deploys::{Deploy, Environment, Source},
        runs::Run,
    },
    forge::DeployScope,
};

pub struct Prod;
pub struct GoLive;
pub struct Ship;
pub struct Promote;

/// A full commit id.
fn commit_ok(s: &str) -> bool {
    s.len() == 40 && s.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
}

/// The pull request the run reported, if it's one of `repo`'s:
/// `https://github.com/<repo>/pull/<n>`.
fn pull_of<'a>(repo: &str, reported: &'a Value) -> Option<&'a str> {
    reported["pull"].as_str().filter(|u| {
        u.strip_prefix(&format!("https://github.com/{repo}/pull/"))
            .is_some_and(|n| (1..=9).contains(&n.len()) && n.bytes().all(|b| b.is_ascii_digit()) && !n.starts_with('0'))
    })
}

/// The commit the run reported pushing, if it's a full commit id.
fn commit_of(reported: &Value) -> Option<&str> {
    reported["commit"].as_str().filter(|c| commit_ok(c))
}

/// Healthy when the run exited 0 and reported what `want` finds.
fn exited_with(run: &Run, found: Option<&str>, what: &str) -> Verdict {
    match exited_0(run) {
        Verdict::Healthy if found.is_none() => Verdict::Unhealthy(format!("the apply didn't report {what}")),
        v => v,
    }
}

fn keep(pairs: &[(&str, Option<&str>)]) -> Map<String, Value> {
    pairs.iter().filter_map(|(k, v)| v.map(|v| ((*k).to_owned(), Value::from(v)))).collect()
}

/// Why `env` (a Ship or Promote environment, being made or planned to)
/// can't have its app: another project's Ship or Promote environment has
/// it.
fn claimed(app: &Platform, env: &Environment) -> Option<String> {
    let name = env.settings.get(p::APP)?;
    for driver in [p::SHIP, p::PROMOTE] {
        let all = match app.db.environments_driven_by(driver) {
            Ok(all) => all,
            Err(e) => return Some(format!("couldn't read who has {name}: {e:#}")),
        };
        if all.iter().any(|e| e.project != env.project && e.settings.get(p::APP) == Some(name)) {
            return Some(format!(
                "the shipped app {name} is another project's: name yours otherwise (its host is \
                 https://<app>.{})",
                p::LOAM_DOMAIN
            ));
        }
    }
    None
}

impl Driver for Prod {
    fn name(&self) -> &'static str {
        p::PROD
    }

    fn refuses(&self, source: &Source) -> Option<String> {
        p::refuses(p::PROD, source.kind.as_str(), &source.reference, &source.place)
    }

    fn mints(&self, _: &Environment) -> Vec<Mint> {
        vec![Mint::Planter { name: p::APP_TOKEN, repo: PlanterRepo::Source, scope: DeployScope::Scaffold }]
    }

    fn verify(&self, _: &Environment, d: &Deploy, run: &Run, reported: &Value) -> Verdict {
        exited_with(run, pull_of(&d.source.place, reported), "the pull request it opened")
    }

    fn kept(&self, _: &Environment, d: &Deploy, reported: &Value) -> Map<String, Value> {
        keep(&[("pull", pull_of(&d.source.place, reported)), ("commit", commit_of(reported))])
    }
}

impl Driver for GoLive {
    fn name(&self) -> &'static str {
        p::GO_LIVE
    }

    fn refuses(&self, source: &Source) -> Option<String> {
        p::refuses(p::GO_LIVE, source.kind.as_str(), &source.reference, &source.place)
    }

    fn mints(&self, _: &Environment) -> Vec<Mint> {
        vec![
            Mint::Planter {
                name: p::POINTERS_TOKEN,
                repo: PlanterRepo::Named(POINTERS_REPO),
                scope: DeployScope::Contents,
            },
            Mint::Planter { name: p::SOURCE_TOKEN, repo: PlanterRepo::Source, scope: DeployScope::Read },
        ]
    }

    /// The box waited for the app to answer before it exited 0.
    fn verify(&self, _: &Environment, _: &Deploy, run: &Run, _: &Value) -> Verdict {
        exited_0(run)
    }

    fn kept(&self, _: &Environment, _: &Deploy, reported: &Value) -> Map<String, Value> {
        keep(&[("commit", commit_of(reported))])
    }

    fn rolls_back(&self) -> bool {
        true
    }
}

impl Driver for Ship {
    fn name(&self) -> &'static str {
        p::SHIP
    }

    fn refuses(&self, source: &Source) -> Option<String> {
        p::refuses(p::SHIP, source.kind.as_str(), &source.reference, &source.place)
    }

    fn mints(&self, _: &Environment) -> Vec<Mint> {
        vec![
            Mint::Planter {
                name: p::SHIPPED_TOKEN,
                repo: PlanterRepo::Named(SHIPPED_REPO),
                scope: DeployScope::Contents,
            },
            Mint::Planter {
                name: p::POINTERS_TOKEN,
                repo: PlanterRepo::Named(POINTERS_REPO),
                scope: DeployScope::Contents,
            },
        ]
    }

    fn claims(&self, app: &Platform, env: &Environment) -> Option<String> {
        claimed(app, env)
    }

    fn verify(&self, _: &Environment, _: &Deploy, run: &Run, reported: &Value) -> Verdict {
        exited_with(run, commit_of(reported), "the commit it pushed to arugula-salad/shipped")
    }

    fn kept(&self, _: &Environment, _: &Deploy, reported: &Value) -> Map<String, Value> {
        keep(&[("commit", commit_of(reported))])
    }
}

impl Driver for Promote {
    fn name(&self) -> &'static str {
        p::PROMOTE
    }

    fn refuses(&self, source: &Source) -> Option<String> {
        p::refuses(p::PROMOTE, source.kind.as_str(), &source.reference, &source.place)
    }

    fn mints(&self, _: &Environment) -> Vec<Mint> {
        vec![Mint::Planter { name: p::SHIPPED_TOKEN, repo: PlanterRepo::Named(SHIPPED_REPO), scope: DeployScope::Pulls }]
    }

    fn claims(&self, app: &Platform, env: &Environment) -> Option<String> {
        claimed(app, env)
    }

    fn verify(&self, _: &Environment, _: &Deploy, run: &Run, reported: &Value) -> Verdict {
        exited_with(run, pull_of(SHIPPED_REPO, reported), "the pull request it opened")
    }

    fn kept(&self, _: &Environment, _: &Deploy, reported: &Value) -> Map<String, Value> {
        keep(&[("pull", pull_of(SHIPPED_REPO, reported)), ("commit", commit_of(reported))])
    }
}

#[cfg(test)]
#[path = "planter_tests.rs"]
mod tests;
