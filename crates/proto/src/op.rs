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

    /// The request when the call has no JSON body to read (no
    /// `Content-Type`), for a route that always took its body as optional.
    fn absent() -> Option<Self> {
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

impl Request for crate::api::PromptRequest {}

impl Request for crate::api::AskRequest {}

impl Request for crate::api::WithdrawRequest {}

impl Request for crate::api::PermitRequest {}

/// The hook input `arugula inbox` passes on as it got it.
impl Request for serde_json::Value {}

impl Request for crate::api::FollowUpRequest {}

impl Request for crate::api::IdeDiffsRequest {}

impl Request for crate::api::IdeMentionRequest {}

impl Request for crate::api::ConversationsQuery {}

impl Request for crate::api::OpenConversationRequest {}

impl Request for crate::api::Subscription {}

impl Request for crate::api::ThreadPostRequest {}

impl Request for crate::api::ThreadReadRequest {}

impl Request for crate::api::FountainQuery {}

impl Request for crate::api::NotifyRequest {}

impl Request for crate::api::StudioLoginRequest {}

impl Request for crate::api::FollowerLinkRequest {}

/// A promote's body may be left off (`Content-Type` and all).
impl Request for crate::hosts::PromoteRequest {
    fn absent() -> Option<Self> {
        Some(Self::default())
    }
}

impl Request for crate::api::InstallAdapterRequest {
    fn absent() -> Option<Self> {
        Some(Self::default())
    }
}

// #578 part a: fs, hosts, setup.
impl Request for crate::fs::FsQuery {}

impl Request for crate::fs::CdRequest {}

impl Request for crate::hosts::AddHost {}

impl Request for crate::setup::SetupQuery {}

/// The body may be left off (the page sends none to start a join).
impl Request for crate::setup::ControlJoinRequest {
    fn absent() -> Option<Self> {
        Some(Self::default())
    }
}

impl Request for crate::setup::ControlConfirmRequest {}

// #578 part b: access and credentials.
impl Request for crate::api::ShareRequest {}

impl Request for crate::api::GuestInviteRequest {}

impl Request for crate::api::AclSetRequest {}

impl Request for crate::api::LinkRequest {}

impl Request for crate::api::InviteRequest {}

impl Request for crate::api::TeamPinsRequest {}

impl Request for crate::api::McpTokenRequest {}

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
    /// The flag (`arugula_proto::flags`) the machine needs on for this
    /// operation's MCP tool or kind to be listed; it works by name either
    /// way. HTTP routes don't read it.
    const FLAG: Option<&'static str> = None;
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
        Machine, PaneId,
        api::{
            AclSetRequest, Adapters, AgentsInventory, AskAnswer, AskRequest, AttentionRequest, ConversationList,
            ConversationsQuery, DetectionAnswer, DriverEntry, Empty, FollowUpRequest, FollowedUp, FollowerLinkRequest,
            FountainAgents, FountainQuery, GuestInvite, GuestInviteRequest, IdeDiffs, IdeDiffsRequest, IdeInfo,
            IdeMentionRequest, IdeMentioned, InboxAnswer, InstallAdapterRequest, InviteRequest, Invited, KeysRequest,
            LinkRequest, McpTokenRequest, MouseRequest, NotifyPref, NotifyRequest, OpenConversationRequest,
            OpenConversationResponse, PaneDiff, PaneSummary, PermitAnswer, PermitRequest, Process, PromptRequest,
            PromptResult, PushKey, PushSubscriptions, Rules, RunResponse, SendRequest, Share, ShareRequest, ShellEnv,
            SigninLink, StudioApps, StudioLoggedIn, StudioLoginRequest, StudioStatus, Subscription, SyncedHost,
            SyncedRotated, TeamPins, TeamPinsRequest, ThreadMessages, ThreadPostRequest, ThreadPosted,
            ThreadReadRequest, TokenInfo, WaitRequest, WaitResult, WithdrawRequest,
        },
        flags::{FlagInfo, FlagSetRequest},
        hosts::{Demoted, Host, PromoteRequest, SandboxList},
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

    /// `POST /api/panes/N/prompt` (#147): prompt the agent in a pane and
    /// wait for its turn (`arugula send --wait`, MCP's `prompt_agent`). It
    /// types as someone, but a pane's agent waiting is not `DRIVES`: it
    /// never was.
    pub struct PanePrompt;

    impl Op for PanePrompt {
        const NAME: &'static str = "pane.prompt";
        const METHOD: Method = Method::Post;
        const PATH: &'static str = "/api/panes/{id}/prompt";
        const ACCESS: Access = Access::Pane(Role::Editor);
        type Path = PaneId;
        type Req = PromptRequest;
        type Res = PromptResult;
    }

    /// `POST /api/panes/N/ask` (`arugula ask`): AskUserQuestion's questions
    /// on a card beside the pane, until someone answers. The card goes if
    /// the caller does.
    pub struct PaneAsk;

    impl Op for PaneAsk {
        const NAME: &'static str = "pane.ask";
        const METHOD: Method = Method::Post;
        const PATH: &'static str = "/api/panes/{id}/ask";
        const ACCESS: Access = Access::Pane(Role::Editor);
        type Path = PaneId;
        type Req = AskRequest;
        type Res = AskAnswer;
    }

    /// `POST /api/panes/N/ask/withdraw`: the asker gave up.
    pub struct PaneAskWithdraw;

    impl Op for PaneAskWithdraw {
        const NAME: &'static str = "pane.ask_withdraw";
        const METHOD: Method = Method::Post;
        const PATH: &'static str = "/api/panes/{id}/ask/withdraw";
        const ACCESS: Access = Access::Pane(Role::Editor);
        type Path = PaneId;
        type Req = WithdrawRequest;
        type Res = Empty;
    }

    /// `POST /api/panes/N/permit` (M29, `arugula hook`): Claude Code's
    /// permission prompt on a card, until someone answers. The card goes if
    /// the caller does.
    pub struct PanePermit;

    impl Op for PanePermit {
        const NAME: &'static str = "pane.permit";
        const METHOD: Method = Method::Post;
        const PATH: &'static str = "/api/panes/{id}/permit";
        const ACCESS: Access = Access::Pane(Role::Editor);
        type Path = PaneId;
        type Req = PermitRequest;
        type Res = PermitAnswer;
    }

    /// `POST /api/panes/N/inbox` (M29, `arugula inbox`): wait for a
    /// follow-up for the agent. The waiter goes if the caller does.
    pub struct PaneInbox;

    impl Op for PaneInbox {
        const NAME: &'static str = "pane.inbox";
        const METHOD: Method = Method::Post;
        const PATH: &'static str = "/api/panes/{id}/inbox";
        const ACCESS: Access = Access::Pane(Role::Editor);
        type Path = PaneId;
        type Req = serde_json::Value;
        type Res = InboxAnswer;
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

    /// `GET /api/ide` (M28): arugulad as Claude Code's IDE, and the other
    /// IDEs registered beside it (`arugula ide`).
    pub struct IdeGet;

    impl Op for IdeGet {
        const NAME: &'static str = "ide.get";
        const METHOD: Method = Method::Get;
        const PATH: &'static str = "/api/ide";
        const ACCESS: Access = Access::Owner;
        type Path = ();
        type Req = Empty;
        type Res = IdeInfo;
    }

    /// `PUT /api/ide {"diffs": NAME}`: which IDE gets Claude Code's diffs
    /// (`arugula ide --diffs`).
    pub struct IdeSet;

    impl Op for IdeSet {
        const NAME: &'static str = "ide.set";
        const METHOD: Method = Method::Put;
        const PATH: &'static str = "/api/ide";
        const ACCESS: Access = Access::Owner;
        type Path = ();
        type Req = IdeDiffsRequest;
        type Res = IdeDiffs;
    }

    /// `POST /api/ide/mention` (M28): put `@file#Lstart-end` in Claude
    /// Code's prompt in a pane, as an IDE does. Typing there needs editor
    /// access, which the handler checks on the pane named in the body.
    pub struct IdeMention;

    impl Op for IdeMention {
        const NAME: &'static str = "ide.mention";
        const METHOD: Method = Method::Post;
        const PATH: &'static str = "/api/ide/mention";
        const ACCESS: Access = Access::Handler;
        type Path = ();
        type Req = IdeMentionRequest;
        type Res = IdeMentioned;
    }

    /// `GET /api/hosts/self/agents` (#145): the agents configured on this
    /// machine, as `chant audit --agents` last found them, and which screen
    /// rule sets run here because of it (`arugula describe --agents`).
    pub struct AgentsGet;

    impl Op for AgentsGet {
        const NAME: &'static str = "agents.get";
        const METHOD: Method = Method::Get;
        const PATH: &'static str = "/api/hosts/self/agents";
        const ACCESS: Access = Access::Owner;
        type Path = ();
        type Req = Empty;
        type Res = AgentsInventory;
    }

    /// `POST /api/hosts/self/agents/refresh`: ask chant again, and wait
    /// (`arugula describe --agents --refresh`).
    pub struct AgentsRefresh;

    impl Op for AgentsRefresh {
        const NAME: &'static str = "agents.refresh";
        const METHOD: Method = Method::Post;
        const PATH: &'static str = "/api/hosts/self/agents/refresh";
        const ACCESS: Access = Access::Owner;
        type Path = ();
        type Req = Empty;
        type Res = AgentsInventory;
    }

    /// `GET /api/agents/adapters` (#111): whether Claude Code's and Codex's
    /// adapters can start here, and the command that installs each.
    pub struct AdaptersList;

    impl Op for AdaptersList {
        const NAME: &'static str = "adapters.list";
        const METHOD: Method = Method::Get;
        const PATH: &'static str = "/api/agents/adapters";
        const ACCESS: Access = Access::Owner;
        type Path = ();
        type Req = Empty;
        type Res = Adapters;
    }

    /// `POST /api/agents/adapters/KIND/install` (#111): its `npm install`,
    /// in a new pane (beside `split`, else a tab) to watch. The body is
    /// optional.
    pub struct AdapterInstall;

    impl Op for AdapterInstall {
        const NAME: &'static str = "adapter.install";
        const METHOD: Method = Method::Post;
        const PATH: &'static str = "/api/agents/adapters/{kind}/install";
        const ACCESS: Access = Access::Owner;
        type Path = String;
        type Req = InstallAdapterRequest;
        type Res = RunResponse;
    }

    /// `GET /api/conversations` (M33): Claude Code conversations on this
    /// machine, newest first, with the block that has each one open
    /// (`arugula claude ls`). The query string is the request.
    pub struct ConversationsList;

    impl Op for ConversationsList {
        const NAME: &'static str = "conversations.list";
        const METHOD: Method = Method::Get;
        const PATH: &'static str = "/api/conversations";
        const ACCESS: Access = Access::Owner;
        type Path = ();
        type Req = ConversationsQuery;
        type Res = ConversationList;
    }

    /// `POST /api/conversations/open` (M33): a conversation as an agent
    /// block, stopped, showing its transcript; the block that already has
    /// it, if one does. `then` continues or forks it.
    pub struct ConversationOpen;

    impl Op for ConversationOpen {
        const NAME: &'static str = "conversation.open";
        const METHOD: Method = Method::Post;
        const PATH: &'static str = "/api/conversations/open";
        const ACCESS: Access = Access::Owner;
        type Path = ();
        type Req = OpenConversationRequest;
        type Res = OpenConversationResponse;
    }

    /// `GET /api/threads/{target}` (M61): a thread's messages, as the caller
    /// may read them (MCP's `read_thread`). The mux checks each thread.
    pub struct ThreadGet;

    impl Op for ThreadGet {
        const NAME: &'static str = "thread.get";
        const METHOD: Method = Method::Get;
        const PATH: &'static str = "/api/threads/{target}";
        const ACCESS: Access = Access::Handler;
        const FLAG: Option<&'static str> = Some(crate::flags::CHAT);
        type Path = crate::ThreadTarget;
        type Req = Empty;
        type Res = ThreadMessages;
    }

    /// `POST /api/threads/{target}` (M61): post in a thread, which an
    /// `@agent` in a pane's one carries to the pane's agent (MCP's
    /// `post_thread`).
    pub struct ThreadPost;

    impl Op for ThreadPost {
        const NAME: &'static str = "thread.post";
        const METHOD: Method = Method::Post;
        const PATH: &'static str = "/api/threads/{target}";
        const ACCESS: Access = Access::Handler;
        const FLAG: Option<&'static str> = Some(crate::flags::CHAT);
        type Path = crate::ThreadTarget;
        type Req = ThreadPostRequest;
        type Res = ThreadPosted;
    }

    /// `POST /api/threads/{target}/read`: the caller has read a thread up to
    /// a message.
    pub struct ThreadRead;

    impl Op for ThreadRead {
        const NAME: &'static str = "thread.read";
        const METHOD: Method = Method::Post;
        const PATH: &'static str = "/api/threads/{target}/read";
        const ACCESS: Access = Access::Handler;
        type Path = crate::ThreadTarget;
        type Req = ThreadReadRequest;
        type Res = Empty;
    }

    /// `GET /api/fountain/agents` (M43): the person's Fountain agents, read
    /// with their own login on this host (`arugula fountain agents`). The
    /// query string is the request.
    pub struct FountainAgentsGet;

    impl Op for FountainAgentsGet {
        const NAME: &'static str = "fountain.agents";
        const METHOD: Method = Method::Get;
        const PATH: &'static str = "/api/fountain/agents";
        const ACCESS: Access = Access::Owner;
        const FLAG: Option<&'static str> = Some(crate::flags::FOUNTAIN);
        type Path = ();
        type Req = FountainQuery;
        type Res = FountainAgents;
    }

    /// `GET /api/machines`: the machines panes run on (`arugula machines`).
    pub struct MachinesList;

    impl Op for MachinesList {
        const NAME: &'static str = "machines.list";
        const METHOD: Method = Method::Get;
        const PATH: &'static str = "/api/machines";
        const ACCESS: Access = Access::Owner;
        type Path = ();
        type Req = Empty;
        type Res = Vec<Machine>;
    }

    /// `POST /api/machines/N/reset`: throw a machine away and start it
    /// again from its image.
    pub struct MachineReset;

    impl Op for MachineReset {
        const NAME: &'static str = "machine.reset";
        const METHOD: Method = Method::Post;
        const PATH: &'static str = "/api/machines/{id}/reset";
        const ACCESS: Access = Access::Owner;
        type Path = u32;
        type Req = Empty;
        type Res = Empty;
    }

    /// `GET /api/push/key` (M29): the daemon's public VAPID key, for a
    /// browser to subscribe with. Anyone here may: it is not a credential.
    pub struct PushKeyGet;

    impl Op for PushKeyGet {
        const NAME: &'static str = "push.key";
        const METHOD: Method = Method::Get;
        const PATH: &'static str = "/api/push/key";
        const ACCESS: Access = Access::Anyone;
        type Path = ();
        type Req = Empty;
        type Res = PushKey;
    }

    /// `POST /api/push/subscribe` (M29): a browser's subscription. Anyone
    /// with access here may; it is theirs, never someone else's (the
    /// handler checks).
    pub struct PushSubscribe;

    impl Op for PushSubscribe {
        const NAME: &'static str = "push.subscribe";
        const METHOD: Method = Method::Post;
        const PATH: &'static str = "/api/push/subscribe";
        const ACCESS: Access = Access::Handler;
        type Path = ();
        type Req = Subscription;
        type Res = PushSubscriptions;
    }

    /// `POST /api/push/test`: send the caller a test notification.
    pub struct PushTest;

    impl Op for PushTest {
        const NAME: &'static str = "push.test";
        const METHOD: Method = Method::Post;
        const PATH: &'static str = "/api/push/test";
        const ACCESS: Access = Access::Handler;
        type Path = ();
        type Req = Empty;
        type Res = PushSubscriptions;
    }

    /// `GET /api/notify` (M29): what "needs you" notifications the caller
    /// gets here. The handler answers each person for themselves.
    pub struct NotifyGet;

    impl Op for NotifyGet {
        const NAME: &'static str = "notify.get";
        const METHOD: Method = Method::Get;
        const PATH: &'static str = "/api/notify";
        const ACCESS: Access = Access::Handler;
        type Path = ();
        type Req = Empty;
        type Res = NotifyPref;
    }

    /// `POST /api/notify` (M29): opt in or out of a session's agents, or all
    /// of them.
    pub struct NotifySet;

    impl Op for NotifySet {
        const NAME: &'static str = "notify.set";
        const METHOD: Method = Method::Post;
        const PATH: &'static str = "/api/notify";
        const ACCESS: Access = Access::Handler;
        type Path = ();
        type Req = NotifyRequest;
        type Res = NotifyPref;
    }
    // #578 part a: fs, hosts, setup.

    /// `GET /api/fs/list` (M7): a directory's entries, on this host or the one a
    /// pane or machine names (`arugula fs ls`). The query string is the request.
    pub struct FsListGet;

    impl Op for FsListGet {
        const NAME: &'static str = "fs.list";
        const METHOD: Method = Method::Get;
        const PATH: &'static str = "/api/fs/list";
        const ACCESS: Access = Access::Owner;
        type Path = ();
        type Req = crate::fs::FsQuery;
        type Res = crate::fs::FsList;
    }

    /// `GET /api/fs/stat` (M7): one file or directory (`arugula fs stat`).
    pub struct FsStatGet;

    impl Op for FsStatGet {
        const NAME: &'static str = "fs.stat";
        const METHOD: Method = Method::Get;
        const PATH: &'static str = "/api/fs/stat";
        const ACCESS: Access = Access::Owner;
        type Path = ();
        type Req = crate::fs::FsQuery;
        type Res = crate::fs::FsEntry;
    }

    /// `GET /api/fs/recent` (M7): directories used lately on a host, newest
    /// first (`arugula fs recent`).
    pub struct FsRecentGet;

    impl Op for FsRecentGet {
        const NAME: &'static str = "fs.recent";
        const METHOD: Method = Method::Get;
        const PATH: &'static str = "/api/fs/recent";
        const ACCESS: Access = Access::Owner;
        type Path = ();
        type Req = crate::fs::FsQuery;
        type Res = Vec<String>;
    }

    /// `POST /api/panes/N/cd` (M7): `cd` typed into a shell waiting at its
    /// prompt (`arugula cd`). An editor's on the pane; it isn't `DRIVES`.
    pub struct PaneCd;

    impl Op for PaneCd {
        const NAME: &'static str = "pane.cd";
        const METHOD: Method = Method::Post;
        const PATH: &'static str = "/api/panes/{id}/cd";
        const ACCESS: Access = Access::Pane(Role::Editor);
        type Path = PaneId;
        type Req = crate::fs::CdRequest;
        type Res = Empty;
    }

    /// `GET /api/host`: who this daemon is. Anyone who reached it may ask; the
    /// owner also hears how it stands with control (#325).
    pub struct HostGet;

    impl Op for HostGet {
        const NAME: &'static str = "host.get";
        const METHOD: Method = Method::Get;
        const PATH: &'static str = "/api/host";
        const ACCESS: Access = Access::Anyone;
        type Path = ();
        type Req = Empty;
        type Res = crate::hosts::HostAnswer;
    }

    /// `GET /api/hosts`: the daemons a client can switch between (`arugula hosts`).
    pub struct HostsList;

    impl Op for HostsList {
        const NAME: &'static str = "hosts.list";
        const METHOD: Method = Method::Get;
        const PATH: &'static str = "/api/hosts";
        const ACCESS: Access = Access::Owner;
        type Path = ();
        type Req = Empty;
        type Res = crate::hosts::HostList;
    }

    /// `POST /api/hosts`: add a host, replacing one with the same name
    /// (`arugula hosts add`).
    pub struct HostAdd;

    impl Op for HostAdd {
        const NAME: &'static str = "host.add";
        const METHOD: Method = Method::Post;
        const PATH: &'static str = "/api/hosts";
        const ACCESS: Access = Access::Owner;
        type Path = ();
        type Req = crate::hosts::AddHost;
        type Res = crate::hosts::Host;
    }

    /// `DELETE /api/hosts/NAME`: drop a host and its tunnel (`arugula hosts rm`).
    pub struct HostRemove;

    impl Op for HostRemove {
        const NAME: &'static str = "host.remove";
        const METHOD: Method = Method::Delete;
        const PATH: &'static str = "/api/hosts/{name}";
        const ACCESS: Access = Access::Owner;
        type Path = String;
        type Req = Empty;
        type Res = Empty;
    }

    /// `POST /api/hosts/NAME/token` (M4c): mint the per-host token that lets one
    /// host dial in, replacing the last (`arugula hosts token`). A POST, so
    /// not read-only whatever it hands back.
    pub struct HostTokenMint;

    impl Op for HostTokenMint {
        const NAME: &'static str = "host_token.mint";
        const METHOD: Method = Method::Post;
        const PATH: &'static str = "/api/hosts/{name}/token";
        const ACCESS: Access = Access::Owner;
        type Path = String;
        type Req = Empty;
        type Res = crate::hosts::HostToken;
    }

    /// `DELETE /api/hosts/NAME/token`: revoke it and drop the host's dial-out
    /// connection (`arugula hosts revoke`).
    pub struct HostTokenRevoke;

    impl Op for HostTokenRevoke {
        const NAME: &'static str = "host_token.revoke";
        const METHOD: Method = Method::Delete;
        const PATH: &'static str = "/api/hosts/{name}/token";
        const ACCESS: Access = Access::Owner;
        type Path = String;
        type Req = Empty;
        type Res = Empty;
    }

    /// `GET /api/setup`: how far along each part of Getting started is;
    /// `?part=control` or `?part=agents` for one part. A JSON object whose keys
    /// depend on the part.
    pub struct SetupGet;

    impl Op for SetupGet {
        const NAME: &'static str = "setup.get";
        const METHOD: Method = Method::Get;
        const PATH: &'static str = "/api/setup";
        const ACCESS: Access = Access::Owner;
        type Path = ();
        type Req = crate::setup::SetupQuery;
        type Res = serde_json::Value;
    }

    /// `POST /api/setup/tailscale`: `tailscale serve` in front of this daemon.
    pub struct SetupTailscale;

    impl Op for SetupTailscale {
        const NAME: &'static str = "setup.tailscale";
        const METHOD: Method = Method::Post;
        const PATH: &'static str = "/api/setup/tailscale";
        const ACCESS: Access = Access::Owner;
        type Path = ();
        type Req = Empty;
        type Res = crate::setup::Outcome;
    }

    /// `POST /api/setup/control`: ask Arugula control to add this machine; the
    /// answer is the code and where to approve it. The body may be left off.
    pub struct SetupControl;

    impl Op for SetupControl {
        const NAME: &'static str = "setup.control";
        const METHOD: Method = Method::Post;
        const PATH: &'static str = "/api/setup/control";
        const ACCESS: Access = Access::Owner;
        type Path = ();
        type Req = crate::setup::ControlJoinRequest;
        type Res = serde_json::Value;
    }

    /// `POST /api/setup/control/confirm`: save the join, or drop it, once the
    /// person has compared the account's fingerprints.
    pub struct SetupControlConfirm;

    impl Op for SetupControlConfirm {
        const NAME: &'static str = "setup.control_confirm";
        const METHOD: Method = Method::Post;
        const PATH: &'static str = "/api/setup/control/confirm";
        const ACCESS: Access = Access::Owner;
        type Path = ();
        type Req = crate::setup::ControlConfirmRequest;
        type Res = serde_json::Value;
    }

    /// `POST /api/setup/claude`: add Arugula's MCP server to Claude Code.
    pub struct SetupClaude;

    impl Op for SetupClaude {
        const NAME: &'static str = "setup.claude";
        const METHOD: Method = Method::Post;
        const PATH: &'static str = "/api/setup/claude";
        const ACCESS: Access = Access::Owner;
        type Path = ();
        type Req = Empty;
        type Res = crate::setup::Outcome;
    }

    /// `POST /api/setup/agents/KIND` (#335): an agent's adapter installed or
    /// brought up to the pin, and for Claude Code the MCP server added
    /// (`arugula setup`).
    pub struct SetupAgent;

    impl Op for SetupAgent {
        const NAME: &'static str = "setup.agent";
        const METHOD: Method = Method::Post;
        const PATH: &'static str = "/api/setup/agents/{kind}";
        const ACCESS: Access = Access::Owner;
        type Path = String;
        type Req = Empty;
        type Res = crate::setup::Outcome;
    }

    // #578 part b: access and credentials. Every one is the owner's (the
    // default of `authz`'s policy, which had no arm for any of them).

    /// `GET /api/shares`: the read-only share links made here, without their
    /// tokens. A credential: a link is access.
    pub struct SharesList;

    impl Op for SharesList {
        const NAME: &'static str = "shares.list";
        const METHOD: Method = Method::Get;
        const PATH: &'static str = "/api/shares";
        const ACCESS: Access = Access::Owner;
        const CREDENTIAL: bool = true;
        type Path = ();
        type Req = Empty;
        type Res = Vec<Share>;
    }

    /// `POST /api/shares`: a read-only link to one terminal pane (`arugula
    /// share`).
    pub struct ShareMint;

    impl Op for ShareMint {
        const NAME: &'static str = "share.mint";
        const METHOD: Method = Method::Post;
        const PATH: &'static str = "/api/shares";
        const ACCESS: Access = Access::Owner;
        type Path = ();
        type Req = ShareRequest;
        type Res = Share;
    }

    /// `DELETE /api/shares/N`: end a share link now (`arugula shares revoke`).
    pub struct ShareRevoke;

    impl Op for ShareRevoke {
        const NAME: &'static str = "share.revoke";
        const METHOD: Method = Method::Delete;
        const PATH: &'static str = "/api/shares/{id}";
        const ACCESS: Access = Access::Owner;
        type Path = u32;
        type Req = Empty;
        type Res = Empty;
    }

    /// `GET /api/guests` (M65): the ssh invites made here. A credential. A
    /// build without Labs answers 501 to every method.
    pub struct GuestsList;

    impl Op for GuestsList {
        const NAME: &'static str = "guests.list";
        const METHOD: Method = Method::Get;
        const PATH: &'static str = "/api/guests";
        const ACCESS: Access = Access::Owner;
        const CREDENTIAL: bool = true;
        type Path = ();
        type Req = Empty;
        type Res = Vec<GuestInvite>;
    }

    /// `POST /api/guests`: an ssh invite to one terminal pane (`arugula share
    /// --guest`).
    pub struct GuestMint;

    impl Op for GuestMint {
        const NAME: &'static str = "guest.mint";
        const METHOD: Method = Method::Post;
        const PATH: &'static str = "/api/guests";
        const ACCESS: Access = Access::Owner;
        type Path = ();
        type Req = GuestInviteRequest;
        type Res = GuestInvite;
    }

    /// `DELETE /api/guests/N`: end an ssh invite (`arugula guests revoke`).
    pub struct GuestRevoke;

    impl Op for GuestRevoke {
        const NAME: &'static str = "guest.revoke";
        const METHOD: Method = Method::Delete;
        const PATH: &'static str = "/api/guests/{id}";
        const ACCESS: Access = Access::Owner;
        type Path = u32;
        type Req = Empty;
        type Res = Empty;
    }

    /// `GET /api/acl`: who may do what in each session, the audit log and the
    /// devices that called (`arugula access`).
    pub struct AclGet;

    impl Op for AclGet {
        const NAME: &'static str = "acl.get";
        const METHOD: Method = Method::Get;
        const PATH: &'static str = "/api/acl";
        const ACCESS: Access = Access::Owner;
        type Path = ();
        type Req = Empty;
        type Res = serde_json::Value;
    }

    /// `POST /api/acl`: give someone a role on a session, or take it away.
    pub struct AclSet;

    impl Op for AclSet {
        const NAME: &'static str = "acl.set";
        const METHOD: Method = Method::Post;
        const PATH: &'static str = "/api/acl";
        const ACCESS: Access = Access::Owner;
        type Path = ();
        type Req = AclSetRequest;
        type Res = serde_json::Value;
    }

    /// `POST /api/links` (M19): a read-only link to one session, for a
    /// browser that holds the key.
    pub struct LinkMint;

    impl Op for LinkMint {
        const NAME: &'static str = "link.mint";
        const METHOD: Method = Method::Post;
        const PATH: &'static str = "/api/links";
        const ACCESS: Access = Access::Owner;
        type Path = ();
        type Req = LinkRequest;
        type Res = serde_json::Value;
    }

    /// `POST /api/invite` (#233): share a session with someone and tell them
    /// (`arugula invite`). An agent asks the user instead (the handler
    /// refuses it).
    pub struct InviteSend;

    impl Op for InviteSend {
        const NAME: &'static str = "invite.send";
        const METHOD: Method = Method::Post;
        const PATH: &'static str = "/api/invite";
        const ACCESS: Access = Access::Owner;
        type Path = ();
        type Req = InviteRequest;
        type Res = Invited;
    }

    /// `GET /api/team-pins` (#233): the teams pinned here.
    pub struct TeamPinsGet;

    impl Op for TeamPinsGet {
        const NAME: &'static str = "team_pins.get";
        const METHOD: Method = Method::Get;
        const PATH: &'static str = "/api/team-pins";
        const ACCESS: Access = Access::Owner;
        type Path = ();
        type Req = Empty;
        type Res = TeamPins;
    }

    /// `POST /api/team-pins`: the teams the owner's browser pinned, and those
    /// it left.
    pub struct TeamPinsSet;

    impl Op for TeamPinsSet {
        const NAME: &'static str = "team_pins.set";
        const METHOD: Method = Method::Post;
        const PATH: &'static str = "/api/team-pins";
        const ACCESS: Access = Access::Owner;
        type Path = ();
        type Req = TeamPinsRequest;
        type Res = TeamPins;
    }

    /// `GET /api/mcp/tokens` (M16): the client tokens made here, without the
    /// tokens. A credential.
    pub struct McpTokensList;

    impl Op for McpTokensList {
        const NAME: &'static str = "mcp_tokens.list";
        const METHOD: Method = Method::Get;
        const PATH: &'static str = "/api/mcp/tokens";
        const ACCESS: Access = Access::Owner;
        const CREDENTIAL: bool = true;
        type Path = ();
        type Req = Empty;
        type Res = Vec<TokenInfo>;
    }

    /// `POST /api/mcp/tokens`: a token for an MCP client (`arugula mcp
    /// token`); a token of the same name is replaced.
    pub struct McpTokenMint;

    impl Op for McpTokenMint {
        const NAME: &'static str = "mcp_token.mint";
        const METHOD: Method = Method::Post;
        const PATH: &'static str = "/api/mcp/tokens";
        const ACCESS: Access = Access::Owner;
        type Path = ();
        type Req = McpTokenRequest;
        type Res = TokenInfo;
    }

    /// `DELETE /api/mcp/tokens/NAME`: cut a client token off.
    pub struct McpTokenRevoke;

    impl Op for McpTokenRevoke {
        const NAME: &'static str = "mcp_token.revoke";
        const METHOD: Method = Method::Delete;
        const PATH: &'static str = "/api/mcp/tokens/{name}";
        const ACCESS: Access = Access::Owner;
        type Path = String;
        type Req = Empty;
        type Res = Empty;
    }

    /// `GET /api/signin-link`: the link that signs a browser in here
    /// (`arugula web`). A credential, and served over the Unix socket only:
    /// `authz` never sees a socket request, so its `ACCESS` is what the
    /// socket's owner is.
    pub struct SigninLinkGet;

    impl Op for SigninLinkGet {
        const NAME: &'static str = "signin_link.get";
        const METHOD: Method = Method::Get;
        const PATH: &'static str = "/api/signin-link";
        const ACCESS: Access = Access::Owner;
        const CREDENTIAL: bool = true;
        type Path = ();
        type Req = Empty;
        type Res = SigninLink;
    }

    // #578 part c: the home daemon's synced history and sandboxes, and studio.

    /// `GET /api/synced`: the hosts whose history this daemon keeps
    /// (`arugula synced`).
    pub struct SyncedList;

    impl Op for SyncedList {
        const NAME: &'static str = "synced.list";
        const METHOD: Method = Method::Get;
        const PATH: &'static str = "/api/synced";
        const ACCESS: Access = Access::Owner;
        type Path = ();
        type Req = Empty;
        type Res = Vec<SyncedHost>;
    }

    /// `POST /api/synced/rotate-key`: re-encrypt it all under a new key.
    pub struct SyncedRotate;

    impl Op for SyncedRotate {
        const NAME: &'static str = "synced.rotate";
        const METHOD: Method = Method::Post;
        const PATH: &'static str = "/api/synced/rotate-key";
        const ACCESS: Access = Access::Owner;
        type Path = ();
        type Req = Empty;
        type Res = SyncedRotated;
    }

    /// `DELETE /api/synced/NAME`: forget a host's synced history.
    pub struct SyncedForget;

    impl Op for SyncedForget {
        const NAME: &'static str = "synced.forget";
        const METHOD: Method = Method::Delete;
        const PATH: &'static str = "/api/synced/{host}";
        const ACCESS: Access = Access::Owner;
        type Path = String;
        type Req = Empty;
        type Res = Empty;
    }

    /// `GET /api/sandboxes`: the provider's sandboxes (`arugula sandboxes`).
    /// Labs; home daemons only.
    pub struct SandboxesList;

    impl Op for SandboxesList {
        const NAME: &'static str = "sandboxes.list";
        const METHOD: Method = Method::Get;
        const PATH: &'static str = "/api/sandboxes";
        const ACCESS: Access = Access::Owner;
        type Path = ();
        type Req = Empty;
        type Res = SandboxList;
    }

    /// `POST /api/sandboxes/NAME/promote`: keep a daemon running in a sandbox.
    pub struct SandboxPromote;

    impl Op for SandboxPromote {
        const NAME: &'static str = "sandbox.promote";
        const METHOD: Method = Method::Post;
        const PATH: &'static str = "/api/sandboxes/{name}/promote";
        const ACCESS: Access = Access::Owner;
        type Path = String;
        type Req = PromoteRequest;
        type Res = Host;
    }

    /// `DELETE /api/sandboxes/NAME/resident`: stop that daemon, take its host off the list.
    pub struct SandboxDemote;

    impl Op for SandboxDemote {
        const NAME: &'static str = "sandbox.demote";
        const METHOD: Method = Method::Delete;
        const PATH: &'static str = "/api/sandboxes/{name}/resident";
        const ACCESS: Access = Access::Owner;
        type Path = String;
        type Req = Empty;
        type Res = Demoted;
    }

    /// `GET /api/studio` (M35): which studio, and whether there's a token.
    /// Labs. A way into a studio box is the owner's.
    pub struct StudioGet;

    impl Op for StudioGet {
        const NAME: &'static str = "studio.get";
        const METHOD: Method = Method::Get;
        const PATH: &'static str = "/api/studio";
        const ACCESS: Access = Access::Owner;
        type Path = ();
        type Req = Empty;
        type Res = StudioStatus;
    }

    /// `POST /api/studio`: keep a studio token (`arugula studio login`).
    pub struct StudioLogin;

    impl Op for StudioLogin {
        const NAME: &'static str = "studio.login";
        const METHOD: Method = Method::Post;
        const PATH: &'static str = "/api/studio";
        const ACCESS: Access = Access::Owner;
        type Path = ();
        type Req = StudioLoginRequest;
        type Res = StudioLoggedIn;
    }

    /// `DELETE /api/studio`: forget the token and every follower link.
    pub struct StudioLogout;

    impl Op for StudioLogout {
        const NAME: &'static str = "studio.logout";
        const METHOD: Method = Method::Delete;
        const PATH: &'static str = "/api/studio";
        const ACCESS: Access = Access::Owner;
        type Path = ();
        type Req = Empty;
        type Res = Empty;
    }

    /// `GET /api/studio/apps`: the person's apps, with the blocks that show them.
    pub struct StudioAppsList;

    impl Op for StudioAppsList {
        const NAME: &'static str = "studio.apps";
        const METHOD: Method = Method::Get;
        const PATH: &'static str = "/api/studio/apps";
        const ACCESS: Access = Access::Owner;
        type Path = ();
        type Req = Empty;
        type Res = StudioApps;
    }

    /// `PUT /api/studio/followers/APP`: keep an app's hud follower link.
    pub struct StudioFollow;

    impl Op for StudioFollow {
        const NAME: &'static str = "studio.follow";
        const METHOD: Method = Method::Put;
        const PATH: &'static str = "/api/studio/followers/{app}";
        const ACCESS: Access = Access::Owner;
        type Path = String;
        type Req = FollowerLinkRequest;
        type Res = Empty;
    }

    /// `DELETE /api/studio/followers/APP`: drop it.
    pub struct StudioUnfollow;

    impl Op for StudioUnfollow {
        const NAME: &'static str = "studio.unfollow";
        const METHOD: Method = Method::Delete;
        const PATH: &'static str = "/api/studio/followers/{app}";
        const ACCESS: Access = Access::Owner;
        type Path = String;
        type Req = Empty;
        type Res = Empty;
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
            ("prompt", ops::PanePrompt::path(&7)),
            ("ask", ops::PaneAsk::path(&7)),
            ("ask/withdraw", ops::PaneAskWithdraw::path(&7)),
            ("permit", ops::PanePermit::path(&7)),
            ("inbox", ops::PaneInbox::path(&7)),
            ("process", ops::PaneProcess::path(&7)),
            ("detection", ops::PaneDetection::path(&7)),
            ("diff", ops::PaneDiffOf::path(&7)),
            ("drivers", ops::PaneDrivers::path(&7)),
        ] {
            assert_eq!(got, format!("/api/panes/7/{path}"));
        }
        assert_eq!(ops::MachineReset::path(&4), "/api/machines/4/reset");
        assert_eq!(ops::AdapterInstall::path(&"claude".to_owned()), "/api/agents/adapters/claude/install");
        assert_eq!(1u32.fill("/api/sessions/{id}/secrets"), "/api/sessions/1/secrets");
        assert_eq!(crate::ThreadTarget::Session(2).fill("/api/threads/{target}"), "/api/threads/session-2");
        assert!(crate::api::Empty::without_body().is_some());
    }
}
