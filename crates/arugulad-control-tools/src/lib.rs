//! MCP's tools for what lives in Arugula control, as this machine's owner,
//! through their grants (#232): Cloud workspaces (`cloud`, `read_cloud`,
//! #1094), linked repos (`forge_read`, `forge_draft`, `link_repo`, M169),
//! conversations (`read_thread`, `search_threads`, `post_thread`, M198),
//! native issues (`issue_write`, M200), boards (`board_read`,
//! `board_write`), runs (`start_run`, `read_run`, M216), deploys
//! (`plan_deploy`, `read_deploy`, M252) and agent sessions
//! (`session_start`, `session_reply`, aru-121; `read_session`, Fleet 5),
//! with `list`'s kinds `cloud_workspaces`, `cloud_tabs`, `linked_repos`,
//! `threads` and `sessions`.
//!
//! Each call is a route on control's platform signed with this machine's
//! key; nothing is kept here. They're registered into the base's tool list
//! where they were (`mcp::registry`), and tell the base what it asks of
//! them ([`ControlTools`]): whether they reach control from here, for
//! `capabilities` and the card, and `wait` on a run or on sessions. Reading a native
//! issue stays the base's (`mcp::issues`), since `read_forge` does it too.
//!
//! This is the crate `arugulad-control-tools` (#232): the binary calls
//! [`register`] before `arugulad_base::main`, and the daemon reaches it
//! only through `mcp::registry`.

mod agent_sessions;
mod boards;
mod cloud;
mod deploys;
mod forge_ops;
mod issues;
mod repos;
mod runs;
mod threads;

use std::time::Duration;

use arugula_proto::flags;
use futures_util::future::BoxFuture;
use rmcp::handler::server::common::schema_for_type;
use serde_json::Value;

// What the code here names as `crate::…`: the mux's modules and the daemon's,
// as it named them when it was the daemon's.
use arugula_mux::mux;
use arugulad_base::{control, fs, lab_flags, mcp, server};

use crate::{
    mcp::{
        registry::{self, ControlTools},
        results::{BoardsReach, CloudReach, RunsReach, ThreadsReach},
        tools::{Args, Call, Def, Human, Needs, NoArgs, Out, line, parse, table},
    },
    server::App,
};

