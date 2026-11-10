//! `wait` with `pr`: each pull request read from GitHub itself, with the
//! person's own `gh` login (never the platform's mirror, which webhooks
//! feed and whose poller can lag a green PR's `mergeable` by minutes). The
//! requests are conditional ([`arugula_forge_github::EtagCache`]), so asking
//! every few seconds costs a 304 while nothing moves.
//!
//! A pull request the login can't see answers as one that isn't there
//! (GitHub's own 404 for a private repository, and a refusal alike).

use std::sync::Arc;

use arugulad_base::{
    forge_wait::{PrRef, PrVerdict},
    forges::PrRead,
};

use crate::{
    Error,
    github::{self, GhToken},
    review::Runner,
};

/// Each of `prs`, read now.
pub async fn read(app: &Arc<crate::server::App>, prs: &[PrRef]) -> Vec<PrRead> {
    let runner = match crate::api::agent_env(app).await {
        Ok((home, env)) => Runner::Local { env, home },
        Err(e) => return prs.iter().map(|_| PrRead::NoLogin(e.1.clone())).collect(),
    };
    futures_util::future::join_all(prs.iter().map(|pr| one(&runner, pr))).await
}

async fn one(runner: &Runner, pr: &PrRef) -> PrRead {
    let gh = github::client(&github::api_for(&pr.host), GhToken::new(runner.clone(), &pr.host));
    let polled = match gh.poll(&pr.repo, pr.number).await {
        Ok(p) => p,
        Err(Error::NotFound(_) | Error::Denied(_)) => return PrRead::Gone,
        Err(Error::Login(why)) => return PrRead::NoLogin(why),
        Err(e) => return PrRead::Failed(e.to_string()),
    };
    // REST doesn't say whether the PR is in a merge queue. A failed read of
    // it leaves it unsaid: the checks and `mergeable` still stand.
    let queue = if polled.item.state == arugula_proto::forge::ItemState::Open && !polled.item.draft {
        gh.merge_queue(&pr.repo, pr.number).await.ok().flatten()
    } else {
        None
    };
    PrRead::Verdict(Box::new(PrVerdict::of(pr, &polled.item, &polled.checks, queue)))
}
