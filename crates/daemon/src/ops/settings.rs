//! The owner's settings routes (#575): the agents configured here, the
//! adapters that start them, and the machines panes run on. HTTP only. All
//! are the owner's (`serve` checks that before the handler runs). The IDE's
//! (`ide`, and a mention in a pane) are `ide/ops.rs`.

use arugula_proto::{
    Machine,
    api::{Adapters, AgentsInventory, Empty, InstallAdapterRequest, RunRequest, RunResponse},
    op::ops::{AdapterInstall, AdaptersList, AgentsGet, AgentsRefresh, MachineReset, MachinesList},
};
use axum::http::StatusCode;

use super::{Cx, Handle, OpError};
use crate::{
    api::{agent_env, agents_json, bad},
    mux::Api,
};

impl Handle for AgentsGet {
    async fn handle(cx: &Cx<'_>, _: (), _: Empty) -> Result<AgentsInventory, OpError> {
        Ok(agents_json(&cx.app.mux.inventory.snapshot()))
    }
}

impl Handle for AgentsRefresh {
    async fn handle(cx: &Cx<'_>, _: (), _: Empty) -> Result<AgentsInventory, OpError> {
        Ok(agents_json(&cx.app.mux.inventory.refresh_now().await))
    }
}

impl Handle for AdaptersList {
    async fn handle(cx: &Cx<'_>, _: (), _: Empty) -> Result<Adapters, OpError> {
        let (home, env) = agent_env(cx.app).await?;
        let list = tokio::task::spawn_blocking(move || crate::agent::adapters::all(&home, &env))
            .await
            .map_err(|e| OpError::Failed(e.to_string()))?;
        Ok(Adapters { adapters: list })
    }
}

impl Handle for AdapterInstall {
    async fn handle(cx: &Cx<'_>, kind: String, req: InstallAdapterRequest) -> Result<RunResponse, OpError> {
        let kind: crate::agent::defs::Kind =
            serde_json::from_value(serde_json::json!(kind)).map_err(|_| bad(format!("no agent {kind}")))?;
        let a = crate::agent::adapters::of(kind).ok_or_else(|| bad("that agent has no adapter to install"))?;
        let (home, env) = agent_env(cx.app).await?;
        let (st, home) = tokio::task::spawn_blocking(move || (crate::agent::adapters::status(a, &home, &env), home))
            .await
            .map_err(|e| OpError::Failed(e.to_string()))?;
        if let crate::agent::adapters::State::NoNode { .. } = st.state {
            return Err(bad(st.why()).into());
        }
        let run = RunRequest {
            command: Some(crate::agent::adapters::install_command(&st, &home)),
            session: req.session,
            split: req.split,
            from_pane: req.from_pane.or(req.split),
            ..Default::default()
        };
        match cx.app.mux.api(|r| Api::Run(run, r)).await {
            Some(Ok(pane)) => Ok(RunResponse { pane }),
            Some(Err(e)) => Err(bad(e).into()),
            None => Err(shutting_down()),
        }
    }
}

impl Handle for MachinesList {
    async fn handle(cx: &Cx<'_>, _: (), _: Empty) -> Result<Vec<Machine>, OpError> {
        Ok(cx.app.mux.api(Api::Machines).await.unwrap_or_default())
    }
}

impl Handle for MachineReset {
    async fn handle(cx: &Cx<'_>, id: u32, _: Empty) -> Result<Empty, OpError> {
        match cx.app.mux.api(|r| Api::ResetMachine(id, r)).await {
            Some(Ok(())) => Ok(Empty {}),
            Some(Err(e)) => Err(OpError::Status(StatusCode::NOT_FOUND, e)),
            None => Err(shutting_down()),
        }
    }
}

fn shutting_down() -> OpError {
    OpError::Status(StatusCode::SERVICE_UNAVAILABLE, "daemon is shutting down".into())
}
