//! Which model a session's agent uses (Arugula #215): the word its ask
//! names ("use sonnet", [`asked_in`]), else its project's setting, "agents
//! use model …" ([`show`], [`set`]), else the strongest, which its box
//! says outright (Opus for Claude Code; Codex picks its own). The box
//! reports the model it runs on, and the card shows it.

use std::sync::Arc;

use arugula_control_common::{ApiError, err};
use arugula_control_wire::{sessions::SessionSettings, work::MODELS};
use arugula_e2e::team::TeamRole;
use axum::{
    Json,
    extract::{Path, State},
    http::StatusCode,
};

use crate::{Platform, auth::Person, forge::links::membership};

/// The model an ask's text names, as a person says it: "use opus",
/// "using sonnet", "on haiku" (`MODELS`' words, in any case), after the
/// mention. The first named wins.
pub fn asked_in(text: &str) -> Option<&'static str> {
    let words: Vec<String> =
        text.split(|c: char| !c.is_alphanumeric()).filter(|w| !w.is_empty()).map(str::to_lowercase).collect();
    words.windows(2).find_map(|w| match w[0].as_str() {
        "use" | "using" | "on" | "with" => MODELS.iter().copied().find(|m| *m == w[1]),
        _ => None,
    })
}

fn settings(model: Option<String>) -> SessionSettings {
    SessionSettings { model, models: MODELS.iter().map(|m| (*m).to_owned()).collect() }
}

/// `GET /api/projects/{project}/sessions/settings`, for its members: the
/// model its agents use (`None`: the strongest), and the ones it may.
pub async fn show(
    State(app): State<Arc<Platform>>,
    Person { account }: Person,
    Path(project): Path<String>,
) -> Result<Json<SessionSettings>, ApiError> {
    let (project, _) = membership(&app, &account, &project, false).await?;
    Ok(Json(settings(app.db.agents_model(&project)?)))
}

/// `PUT /api/projects/{project}/sessions/settings` [`SessionSettings`], for
/// its owners and editors: the model its agents use from their next
/// session on (`None`: the strongest again).
pub async fn set(
    State(app): State<Arc<Platform>>,
    Person { account }: Person,
    Path(project): Path<String>,
    Json(b): Json<SessionSettings>,
) -> Result<Json<SessionSettings>, ApiError> {
    let (project, role) = membership(&app, &account, &project, true).await?;
    if role == TeamRole::Viewer {
        return Err(err(StatusCode::FORBIDDEN, "a team's owners and editors choose its agents' model"));
    }
    let model = b.model.as_deref().map(str::trim).filter(|m| !m.is_empty());
    if !app.db.set_agents_model(&project, model)? {
        return Err(err(StatusCode::BAD_REQUEST, &format!("a model is one of {}", MODELS.join(", "))));
    }
    Ok(Json(settings(app.db.agents_model(&project)?)))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The word in an ask: after "use", "using", "on" or "with", one of
    /// ours only.
    #[test]
    fn an_ask_names_its_model() {
        assert_eq!(asked_in("fix the login page, use opus"), Some("opus"));
        assert_eq!(asked_in("Using Sonnet: tidy the README"), Some("sonnet"));
        assert_eq!(asked_in("on haiku, what does `just e2e` do?"), Some("haiku"));
        assert_eq!(asked_in("with fable please, then use opus"), Some("fable"), "the first");
        assert_eq!(asked_in("the opus page is slow"), None, "named, not asked for");
        assert_eq!(asked_in("use gpt-9"), None, "not one of ours");
        assert_eq!(asked_in("use the force"), None);
    }
}
