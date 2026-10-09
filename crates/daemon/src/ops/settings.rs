//! The owner's settings routes (#575): arugulad as Claude Code's IDE (`ide`,
//! and a mention in a pane), the agents configured here, the adapters that
//! start them, and the machines panes run on. HTTP only. All are the owner's
//! (`serve` checks that before the handler runs) but a mention, which an
//! editor may send to a pane they can type in.

use arugula_proto::{
    Machine,
    api::{
        Adapters, AgentsInventory, Empty, IdeDiffs, IdeDiffsRequest, IdeInfo, IdeMentionRequest, IdeMentioned,
        IdeOther, InstallAdapterRequest, RunRequest, RunResponse,
    },
    op::ops::{
        AdapterInstall, AdaptersList, AgentsGet, AgentsRefresh, IdeGet, IdeMention, IdeSet, MachineReset, MachinesList,
    },
};
use axum::http::StatusCode;

use super::{Cx, Handle, OpError};
use crate::{
    api::{agent_env, agents_json, bad},
    mux::Api,
};

impl Handle for IdeGet {
    async fn handle(cx: &Cx<'_>, _: (), _: Empty) -> Result<IdeInfo, OpError> {
        let Some(ide) = &cx.app.ide else {
            return Ok(IdeInfo { on: false, name: None, port: None, lock_dir: None, diffs: None, others: None });
        };
        let others = ide
            .others()
            .into_iter()
            .map(|o| IdeOther { name: o.name, port: o.port, pid: o.pid, folders: o.folders, alive: o.alive })
            .collect();
        Ok(IdeInfo {
            on: true,
            name: Some(crate::ide::NAME.into()),
            port: Some(ide.port),
            lock_dir: Some(ide.lock_dir.clone()),
            diffs: Some(ide.diffs_to().unwrap_or_else(|| crate::ide::NAME.into())),
            others: Some(others),
        })
    }
}

impl Handle for IdeSet {
    async fn handle(cx: &Cx<'_>, _: (), req: IdeDiffsRequest) -> Result<IdeDiffs, OpError> {
        let ide = cx.app.ide.as_ref().ok_or_else(|| bad("arugulad isn't Claude Code's IDE here (--no-claude-ide)"))?;
        ide.set_diffs_to(Some(req.diffs)).map_err(|e| OpError::Failed(e.to_string()))?;
        Ok(IdeDiffs { diffs: ide.diffs_to().unwrap_or_else(|| crate::ide::NAME.into()) })
    }
}

impl Handle for IdeMention {
    async fn handle(cx: &Cx<'_>, _: (), m: IdeMentionRequest) -> Result<IdeMentioned, OpError> {
        let who = cx.principal()?;
        if !who.is_owner() {
            match cx.app.mux.api(|r| Api::RoleOn(who, m.pane, r)).await.flatten() {
                Some((role, _)) if role >= arugula_core::Role::Editor => {}
                Some(_) => return Err(OpError::Forbidden("you're watching that session".into())),
                None => return Err(OpError::NoPane(m.pane)),
            }
        }
        let ide = cx.app.ide.as_ref().ok_or_else(|| bad("arugulad isn't Claude Code's IDE here"))?;
        let conns = cx.app.mux.api(|r| Api::IdeConns(m.pane, r)).await.unwrap_or_default();
        if conns.is_empty() {
            return Err(OpError::Status(
                StatusCode::CONFLICT,
                format!("Claude Code in %{} isn't connected to Arugula", m.pane),
            ));
        }
        // From 0, as VS Code's extension sends them.
        let params = serde_json::json!({
            "filePath": m.file, "lineStart": m.start.saturating_sub(1), "lineEnd": m.end.max(m.start).saturating_sub(1),
        });
        for c in &conns {
            ide.notify(Some(*c), "at_mentioned", params.clone());
        }
        Ok(IdeMentioned { sent: conns.len() })
    }
}

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
