//! The IDE's operations (#575): what Claude Code's IDE is here (`ide`), where
//! its diffs go, and an editor's mention in a pane. HTTP only. All are the
//! owner's (`serve` checks that before the handler runs) but a mention, which
//! an editor may send to a pane they can type in.

use arugula_proto::{
    api::{Empty, IdeDiffs, IdeDiffsRequest, IdeInfo, IdeMentionRequest, IdeMentioned, IdeOther},
    op::ops::{IdeGet, IdeMention, IdeSet},
};
use axum::http::StatusCode;

use crate::{
    api::bad,
    mux::Api,
    ops::{Cx, Handle, OpError},
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
