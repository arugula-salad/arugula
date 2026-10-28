//! The GitHub adapter (M38): a forge block's reads and writes through
//! `arugula-forge-github` (M160), which maps GitHub's REST API onto the
//! normalized model. Requests go through the daemon's own HTTP client with
//! a token from the person's `gh` login (`gh auth token --hostname H`, run
//! with their shell environment, #74), held in memory only and asked for
//! again after a 401: [`GhToken`] is the crate's `TokenSource`. GitHub
//! Enterprise is the same with its API at `https://HOST/api/v3`.
//!
//! What's particular to GitHub (S23) is in the crate's docs: ETags, check
//! runs and statuses, pending review requests, branch protection, *Rerun*
//! and the rate limits. The block's state says when it backs off
//! (`rate.backoff`).

use std::{
    collections::HashMap,
    sync::{Arc, LazyLock, Mutex},
};

use arugula_forge_github::{EtagCache, Purpose, Token, TokenSource};
use futures_util::future::BoxFuture;
use tracing::info;

use super::{
    Adapter, Error, Polled, RateLimit, Sent, Write,
    model::{Event, Item, Linked, Me, Review, Reviewer},
};
use crate::review::Runner;

pub use arugula_forge_github::parse::{host_of_api, is_github_host, issue_url, pr_url};

/// The API base for a GitHub host: `https://api.github.com`, or GitHub
/// Enterprise's `https://HOST/api/v3`. (`ARUGULA_GITHUB_API` stands in
/// for github.com's in tests.)
pub fn api_for(host: &str) -> String {
    if is_github_host(host) {
        std::env::var("ARUGULA_GITHUB_API").ok().filter(|a| !a.is_empty()).unwrap_or("https://api.github.com".into())
    } else {
        format!("https://{host}/api/v3")
    }
}

/// Every URL's last answer while the daemon runs, shared by the blocks: a
/// second block on the same host asks `user` and `user/teams` for free,
/// and so does a block after `refresh`.
static CACHE: LazyLock<EtagCache> = LazyLock::new(EtagCache::default);

/// A block's GitHub client.
pub type Github = arugula_forge_github::Github<GhToken>;

/// GitHub at `api`, with the daemon's HTTP client and shared cache.
pub fn client(api: &str, token: GhToken) -> Github {
    Github::new(api, super::http(), token).with_cache(CACHE.clone())
}

// ---------------------------------------------------------------- gh

/// What [`GhToken::get`] says when there's no `gh` on the host.
pub const NO_GH: &str = "no gh here: install GitHub's CLI and `gh auth login`, then refresh";

const TOKEN: &str = r#"command -v gh >/dev/null 2>&1 || { echo arugula-no-gh; exit 0; }
exec gh auth token --hostname "$1" 2>/dev/null"#;

/// Tokens held while the daemon runs, by host. Memory only.
static TOKENS: LazyLock<Mutex<HashMap<String, String>>> = LazyLock::new(Mutex::default);
/// Hosts `gh` has no login for (asked once per daemon).
static UNKNOWN: LazyLock<Mutex<std::collections::HashSet<String>>> = LazyLock::new(Mutex::default);

/// Whether `gh` is logged in to `host` (GitHub Enterprise, when a remote
/// or link is on a host no `tea` login is for). `gh auth token` answers
/// from the keyring without asking the host, unlike `gh auth status`.
pub async fn knows(runner: &Runner, host: &str) -> bool {
    let host = host.to_ascii_lowercase();
    if TOKENS.lock().unwrap().contains_key(&host) {
        return true;
    }
    if UNKNOWN.lock().unwrap().contains(&host) {
        return false;
    }
    let ok = GhToken::new(runner.clone(), &host).get(false).await.is_ok();
    if !ok {
        UNKNOWN.lock().unwrap().insert(host);
    }
    ok
}

/// Where the GitHub adapter gets its token: `gh`, on the block's host;
/// and (M40) on github.com with no `gh` login, Arugula control's GitHub
/// App, read-only, for the block's repository.
#[derive(Clone)]
pub struct GhToken {
    runner: Runner,
    host: String,
    repo: Option<String>,
    /// Reading through the App: the account's GitHub login.
    app: Arc<Mutex<Option<String>>>,
}

impl GhToken {
    pub fn new(runner: Runner, host: &str) -> Self {
        Self { runner, host: host.to_ascii_lowercase(), repo: None, app: Arc::default() }
    }

    /// ...that falls back to control's App for this repository.
    pub fn for_repo(mut self, repo: &str) -> Self {
        self.repo = Some(repo.to_owned());
        self
    }

    /// The person's GitHub login when it reads through the App.
    pub fn via_app(&self) -> Option<String> {
        self.app.lock().unwrap().clone()
    }

    /// The token; `fresh`: the last one was refused, ask again.
    pub async fn get(&self, fresh: bool) -> Result<String, String> {
        if self.via_app().is_some() {
            return self.app_token(fresh).await;
        }
        match self.gh(fresh).await {
            Ok(t) => Ok(t),
            Err(e) if self.host == "github.com" && self.repo.is_some() => match self.app_token(fresh).await {
                Ok(t) => {
                    info!(host = self.host, "no gh login: reading through Arugula control's GitHub App");
                    Ok(t)
                }
                Err(why) => Err(format!("{e} (and Arugula control's GitHub App can't read it: {why})")),
            },
            Err(e) => Err(e),
        }
    }

