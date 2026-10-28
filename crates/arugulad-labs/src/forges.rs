//! What a forge block does to reach Forgejo and GitLab, which are Labs
//! (#457): a Forgejo block's `tea` login and its adapter, and GitLab's
//! connection through `glab`. The block asks through `lab_hooks` and keeps
//! what comes back (`forge/mod.rs`, `connect_tea`, `login_tea`,
//! `connect_gitlab`); a build without Labs answers that it lacks them.

use std::sync::Arc;

use arugula_proto::forge::ForgeLogin;
use futures_util::future::BoxFuture;

use super::{
    gitlab,
    tea::{Login, Tea, TokenSource},
};
use crate::{
    forge::{Adapter, TeaAsk, TeaFound, TeaMissed, http, login, model::Provider},
    review::Runner,
};

fn adapter(provider: Provider, login: &Login, tea: Arc<Tea>) -> Arc<dyn Adapter> {
    match provider {
        Provider::Forgejo => Arc::new(super::forgejo::Forgejo::new(&login.api(), http(), TokenSource::new(tea, login))),
        Provider::Github => {
            let host = login::url_host(&login.url).unwrap_or_default();
            let token = crate::forge::github::GhToken::new(tea.runner.clone(), &host);
            Arc::new(crate::forge::github::client(&crate::forge::github::api_for(&host), token))
        }
        // GitLab connects through glab ([`gitlab_connect`]), never a tea
        // login: anything else reads anonymously.
        Provider::Gitlab => {
            let api = format!("{}/api/v4", login.url.trim_end_matches('/'));
            let note = gitlab::read_only_note(&login::url_host(&login.url).unwrap_or_default(), "not a glab login");
            Arc::new(gitlab::Gitlab::new(&api, http(), None, Some(note)))
        }
    }
}

/// The logins to pick from, as the block shows them.
fn candidates(logins: &[Login]) -> Option<Vec<ForgeLogin>> {
    Some(logins.iter().map(|l| ForgeLogin { name: l.name.clone(), url: l.url.clone(), user: l.user.clone() }).collect())
}

fn found(provider: Provider, login: Login, tea: Arc<Tea>) -> TeaFound {
    let adapter = adapter(provider, &login, tea);
    TeaFound { api: login.api(), name: login.name, adapter }
}

/// A Forgejo block (or a host `gh` doesn't know): the `tea` login for its
/// host, from the config, resolved, or picked.
pub async fn tea_connect(runner: Runner, ask: TeaAsk) -> Result<TeaFound, TeaMissed> {
    let tea = Arc::new(Tea { runner });
    let logins = tea.logins().await.map_err(|error| TeaMissed { candidates: None, error })?;
    let TeaAsk { want, host, provider, repo } = ask;
    let login = match want {
        Some(name) => logins.iter().find(|l| l.name == name).cloned().ok_or_else(|| TeaMissed {
            candidates: candidates(&logins),
            error: format!("tea has no login {name:?} any more: `tea login add`, or pick another"),
        })?,
        None => {
            let Some(host) = host else {
                // No host to go by: the default login, or the only one.
                let pick = logins.iter().find(|l| l.default).or(logins.first().filter(|_| logins.len() == 1));
                let l = pick.cloned().ok_or_else(|| TeaMissed {
                    candidates: candidates(&logins),
                    error: "which forge? pick a tea login".to_owned(),
                })?;
                return Ok(found(provider, l, tea));
            };
            let lookup_tea = tea.clone();
            let lookup = move |l: Login| -> BoxFuture<'static, Result<Vec<String>, String>> {
                let (tea, repo) = (lookup_tea.clone(), repo.clone());
                Box::pin(async move { adapter(provider, &l, tea).repo_urls(&repo).await.map_err(|e| e.to_string()) })
            };
            let r = super::tea::resolve(logins.clone(), &host, &lookup).await;
            match r.login {
                Some(l) => l,
                None => {
                    return Err(TeaMissed {
                        candidates: candidates(&r.candidates),
                        error: r.error.unwrap_or_else(|| format!("no tea login for {host}")),
                    });
                }
            }
        }
    };
    Ok(found(provider, login, tea))
}

/// `login {name}` on a Forgejo block: that `tea` login.
pub async fn tea_login(runner: Runner, provider: Provider, name: &str) -> Result<TeaFound, String> {
    let tea = Arc::new(Tea { runner });
    let logins = tea.logins().await?;
    let l = logins.iter().find(|l| l.name == name).cloned().ok_or_else(|| format!("tea has no login {name:?}"))?;
    Ok(found(provider, l, tea))
}

/// GitLab (M39): glab's token for the host, or anonymous and read-only;
/// and the login it reads as.
pub async fn gitlab_connect(runner: Runner, host: &str, api: &str) -> (Arc<dyn Adapter>, Option<String>) {
    let g = gitlab::connect(runner, host, api, http()).await;
    (Arc::new(g.adapter), g.login)
}
