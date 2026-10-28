//! Forgejo and GitLab, the forges that are Labs' (#457): what a forge block
//! asks of them, as one trait, [`OtherForges`], which Labs registers
//! ([`install`]). Each default is what a build without Labs does: says it
//! hasn't them. (A block of theirs is refused first, by
//! `lab_flags::forge_allowed`, so these are asked only if that changes.)

use std::sync::{Arc, OnceLock};

use futures_util::future::BoxFuture;

use super::{Adapter, TeaAsk, TeaFound, TeaMissed, model::Provider};
use crate::{lab_flags::not_built, review::Runner};

/// Labs' forges, as a forge block calls them.
pub trait OtherForges: Send + Sync + 'static {
    /// The path a webhook for `provider` is made to deliver to.
    fn hook_path(&self, provider: Provider) -> Result<&'static str, String> {
        Err(not_built(&format!("A {provider:?} webhook")))
    }

    /// A Forgejo block's `tea` login and adapter ([`TeaAsk`]).
    fn tea_connect<'a>(&'a self, _runner: Runner, _ask: TeaAsk) -> BoxFuture<'a, Result<TeaFound, TeaMissed>> {
        Box::pin(async { Err(TeaMissed { candidates: None, error: not_built("A Forgejo block") }) })
    }

    /// A Forgejo block's `login {name}`: that `tea` login and its adapter.
    fn tea_login<'a>(
        &'a self,
        _runner: Runner,
        _provider: Provider,
        _name: &'a str,
    ) -> BoxFuture<'a, Result<TeaFound, String>> {
        Box::pin(async { Err(not_built("A Forgejo login")) })
    }

    /// A GitLab block's adapter (glab's token for `host`, or anonymous and
    /// read-only) and the login it reads as.
    #[allow(clippy::type_complexity)]
    fn gitlab_connect<'a>(
        &'a self,
        _runner: Runner,
        _host: &'a str,
        _api: &'a str,
    ) -> BoxFuture<'a, Result<(Arc<dyn Adapter>, Option<String>), String>> {
        Box::pin(async { Err(not_built("A GitLab block")) })
    }
}

/// A build without Labs: every default.
struct Absent;

impl OtherForges for Absent {}

static OTHERS: OnceLock<Box<dyn OtherForges>> = OnceLock::new();

/// Puts Labs' forges in this daemon (once, before `main`).
pub fn install(others: Box<dyn OtherForges>) {
    if OTHERS.set(others).is_err() {
        tracing::warn!("Labs' forges were registered twice");
    }
}

/// Labs' forges, or a build without them.
pub fn others() -> &'static dyn OtherForges {
    OTHERS.get().map_or(&Absent, |o| o.as_ref())
}
