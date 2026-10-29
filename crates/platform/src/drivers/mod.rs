//! Drivers (M219, platform step 7, native Arugula #15; aru-102 "A driver
//! is a run with a credential"): what applies a deploy to its target.
//!
//! A driver has two halves:
//! - **the platform's** ([`Driver`]): the plan a deploy names (its text and
//!   digest), the short-lived tokens it mints at apply ([`Mint`]), checking
//!   what the apply did ([`Driver::verify`]), and whether its target rolls
//!   back ([`Driver::rolls_back`]: to the environment's last deploy that
//!   succeeded, as a new deploy, approved again);
//! - **the box's**: the apply itself, which runs on the project's hosted
//!   workspace as a deploy's run (`kind deploy`, `arugulad --hosted`'s
//!   `deploy_run`), with the environment's secrets for that run only.
//!
//! **Credentials.** An environment's secrets are sealed per project
//! (M209's `keys`, `environment/<env>/<NAME>`) and given to the run's
//! machine once, while the run runs ([`crate::deploys::apply::secrets`]).
//! A driver that can mint a short-lived token from a long-lived one the
//! platform keeps says so ([`Driver::mints`]): the box gets only the short
//! one. Never a kubeconfig.
//!
//! **Settings** (M220): what a driver needs that isn't secret (chant→Fly's
//! Fly app, where its chant op is) is the environment's, checked when it's
//! made ([`Driver::settings`]); the plan names each, so the approval
//! covers them, and a box checks them again
//! (`arugula_e2e::drivers::planned`).
//!
//! **What the apply reported** (M220): a driver's box half ends its run
//! with what it did (its run's last event's `result`): the driver checks it
//! ([`Driver::verify`]), the deploy's log keeps what it says
//! ([`Driver::kept`]), and the run links chant's ledger entry
//! ([`Driver::ledger`], aru-098).
//!
//! **Roll back** (M220): a deploy back to a source the environment ran
//! before, planned ([`Driver::plan_roll_back`]) and approved as any other,
//! from what that deploy's log kept ([`Driver::roll_back_to`]).
//!
//! **chant→Fly** ([`chant_fly::ChantFly`]) is the first real driver.
//! **arugula-release** ([`arugula_release::ArugulaRelease`], M221) is
//! Arugula's own release, for Arugula's own project only
//! ([`Driver::allowed_in`]).
//!
//! **planter's** ([`planter`], M233): Prod, Go live, Ship and Promote,
//! through planter's GitHub App, the deploy App (aru-108).
//!
//! **The fake driver** ([`fake::Fake`], `fake`) is for tests and testnet:
//! environments may name it only where the platform says so
//! (`ARUGULA_PLATFORM_FAKE_DRIVER=1`), and boxes run it only where they
//! say so too.

pub mod apps_platform;
pub mod arugula_release;
pub mod chant_fly;
pub mod fake;
pub mod mint;
pub mod planter;

use arugula_e2e::drivers::{RollBack, Settings};
use serde_json::{Map, Value};
use sha2::{Digest as _, Sha256};

pub use mint::{Held, Mint, mint, revoke_run};

use crate::{
    Platform,
    db::{
        deploys::{Deploy, Environment, Source},
        runs::Run,
    },
};

/// A plan: what a person reads before they approve, and its SHA-256 (hex),
/// which the deploy and its approval name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Planned {
    pub text: String,
    pub digest: String,
}

impl Planned {
    pub fn of(text: String) -> Self {
        let digest = hex::encode(Sha256::digest(text.as_bytes()));
        Planned { text, digest }
    }
}

/// What checking an applied deploy found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Verdict {
    /// It did what it was asked.
    Healthy,
    /// It didn't, and why: the deploy fails, and its owners are told.
    Unhealthy(String),
}

/// A driver's platform half. The apply runs on the box.
pub trait Driver: Send + Sync {
    /// As environments name it (`chant-fly`).
    fn name(&self) -> &'static str;

    /// The plan for deploying `source` to `env`: what a person reads
    /// before they approve, its digest the deploy's `plan`.
    fn plan(&self, env: &Environment, source: &Source) -> Planned {
        plan_with(self.name(), env, source, None)
    }

    /// Its settings on an environment, defaults filled in: `Err` why
    /// they're not ones it takes.
    fn settings(&self, given: &Settings) -> Result<Settings, String> {
        arugula_e2e::drivers::settings(self.name(), given)
    }

    /// The plan for rolling `env` back to `source`, which `to` (an earlier
    /// deploy of it that succeeded) ran: `None` when its target doesn't
    /// roll back.
    fn plan_roll_back(&self, env: &Environment, source: &Source, to: &RollBack) -> Option<Planned> {
        (self.rolls_back() && arugula_e2e::drivers::roll_back_ok(self.name(), &env.settings, to))
            .then(|| plan_with(self.name(), env, source, Some(to)))
    }

    /// What a roll back to a deploy goes back to, from what its log kept
    /// when it succeeded ([`Driver::kept`]): `None` when it can't.
    fn roll_back_to(&self, kept: &Value) -> Option<Settings> {
        let _ = kept;
        self.rolls_back().then(Settings::new)
    }

