//! What a forge block does to reach Forgejo and GitLab, which are Labs
//! (#457): a Forgejo block's `tea` login and its adapter, and GitLab's
//! connection through `glab`. It sits here, and not in `labs/`, because it
//! is the block's own methods and needs its private fields (as the
//! multiplexer's `thread_ops.rs` does). Only a build with Labs has this
//! file; `mod.rs` has the twins, which answer that the build lacks them.

use std::sync::Arc;

use futures_util::future::BoxFuture;
use tracing::info;

use super::{Adapter, ForgeBlock, http, login, model::Provider, to_value};
use crate::{
    labs::{
        gitlab,
        tea::{Login, Tea, TokenSource},
    },
    review::Runner,
};
use arugula_proto::forge::{ForgeLogin, LoggedIn};

fn adapter(provider: Provider, login: &Login, tea: Arc<Tea>) -> Arc<dyn Adapter> {
    match provider {
        Provider::Forgejo => {
            Arc::new(crate::labs::forgejo::Forgejo::new(&login.api(), http(), TokenSource::new(tea, login)))
        }
        Provider::Github => {
            let host = login::url_host(&login.url).unwrap_or_default();
            let token = super::github::GhToken::new(tea.runner.clone(), &host);
            Arc::new(super::github::Github::new(&super::github::api_for(&host), http(), token))
        }
        // GitLab connects through glab ([`ForgeBlock::connect_gitlab`]),
        // never a tea login: anything else reads anonymously.
        Provider::Gitlab => {
            let api = format!("{}/api/v4", login.url.trim_end_matches('/'));
            let note = gitlab::read_only_note(&login::url_host(&login.url).unwrap_or_default(), "not a glab login");
            Arc::new(gitlab::Gitlab::new(&api, http(), None, Some(note)))
        }
    }
}

impl ForgeBlock {
    /// A Forgejo block (or a host `gh` doesn't know): the `tea` login for
    /// its host, from the config, resolved, or picked.
    pub(super) async fn connect_tea(&self, runner: Runner) -> Result<Arc<dyn Adapter>, String> {
        let tea = Arc::new(Tea { runner });
        let logins = tea.logins().await?;
        let (want, host, provider, repo) = {
            let c = self.config.lock().unwrap();
            let host = c.host.clone().or_else(|| c.api.as_deref().and_then(login::url_host));
            (c.login.clone(), host, c.provider, c.repo.clone())
        };
        let login = match want {
            Some(name) => logins
                .iter()
                .find(|l| l.name == name)
                .cloned()
                .ok_or_else(|| format!("tea has no login {name:?} any more: `tea login add`, or pick another"))
                .inspect_err(|_| self.candidates(&logins))?,
            None => {
                let Some(host) = host else {
                    // No host to go by: the default login, or the only one.
                    let pick = logins.iter().find(|l| l.default).or(logins.first().filter(|_| logins.len() == 1));
                    let l = pick.cloned().ok_or_else(|| {
                        self.candidates(&logins);
                        "which forge? pick a tea login".to_owned()
                    })?;
                    return Ok(self.connected(provider, l, tea));
                };
                let lookup_tea = tea.clone();
                let lookup = move |l: Login| -> BoxFuture<'static, Result<Vec<String>, String>> {
                    let (tea, repo) = (lookup_tea.clone(), repo.clone());
                    Box::pin(
                        async move { adapter(provider, &l, tea).repo_urls(&repo).await.map_err(|e| e.to_string()) },
                    )
                };
                let r = crate::labs::tea::resolve(logins.clone(), &host, &lookup).await;
                match r.login {
                    Some(l) => l,
                    None => {
                        self.candidates(&r.candidates);
                        return Err(r.error.unwrap_or_else(|| format!("no tea login for {host}")));
                    }
                }
            }
        };
        Ok(self.connected(provider, login, tea))
    }

    /// GitLab (M39): glab's token for the host, or anonymous and read-only.
    pub(super) async fn connect_gitlab(&self) -> Result<Arc<dyn Adapter>, String> {
        let runner = Runner::user(&self.ctx).await?;
        let (host, api) = {
            let c = self.config.lock().unwrap();
            let host = c.host.clone().or_else(|| c.api.as_deref().and_then(login::url_host));
            let host = host.ok_or("which GitLab? open it from a link or a clone")?;
            let api = c.api.clone().unwrap_or_else(|| format!("https://{host}/api/v4"));
            (host, api)
        };
        let g = gitlab::connect(runner, &host, &api, http()).await;
        let a: Arc<dyn Adapter> = Arc::new(g.adapter);
        {
            let mut c = self.config.lock().unwrap();
            c.login = g.login.clone();
            c.api = Some(api.clone());
        }
        {
            let mut st = self.state.lock().unwrap();
            st.login = g.login.clone();
            st.api = Some(api);
            st.read_only = a.read_only();
            st.logins.clear();
        }
        info!(pane = self.ctx.id, host, login = g.login, "gitlab block connected");
        *self.adapter.lock().unwrap() = Some(a.clone());
        Ok(a)
    }

    fn connected(&self, provider: Provider, login: Login, tea: Arc<Tea>) -> Arc<dyn Adapter> {
        let a = adapter(provider, &login, tea);
        let api = login.api();
        self.attach(login.name, api, a)
    }

    fn candidates(&self, logins: &[Login]) {
        self.state.lock().unwrap().logins = logins
            .iter()
            .map(|l| ForgeLogin { name: l.name.clone(), url: l.url.clone(), user: l.user.clone() })
            .collect();
    }

    /// `login {name}` on a Forgejo block: use that `tea` login from now on.
    pub(super) async fn login_tea(self: &Arc<Self>, name: &str) -> Result<serde_json::Value, String> {
        let tea = Arc::new(Tea { runner: self.runner().await? });
        let logins = tea.logins().await?;
        let l = logins.iter().find(|l| l.name == name).cloned().ok_or_else(|| format!("tea has no login {name:?}"))?;
        *self.you.lock().unwrap() = None;
        let provider = self.config.lock().unwrap().provider;
        self.connected(provider, l, tea);
        self.read(true).await;
        self.ctx.changed();
        to_value(LoggedIn { login: name.to_owned() })
    }
}
