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
//! **The fake driver** ([`fake::Fake`], `fake`) is for tests and testnet:
//! environments may name it only where the platform says so
//! (`ARUGULA_PLATFORM_FAKE_DRIVER=1`), and boxes run it only where they
//! say so too.

pub mod fake;
pub mod mint;

use sha2::{Digest as _, Sha256};

pub use mint::{Held, Mint, mint, revoke_run};

use crate::db::{
    deploys::{Deploy, Environment, Source},
    runs::Run,
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
    fn plan(&self, env: &Environment, source: &Source) -> Planned;

    /// The secret names it takes (`arugula_e2e::deploy::driver_secrets`):
    /// an environment holds only these, and a box hands it only these.
    fn secret_names(&self) -> &'static [&'static str] {
        arugula_e2e::deploy::driver_secrets(self.name()).unwrap_or_default()
    }

    /// The short-lived tokens it mints at apply, each from a long-lived
    /// credential the platform keeps, given to the run with the
    /// environment's secrets.
    fn mints(&self, env: &Environment) -> Vec<Mint> {
        let _ = env;
        vec![]
    }

    /// Whether `d`, applied by `run` (ended `succeeded`), did what it was
    /// asked.
    fn verify(&self, d: &Deploy, run: &Run) -> Verdict;

    /// Whether its target rolls back: a deploy of the environment's last
    /// source that succeeded, planned and approved as any other is.
    fn rolls_back(&self) -> bool {
        false
    }
}

/// A driver whose box half isn't built yet (M220's chant→Fly, M222's
/// apps-platform): environments may name it, deploys may be planned and
/// approved, and a box refuses to apply it until its driver lands.
///
/// Each driver's target must refuse a deploy id it applied (`DEPLOY_ID`),
/// since each deploy's box is new and can't know: chant→Fly by chant's
/// ledger, apps-platform by the `Arugula-Deploy: <id>` trailer.
pub struct Unbuilt(pub &'static str);

impl Driver for Unbuilt {
    fn name(&self) -> &'static str {
        self.0
    }

    fn plan(&self, env: &Environment, source: &Source) -> Planned {
        plan_of(self.0, env, source)
    }

    fn verify(&self, _: &Deploy, run: &Run) -> Verdict {
        exited_0(run)
    }
}

/// The plan every driver here makes ([`arugula_e2e::deploy::plan_text`]):
/// it names the driver, so the approval (over its digest) binds the
/// driver, and a box recomputes it before it applies.
pub fn plan_of(driver: &str, env: &Environment, source: &Source) -> Planned {
    Planned::of(arugula_e2e::deploy::plan_text(
        driver,
        &env.name,
        source.kind.as_str(),
        &source.reference,
        &source.place,
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

static CHANT_FLY: Unbuilt = Unbuilt("chant-fly");
static APPS_PLATFORM: Unbuilt = Unbuilt("apps-platform");
static FAKE: fake::Fake = fake::Fake;

/// Whether environments here may name the fake driver: tests, and a
/// platform started with `ARUGULA_PLATFORM_FAKE_DRIVER=1` (testnet).
pub fn fake_allowed() -> bool {
    cfg!(test) || std::env::var("ARUGULA_PLATFORM_FAKE_DRIVER").is_ok_and(|v| v == "1")
}

/// The driver called `name`, if there is one here.
pub fn driver(name: &str) -> Option<&'static dyn Driver> {
    match name {
        "chant-fly" => Some(&CHANT_FLY),
        "apps-platform" => Some(&APPS_PLATFORM),
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
