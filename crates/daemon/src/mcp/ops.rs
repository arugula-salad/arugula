//! MCP's half of an operation (#451, #572): the tool it is, or the kind
//! of a grouped tool (#349) it is, with the agent's arguments and answer.
//! The tool list (`tools.rs`) takes its row from here ([`def`], [`kind`]),
//! and a call to it runs the operation's handler ([`call`]).
//!
//! An agent's arguments aren't the HTTP request: a pane is `7` or `"%7"`,
//! and there is no path. So the arguments are a type of their own, which
//! [`McpOp::args`] turns into the operation's, and the answer adds the
//! `summary` sentence (and may be shaped for agents: `list` kind panes is
//! confined to an agent block's tab). What an agent's token reaches (its
//! tab, its session, its machine) is checked in `args`, which may ask.

use schemars::JsonSchema;
use serde::{Serialize, de::DeserializeOwned};

use std::future::Future;

use super::tools::{Args, Call, Def, Human, Kind, Out, output_schema, parse};
use crate::ops::{Cx, Handle, OpError, Via};

/// Where an operation shows in the tool list.
pub enum Surface {
    /// A tool of its own.
    Tool {
        name: &'static str,
        title: &'static str,
        /// For agents: curated, unlike the CLI's terse help.
        description: &'static str,
        destructive: bool,
        idempotent: bool,
        open_world: bool,
        /// Its place on the person's card.
        human: Human,
    },
    /// A kind of a grouped tool, which stays written out in `tools.rs`.
    Kind { tool: &'static str, kind: &'static str, description: &'static str },
}

/// An operation as an MCP tool, or as one kind of a grouped one. `M` is
/// whose it is, as for [`Handle`]: a feature crate's operation is
/// `McpOp<Mine>`, listed with [`def_of`] or [`kind_of`] and called with
/// [`call_of`].
pub trait McpOp<M = ()>: Handle<M> {
    const SURFACE: Surface;
    /// What an agent passes; its doc comments are the schema's.
    type Args: DeserializeOwned + JsonSchema + Sync + 'static;

    /// What `answer` hands to `done`, for a tool of its own: its
    /// `outputSchema` is this type's, with `summary` (`output_schema`). A
    /// kind of a grouped tool lists none, and says `Value`.
    type Answer: Serialize + JsonSchema + 'static;

    /// The operation's path and request from the agent's arguments, once
    /// what this agent's token reaches has been checked.
    fn args(call: &Call<'_>, a: &Self::Args) -> impl Future<Output = Result<(Self::Path, Self::Req), String>> + Send;

    /// The answer an agent gets, with its `summary` sentence (`done`).
    fn answer(call: &Call<'_>, a: &Self::Args, path: &Self::Path, res: Self::Res) -> Out;
}

/// A tool's row in the list. Whether a read-only token may call it follows
/// from the operation's method and `CREDENTIAL` (`ops::read_only`).
pub fn def<O: McpOp>() -> Def {
    def_of::<O, ()>()
}

/// [`def`], for `M`'s operation.
pub fn def_of<O: McpOp<M>, M>() -> Def {
    let (name, title, description, destructive, idempotent, open_world, human) = match O::SURFACE {
        Surface::Tool { name, title, description, destructive, idempotent, open_world, human } => {
            (name, title, description, destructive, idempotent, open_world, human)
        }
        Surface::Kind { tool, kind, .. } => panic!("{} is {tool}'s kind {kind}, not a tool", O::NAME),
    };
    Def {
        name,
        title,
        description,
        args: Args::One(rmcp::handler::server::common::schema_for_type::<O::Args>),
        read_only: crate::ops::read_only::<O>(),
        destructive,
        idempotent,
        open_world,
        flag: O::FLAG,
        output: Some(output_schema::<O::Answer>),
        human,
    }
}

/// A grouped tool's row for a kind.
pub const fn kind<O: McpOp>() -> Kind {
    kind_of::<O, ()>()
}

/// [`kind`], for `M`'s operation.
pub const fn kind_of<O: McpOp<M>, M>() -> Kind {
    let Surface::Kind { kind, description, .. } = O::SURFACE else {
        panic!("an operation that is a tool of its own isn't a kind")
    };
    Kind { name: kind, description, schema: rmcp::handler::server::common::schema_for_type::<O::Args>, flag: O::FLAG }
}

/// A call to the operation's tool (or kind): its arguments, the handler, its
/// answer; a refusal or a pane that's gone, as a sentence.
pub async fn call<O: McpOp>(call: &Call<'_>, args: serde_json::Value) -> Out {
    call_of::<O, ()>(call, args).await
}

/// [`call`], for `M`'s operation.
pub async fn call_of<O: McpOp<M>, M>(call: &Call<'_>, args: serde_json::Value) -> Out {
    let a: O::Args = parse(args)?;
    let (path, req) = O::args(call, &a).await?;
    let cx = Cx { app: call.app(), via: Via::Mcp(call) };
    let done = match crate::ops::flagged::<O>(call.app()) {
        Ok(()) => <O as Handle<M>>::handle(&cx, path.clone(), req).await,
        Err(e) => Err(e),
    };
    match done {
        Ok(res) => O::answer(call, &a, &path, res),
        Err(OpError::NoPane(id)) => Err(call.gone(id).await),
        Err(
            OpError::Forbidden(why)
            | OpError::Unreachable(why)
            | OpError::NoFlag(why)
            | OpError::Failed(why)
            | OpError::Status(_, why)
            | OpError::Text(_, why),
        ) => Err(why),
    }
}
