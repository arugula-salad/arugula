//! The pane verbs that read a pane: `process`, `detection`, `diff` and
//! `drivers` (#574). HTTP only. A pane's viewers read the first three;
//! `drivers` is the owner's (`serve` checks that before a handler runs).

use arugula_proto::{
    PaneId,
    api::{Detection, DetectionAnswer, DetectionRule, DriverEntry, Empty, NoDetection, PaneDiff, Process},
    op::ops::{PaneDetection, PaneDiffOf, PaneDrivers, PaneProcess},
};
use axum::http::StatusCode;

use super::{Cx, Handle, OpError};
use crate::{
    api::{ApiError, pane},
    history,
    mux::Api,
};

impl Handle for PaneProcess {
    async fn handle(cx: &Cx<'_>, id: PaneId, _: Empty) -> Result<Process, OpError> {
        let app = cx.app;
        let p = pane(app, id).await?;
        // On a machine: ask it (its processes aren't ours to read).
        if let Some(Some(m)) = app.mux.api(|r| Api::MachineOf(id, r)).await {
            return Ok(crate::api::guest_process(app, id, &m).await?);
        }
        let pid =
            p.pid_now().ok_or_else(|| ApiError(StatusCode::CONFLICT, "nothing is running in that pane".into()))?;
        use crate::procinfo;
        let tpgid = procinfo::foreground(pid).unwrap_or(pid);
        Ok(Process {
            pid,
            foreground: tpgid,
            comm: procinfo::comm(tpgid).unwrap_or_default(),
            argv: procinfo::argv(tpgid).unwrap_or_default(),
            exe: procinfo::exe(tpgid).map(|p| p.display().to_string()),
            cwd: procinfo::cwd(tpgid).map(|p| p.display().to_string()),
        })
    }
}

/// How the screen of the agent in a pane reads, rule by rule (#145,
/// `arugula describe %N --detection`): `{agent: null, command}` when no
/// agent's screen is read there.
impl Handle for PaneDetection {
    async fn handle(cx: &Cx<'_>, id: PaneId, _: Empty) -> Result<DetectionAnswer, OpError> {
        let app = cx.app;
        let p = pane(app, id).await?;
        let (found, command) = tokio::task::spawn_blocking(move || (p.detection(), p.command()))
            .await
            .map_err(|e| ApiError(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
        Ok(match found {
            Some(d) => DetectionAnswer::Read(Detection {
                agent: d.agent.into(),
                name: d.name.into(),
                shown: d.shown.map(Into::into),
                fired: d.fired.map(Into::into),
                title: d.title,
                rules: d
                    .rules
                    .into_iter()
                    .map(|r| DetectionRule {
                        rule: r.rule.into(),
                        state: r.state.into(),
                        priority: r.priority,
                        region: r.region,
                        text: r.text,
                        matched: r.matched,
                    })
                    .collect(),
                unread: d.unread,
                // Not read here: what chant found configured instead.
                configured: d
                    .unread
                    .then(|| app.mux.inventory.snapshot().runtimes().into_iter().map(str::to_owned).collect()),
            }),
            None => DetectionAnswer::NoAgent(NoDetection { agent: None, command }),
        })
    }
}

/// The edit a pane's diff card shows (M28), before and after, for changing
/// it before accepting. A pane the caller can't see is a 404, as for a pane
/// that isn't there.
impl Handle for PaneDiffOf {
    async fn handle(cx: &Cx<'_>, id: PaneId, _: Empty) -> Result<PaneDiff, OpError> {
        let app = cx.app;
        let who = cx.principal()?;
        if !who.is_owner() && app.mux.api(|r| Api::RoleOn(who, id, r)).await.flatten().is_none() {
            return Err(OpError::NoPane(id));
        }
        let (info, old, new) = app
            .mux
            .api(|r| Api::DiffOf(id, r))
            .await
            .flatten()
            .ok_or_else(|| ApiError(StatusCode::NOT_FOUND, format!("%{id} has no edit waiting")))?;
        Ok(PaneDiff { diff: info, old, new })
    }
}

/// Who typed in a pane, by handoff (M13).
impl Handle for PaneDrivers {
    async fn handle(cx: &Cx<'_>, id: PaneId, _: Empty) -> Result<Vec<DriverEntry>, OpError> {
        let dir = cx.app.mux.store.pane_dir(id);
        Ok(tokio::task::spawn_blocking(move || history::drivers(&dir)).await.unwrap_or_default())
    }
}
