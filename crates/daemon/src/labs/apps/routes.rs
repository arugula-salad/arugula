//! Studio's HTTP routes (M35): `/api/studio`, its apps and follower links,
//! added to the API's router by [`crate::labs::routes`], and the config an
//! app block is made from.

use std::{collections::HashMap, sync::Arc};

use arugula_proto::{
    PaneId,
    api::{Empty, StudioApp, StudioAppRow, StudioApps, StudioLoggedIn, StudioLoginRequest, StudioStatus},
};
use axum::{Router, http::StatusCode};

use crate::{
    api::{ApiError, Res, bad},
    mux::Api,
    server::App,
};

/// Adds studio's routes to the API's: operations (`ops/studio.rs`).
pub fn routes(r: Router<Arc<App>>) -> Router<Arc<App>> {
    use crate::ops::OpRoutes;
    use arugula_proto::op::ops::{StudioAppsList, StudioFollow, StudioGet, StudioLogin, StudioLogout, StudioUnfollow};
    r.op::<StudioGet>()
        .op::<StudioLogin>()
        .op::<StudioLogout>()
        .op::<StudioAppsList>()
        .op::<StudioFollow>()
        .op::<StudioUnfollow>()
}

/// A studio app block's config (M35) from `{app}` (and optionally `to`,
/// dropped: a deep link is the frame's, never kept): the box and studio
/// filled in from studio's list when not given.
pub async fn app_config(c: &serde_json::Value) -> Result<serde_json::Value, String> {
    let name = c["app"].as_str().filter(|n| !n.is_empty()).ok_or("an app block needs {\"app\": NAME}")?;
    let studio = super::studio::get().ok_or("no studio here")?;
    // With a follower link kept for the app, hud is told who answered,
    // wherever the block was opened from (the picker passes nothing).
    let follower = c["follower"].as_bool().unwrap_or_else(|| studio.follower(name).is_some());
    let mut out = serde_json::json!({ "app": name, "follower": follower });
    match (c["box_url"].as_str(), c["studio"].as_str()) {
        (Some(b), Some(s)) => {
            out["box_url"] = b.into();
            out["studio"] = s.into();
            if let Some(t) = c["title"].as_str() {
                out["title"] = t.into();
            }
        }
        _ => {
            let a = studio.app(name).await?;
            out["box_url"] = a.url.into();
            out["studio"] = studio.url().ok_or("not logged in to a studio")?.into();
            if let Some(t) = a.title {
                out["title"] = t.into();
            }
        }
    }
    Ok(out)
}

fn studio() -> Res<Arc<super::studio::Studio>> {
    super::studio::get().ok_or_else(|| ApiError(StatusCode::SERVICE_UNAVAILABLE, "no studio here".into()))
}

/// `GET /api/studio` (M35): which studio, and whether there's a token.
/// Never the token.
pub async fn studio_status() -> Res<StudioStatus> {
    Ok(studio()?.status())
}

/// `arugula studio login`: keep a studio token, once studio takes it.
pub async fn studio_login(req: StudioLoginRequest) -> Res<StudioLoggedIn> {
    let apps = studio()?.login(&req.url, &req.token).await.map_err(bad)?;
    Ok(StudioLoggedIn { apps: apps.into_iter().map(studio_app).collect() })
}

fn studio_app(a: super::studio::AppInfo) -> StudioApp {
    StudioApp { name: a.name, title: a.title, url: a.url, status: a.status }
}

pub async fn studio_logout() -> Res<Empty> {
    studio()?.logout().map_err(bad)?;
    Ok(Empty {})
}

/// The person's apps, from studio, with the app blocks that show them.
pub async fn studio_apps(app: &App) -> Res<StudioApps> {
    let s = studio()?;
    let apps = s.apps().await.map_err(bad)?;
    let mut blocks: HashMap<String, Vec<PaneId>> = HashMap::new();
    for p in app.mux.api(Api::Panes).await.unwrap_or_default() {
        if p.info.kind == arugula_proto::BlockType::App
            && let Some(b) = app.mux.api(|r| Api::Block(p.info.id, r)).await.flatten()
            && let Some(name) = b.config()["app"].as_str()
        {
            blocks.entry(name.to_owned()).or_default().push(p.info.id);
        }
    }
    let list: Vec<StudioAppRow> = apps
        .into_iter()
        .map(|a| {
            let blocks = blocks.get(&a.name).cloned().unwrap_or_default();
            StudioAppRow { app: studio_app(a), blocks }
        })
        .collect();
    Ok(StudioApps { studio: s.url(), apps: list })
}

/// Keep a hud follower link for an app (`hud share --role follower` in
/// its box), or with none drop it: app blocks with `follower` enter with it
/// and name who answered.
pub async fn studio_follow(name: &str, link: Option<&str>) -> Res<Empty> {
    studio()?.set_follower(name, link).map_err(bad)?;
    Ok(Empty {})
}
