//! One-time import of a legacy provider agent into an account-owned definition.
//! Only a read uses the old credential; execution always uses platform funding.
use crate::{
    ApiError, App,
    auth::Session,
    db::cloud::{AgentDefinition, Skill},
    err,
};
use axum::{Json, extract::State, http::StatusCode};
use serde::Deserialize;
use serde_json::{Value, json};
use std::{collections::BTreeMap, sync::Arc};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Import {
    agent_id: String,
    api_key: String,
}

pub async fn import(
    State(app): State<Arc<App>>,
    session: Session,
    Json(input): Json<Import>,
) -> Result<Json<Value>, ApiError> {
    let cloud = super::enabled(&app, &session.account)?;
    if input.agent_id.is_empty() || input.agent_id.len() > 128 || input.api_key.is_empty() || input.api_key.len() > 4096
    {
        return Err(err(StatusCode::UNPROCESSABLE_ENTITY, "provide the old agent ID and an API credential"));
    }
    let source = cloud
        .provider
        .legacy_agent(input.api_key, &input.agent_id)
        .await
        .map_err(|_| err(StatusCode::BAD_GATEWAY, "could not read the old agent; check its ID and credential"))?;
    let (definition, warnings) = convert(&source)?;
    let agent = app.db.create_cloud_agent(&session.account, definition)?;
    app.analytics.capture(&session.account, "cloud agent imported", json!({ "warnings": warnings.len() }));
    Ok(Json(json!({"agent":agent,"warnings":warnings})))
}

fn convert(source: &Value) -> Result<(AgentDefinition, Vec<String>), ApiError> {
    let mut warnings = vec!["Reconnect required secrets before running. Existing conversations remain on the old account and are not copied by this import.".to_owned()];
    let mut tools = source["mcp_servers"].as_object().cloned().unwrap_or_default();
    let mut names = Vec::new();
    for (index, (_, tool)) in tools.iter_mut().enumerate() {
        for field in ["headers", "env"] {
            if let Some(values) = tool[field].as_object_mut() {
                for (position, (_, value)) in values.iter_mut().enumerate() {
                    let name = format!("IMPORTED_TOOL_{index}_{field}_{position}").to_uppercase();
                    // Even non-secret environment values are reconnected by
                    // the owner: imports never persist literal credentials.
                    *value = json!(format!("${{{name}}}"));
                    names.push(name);
                }
            }
        }
        if let Some(url) = tool["url"].as_str().and_then(|s| url::Url::parse(s).ok())
            && (!url.username().is_empty() || url.password().is_some() || url.query().is_some())
        {
            *tool = json!({});
            warnings
                .push("A tool URL contained credentials or query parameters; reconfigure that tool before use.".into());
        }
    }
    let mut skills = Vec::new();
    for skill in source["skills"].as_array().into_iter().flatten() {
        if let (Some(name), Some(content)) = (skill["name"].as_str(), skill["content"].as_str()) {
            skills.push(Skill { name: name.into(), content: content.into() });
        } else {
            warnings.push("A repository-sourced skill needs to be added as an inline skill.".into());
        }
    }
    let definition = AgentDefinition {
        name: source["name"].as_str().unwrap_or("Imported agent").into(),
        instructions: source["system"]
            .as_str()
            .filter(|s| !s.trim().is_empty())
            .unwrap_or("Follow the user's task in the checked-out repository.")
            .into(),
        mcp_servers: tools.into_iter().collect::<BTreeMap<_, _>>(),
        skills,
        secret_names: names,
    };
    definition.validate().map_err(|_| {
        err(
            StatusCode::UNPROCESSABLE_ENTITY,
            "the old definition exceeds cloud agent limits; reduce its size or tool count before importing",
        )
    })?;
    Ok((definition, warnings))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn imports_configuration_without_tool_credentials_or_provider_funding() {
        let source = json!({"name":"Legacy","system":"Review changes","model":"private-model","mcp_servers":{"service":{"url":"https://tools.example/mcp","headers":{"Authorization":"Bearer old-secret"},"env":{"TOKEN":"old-token"}}},"skills":[{"name":"review","content":"Review carefully"}]});
        let (definition, warnings) = convert(&source).unwrap();
        let encoded = serde_json::to_string(&definition).unwrap();
        assert!(!encoded.contains("old-secret"));
        assert!(!encoded.contains("old-token"));
        assert!(!encoded.contains("private-model"));
        assert_eq!(definition.secret_names.len(), 2);
        assert_eq!(definition.instructions, "Review changes");
        assert_eq!(definition.skills.len(), 1);
        assert!(!warnings.is_empty());
    }
}