/// Lists the tools where they were: `forge_read` and `forge_draft` after
/// `draft`, the rest after `device_call`, and `list`'s kinds after
/// `agents`. Before any other crate that lists a tool after one of these.
pub fn register() {
    /// What a tool is listed after, its row and its call.
    type Listed = (&'static str, fn() -> Def, registry::Run);
    let tools: [Listed; 18] = [
        ("draft", forge_read_def, run_forge_read),
        ("forge_read", forge_draft_def, run_forge_draft),
        ("device_call", read_cloud_def, run_read_cloud),
        ("read_cloud", cloud_def, run_cloud),
        ("cloud", link_repo_def, run_link_repo),
        ("link_repo", read_thread_def, run_read_thread),
        (threads::READ_TOOL, search_threads_def, run_search_threads),
        (threads::SEARCH_TOOL, post_thread_def, run_post_thread),
        (threads::POST_TOOL, issue_write_def, run_issue_write),
        (issues::WRITE_TOOL, board_read_def, run_board_read),
        (boards::READ_TOOL, board_write_def, run_board_write),
        (boards::WRITE_TOOL, start_run_def, run_start_run),
        (runs::START_TOOL, read_run_def, run_read_run),
        (runs::READ_TOOL, plan_deploy_def, run_plan_deploy),
        (deploys::PLAN_TOOL, read_deploy_def, run_read_deploy),
        (deploys::READ_TOOL, session_start_def, run_session_start),
        (agent_sessions::START_TOOL, session_reply_def, run_session_reply),
        (agent_sessions::REPLY_TOOL, read_session_def, run_read_session),
    ];
    for (after, def, run) in tools {
        registry::tool(registry::Tool { after: Some(after), def, run });
    }
    let kinds: [(&'static str, &'static crate::mcp::tools::Kind, registry::Run); 5] = [
        ("agents", &cloud::LIST_WORKSPACES, run_list_cloud_workspaces),
        ("cloud_workspaces", &cloud::LIST_TABS, run_list_cloud_tabs),
        ("cloud_tabs", &repos::LIST_LINKED, run_list_linked_repos),
        ("linked_repos", &threads::LIST, run_list_threads),
        ("threads", &agent_sessions::LIST, run_list_sessions),
    ];
    for (after, kind, run) in kinds {
        registry::kind(registry::ToolKind { tool: "list", after: Some(after), kind, run });
    }
    registry::control_tools(Box::new(Reach));
}

/// What the base asks of these tools.
struct Reach;

impl ControlTools for Reach {
    fn cloud_reach<'a>(&'a self, app: &'a App) -> BoxFuture<'a, CloudReach> {
        Box::pin(cloud::cloud_reach(app))
    }

    fn threads_reach<'a>(&'a self, app: &'a App) -> BoxFuture<'a, ThreadsReach> {
        Box::pin(threads::threads_reach(app))
    }

    fn runs_reach<'a>(&'a self, app: &'a App) -> BoxFuture<'a, RunsReach> {
        Box::pin(runs::runs_reach(app))
    }

    fn boards_reach<'a>(&'a self, app: &'a App) -> BoxFuture<'a, BoardsReach> {
        Box::pin(boards::boards_reach(app))
    }

    fn wait_run<'a>(&'a self, call: &'a Call<'a>, run: &'a str, limit: Duration) -> BoxFuture<'a, Out> {
        Box::pin(runs::wait_run(call, run, limit))
    }

    fn wait_sessions<'a>(&'a self, call: &'a Call<'a>, ids: &'a str, limit: Duration) -> BoxFuture<'a, Out> {
        Box::pin(agent_sessions::wait_sessions(call, ids, limit))
    }
}

// ---------------------------------------------------------------- calls

/// A tool whose arguments are one struct: they're parsed, then it runs.
macro_rules! one {
    ($run:ident, $f:path) => {
        fn $run<'a>(c: &'a Call<'a>, _: Option<&'static str>, args: Value) -> BoxFuture<'a, Out> {
            Box::pin(async move { $f(c, parse(args)?).await })
        }
    };
}

/// A grouped tool: its kind and arguments.
macro_rules! grouped {
    ($run:ident, $tool:expr, |$c:ident, $kind:ident, $args:ident| $body:expr) => {
        fn $run<'a>($c: &'a Call<'a>, kind: Option<&'static str>, $args: Value) -> BoxFuture<'a, Out> {
            Box::pin(async move {
                let Some($kind) = kind else {
                    return Err(format!("{} needs a kind", $tool));
                };
                $body.await
            })
        }
    };
}

/// A list of `list`'s, with no arguments.
macro_rules! no_args {
    ($run:ident, $f:path) => {
        fn $run<'a>(c: &'a Call<'a>, _: Option<&'static str>, args: Value) -> BoxFuture<'a, Out> {
            Box::pin(async move {
                parse::<NoArgs>(args)?;
                $f(c).await
            })
        }
    };
}

