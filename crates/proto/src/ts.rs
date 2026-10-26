//! The web client's copy of these types, `web/src/shared/proto.gen.ts`, made
//! from the Rust (ts-rs, the `ts` feature). [`current`] fails when the
//! committed file differs from what the types say now; `just proto-ts`
//! writes it again.

use std::{any::TypeId, collections::HashSet, path::PathBuf};

use ts_rs::{Config, TS, TypeVisitor};

use crate::{
    ClientMsg, RemoteRef, ServerMsg,
    api::{
        ActRequest, ActResponse, GuestInvite, GuestInviteRequest, InviteRequest, Invited, NotifyPref, NotifyRequest,
        OpenConversationRequest, OpenConversationResponse, OpenRequest, OpenResponse, RunRequest, RunResponse, Share,
        ShareRequest, TeamPins, TeamPinsRequest,
    },
    flags::{FlagInfo, FlagSetRequest},
    forge::ForgeState,
    hosts::{ControlState, HostFeatures, HostInfo},
};

/// Rust's aliases, which ts-rs sees through: the web client names them.
const ALIASES: &[&str] = &["PaneId", "TabId", "SessionId", "NodeId", "ClientId", "MachineId"];

/// Every type reachable from the roots, declared once, by name.
struct Collect<'a> {
    cfg: &'a Config,
    seen: HashSet<TypeId>,
    decls: Vec<(String, String)>,
}

impl TypeVisitor for Collect<'_> {
    fn visit<T: TS + 'static + ?Sized>(&mut self) {
        // Only types with a declaration of their own: derived ones.
        if T::output_path().is_none() || !self.seen.insert(TypeId::of::<T>()) {
            return;
        }
        let mut decl = T::docs().unwrap_or_default();
        decl.push_str("export ");
        decl.push_str(&T::decl(self.cfg));
        self.decls.push((T::ident(self.cfg), decl));
        T::visit_dependencies(self);
    }
}

fn generate() -> String {
    // Offsets and times stay far below 2^53, so a number is exact.
    let cfg = Config::new().with_large_int("number");
    let mut c = Collect { cfg: &cfg, seen: HashSet::new(), decls: Vec::new() };
    c.visit::<ServerMsg>();
    c.visit::<ClientMsg>();
    c.visit::<ActRequest>();
    c.visit::<ActResponse>();
    c.visit::<TeamPinsRequest>();
    c.visit::<TeamPins>();
    c.visit::<HostFeatures>();
    c.visit::<FlagInfo>();
    c.visit::<crate::api::DocLinkUrl>();
    c.visit::<crate::api::DocRow>();
    c.visit::<crate::api::DocOpenRequest>();
    c.visit::<crate::api::DocOpened>();
    c.visit::<crate::api::Ports>();
    c.visit::<FlagSetRequest>();
    c.visit::<crate::flags::Source>();
    c.visit::<crate::telemetry::TelemetryInfo>();
    c.visit::<crate::telemetry::TelemetrySetRequest>();
    c.visit::<crate::telemetry::TelemetryNoteRequest>();
    c.visit::<ControlState>();
    c.visit::<HostInfo>();
    c.visit::<InviteRequest>();
    c.visit::<Invited>();
    c.visit::<RunRequest>();
    c.visit::<RunResponse>();
    c.visit::<OpenRequest>();
    c.visit::<OpenResponse>();
    c.visit::<OpenConversationRequest>();
    c.visit::<OpenConversationResponse>();
    c.visit::<NotifyRequest>();
    c.visit::<NotifyPref>();
    c.visit::<ShareRequest>();
    c.visit::<Share>();
    c.visit::<GuestInviteRequest>();
    c.visit::<GuestInvite>();
    c.visit::<ForgeState>();
    c.visit::<RemoteRef>();
    c.visit::<crate::workspace::Record>();
    c.visit::<crate::workspace::MemberWhy>();
    c.visit::<crate::workspace::Lease>();
    // Chat held by control (M122): what control's page says to control.
    {
        use arugula_control_wire::chat::*;
        c.visit::<ChatRooms>();
        c.visit::<ChatRoomDetail>();
        c.visit::<ChatMessages>();
        c.visit::<ChatNewChannel>();
        c.visit::<ChatNewDm>();
        c.visit::<ChatChannel>();
        c.visit::<ChatChannels>();
        c.visit::<ChatAddMembers>();
        c.visit::<ChatOpenTarget>();
        c.visit::<ChatRoomChange>();
        c.visit::<ChatPost>();
        c.visit::<ChatPosted>();
        c.visit::<ChatEdit>();
        c.visit::<ChatReact>();
        c.visit::<ChatPins>();
        c.visit::<ChatTopicSet>();
        c.visit::<ChatRead>();
        c.visit::<ChatThreadRead>();
        c.visit::<ChatFollow>();
        c.visit::<ChatThreads>();
        c.visit::<ChatThreadItem>();
        c.visit::<ChatMute>();
        c.visit::<ChatLevel>();
        c.visit::<NotifyPrefs>();
        c.visit::<NotifySet>();
        c.visit::<ChatFound>();
        c.visit::<ChatOpen>();
        c.visit::<ChatLive>();
        c.visit::<ChatKeywords>();
        c.visit::<ChatTeamSettings>();
        c.visit::<ChatBeat>();
        c.visit::<ChatPresence>();
        c.visit::<ChatSaved>();
        c.visit::<ChatUnread>();
        c.visit::<ChatSave>();
        c.visit::<ChatSavedDone>();
    }
    // The platform page's projects (M150).
    c.visit::<arugula_control_wire::projects::Projects>();
    {
        use arugula_control_wire::chat_sidebar::*;
        c.visit::<ChatSidebar>();
        c.visit::<ChatSection>();
        c.visit::<ChatSort>();
    }
    // The platform page's projects (M150).
    c.visit::<arugula_control_wire::projects::Projects>();
    // A project's linked repos (M163).
    c.visit::<arugula_control_wire::projects::LinkedRepos>();
    c.visit::<arugula_control_wire::projects::LinkRepo>();
    // Content mirroring, and bodies on read (M166).
    c.visit::<arugula_control_wire::projects::RepoSettings>();
    c.visit::<arugula_control_wire::projects::ForgeContent>();
    // Drafts on linked repos (M170).
    c.visit::<arugula_control_wire::projects::ForgeDrafts>();
    // The inbox (M171).
    c.visit::<arugula_control_wire::projects::Inbox>();
    c.visit::<arugula_control_wire::projects::InboxMark>();
    c.visit::<arugula_control_wire::projects::InboxMarked>();
    // Sending one as the person (M168).
    c.visit::<arugula_control_wire::projects::SendDraft>();
    c.visit::<arugula_control_wire::projects::DraftSent>();
    // A project's runs on the platform page (M215).
    {
        use arugula_control_wire::runs::*;
        c.visit::<RunList>();
        c.visit::<RunView>();
        c.visit::<RunLogPage>();
    }
    // An agent session's card (M257).
    {
        use arugula_control_wire::sessions::*;
        c.visit::<SessionView>();
        c.visit::<SessionAct>();
        c.visit::<SessionPr>();
        c.visit::<SessionPrEdit>();
        c.visit::<SessionPrSend>();
        c.visit::<SessionPrSent>();
    }
    // Native issues (M195).
    {
        use arugula_control_wire::issues::*;
        c.visit::<IssueList>();
        c.visit::<IssueQuery>();
        c.visit::<NewIssue>();
        c.visit::<IssuePatch>();
        c.visit::<IssuePatched>();
        c.visit::<IssueEvents>();
        c.visit::<IssueHome>();
        c.visit::<IssueHomeChange>();
        c.visit::<IssueExportQuery>();
        c.visit::<IssueExport>();
    }
    c.decls.sort();
    let mut out = String::from(
        "// Generated from crates/proto by `just proto-ts`: don't edit. Change the\n\
         // Rust types and run it again; CI checks this file is current.\n\n",
    );
    for a in ALIASES {
        out.push_str(&format!("export type {a} = number;\n"));
    }
    for (_, decl) in &c.decls {
        out.push('\n');
        out.push_str(decl);
        out.push('\n');
    }
    out.push('\n');
    out.push_str(&ops_table());
    out.push_str(&flags_table());
    out
}

