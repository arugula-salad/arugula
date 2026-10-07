//! Studio's HTTP routes (M35): `/api/studio`, its apps and follower links,
//! added to the API's router by [`crate::labs::routes`], and the config an
//! app block is made from.

use std::{collections::HashMap, sync::Arc};

use arugula_proto::{
    PaneId,
    api::{
        Empty, FollowerLinkRequest, StudioApp, StudioAppRow, StudioApps, StudioLoggedIn, StudioLoginRequest,
        StudioStatus,
    },
};
use axum::{
    Json, Router,
    extract::{Path, State},
    http::StatusCode,
    routing::get,
};

use crate::{
    api::{ApiError, Res, bad},
    mux::Api,
    server::App,
};

type AppState = State<Arc<App>>;

/// Adds studio's routes to the API's.
pub fn routes(r: Router<Arc<App>>) -> Router<Arc<App>> {
    r.route("/api/studio", get(studio_status).post(studio_login).delete(studio_logout))
        .route("/api/studio/apps", get(studio_apps))
        .route("/api/studio/followers/{app}", axum::routing::put(studio_follower).delete(studio_unfollow))
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
async fn studio_status() -> Res<Json<StudioStatus>> {
    Ok(Json(studio()?.status()))
}

/// `arugula studio login`: keep a studio token, once studio takes it.
async fn studio_login(Json(req): Json<StudioLoginRequest>) -> Res<Json<StudioLoggedIn>> {
    let apps = studio()?.login(&req.url, &req.token).await.map_err(bad)?;
    Ok(Json(StudioLoggedIn { apps: apps.into_iter().map(studio_app).collect() }))
}

fn studio_app(a: super::studio::AppInfo) -> StudioApp {
    StudioApp { name: a.name, title: a.title, url: a.url, status: a.status }
}

async fn studio_logout() -> Res<Json<Empty>> {
    studio()?.logout().map_err(bad)?;
    Ok(Json(Empty {}))
}

/// The person's apps, from studio, with the app blocks that show them.
async fn studio_apps(State(app): AppState) -> Res<Json<StudioApps>> {
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
    Ok(Json(StudioApps { studio: s.url(), apps: list }))
}

/// Keep a hud follower link for an app (`hud share --role follower` in
/// its box): app blocks with `follower` enter with it and name who
/// answered.
async fn studio_follower(Path(name): Path<String>, Json(req): Json<FollowerLinkRequest>) -> Res<Json<Empty>> {
    studio()?.set_follower(&name, Some(&req.link)).map_err(bad)?;
    Ok(Json(Empty {}))
}

async fn studio_unfollow(Path(name): Path<String>) -> Res<Json<Empty>> {
    studio()?.set_follower(&name, None).map_err(bad)?;
    Ok(Json(Empty {}))
}