grouped!(run_forge_read, "forge_read", |c, kind, args| forge_ops::forge_ops(c, "forge_read", kind, args));
grouped!(run_forge_draft, "forge_draft", |c, kind, args| forge_ops::forge_ops(c, "forge_draft", kind, args));
grouped!(run_read_cloud, "read_cloud", |c, kind, args| cloud::read_cloud(c, kind, args));
grouped!(run_cloud, "cloud", |c, kind, args| cloud::drive_cloud(c, kind, args));
grouped!(run_link_repo, "link_repo", |c, kind, args| async move {
    match kind {
        "link" | "unlink" => repos::link_repo(c, kind, parse(args)?).await,
        k => Err(format!("link_repo kind {k} isn't wired up")),
    }
});
one!(run_read_thread, threads::read_thread);
one!(run_search_threads, threads::search_threads);
one!(run_post_thread, threads::post_thread);
grouped!(run_issue_write, issues::WRITE_TOOL, |c, kind, args| issues::issue_write(c, kind, args));
one!(run_board_read, boards::board_read);
grouped!(run_board_write, boards::WRITE_TOOL, |c, kind, args| boards::board_write(c, kind, args));
one!(run_start_run, runs::start_run);
one!(run_read_run, runs::read_run);
one!(run_plan_deploy, deploys::plan_deploy);
one!(run_read_deploy, deploys::read_deploy);
one!(run_session_start, agent_sessions::session_start);
one!(run_session_reply, agent_sessions::session_reply);
one!(run_read_session, agent_sessions::read_session);
one!(run_list_sessions, agent_sessions::list_sessions);
no_args!(run_list_cloud_workspaces, cloud::list_cloud_workspaces);
one!(run_list_cloud_tabs, cloud::list_cloud_tabs);
one!(run_list_linked_repos, repos::list_linked_repos);
no_args!(run_list_threads, threads::list_threads);

// ---------------------------------------------------------------- the tools

fn forge_read_def() -> Def {
    Def {
        name: "forge_read",
        title: "Read a linked repo's PRs and issues",
        description: "Read PRs and issues on GitHub repos linked to the user's projects in Arugula control (list kind linked_repos lists them), with no block open: a PR, an issue, a list of them, a PR's checks, or drafts waiting for a person. As the user (the owner of this machine), and only what GitHub lets them read. For a repo that isn't linked, show (kind pr or issue) opens a block instead. Also Arugula's own (native) issues: kind issue with item \"#N\" (in the current project) or \"iss_…\", and kind native_issues (with this machine's threads.read grant).",
        args: Args::Kinds { kinds: table(forge_ops::READ), default: None },
        read_only: true,
        destructive: false,
        idempotent: true,
        open_world: true,
        flag: None,
        output: None,
        human: Human::In("forge_draft"),
    }
}

fn forge_draft_def() -> Def {
    Def {
        name: "forge_draft",
        title: "Draft on a linked repo",
        description: "Draft a comment, a review or a new issue on a GitHub repo linked to the user's projects in Arugula control, with no block open, or discard a draft. Nothing reaches GitHub in an agent's name: the draft waits for a person to send (from the web page, their phone or the arugula CLI) or discard. Returns the draft at once; forge_read kind draft shows what became of it. Native issues in Arugula control aren't drafted: issue_write writes to them at once.",
        args: Args::Kinds { kinds: table(forge_ops::DRAFT), default: None },
        read_only: false,
        destructive: true,
        idempotent: false,
        open_world: true,
        flag: None,
        output: None,
        human: line(
            "read your projects' linked GitHub repos and draft comments, reviews or issues for you to send",
            "List the open PRs on our linked repo and draft a review of the oldest.",
            Needs::Control,
        ),
    }
}

fn read_cloud_def() -> Def {
    Def {
        name: "read_cloud",
        title: "Read a Cloud workspace",
        description: "Read one of the user's Cloud workspaces in Arugula control (list kind cloud_workspaces lists them, cloud_tabs their tabs): a tab's transcript or what it asks, or its machine's files, diff or git status. A Cloud workspace (hw_…) is a machine of a project: its tabs are its agent blocks, by pane number, and reading one wakes it.",
        args: Args::Kinds { kinds: table(cloud::READ), default: None },
        read_only: true,
        destructive: false,
        idempotent: true,
        open_world: true,
        flag: None,
        output: None,
        human: Human::In("cloud"),
    }
}

