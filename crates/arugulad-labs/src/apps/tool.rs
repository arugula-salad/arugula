//! MCP's `show {kind: "app"}` (M35): a studio app in an app block, or the
//! user's apps. The tool's row is the base's (`mcp/tools.rs`); it calls
//! this through `lab_hooks`.

use arugula_proto::{BlockType, api::OpenRequest, flags};
use serde::Serialize;
use serde_json::json;

use super::studio::AppInfo;
use crate::mcp::{
    results,
    tools::{Call, OpenAppArgs, Out, PaneArg, done},
};

/// The user's studio apps.
#[derive(Debug, Serialize)]
pub struct Apps {
    pub apps: Vec<AppInfo>,
}

pub async fn open_app(call: &Call<'_>, a: OpenAppArgs) -> Out {
    crate::lab_flags::flag(call.app.control.state_dir(), flags::STUDIO)?;
    let studio = super::studio::get().ok_or("no studio here")?;
    let Some(name) = a.app else {
        let apps = studio.apps().await?;
        let names: Vec<&str> = apps.iter().map(|a| a.name.as_str()).collect();
        return done(format!("{} apps: {}", apps.len(), names.join(", ")), Apps { apps });
    };
    let beside = match (a.beside.as_ref().map(PaneArg::id).transpose()?, call.own_pane()) {
        (Some(b), _) => Some(b),
        (None, own) => own,
    };
    if let Some(b) = beside {
        call.readable(b).await?;
    }
    let config = super::routes::app_config(&json!({ "app": name })).await?;
    let req = OpenRequest {
        kind: BlockType::App,
        config,
        session: None,
        split: beside,
        from_pane: beside,
        vm: false,
        image: None,
        host: None,
        local: true,
    };
    let block = call.open(req).await?;
    done(format!("Opened app {name} in block %{block}"), results::AppOpened { block, app: name })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn apps_as_json() {
        assert_eq!(serde_json::to_value(Apps { apps: vec![] }).unwrap(), json!({ "apps": [] }));
    }
}
