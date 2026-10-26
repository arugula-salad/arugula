//! App blocks (M35): an app's box beside your terminals, framed on its own
//! origin, so there's no block site or proxy.
//!
//! - **Config** `{box_url, title?, app?}`: the box's origin, what to call
//!   it, and its name where it came from. A block saved by an older daemon
//!   (`{box_url, app, studio, follower}`) is brought back from the same
//!   fields; the rest is ignored.
//! - **Getting in** is the box's own: it signs the person in with their
//!   Arugula account, through the hub (`/oauth/hud`, M237), so the daemon
//!   holds no token, mints nothing and never enters a box itself. Until
//!   native #206 a studio login kept here listed the apps and minted a way
//!   in; it's gone, with the `studio` flag (`flags::RETIRED`).
//! - **Reload** (`reload`) has every client draw the frame again.

use std::sync::{Arc, Mutex};

use arugula_proto::{BlockType, Project, WorkKind};
use futures_util::future::BoxFuture;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::block::{Block, BlockCtx, Summary, no_method};

#[derive(Debug, Clone, Deserialize)]
struct Config {
    #[serde(alias = "url")]
    box_url: String,
    #[serde(default)]
    title: Option<String>,
    #[serde(default)]
    app: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize)]
struct State {
    box_url: String,
    title: Option<String>,
    app: Option<String>,
    /// Bumped by `reload`: every client draws the frame again.
    reloads: u64,
}

pub struct AppBlock {
    ctx: BlockCtx,
    config: Config,
    state: Mutex<State>,
}

/// An app block's config from what someone asked for (`{box_url}` or
/// `{url}`, `title?`, `app?`): the box's origin checked, nothing else kept.
pub fn config(c: &Value) -> Result<Value, String> {
    let c: Config = serde_json::from_value(c.clone())
        .map_err(|_| "an app block needs the app's address: {\"box_url\": \"https://…\"}".to_owned())?;
    let mut out = json!({ "box_url": box_origin(&c.box_url)? });
    if let Some(t) = c.title.as_deref().map(str::trim).filter(|t| !t.is_empty()) {
        out["title"] = json!(t);
    }
    if let Some(a) = c.app.as_deref().map(str::trim).filter(|a| !a.is_empty()) {
        out["app"] = json!(a);
    }
    Ok(out)
}

/// A box's origin from what someone gave (`https://x.example/path`, or a
/// bare host).
pub fn box_origin(s: &str) -> Result<String, String> {
    let s = s.trim();
    let full = if s.contains("://") { s.to_owned() } else { format!("https://{s}") };
    let u = reqwest::Url::parse(&full).map_err(|e| format!("not an app's address: {e}"))?;
    if !matches!(u.scheme(), "http" | "https") || u.host_str().is_none() {
        return Err(format!("not an app's address: {s}"));
    }
    Ok(u.origin().ascii_serialization())
}

impl AppBlock {
    pub fn create(ctx: BlockCtx, config: Value) -> Result<Arc<dyn Block>, String> {
        let config: Config = serde_json::from_value(self::config(&config)?).map_err(|e| format!("app config: {e}"))?;
        let state = Mutex::new(State {
            box_url: config.box_url.clone(),
            title: config.title.clone(),
            app: config.app.clone(),
            reloads: 0,
        });
        // Clients that see it get its state now: nothing else changes it
        // until a reload.
        ctx.changed();
        Ok(Arc::new(Self { ctx, config, state }))
    }

    /// What it's called: its title, its name, else the box's host.
    fn name(&self) -> String {
        let c = &self.config;
        c.title.clone().or_else(|| c.app.clone()).unwrap_or_else(|| {
            reqwest::Url::parse(&c.box_url).ok().and_then(|u| u.host_str().map(str::to_owned)).unwrap_or_default()
        })
    }
}

impl Block for AppBlock {
    fn kind(&self) -> BlockType {
        BlockType::App
    }

    fn config(&self) -> Value {
        let c = &self.config;
        let mut v = json!({ "box_url": c.box_url });
        if let Some(t) = &c.title {
            v["title"] = json!(t);
        }
        if let Some(a) = &c.app {
            v["app"] = json!(a);
        }
        v
    }

    fn state(&self) -> Value {
        serde_json::to_value(&*self.state.lock().unwrap()).unwrap_or_default()
    }

    fn text(&self) -> String {
        format!("{}\n{}\n", self.name(), self.config.box_url)
    }

    fn call(&self, method: &str, _args: Value) -> BoxFuture<'static, Result<Value, String>> {
        match method {
            "reload" => {
                self.state.lock().unwrap().reloads += 1;
                self.ctx.changed();
                Box::pin(async { Ok(json!({})) })
            }
            "state" => {
                let s = self.state();
                Box::pin(async move { Ok(s) })
            }
            m => {
                let e = no_method(BlockType::App, m);
                Box::pin(async move { Err(e) })
            }
        }
    }

    fn summary(&self) -> Summary {
        Summary {
            work: Some(WorkKind::App),
            project: Some(Project { root: self.config.box_url.clone(), name: self.name() }),
            title: Some(self.name()),
            ..Summary::default()
        }
    }

    fn close(&self) {}
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_address_is_its_origin() {
        assert_eq!(box_origin("https://pinboard.studio.example/x?y").unwrap(), "https://pinboard.studio.example");
        assert_eq!(box_origin("b.example").unwrap(), "https://b.example");
        assert_eq!(box_origin("http://127.0.0.1:9/").unwrap(), "http://127.0.0.1:9");
        assert!(box_origin("ftp://x").is_err());
        assert!(box_origin("").is_err());
    }

    #[test]
    fn a_config_keeps_the_box_and_its_names() {
        assert_eq!(
            config(&json!({ "url": "https://p.example/", "title": " Pinboard " })).unwrap(),
            json!({ "box_url": "https://p.example", "title": "Pinboard" })
        );
        // One an older daemon saved: its studio and follower are dropped.
        let old = json!({ "box_url": "https://p.example", "app": "pinboard", "studio": "https://s.example", "follower": true });
        assert_eq!(config(&old).unwrap(), json!({ "box_url": "https://p.example", "app": "pinboard" }));
        // A name alone was studio's to look up; there's nothing to ask now.
        assert!(config(&json!({ "app": "pinboard" })).unwrap_err().contains("address"));
    }
}