/// Each operation's method and path, by name, as a constant the web client
/// builds its requests from (`opPath` in `web/src/api.ts`).
fn ops_table() -> String {
    use crate::op::Op;
    let mut rows: Vec<(&str, &str, &str)> = Vec::new();
    macro_rules! row {
        ($o:ty) => {
            rows.push((<$o as Op>::NAME, <$o as Op>::METHOD.as_str(), <$o as Op>::PATH));
        };
    }
    crate::every_op!(row);
    rows.sort();
    let mut out =
        String::from("/** Each operation's method and path (`arugula_proto::op`), by name. */\nexport const OPS = {\n");
    for (name, method, path) in rows {
        out.push_str(&format!("  \"{name}\": {{ method: \"{method}\", path: \"{path}\" }},\n"));
    }
    out.push_str("} as const;\n\nexport type OpName = keyof typeof OPS;\n");
    out
}

/// The Labs flags (`arugula_proto::flags::FLAGS`), for control's page,
/// which sets them for an account or a team without a daemon to ask
/// (#347).
fn flags_table() -> String {
    let mut out =
        String::from("\n/** The Labs flags (`arugula_proto::flags::FLAGS`), in order. */\nexport const FLAGS = [\n");
    for f in crate::flags::FLAGS {
        let s = |v: &str| serde_json::to_string(v).expect("a string serializes");
        out.push_str(&format!(
            "  {{ name: {}, title: {}, about: {}, default: {}, local_only: {} }},\n",
            s(f.name),
            s(f.title),
            s(f.about),
            f.default,
            f.local_only
        ));
    }
    out.push_str("] as const;\n");
    out
}

fn path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../web/src/shared/proto.gen.ts")
}

#[test]
fn current() {
    let want = generate();
    if std::env::var_os("ARUGULA_WRITE_TS").is_some() {
        std::fs::write(path(), &want).unwrap();
        return;
    }
    let have = std::fs::read_to_string(path()).unwrap_or_default();
    assert!(have == want, "web/src/shared/proto.gen.ts is stale: run `just proto-ts`");
}
