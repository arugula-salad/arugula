//! Operations (#451, #572): one declaration per thing a client can ask
//! the daemon to do, holding its wire half: the name, the route, the
//! request and answer types, and who may call it. The daemon implements
//! each one once (`crates/daemon/src/ops/`), and its HTTP route, the
//! access check for it and its MCP tool come from that; the CLI calls it
//! through [`Op`] instead of spelling the method and path itself.
//!
//! The design, and what doesn't fit it, is in `docs/operations.md`. Only
//! the operations in [`ops`] are declared so far (the pilot's, `rules` and
//! `wait` #573, and the pane verbs #574); every other route is still written out in the
//! daemon's `api.rs`.

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

/// A pane, or a session (`/api/sessions/{id}/…`): `SessionId` is the same
/// `u32`.
impl PathArgs for PaneId {
    fn fill(&self, template: &str) -> String {
        fill_hole(template, self)
    }
}

/// A name, as a flag's (`PUT /api/flags/{name}`).
impl PathArgs for String {
    fn fill(&self, template: &str) -> String {
        fill_hole(template, self)
    }
}

/// An index, as a standing rule's (`DELETE /api/rules/{index}`).
impl PathArgs for usize {
    fn fill(&self, template: &str) -> String {
        fill_hole(template, self)
    }
}

/// A thread's target, `pane-7` or `session-2` (`/api/threads/{target}`).
impl PathArgs for crate::ThreadTarget {
    fn fill(&self, template: &str) -> String {
        fill_hole(template, &self.key())
    }
}

/// `template` with its `{…}` replaced by `value`.
fn fill_hole(template: &str, value: &dyn std::fmt::Display) -> String {
    match (template.find('{'), template.find('}')) {
        (Some(a), Some(b)) if a < b => format!("{}{value}{}", &template[..a], &template[b + 1..]),
        _ => template.to_owned(),
    }
}

/// A request's body, or for a GET its query string (the CLI writes it as
/// the same `key=value` pairs, and the daemon reads it with axum's `Query`).
/// A type the route takes as JSON says nothing more; one that stands for "no
/// body" says what it is without one.
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

impl Request for crate::flags::FlagSetRequest {}

impl Request for crate::api::WaitRequest {}

impl Request for crate::api::SendRequest {}

impl Request for crate::api::KeysRequest {}

impl Request for crate::api::MouseRequest {}

impl Request for crate::api::AttentionRequest {}

impl Request for crate::api::FollowUpRequest {}

