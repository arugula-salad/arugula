//! The drivers a project's environments may name (M252, native Arugula
//! #52), as the page asks for them when an owner makes an environment:
//! each one's settings ([`arugula_e2e::drivers::setting_fields`]), the
//! secret names it takes, and which of those the platform mints for each
//! run instead. A driver some projects only may use (M221's
//! arugula-release) is listed only there; the fake one only where the
//! platform allows it (testnet).

use std::sync::Arc;

use axum::{
    Json,
    extract::{Path, State},
};
use serde_json::{Value, json};

use super::{J, project_as};
use crate::{Platform, auth::Person, db::deploys::Environment};

pub const PROJECT_DRIVERS: &str = "/api/projects/{project}/drivers";

/// How a driver's secret is had: `always` minted for each run (an
/// environment can't hold it), minted `unless_held` by the environment, or
/// held (`null`).
fn minted(d: &dyn crate::drivers::Driver, project: &str, name: &str) -> Value {
    let env = |secret_names: Vec<String>| Environment {
        id: String::new(),
        project: project.to_owned(),
        name: String::new(),
        driver: d.name().to_owned(),
        secret_names,
        settings: Default::default(),
        created_at: 0,
        updated_at: 0,
    };
    let mints = |e: &Environment| d.mints(e).iter().any(|m| m.name() == name);
    if mints(&env(vec![name.to_owned()])) {
        json!("always")
    } else if mints(&env(vec![])) {
        json!("unless_held")
    } else {
        Value::Null
    }
}

/// The drivers `project`'s environments may name, as [`drivers`] answers.
pub(crate) fn catalog(project: &str) -> Vec<Value> {
    crate::drivers::names()
        .into_iter()
        .filter_map(crate::drivers::driver)
        .filter(|d| d.allowed_in(project).is_ok())
        .map(|d| {
            let secrets: Vec<Value> =
                d.secret_names().iter().map(|n| json!({ "name": n, "minted": minted(d, project, n) })).collect();
            json!({
                "name": d.name(),
                "settings": arugula_e2e::drivers::setting_fields(d.name()),
                "secrets": secrets,
                "rolls_back": d.rolls_back(),
            })
        })
        .collect()
}

/// `GET /api/projects/{project}/drivers`: what a project's environments may
/// be made with, for its members.
pub async fn drivers(State(app): State<Arc<Platform>>, s: Person, Path(project): Path<String>) -> J {
    let (p, _) = project_as(&app, &s.account, &project, false).await?;
    Ok(Json(json!({ "drivers": catalog(&p.id) })))
}