fn cloud_def() -> Def {
    Def {
        name: "cloud",
        title: "Drive a Cloud workspace",
        description: "Drive the user's Cloud workspaces in Arugula control, as the user (the owner of this machine): make one in a project, open a tab (an agent on its machine), prompt it and wait for its turn, interrupt it, answer what it asks, close it, rebuild it. A Cloud workspace (hw_…) is a machine of its project: its tabs are agent blocks on it (claude or codex), by pane number, which the project's people see too; the machine tools reach it with cloud_workspace. Changing its settings or deleting it is the user's, on Control.",
        args: Args::Kinds { kinds: table(cloud::DRIVE), default: None },
        read_only: false,
        destructive: true,
        idempotent: false,
        open_world: true,
        flag: None,
        output: None,
        human: line(
            "drive your Cloud workspaces: open an agent there, prompt it and follow its turn",
            "Open a Claude tab in my Cloud workspace and have it fix the flaky test.",
            Needs::Cloud,
        ),
    }
}

fn link_repo_def() -> Def {
    Def {
        name: "link_repo",
        title: "Link a GitHub repo to a project",
        description: "Link a GitHub repo to one of the user's projects in Arugula control, or unlink it, as the user (the owner of this machine). Linking takes admin or maintain on the repo on GitHub; unlinking purges its rows within a day. list kind linked_repos lists them.",
        args: Args::Kinds { kinds: table(repos::LINK), default: None },
        read_only: false,
        destructive: true,
        idempotent: true,
        open_world: true,
        flag: None,
        output: None,
        human: Human::In("forge_draft"),
    }
}

// M198: agents in conversations, through the owner's grants.
fn read_thread_def() -> Def {
    Def {
        name: threads::READ_TOOL,
        title: "Read a conversation",
        description: threads::READ_DESCRIPTION,
        args: Args::One(schema_for_type::<threads::ReadThreadArgs>),
        read_only: true,
        destructive: false,
        idempotent: true,
        open_world: true,
        flag: None,
        output: None,
        human: line(
            "read, search and post in your projects' conversations",
            "Search our project's channel for what we decided about sign-in.",
            Needs::Threads,
        ),
    }
}

fn search_threads_def() -> Def {
    Def {
        name: threads::SEARCH_TOOL,
        title: "Search conversations",
        description: threads::SEARCH_DESCRIPTION,
        args: Args::One(schema_for_type::<threads::SearchThreadsArgs>),
        read_only: true,
        destructive: false,
        idempotent: true,
        open_world: true,
        flag: None,
        output: None,
        human: Human::In(threads::READ_TOOL),
    }
}

fn post_thread_def() -> Def {
    Def {
        name: threads::POST_TOOL,
        title: "Post in a conversation",
        description: threads::POST_DESCRIPTION,
        args: Args::One(schema_for_type::<threads::PostThreadArgs>),
        read_only: false,
        destructive: false,
        idempotent: false,
        open_world: true,
        flag: None,
        output: None,
        human: Human::In(threads::READ_TOOL),
    }
}

// M200: native issues, written at once through the owner's grants.
fn issue_write_def() -> Def {
    Def {
        name: issues::WRITE_TOOL,
        title: "Write to a native issue",
        description: issues::WRITE_DESCRIPTION,
        args: Args::Kinds { kinds: table(issues::WRITE), default: None },
        read_only: false,
        destructive: true,
        idempotent: false,
        open_world: true,
        flag: None,
        output: None,
        human: line(
            "file your project's issues and keep them up to date",
            "File an issue for the crash we just found, with the steps.",
            Needs::Issues,
        ),
    }
}

// Boards, step 3: a project's board, through the owner's
// grants, behind the `boards` flag.
fn board_read_def() -> Def {
    Def {
        name: boards::READ_TOOL,
        title: "Read a board",
        description: boards::READ_DESCRIPTION,
        args: Args::One(schema_for_type::<boards::ReadBoardArgs>),
        read_only: true,
        destructive: false,
        idempotent: true,
        open_world: true,
        flag: Some(flags::BOARDS),
        output: None,
        human: line(
            "read your project's board and keep its rows current",
            "Mark the sign-in row done on our board.",
            Needs::Control,
        ),
    }
}

fn board_write_def() -> Def {
    Def {
        name: boards::WRITE_TOOL,
        title: "Write to a board",
        description: boards::WRITE_DESCRIPTION,
        args: Args::Kinds { kinds: table(boards::WRITE), default: None },
        read_only: false,
        destructive: true,
        idempotent: false,
        open_world: true,
        flag: Some(flags::BOARDS),
        output: None,
        human: Human::In(boards::READ_TOOL),
    }
}