/// One operation's wire half.
pub trait Op: Send + Sync + 'static {
    /// Its own name, `noun.verb`, not any surface's (the CLI command or MCP
    /// tool it is reached by may be renamed or dropped): for logs, errors
    /// and the checked-in tables.
    const NAME: &'static str;
    const METHOD: Method;
    /// Its route, with `{id}` where [`Op::Path`] goes.
    const PATH: &'static str;
    const ACCESS: Access;
    /// Only listed where the machine has the `labs` flag on
    /// (`arugula_proto::flags`); it works either way.
    const LABS: bool = false;
    /// Types into a pane, so needs the owner's trust on their machine
    /// (M14): `authz::check` asks the mux whether the caller may (`ops::drives`).
    const DRIVES: bool = false;
    /// A GET whose answer grants access (a sign-in link, a list of tokens):
    /// a read-only MCP token can't call it. Any other GET is read-only.
    const CREDENTIAL: bool = false;
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
        api::{
            AttentionRequest, DetectionAnswer, DriverEntry, Empty, FollowUpRequest, FollowedUp, KeysRequest,
            MouseRequest, PaneDiff, PaneSummary, Process, Rules, SendRequest, ShellEnv, WaitRequest, WaitResult,
        },
        flags::{FlagInfo, FlagSetRequest},
    };

    /// `GET /api/panes`: every pane and block (`arugula ls`, MCP's `list`
    /// kind panes).
    pub struct ListPanes;

    impl Op for ListPanes {
        const NAME: &'static str = "panes.list";
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
        const NAME: &'static str = "pane.close";
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
        const NAME: &'static str = "shell_env.get";
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
        const NAME: &'static str = "shell_env.refresh";
        const METHOD: Method = Method::Post;
        const PATH: &'static str = "/api/hosts/self/shell-env/refresh";
        const ACCESS: Access = Access::Owner;
        type Path = ();
        type Req = Empty;
        type Res = ShellEnv;
    }

    /// `GET /api/flags` (#464): every flag with its state, for the owner's
    /// Settings.
    pub struct FlagsList;

    impl Op for FlagsList {
        const NAME: &'static str = "flags.list";
        const METHOD: Method = Method::Get;
        const PATH: &'static str = "/api/flags";
        const ACCESS: Access = Access::Owner;
        type Path = ();
        type Req = Empty;
        type Res = Vec<FlagInfo>;
    }

    /// `PUT /api/flags/NAME` (#464): turn a flag on or off here. It takes
    /// effect at once; pages read `HostFeatures` when they load.
    pub struct FlagSet;

    impl Op for FlagSet {
        const NAME: &'static str = "flag.set";
        const METHOD: Method = Method::Put;
        const PATH: &'static str = "/api/flags/{name}";
        const ACCESS: Access = Access::Owner;
        type Path = String;
        type Req = FlagSetRequest;
        type Res = FlagInfo;
    }

    /// `GET /api/rules` (#166): the standing permission rules agent blocks
    /// answer from (`arugula rules`).
    pub struct RulesList;

    impl Op for RulesList {
        const NAME: &'static str = "rules.list";
        const METHOD: Method = Method::Get;
        const PATH: &'static str = "/api/rules";
        const ACCESS: Access = Access::Owner;
        type Path = ();
        type Req = Empty;
        type Res = Rules;
    }

    /// `DELETE /api/rules`: forget every standing rule (`arugula rules
    /// --forget-all`).
    pub struct RulesForgetAll;

    impl Op for RulesForgetAll {
        const NAME: &'static str = "rules.forget_all";
        const METHOD: Method = Method::Delete;
        const PATH: &'static str = "/api/rules";
        const ACCESS: Access = Access::Owner;
        type Path = ();
        type Req = Empty;
        type Res = Empty;
    }

    /// `DELETE /api/rules/N`: forget one standing rule (`arugula rules
    /// --forget N`).
    pub struct RuleForget;

    impl Op for RuleForget {
        const NAME: &'static str = "rule.forget";
        const METHOD: Method = Method::Delete;
        const PATH: &'static str = "/api/rules/{index}";
        const ACCESS: Access = Access::Owner;
        type Path = usize;
        type Req = Empty;
        type Res = Empty;
    }

    /// `GET /api/panes/N/wait?until=…`: wait for a command's end, an exit, a
    /// match or an agent's attention (`arugula wait`). The query string is
    /// the request.
    pub struct PaneWait;

    impl Op for PaneWait {
        const NAME: &'static str = "pane.wait";
        const METHOD: Method = Method::Get;
        const PATH: &'static str = "/api/panes/{id}/wait";
        const ACCESS: Access = Access::Pane(Role::Viewer);
        type Path = PaneId;
        type Req = WaitRequest;
        type Res = WaitResult;
    }
    /// `POST /api/panes/N/send`: type text into a pane, with Enter or not
    /// (`arugula send`).
    pub struct PaneSend;

    impl Op for PaneSend {
        const NAME: &'static str = "pane.send";
        const METHOD: Method = Method::Post;
        const PATH: &'static str = "/api/panes/{id}/send";
        const ACCESS: Access = Access::Pane(Role::Editor);
        const DRIVES: bool = true;
        type Path = PaneId;
        type Req = SendRequest;
        type Res = Empty;
    }

    /// `POST /api/panes/N/keys`: press named keys in a pane (`arugula keys`).
    pub struct PaneKeys;

    impl Op for PaneKeys {
        const NAME: &'static str = "pane.keys";
        const METHOD: Method = Method::Post;
        const PATH: &'static str = "/api/panes/{id}/keys";
        const ACCESS: Access = Access::Pane(Role::Editor);
        const DRIVES: bool = true;
        type Path = PaneId;
        type Req = KeysRequest;
        type Res = Empty;
    }

    /// `POST /api/panes/N/mouse`: a click, press, release or drag at a cell
    /// (`arugula mouse`).
    pub struct PaneMouse;

    impl Op for PaneMouse {
        const NAME: &'static str = "pane.mouse";
        const METHOD: Method = Method::Post;
        const PATH: &'static str = "/api/panes/{id}/mouse";
        const ACCESS: Access = Access::Pane(Role::Editor);
        const DRIVES: bool = true;
        type Path = PaneId;
        type Req = MouseRequest;
        type Res = Empty;
    }

    /// `POST /api/panes/N/attention`: say what a pane wants (`arugula
    /// attention STATE`, a hook's call). It types nothing.
    pub struct PaneAttention;

    impl Op for PaneAttention {
        const NAME: &'static str = "pane.attention";
        const METHOD: Method = Method::Post;
        const PATH: &'static str = "/api/panes/{id}/attention";
        const ACCESS: Access = Access::Pane(Role::Editor);
        type Path = PaneId;
        type Req = AttentionRequest;
        type Res = Empty;
    }

    /// `POST /api/panes/N/followup` (M29): the agent in a pane's next
    /// instruction.
    pub struct PaneFollowUp;

    impl Op for PaneFollowUp {
        const NAME: &'static str = "pane.followup";
        const METHOD: Method = Method::Post;
        const PATH: &'static str = "/api/panes/{id}/followup";
        const ACCESS: Access = Access::Pane(Role::Editor);
        const DRIVES: bool = true;
        type Path = PaneId;
        type Req = FollowUpRequest;
        type Res = FollowedUp;
    }

    /// `GET /api/panes/N/process`: a pane's foreground process (`arugula
    /// process`).
    pub struct PaneProcess;

    impl Op for PaneProcess {
        const NAME: &'static str = "pane.process";
        const METHOD: Method = Method::Get;
        const PATH: &'static str = "/api/panes/{id}/process";
        const ACCESS: Access = Access::Pane(Role::Viewer);
        type Path = PaneId;
        type Req = Empty;
        type Res = Process;
    }

    /// `GET /api/panes/N/detection` (#145): how the screen of the agent in a
    /// pane reads, rule by rule (`arugula describe --detection`).
    pub struct PaneDetection;

    impl Op for PaneDetection {
        const NAME: &'static str = "pane.detection";
        const METHOD: Method = Method::Get;
        const PATH: &'static str = "/api/panes/{id}/detection";
        const ACCESS: Access = Access::Pane(Role::Viewer);
        type Path = PaneId;
        type Req = Empty;
        type Res = DetectionAnswer;
    }

    /// `GET /api/panes/N/diff` (M28): the edit a pane's diff card shows,
    /// before and after.
    pub struct PaneDiffOf;

    impl Op for PaneDiffOf {
        const NAME: &'static str = "pane.diff";
        const METHOD: Method = Method::Get;
        const PATH: &'static str = "/api/panes/{id}/diff";
        const ACCESS: Access = Access::Pane(Role::Viewer);
        type Path = PaneId;
        type Req = Empty;
        type Res = PaneDiff;
    }

    /// `GET /api/panes/N/drivers` (M13): who typed in a pane, by handoff
    /// (`arugula log --who`). The owner's: a guest has no route to it.
    pub struct PaneDrivers;

    impl Op for PaneDrivers {
        const NAME: &'static str = "pane.drivers";
        const METHOD: Method = Method::Get;
        const PATH: &'static str = "/api/panes/{id}/drivers";
        const ACCESS: Access = Access::Owner;
        type Path = PaneId;
        type Req = Empty;
        type Res = Vec<DriverEntry>;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paths_fill_their_pane() {
        assert_eq!(ops::ClosePane::path(&7), "/api/panes/7/close");
        assert_eq!(ops::ListPanes::path(&()), "/api/panes");
        assert_eq!(ops::FlagSet::path(&"labs".to_owned()), "/api/flags/labs");
        assert_eq!(ops::RuleForget::path(&3), "/api/rules/3");
        assert_eq!(ops::PaneWait::path(&7), "/api/panes/7/wait");
        for (path, got) in [
            ("send", ops::PaneSend::path(&7)),
            ("keys", ops::PaneKeys::path(&7)),
            ("mouse", ops::PaneMouse::path(&7)),
            ("attention", ops::PaneAttention::path(&7)),
            ("followup", ops::PaneFollowUp::path(&7)),
            ("process", ops::PaneProcess::path(&7)),
            ("detection", ops::PaneDetection::path(&7)),
            ("diff", ops::PaneDiffOf::path(&7)),
            ("drivers", ops::PaneDrivers::path(&7)),
        ] {
            assert_eq!(got, format!("/api/panes/7/{path}"));
        }
        assert_eq!(1u32.fill("/api/sessions/{id}/secrets"), "/api/sessions/1/secrets");
        assert_eq!(crate::ThreadTarget::Session(2).fill("/api/threads/{target}"), "/api/threads/session-2");
        assert!(crate::api::Empty::without_body().is_some());
    }
}