    async fn app_token(&self, fresh: bool) -> Result<String, String> {
        let repo = self.repo.as_deref().unwrap_or_default();
        let (t, login) = crate::forge::live::app_token(repo, fresh).await?;
        *self.app.lock().unwrap() = Some(login);
        Ok(t)
    }

    async fn gh(&self, fresh: bool) -> Result<String, String> {
        if !fresh && let Some(t) = TOKENS.lock().unwrap().get(&self.host).cloned() {
            return Ok(t);
        }
        let (out, _) = self.runner.sh(TOKEN, std::slice::from_ref(&self.host)).await?;
        if out.starts_with(b"arugula-no-gh") {
            return Err(NO_GH.into());
        }
        let t = String::from_utf8_lossy(&out).trim().to_owned();
        if t.is_empty() || t.contains(char::is_whitespace) {
            return Err(format!("gh has no login for {}: `gh auth login --hostname {}`", self.host, self.host));
        }
        TOKENS.lock().unwrap().insert(self.host.clone(), t.clone());
        Ok(t)
    }
}

/// The block's repository and host are fixed when it's made, so the
/// repository and purpose asked are not looked at: one token reads and
/// writes, as `gh`'s does. Through the App, the token names the person.
impl TokenSource for GhToken {
    fn token<'a>(&'a self, _repo: &'a str, _: Purpose, fresh: bool) -> BoxFuture<'a, Result<Option<Token>, String>> {
        Box::pin(async move {
            let secret = self.get(fresh).await?;
            Ok(Some(Token { secret, login: self.via_app() }))
        })
    }
}

// ---------------------------------------------------------------- the adapter

impl Adapter for Github {
    fn me(&self) -> BoxFuture<'_, Result<Me, Error>> {
        Box::pin(Github::me(self))
    }

    fn poll<'a>(&'a self, repo: &'a str, number: u64) -> BoxFuture<'a, Result<Polled, Error>> {
        Box::pin(Github::poll(self, repo, number))
    }

    fn rest<'a>(
        &'a self,
        repo: &'a str,
        number: u64,
    ) -> BoxFuture<'a, Result<(Vec<Review>, Vec<Reviewer>, Vec<Event>), Error>> {
        Box::pin(Github::rest(self, repo, number))
    }

    fn write<'a>(&'a self, repo: &'a str, number: u64, w: &'a Write) -> BoxFuture<'a, Result<Sent, Error>> {
        Box::pin(async move {
            match w {
                Write::Comment { body } => self.comment(repo, number, body).await,
                Write::Review { event, body } => self.review(repo, number, *event, body.as_deref()).await,
                Write::Merge { style } => self.merge(repo, number, style.as_deref()).await,
                Write::Rerun => self.rerun_failed(repo, number).await,
                Write::Live { .. } => Err(Error::Http("live updates go through the block's hook".into())),
            }
        })
    }

    fn repo_urls<'a>(&'a self, repo: &'a str) -> BoxFuture<'a, Result<Vec<String>, Error>> {
        Box::pin(Github::repo_urls(self, repo))
    }

    fn rerun_api(&self) -> bool {
        true
    }

    fn merge_styles<'a>(&'a self, repo: &'a str) -> BoxFuture<'a, Result<Option<Vec<String>>, Error>> {
        Box::pin(Github::merge_styles(self, repo))
    }

    fn read_only(&self) -> Option<String> {
        self.tokens().via_app().map(|_| {
            "read-only: no gh login here, so it reads through Arugula control's GitHub App; writes go out as you, so they need your own login (`gh auth login`, then refresh)".to_owned()
        })
    }

    fn issue<'a>(&'a self, repo: &'a str, number: u64) -> BoxFuture<'a, Result<(Item, String), Error>> {
        Box::pin(Github::issue(self, repo, number))
    }

    fn issue_events<'a>(
        &'a self,
        repo: &'a str,
        number: u64,
    ) -> BoxFuture<'a, Result<(Vec<Event>, Vec<Linked>), Error>> {
        Box::pin(Github::issue_events(self, repo, number))
    }

    fn pr_by_head<'a>(&'a self, repo: &'a str, branch: &'a str) -> BoxFuture<'a, Result<Option<Linked>, Error>> {
        Box::pin(Github::pr_by_head(self, repo, branch))
    }

    fn new_issue<'a>(
        &'a self,
        repo: &'a str,
        title: &'a str,
        body: &'a str,
    ) -> BoxFuture<'a, Result<(u64, Sent), Error>> {
        Box::pin(Github::new_issue(self, repo, title, body))
    }

    fn default_branch<'a>(&'a self, repo: &'a str) -> BoxFuture<'a, Result<String, Error>> {
        Box::pin(Github::default_branch(self, repo))
    }

    fn rate(&self) -> Option<RateLimit> {
        Some(Github::rate(self))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn api_bases() {
        assert_eq!(api_for("ghe.example.com"), "https://ghe.example.com/api/v3");
    }
}