// M216: a project's runs, through the owner's grants.
fn start_run_def() -> Def {
    Def {
        name: runs::START_TOOL,
        title: "Start a run",
        description: runs::START_DESCRIPTION,
        args: Args::One(schema_for_type::<runs::StartRunArgs>),
        read_only: false,
        destructive: true,
        idempotent: false,
        open_world: true,
        flag: None,
        output: None,
        human: line(
            "start your project's runs and follow them to the end",
            "Start the nightly eval run and tell me how it went.",
            Needs::Runs,
        ),
    }
}

fn read_run_def() -> Def {
    Def {
        name: runs::READ_TOOL,
        title: "Read a run",
        description: runs::READ_DESCRIPTION,
        args: Args::One(schema_for_type::<runs::ReadRunArgs>),
        read_only: true,
        destructive: false,
        idempotent: true,
        open_world: true,
        flag: None,
        output: None,
        human: Human::In(runs::START_TOOL),
    }
}

// M252: plan a project's deploy and ask; an owner approves.
fn plan_deploy_def() -> Def {
    Def {
        name: deploys::PLAN_TOOL,
        title: "Plan a deploy and ask for approval",
        description: deploys::PLAN_DESCRIPTION,
        args: Args::One(schema_for_type::<deploys::PlanDeployArgs>),
        read_only: false,
        destructive: false,
        idempotent: false,
        open_world: true,
        flag: None,
        output: None,
        human: line(
            "plan a deploy and ask an owner to approve it",
            "Plan a deploy of main to staging.",
            Needs::Control,
        ),
    }
}

fn read_deploy_def() -> Def {
    Def {
        name: deploys::READ_TOOL,
        title: "Read a deploy",
        description: deploys::READ_DESCRIPTION,
        args: Args::One(schema_for_type::<deploys::ReadDeployArgs>),
        read_only: true,
        destructive: false,
        idempotent: true,
        open_world: true,
        flag: None,
        output: None,
        human: Human::In(deploys::PLAN_TOOL),
    }
}

// Fleet 3 (aru-121): a person's agent starts, answers and prompts
// agent sessions as them, by their grants.
fn session_start_def() -> Def {
    Def {
        name: agent_sessions::START_TOOL,
        title: "Start an agent session",
        description: agent_sessions::START_DESCRIPTION,
        args: Args::One(schema_for_type::<agent_sessions::SessionStartArgs>),
        read_only: false,
        destructive: false,
        idempotent: false,
        open_world: true,
        flag: None,
        output: None,
        human: line(
            "hand an issue to an agent session on your project's cloud workspaces, and answer it",
            "Start a session on issue #260 and tell me when it pushes.",
            Needs::Control,
        ),
    }
}

fn session_reply_def() -> Def {
    Def {
        name: agent_sessions::REPLY_TOOL,
        title: "Answer or prompt an agent session",
        description: agent_sessions::REPLY_DESCRIPTION,
        args: Args::One(schema_for_type::<agent_sessions::SessionReplyArgs>),
        read_only: false,
        destructive: false,
        idempotent: false,
        open_world: true,
        flag: None,
        output: None,
        human: Human::In(agent_sessions::START_TOOL),
    }
}

// Fleet 5: following a wave of them.
fn read_session_def() -> Def {
    Def {
        name: agent_sessions::READ_TOOL,
        title: "Read an agent session",
        description: agent_sessions::READ_DESCRIPTION,
        args: Args::One(schema_for_type::<agent_sessions::ReadSessionArgs>),
        read_only: true,
        destructive: false,
        idempotent: true,
        open_world: true,
        flag: None,
        output: None,
        human: line(
            "follow a wave of agent sessions: which are done, their branches, and what each said",
            "Wait for the sessions on #262's issues and tell me what each one did.",
            Needs::Control,
        ),
    }
}
