//! `rules`: the standing permission rules agent blocks answer from (#166),
//! listed and forgotten for `arugula rules`. HTTP only, and the owner's
//! (`serve` checks that before the handler runs).

use arugula_proto::{
    api::{Empty, Rules, StandingRule},
    op::ops::{RuleForget, RulesForgetAll, RulesList},
};

use super::{Cx, Handle, OpError};
use axum::http::StatusCode;

impl Handle for RulesList {
    async fn handle(cx: &Cx<'_>, _: (), _: Empty) -> Result<Rules, OpError> {
        let rules = cx
            .app
            .rules
            .list()
            .into_iter()
            .enumerate()
            .map(|(i, r)| StandingRule {
                text: r.describe(),
                index: i,
                tool: r.tool,
                prefix: r.prefix,
                cwd: r.cwd,
                sprite: r.sprite,
                at_ms: r.at_ms,
                from: r.from,
            })
            .collect();
        Ok(Rules { rules })
    }
}

impl Handle for RulesForgetAll {
    async fn handle(cx: &Cx<'_>, _: (), _: Empty) -> Result<Empty, OpError> {
        cx.app.rules.forget(None).map_err(|e| OpError::Status(StatusCode::INTERNAL_SERVER_ERROR, e))?;
        Ok(Empty {})
    }
}

impl Handle for RuleForget {
    async fn handle(cx: &Cx<'_>, index: usize, _: Empty) -> Result<Empty, OpError> {
        cx.app.rules.forget(Some(index)).map_err(|e| OpError::Status(StatusCode::NOT_FOUND, e))?;
        Ok(Empty {})
    }
}
