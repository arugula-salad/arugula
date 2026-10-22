//! The apps-platform driver (M222, platform step 7, native Arugula #18;
//! aru-104): a deploy to home-cloud's app platform is a commit to its list
//! repository, which Flux applies.
//!
//! - **Plan** ([`arugula_e2e::deploy::apps_platform::plan_lines`]): the
//!   list repository, the pointer's path (`apps/<name>.yaml`), the change
//!   (the pointer's `GitRepository` pinned at the approved commit of the
//!   app's repository) and how it's checked. Only a commit of one of the
//!   list's organization's repositories is planned ([`Driver::refuses`]).
//! - **Apply** runs on the deploy's box (`arugulad _apps-platform`): it
//!   clones the list, pins the pointer, commits and pushes, over HTTPS with
//!   the run's token in its environment only.
//! - **The token writes the list only**: an installation token minted for
//!   the run, `contents: write` on [`LIST_REPO`] alone, revoked when the run
//!   ends ([`Mint::GithubWrite`]). An environment that holds a fine-grained
//!   token of its own under [`WRITE_TOKEN`] (scoped by whoever made it)
//!   gets that instead, and nothing is minted for it. The app's own
//!   repository is read with a read-only token minted the same way
//!   ([`Mint::GithubRead`], [`READ_TOKEN`]), for its `app.yaml`.
//! - **Verify** is on the box: the run exits 0 only once the app's public
//!   address answers its health path (held to the deployed revision when
//!   the app reports one), so the platform's check is the run's exit
//!   ([`exited_0`]). The box revokes both tokens right after its push.
//! - **Roll back** is a deploy of the environment's last good commit; the
//!   box writes it as a revert commit of the deploy it undoes.

use arugula_e2e::deploy::apps_platform::{self as ap, LIST_REPO};

use super::{Driver, Mint, Verdict, exited_0};
use crate::db::{
    deploys::{Deploy, Environment, Source},
    runs::Run,
};

/// Its name, as environments name it.
pub const NAME: &str = ap::DRIVER;

/// The name the run gets the list's write token under.
pub const WRITE_TOKEN: &str = "APPS_PLATFORM_TOKEN";

/// The name the run gets the app repository's read token under.
pub const READ_TOKEN: &str = "SOURCE_TOKEN";

pub struct AppsPlatform;

impl Driver for AppsPlatform {
    fn name(&self) -> &'static str {
        NAME
    }

    fn refuses(&self, source: &Source) -> Option<String> {
        ap::refuses(source.kind.as_str(), &source.reference, &source.place)
    }

    fn mints(&self, env: &Environment) -> Vec<Mint> {
        let mut m = vec![Mint::GithubRead { name: READ_TOKEN }];
        if !env.secret_names.iter().any(|n| n == WRITE_TOKEN) {
            m.push(Mint::GithubWrite { name: WRITE_TOKEN, repo: LIST_REPO });
        }
        m
    }

    fn verify(&self, _: &Environment, _: &Deploy, run: &Run, _: &serde_json::Value) -> Verdict {
        exited_0(run)
    }

    fn rolls_back(&self) -> bool {
        true
    }
}
