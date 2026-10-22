//! The chant→Fly driver (M220, platform step 7, native Arugula #16; aru-104
//! "Drivers come in order"): a release chant manages, on Fly.
//!
//! **Its plan** ([`arugula_e2e::drivers::plan_text`]) names the source, the
//! environment, its settings (the Fly app, where the chant op is) and what
//! the box will do: check out the source; in the chant member, `npm ci`,
//! then `chant run <op>`; at each gate the op stops at, `chant approve` it
//! as the approver (the approval's account, relayed by arugula, the
//! approval noted); `chant run <op>` again; link chant's ledger entry; and
//! check the Fly app is healthy. Its credentials, given to that run only:
//! the environment's `FLY_API_TOKEN`, a Fly deploy token for one app, and
//! `GITHUB_TOKEN`, which pushes chant's ledger: a GitHub App installation
//! token minted for the run, writing the deploy's repository's contents
//! only ([`Mint::GithubWriteSource`], revoked when the run ends), unless
//! the environment holds one of its own.
//!
//! **Its box half** is arugulad's (`work_deploy`'s chant→Fly script). It
//! ends its run with what it did: chant's ledger entry (a link to the line
//! in `<env>/releases.jsonl` on `chant/lifecycle`, as pushed to origin),
//! chant's release record, and the Fly app's image and health.
//!
//! **Verify:** the run exited 0, Fly said every machine was started with
//! its checks passing, and chant's ledger entry reached origin. The log
//! keeps the image and chant's record ([`ChantFly::kept`]), and the run
//! links the entry (aru-098).
//!
//! **Roll back** to a source the environment ran before: Fly's own (`fly
//! deploy --image` of the image that deploy ran, as its log kept it), then
//! chant's ledger records the release running again (`chant components
//! release`, as the approver), and the run links that entry.

use arugula_e2e::drivers::{
    CHANT_COMPONENT, CHANT_DIGEST, CHANT_ENV, FLY_APP, IMAGE, Settings, fly_image_ok, ledger_link_ok, word,
};
use serde_json::{Map, Value};

use super::{Driver, Mint, Verdict, exited_0};
use crate::db::{
    deploys::{Deploy, Environment},
    runs::Run,
};

pub use arugula_e2e::drivers::CHANT_FLY as NAME;

pub struct ChantFly;

/// The name the run gets its GitHub token under.
pub const GITHUB_TOKEN: &str = "GITHUB_TOKEN";

/// The log's name for chant's ledger entry.
const LEDGER: &str = "chant_ledger";

/// A string in `v` at `path`, if it's one word starting with a letter or
/// a digit.
fn at<'a>(v: &'a Value, path: &[&str]) -> Option<&'a str> {
    path.iter().try_fold(v, |v, k| v.get(k))?.as_str().filter(|s| word(s) && s.as_bytes()[0].is_ascii_alphanumeric())
}

/// The image Fly runs, as reported, if it's the environment's app's, by
/// digest.
fn image<'a>(env: &Environment, reported: &'a Value) -> Option<&'a str> {
    let app = env.settings.get(FLY_APP).map_or("", String::as_str);
    at(reported, &["fly", "image"]).filter(|i| fly_image_ok(app, i))
}

/// chant's release record, as reported, if it's in the environment's
/// ledger environment and for the deploy's commit.
fn record<'a>(env: &Environment, d: &Deploy, reported: &'a Value) -> Option<&'a Value> {
    let r = reported.get("chant_record")?;
    let ok = at(r, &["env"]) == env.settings.get(CHANT_ENV).map(String::as_str)
        && at(r, &["gitSha"]) == Some(d.source.reference.as_str())
        && at(r, &["component"]).is_some()
        && at(r, &["digest"]).is_some();
    ok.then_some(r)
}

impl Driver for ChantFly {
    fn name(&self) -> &'static str {
        NAME
    }

    fn mints(&self, env: &Environment) -> Vec<Mint> {
        if env.secret_names.iter().any(|n| n == GITHUB_TOKEN) {
            vec![]
        } else {
            vec![Mint::GithubWriteSource { name: GITHUB_TOKEN }]
        }
    }

    fn verify(&self, env: &Environment, d: &Deploy, run: &Run, reported: &Value) -> Verdict {
        if let Verdict::Unhealthy(why) = exited_0(run) {
            return Verdict::Unhealthy(why);
        }
        if reported["fly"]["healthy"] != Value::Bool(true) {
            return Verdict::Unhealthy("its Fly app isn't healthy after the apply".into());
        }
        if image(env, reported).is_none() {
            return Verdict::Unhealthy("the apply didn't say which of its Fly app's images it runs, by digest".into());
        }
        if record(env, d, reported).is_none() {
            return Verdict::Unhealthy(
                "the apply's chant release record isn't this commit's, in the environment's ledger environment".into(),
            );
        }
        if self.ledger(env, d, reported).is_none() {
            return Verdict::Unhealthy("the apply wrote no entry in chant's ledger that reached origin".into());
        }
        Verdict::Healthy
    }

    fn kept(&self, env: &Environment, d: &Deploy, reported: &Value) -> Map<String, Value> {
        let mut m = Map::new();
        if let Some(i) = image(env, reported) {
            m.insert(IMAGE.into(), i.into());
        }
        if let Some(r) = record(env, d, reported) {
            for (k, field) in [(CHANT_COMPONENT, "component"), (CHANT_ENV, "env"), (CHANT_DIGEST, "digest")] {
                if let Some(v) = at(r, &[field]) {
                    m.insert(k.into(), v.into());
                }
            }
        }
        if let Some(l) = self.ledger(env, d, reported) {
            m.insert(LEDGER.into(), l.into());
        }
        m
    }

    /// Only a line of chant's ledger in the deploy's own repository on
    /// GitHub, at a commit: never another URL, which a page might open.
    fn ledger(&self, _: &Environment, d: &Deploy, reported: &Value) -> Option<String> {
        reported[LEDGER].as_str().filter(|l| ledger_link_ok(&d.source.place, l)).map(str::to_owned)
    }

    fn rolls_back(&self) -> bool {
        true
    }

    /// The image and chant's record a succeeded deploy's log kept: all
    /// four, or it can't be gone back to.
    fn roll_back_to(&self, kept: &Value) -> Option<Settings> {
        [IMAGE, CHANT_COMPONENT, CHANT_ENV, CHANT_DIGEST]
            .iter()
            .map(|k| at(kept, &[k]).map(|v| ((*k).to_owned(), v.to_owned())))
            .collect()
    }
}
