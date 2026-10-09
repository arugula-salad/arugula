//! `threads` (M61): what the people working on a pane or a session say, read,
//! posted and marked read. Over HTTP (the web, `arugula`'s clients) each
//! person reads and posts as themselves; MCP's `read_thread` and
//! `post_thread` are an agent's: read as the owner, posted as the agent.

use arugula_proto::{
    Driver, PaneId, ThreadTarget,
    api::{
        Empty, Invitable, ThreadAgent, ThreadMessages, ThreadPostRequest, ThreadPosted, ThreadReadRequest, Unreached,
        UnreachedWhy,
    },
    op::ops::{ThreadGet, ThreadPost, ThreadRead},
};
use axum::http::StatusCode;

use super::{Cx, Handle, OpError, Via};
use crate::{
    acl::Principal,
    mcp::{
        ops::{McpOp, Surface},
        results,
        tools::{Call, Out, PostThreadArgs, ThreadArgs, done},
    },
    mux::{Api, Cmd},
    server::App,
};

fn thread_err(e: crate::mux::ThreadError) -> OpError {
    OpError::Status(StatusCode::from_u16(e.0).unwrap_or(StatusCode::BAD_REQUEST), e.1)
}

fn gone() -> OpError {
    OpError::Status(StatusCode::SERVICE_UNAVAILABLE, "daemon is shutting down".into())
}

/// Who reads or posts: the person asking, or for an agent's token the owner
/// (what it reaches was checked on its arguments, `McpOp::args`).
fn reader(cx: &Cx<'_>) -> Principal {
    match &cx.via {
        Via::Http { who, .. } => who.clone(),
        Via::Mcp(_) => Principal::Owner,
    }
}

impl Handle for ThreadGet {
    /// A thread's messages, as the caller may read them.
    async fn handle(cx: &Cx<'_>, target: ThreadTarget, _: Empty) -> Result<ThreadMessages, OpError> {
        let who = reader(cx);
        let messages =
            cx.app.mux.api(|r| Api::ThreadGet(target, who, r)).await.ok_or_else(gone)?.map_err(thread_err)?;
        Ok(ThreadMessages { target, messages })
    }
}

impl Handle for ThreadPost {
    /// Post in a thread. Over HTTP, an `@agent` in a pane's thread, from
    /// someone who may drive the pane, also goes to its agent as a follow-up.
    /// An agent's post (MCP) is the owner's, shown as the agent's, and
    /// does neither that nor the offer to invite.
    async fn handle(cx: &Cx<'_>, target: ThreadTarget, req: ThreadPostRequest) -> Result<ThreadPosted, OpError> {
        let (who, as_agent, owner) = match &cx.via {
            Via::Http { who, owner, .. } => (who.clone(), None, *owner),
            Via::Mcp(call) => {
                let by = Driver { who: call.by(), name: format!("{} (agent)", call.client()) };
                (Principal::Owner, Some(by), false)
            }
        };
        let post = crate::mux::ThreadPost { target, who, as_agent, text: req.text, quote: req.quote };
        let (msg, to_agent, mut unreached) =
            cx.app.mux.api(|r| Api::ThreadPost(post, r)).await.ok_or_else(gone)?.map_err(thread_err)?;
        let mut agent = None;
        if matches!(cx.via, Via::Http { .. })
            && to_agent
            && let ThreadTarget::Pane(pane) = target
        {
            agent = Some(match tell_agent(cx.app, pane, &msg).await {
                Ok(now) => ThreadAgent { delivered: Some(now), error: None },
                Err(e) => ThreadAgent { delivered: None, error: Some(e) },
            });
        }
        // The owner, who sees every grant and roster already, is offered to
        // invite whom an @ named but who can't read the thread (#297). Nobody
        // else's post does any of this, so theirs says nothing about who exists.
        let invitable = if owner { Some(invitable(cx.app, target, &mut unreached).await) } else { None };
        Ok(ThreadPosted { message: msg, agent, unreached, invitable })
    }
}

impl Handle for ThreadRead {
    async fn handle(cx: &Cx<'_>, target: ThreadTarget, req: ThreadReadRequest) -> Result<Empty, OpError> {
        let who = reader(cx);
        crate::labs::chat().map_err(|e| OpError::Status(StatusCode::NOT_IMPLEMENTED, e))?;
        cx.app.mux.send(Cmd::Api(Api::ThreadRead(target, who, req.upto)));
        Ok(Empty {})
    }
}

