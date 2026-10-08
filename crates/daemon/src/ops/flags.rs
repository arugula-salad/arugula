//! `flags`: the named flags in the state dir's `flags.json` (#464), listed
//! and set for the owner's Settings. HTTP only; owner settings are not an
//! agent's business, so there is no MCP tool.

use arugula_proto::{
    api::Empty,
    flags::{self, FlagInfo, FlagSetRequest},
    op::ops::{FlagSet, FlagsList},
};

use super::{Cx, Handle, OpError};

/// Whether this build has what `flag` turns on. A build without Labs can
/// still set `labs`: the file is the same, and nothing follows.
fn built(flag: &str) -> bool {
    flag != flags::LABS || crate::labs::BUILT
}

fn info(f: flags::Flag, on: bool) -> FlagInfo {
    FlagInfo { name: f.name.into(), about: f.about.into(), on, default: f.default, built: built(f.name) }
}

impl Handle for FlagsList {
    async fn handle(cx: &Cx<'_>, _: (), _: Empty) -> Result<Vec<FlagInfo>, OpError> {
        Ok(flags::all(cx.app.control.state_dir()).into_iter().map(|(f, on)| info(f, on)).collect())
    }
}

impl Handle for FlagSet {
    async fn handle(cx: &Cx<'_>, name: String, req: FlagSetRequest) -> Result<FlagInfo, OpError> {
        let dir = cx.app.control.state_dir();
        flags::set(dir, &name, req.on).map_err(|e| match e.kind() {
            std::io::ErrorKind::InvalidInput => OpError::NoFlag(e.to_string()),
            _ => OpError::Failed(format!("couldn't write {}: {e}", flags::FILE)),
        })?;
        tracing::info!(flag = %name, on = req.on, "flag set");
        let (f, on) = flags::all(dir).into_iter().find(|(f, _)| f.name == name).expect("set accepted a known flag");
        Ok(info(f, on))
    }
}