    /// The secret names it takes (`arugula_e2e::deploy::driver_secrets`),
    /// the ones it mints ([`Driver::mints`]) among them: a box hands it only
    /// these, and an environment holds only these, less the ones it mints
    /// whatever the environment holds.
    fn secret_names(&self) -> &'static [&'static str] {
        arugula_e2e::deploy::driver_secrets(self.name()).unwrap_or_default()
    }

    /// Why it can't deploy `source` at all, said when it's planned: `None`
    /// when it can.
    fn refuses(&self, source: &Source) -> Option<String> {
        let _ = source;
        None
    }

    /// Whether `project`'s environments may use it at all (M221): checked
    /// when an environment is made, when a deploy is planned, and before
    /// anything is minted for its run. `Err` why not.
    fn allowed_in(&self, project: &str) -> Result<(), String> {
        let _ = project;
        Ok(())
    }

    /// Why `env` (one being made, or one a deploy is planned to) can't
    /// have what its settings name, given what the platform holds (M233: a
    /// shipped app's name is one project's): `None` when it can.
    fn claims(&self, app: &Platform, env: &Environment) -> Option<String> {
        let _ = (app, env);
        None
    }

    /// The short-lived tokens it mints at apply, each from a long-lived
    /// credential the platform keeps, given to the run with the
    /// environment's secrets.
    fn mints(&self, env: &Environment) -> Vec<Mint> {
        let _ = env;
        vec![]
    }

    /// Whether `d` (to `env`), applied by `run` (ended `succeeded`), did
    /// what it was asked, by what the run reported it did (`Null` when
    /// nothing).
    fn verify(&self, env: &Environment, d: &Deploy, run: &Run, reported: &Value) -> Verdict;

    /// What the deploy's log keeps of what its run reported: checked, each
    /// of the shape it must have for this environment and deploy.
    fn kept(&self, env: &Environment, d: &Deploy, reported: &Value) -> Map<String, Value> {
        let _ = (env, d, reported);
        Map::new()
    }

    /// The entry in chant's ledger the run wrote (aru-098), which the run
    /// links: `None` for a driver that doesn't write one, or one that isn't
    /// a link to the deploy's own repository.
    fn ledger(&self, env: &Environment, d: &Deploy, reported: &Value) -> Option<String> {
        let _ = (env, d, reported);
        None
    }

    /// Whether its target rolls back: a deploy of the environment's last
    /// source that succeeded, planned and approved as any other is.
    fn rolls_back(&self) -> bool {
        false
    }
}

/// A driver whose box half isn't built yet: environments may name it,
/// deploys may be planned and approved, and a box refuses to apply it
/// until its driver lands. (M220's chant→Fly and M222's apps-platform are
/// both built now.)
///
/// Each driver's target must refuse a deploy id it applied (`DEPLOY_ID`),
/// since each deploy's box is new and can't know: chant→Fly by chant's
/// ledger, apps-platform by the `Arugula-Deploy: <id>` trailer.
pub struct Unbuilt(pub &'static str);

impl Driver for Unbuilt {
    fn name(&self) -> &'static str {
        self.0
    }

    fn verify(&self, _: &Environment, _: &Deploy, run: &Run, _: &Value) -> Verdict {
        exited_0(run)
    }
}

/// The plan every driver here makes ([`arugula_e2e::drivers::plan_text`]):
/// it names the driver, the environment's settings and the roll back, so
/// the approval (over its digest) binds them, and a box recomputes it
/// before it applies.
pub fn plan_with(driver: &str, env: &Environment, source: &Source, roll_back: Option<&RollBack>) -> Planned {
    Planned::of(arugula_e2e::drivers::plan_text(
        driver,
        &env.name,
        source.kind.as_str(),
        &source.reference,
        &source.place,
        &env.settings,
        roll_back,
    ))
}

/// Healthy when the run exited 0.
pub fn exited_0(run: &Run) -> Verdict {
    match run.exit_code {
        Some(0) => Verdict::Healthy,
        Some(n) => Verdict::Unhealthy(format!("the apply exited {n}")),
        None => Verdict::Unhealthy("the apply has no exit code".into()),
    }
}

static CHANT_FLY: chant_fly::ChantFly = chant_fly::ChantFly;
static APPS_PLATFORM: apps_platform::AppsPlatform = apps_platform::AppsPlatform;
static ARUGULA_RELEASE: arugula_release::ArugulaRelease = arugula_release::ArugulaRelease;
static FAKE: fake::Fake = fake::Fake;
static PLANTER_PROD: planter::Prod = planter::Prod;
static PLANTER_GO_LIVE: planter::GoLive = planter::GoLive;
static PLANTER_SHIP: planter::Ship = planter::Ship;
static PLANTER_PROMOTE: planter::Promote = planter::Promote;

/// Whether environments here may name the fake driver: tests (the feature
/// crates' too, #314), and a platform started with
/// `ARUGULA_PLATFORM_FAKE_DRIVER=1` (testnet).
pub fn fake_allowed() -> bool {
    cfg!(any(test, feature = "testing")) || std::env::var("ARUGULA_PLATFORM_FAKE_DRIVER").is_ok_and(|v| v == "1")
}

/// The driver called `name`, if there is one here.
pub fn driver(name: &str) -> Option<&'static dyn Driver> {
    match name {
        "chant-fly" => Some(&CHANT_FLY),
        "apps-platform" => Some(&APPS_PLATFORM),
        arugula_release::NAME => Some(&ARUGULA_RELEASE),
        arugula_e2e::deploy::planter::PROD => Some(&PLANTER_PROD),
        arugula_e2e::deploy::planter::GO_LIVE => Some(&PLANTER_GO_LIVE),
        arugula_e2e::deploy::planter::SHIP => Some(&PLANTER_SHIP),
        arugula_e2e::deploy::planter::PROMOTE => Some(&PLANTER_PROMOTE),
        fake::NAME if fake_allowed() => Some(&FAKE),
        _ => None,
    }
}

/// The drivers an environment may name here.
pub fn names() -> Vec<&'static str> {
    let mut v = crate::db::deploys::DRIVERS.to_vec();
    if fake_allowed() {
        v.push(fake::NAME);
    }
    v
}

#[cfg(test)]
mod tests;