/// Whom an owner's `@`s that reached nobody name, of those an invite may
/// name, who can't read the thread and could once invited (not on a
/// private pane's). Their tokens leave `unreached`: the offer says it.
async fn invitable(app: &App, target: ThreadTarget, unreached: &mut Vec<Unreached>) -> Vec<Invitable> {
    let tokens: Vec<String> =
        unreached.iter().filter(|u| u.why == UnreachedWhy::Nobody).map(|u| u.token.clone()).collect();
    if tokens.is_empty() {
        return Vec::new();
    }
    let named: Vec<(String, crate::invite::Person)> = crate::invite::nameable(app)
        .into_iter()
        .filter_map(|p| {
            let t = tokens.iter().find(|t| crate::labs::thread_names(t, &p.id, &p.name))?;
            Some((t.clone(), p))
        })
        .collect();
    if named.is_empty() {
        return Vec::new();
    }
    let ids = named.iter().map(|(_, p)| p.id.clone()).collect();
    let Some(reads) = app.mux.api(|r| Api::CanRead(target, ids, r)).await.flatten() else { return Vec::new() };
    let out: Vec<(String, crate::invite::Person)> =
        named.into_iter().zip(reads).filter(|(_, reads)| !reads).map(|(n, _)| n).collect();
    unreached.retain(|u| !out.iter().any(|(t, _)| *t == u.token));
    // One taken for another (a login by an account's name) says whom.
    out.into_iter().map(|(token, p)| Invitable { token, who: p.id, name: p.name, merged: p.merged }).collect()
}

/// Hand a thread message to the pane's agent, as a follow-up from its
/// author (M61). `Ok(true)`: it went straight in; `Ok(false)`: queued.
async fn tell_agent(app: &App, pane: PaneId, msg: &arugula_proto::ThreadMsg) -> Result<bool, String> {
    let mut text = format!("{} wrote in this pane's thread: {}", msg.name, msg.text);
    if let Some(q) = &msg.quote {
        text.push_str(&format!("\n\nQuoting %{}:\n{}", q.pane, q.text));
    }
    text.push_str("\n\n(Answer in the thread with Arugula's post_thread tool.)");
    let by = Driver { who: msg.who.clone(), name: msg.name.clone() };
    if let Some(b) = app.mux.api(|r| Api::Block(pane, r)).await.flatten() {
        let name = (by.who != "owner").then_some(by.name.as_str());
        return b.call_by("send", serde_json::json!({ "text": text }), name).await.map(|_| true);
    }
    app.mux.api(|r| Api::FollowUp(pane, text, by, r)).await.unwrap_or_else(|| Err("daemon is shutting down".into()))
}

impl McpOp for ThreadGet {
    const SURFACE: Surface = Surface::Tool {
        name: "read_thread",
        title: "Read a thread",
        description: "The people's conversation about a pane or a session: who said what and when, oldest first, with quoted terminal output. When someone writes @agent in a pane's thread, it reaches that pane's agent as a follow-up; answer with post_thread.",
        destructive: false,
        idempotent: true,
        open_world: false,
    };
    type Args = ThreadArgs;

    async fn args(call: &Call<'_>, a: &ThreadArgs) -> Result<(ThreadTarget, Empty), String> {
        Ok((call.thread_of(a.pane.as_ref(), a.session).await?, Empty {}))
    }

    fn answer(_: &Call<'_>, a: &ThreadArgs, &target: &ThreadTarget, res: ThreadMessages) -> Out {
        let after = a.after.unwrap_or(0);
        let msgs: Vec<_> = res.messages.into_iter().filter(|m| m.id > after).collect();
        let last = msgs.last().map(|m| m.id).unwrap_or(after);
        let text: Vec<String> = msgs
            .iter()
            .map(|m| {
                let mut t = format!("#{} {}: {}", m.id, m.name, m.text);
                if let Some(q) = &m.quote {
                    t.push_str(&format!("\n  > (%{}) {}", q.pane, q.text.replace('\n', "\n  > ")));
                }
                t
            })
            .collect();
        done(
            format!(
                "{} message(s) in {}{}",
                msgs.len(),
                target.key(),
                if text.is_empty() { String::new() } else { format!(":\n{}", text.join("\n")) }
            ),
            results::ThreadRead { thread: target, messages: msgs, last },
        )
    }
}

impl McpOp for ThreadPost {
    const SURFACE: Surface = Surface::Tool {
        name: "post_thread",
        title: "Post in a thread",
        description: "Post a message in a pane's or a session's thread, where the people working on it talk; it shows as from an agent. Use it to answer an @agent message or to tell the people something they should see. The result's `unreached` lists any @name that reached no one, and why. Someone who can't see the thread isn't told: call invite_person to ask the user to bring them in.",
        destructive: false,
        idempotent: false,
        open_world: false,
    };
    type Args = PostThreadArgs;

    async fn args(call: &Call<'_>, a: &PostThreadArgs) -> Result<(ThreadTarget, ThreadPostRequest), String> {
        let target = call.thread_of(a.pane.as_ref(), a.session).await?;
        Ok((target, ThreadPostRequest { text: a.text.clone(), quote: None }))
    }

    fn answer(_: &Call<'_>, _: &PostThreadArgs, &target: &ThreadTarget, res: ThreadPosted) -> Out {
        done(
            format!("posted #{} in {}", res.message.id, target.key()),
            results::ThreadPosted { thread: target, message: res.message, unreached: res.unreached },
        )
    }
}
