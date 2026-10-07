//! Fountain's HTTP route: `GET /api/fountain/agents`, added to the API's
//! router by [`crate::labs::routes`].

use std::sync::Arc;

use arugula_proto::api::FountainAgents;
use axum::{
    Json, Router,
    extract::{Query, State},
    routing::get,
};
use serde::Deserialize;

use crate::{
    api::{Res, bad},
    server::App,
};

/// Adds Fountain's routes to the API's.
pub fn routes(r: Router<Arc<App>>) -> Router<Arc<App>> {
    r.route("/api/fountain/agents", get(fountain_agents))
}

#[derive(Debug, Default, Deserialize)]
pub struct FountainQuery {
    pub query: Option<String>,
    pub source: Option<String>,
    pub profile: Option<String>,
}

/// M43: the person's Fountain agents, read with their own login on this
/// host (`arugula fountain agents`): compact cards, filtered.
async fn fountain_agents(State(app): State<Arc<App>>, Query(q): Query<FountainQuery>) -> Res<Json<FountainAgents>> {
    let mut f = super::catalog::Filter::default();
    f.apply(&serde_json::json!({ "query": q.query, "source": q.source })).map_err(bad)?;
    let runner = super::local_runner(&app.mux.shell_env).await;
    let got = super::agents_for(&runner, q.profile.as_deref()).await.map_err(bad)?;
    let rows = super::rows(&got.agents, &f);
    Ok(Json(FountainAgents {
        base_url: got.login.base_url,
        profile: got.login.profile,
        total: got.agents.len(),
        filter: serde_json::to_value(&f).unwrap_or_default(),
        agents: rows.iter().map(|r| serde_json::to_value(r).unwrap_or_default()).collect(),
        unreadable: got.unreadable,
    }))
}
