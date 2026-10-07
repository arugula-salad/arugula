//! Operations (#451, a pilot): one declaration per thing a client can ask
//! the daemon to do, holding its wire half: the name, the route, the
//! request and answer types, and who may call it. The daemon implements
//! each one once (`crates/daemon/src/ops/`), and its HTTP route, the
//! access check for it and its MCP tool come from that; the CLI calls it
//! through [`Op`] instead of spelling the method and path itself.
//!
//! The design, and what doesn't fit it, is in `docs/operations.md`. Only
//! the operations in [`ops`] are declared so far; every other route is
//! still written out in the daemon's `api.rs`.

use arugula_core::Role;
use serde::{Serialize, de::DeserializeOwned};

use crate::PaneId;

/// An HTTP method, as the routes use them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Method {
    Get,
    Post,
    Put,
    Delete,
}

impl Method {
    pub fn as_str(self) -> &'static str {
        match self {
            Method::Get => "GET",
            Method::Post => "POST",
            Method::Put => "PUT",
            Method::Delete => "DELETE",
        }
    }
}

/// Who may call an operation besides the owner, who may call anything.
/// The daemon's access check (`authz.rs`) reads it for the routes that are
/// operations.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Access {
    /// Only the owner.
    Owner,
    /// Someone with this role on the session of the pane (or block) the
    /// path names.
    Pane(Role),
    /// The handler checks: it knows what the call reaches.
    Handler,
    /// Anyone who reached the daemon.
    Anyone,
}

/// What fills a route's `{…}` segment: nothing, or a pane.
pub trait PathArgs: Clone + Send + 'static {
    fn fill(&self, template: &str) -> String;
}

impl PathArgs for () {
    fn fill(&self, template: &str) -> String {
        template.to_owned()
    }
}

impl PathArgs for PaneId {
    fn fill(&self, template: &str) -> String {
        match (template.find('{'), template.find('}')) {
            (Some(a), Some(b)) if a < b => format!("{}{self}{}", &template[..a], &template[b + 1..]),
            _ => template.to_owned(),
        }
    }
}

/// A request's body. A type the route takes as JSON says nothing more; one
/// that stands for "no body" says what it is without one.
pub trait Request: Serialize + DeserializeOwned + Send + 'static {
    /// The request when a route takes no body (a GET, or a POST like
    /// close's): then nothing is sent and nothing is read.
    fn without_body() -> Option<Self> {
        None
    }
}

impl Request for crate::api::Empty {
    fn without_body() -> Option<Self> {
        Some(crate::api::Empty {})
    }
}

/// One operation's wire half.
pub trait Op: Send + Sync + 'static {
    /// Its name: the CLI command or MCP tool it is usually reached by, for
    /// logs and errors.
    const NAME: &'static str;
    const METHOD: Method;
    /// Its route, with `{id}` where [`Op::Path`] goes.
    const PATH: &'static str;
    const ACCESS: Access;
    /// Only listed where the machine has the `labs` file
    /// (`arugula_proto::hosts::labs`); it works either way.
    const LABS: bool = false;
    /// What fills the route's `{…}`.
    type Path: PathArgs;
    type Req: Request;
    type Res: Serialize + DeserializeOwned + Send + 'static;

    /// The route with its path filled in.
    fn path(p: &Self::Path) -> String {
        p.fill(Self::PATH)
    }
}

/// The operations declared so far.
pub mod ops {
    use super::{Access, Method, Op, Role};
    use crate::{
        PaneId,
        api::{Empty, PaneSummary, ShellEnv},
    };

    /// `GET /api/panes`: every pane and block (`arugula ls`, MCP's `list`
    /// kind panes).
    pub struct ListPanes;

    impl Op for ListPanes {
        const NAME: &'static str = "ls";
        const METHOD: Method = Method::Get;
        const PATH: &'static str = "/api/panes";
        const ACCESS: Access = Access::Owner;
        type Path = ();
        type Req = Empty;
        type Res = Vec<PaneSummary>;
    }

    /// `POST /api/panes/N/close`: close a pane, ending what runs in it; its
    /// output stays in history (`arugula close`, MCP's `close`).
    pub struct ClosePane;

    impl Op for ClosePane {
        const NAME: &'static str = "close";
        const METHOD: Method = Method::Post;
        const PATH: &'static str = "/api/panes/{id}/close";
        const ACCESS: Access = Access::Pane(Role::Editor);
        type Path = PaneId;
        type Req = Empty;
        type Res = Empty;
    }

    /// `GET /api/hosts/self/shell-env` (#74): the shell environment blocks
    /// that run your tools get (`arugula shell-env`).
    pub struct ShellEnvGet;

    impl Op for ShellEnvGet {
        const NAME: &'static str = "shell-env";
        const METHOD: Method = Method::Get;
        const PATH: &'static str = "/api/hosts/self/shell-env";
        const ACCESS: Access = Access::Owner;
        type Path = ();
        type Req = Empty;
        type Res = ShellEnv;
    }

    /// `POST /api/hosts/self/shell-env/refresh`: the same, resolved again
    /// (`arugula shell-env --refresh`).
    pub struct ShellEnvRefresh;

    impl Op for ShellEnvRefresh {
        const NAME: &'static str = "shell-env --refresh";
        const METHOD: Method = Method::Post;
        const PATH: &'static str = "/api/hosts/self/shell-env/refresh";
        const ACCESS: Access = Access::Owner;
        type Path = ();
        type Req = Empty;
        type Res = ShellEnv;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paths_fill_their_pane() {
        assert_eq!(ops::ClosePane::path(&7), "/api/panes/7/close");
        assert_eq!(ops::ListPanes::path(&()), "/api/panes");
        assert!(crate::api::Empty::without_body().is_some());
    }
}
