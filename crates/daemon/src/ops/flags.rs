//! `flags`: the named flags in the state dir's `flags.json` (#464, #665),
//! listed and set for the owner's Developer settings, with what control
//! says for the machine's account and team beside them (#347). HTTP only;
//! owner settings are not an agent's business, so there is no MCP tool.

use arugula_proto::{
    api::Empty,
    flags::{self, FlagInfo, FlagSetRequest},
    op::ops::{FlagSet, FlagsList},
};

use super::{Cx, Handle, OpError};

/// Every flag is a Labs feature's, so a build without Labs can still set
/// one: the file is the same, and nothing follows.
fn info(f: flags::Flag, s: flags::State) -> FlagInfo {
    FlagInfo {
        name: f.name.into(),
        title: f.title.into(),
        about: f.about.into(),
        on: s.on,
        default: f.default,
        built: crate::labs::BUILT,
        source: s.source,
        local: s.local,
        account: s.account,
        team: s.team,
    }
}

impl Handle for FlagsList {
    async fn handle(cx: &Cx<'_>, _: (), _: Empty) -> Result<Vec<FlagInfo>, OpError> {
        Ok(flags::explain(cx.app.control.state_dir()).into_iter().map(|(f, s)| info(f, s)).collect())
    }
}

impl Handle for FlagSet {
    async fn handle(cx: &Cx<'_>, name: String, req: FlagSetRequest) -> Result<FlagInfo, OpError> {
        let dir = cx.app.control.state_dir();
        match req.on {
            Some(on) => flags::set(dir, &name, on),
            None => flags::unset(dir, &name),
        }
        .map_err(|e| match e.kind() {
            std::io::ErrorKind::InvalidInput => OpError::NoFlag(e.to_string()),
            _ => OpError::Failed(format!("couldn't write {}: {e}", flags::FILE)),
        })?;
        tracing::info!(flag = %name, on = ?req.on, "flag set");
        let (f, s) = flags::explain(dir).into_iter().find(|(f, _)| f.name == name).expect("set accepted a known flag");
        Ok(info(f, s))
    }
}
