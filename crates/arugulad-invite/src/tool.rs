//! MCP's `invite_person` and `read_invite` (#234): an agent asks the user to
//! bring someone into its session, as a draft on the user's card, and reads
//! what became of it. Registered into the base's tool list after
//! `forge_draft` (`arugulad_base::mcp::registry`), where they were.

use std::sync::Arc;

use arugula_proto::{BlockType, PaneId, StartedBy, api::OpenRequest};
use arugulad_base::mcp::{
    registry, results,
    tools::{Args, Call, Def, Human, Needs, Out, PaneArg, done, line, output_schema, parse},
};
use futures_util::future::BoxFuture;
use rmcp::handler::server::common::schema_for_type;
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::{Value, json};

use crate::mux::Api;

/// Invites one caller may have waiting for the user (#234).
const INVITES_WAITING: u64 = 5;

#[derive(Deserialize, JsonSchema)]
pub struct InvitePersonArgs {
    /// Whom: a name this machine knows (a teammate, someone the session is
    /// shared with), tailnet:<login> or account:<id>.
    pub who: String,
    /// viewer (the default) or editor.
    #[serde(default)]
    pub role: Option<String>,
    /// Where it opens, and so the session (default: your own pane; a full
    /// client outside a pane must say).
    #[serde(default)]
    pub pane: Option<PaneArg>,
    /// Why you want them there, at most 500 characters: the user reads it on
    /// the card, the person in their notification.
    pub note: String,
    /// Their window follows yours: to watch. Opening the invite puts them
    /// on the tab you are on (default: no).
    #[serde(default)]
    pub follow: bool,
}

#[derive(Deserialize, JsonSchema)]
pub struct ReadInviteArgs {
    /// The draft id invite_person returned.
    pub draft: String,
    /// The pane you invited from, if you gave one then.
    #[serde(default)]
    pub pane: Option<PaneArg>,
}

fn invite_person_def() -> Def {
    Def {
        name: "invite_person",
        title: "Ask to invite a person",
        description: "Ask the user to bring someone into the session you work in (your pane's): a teammate, someone it's shared with, or tailnet:<login>, as a viewer (default) or an editor, with a note saying why. Nothing is shared in an agent's name: it waits as a card beside you that only the session's owner sends (after editing the role or note, if they like) or declines. Only the owner's own agents may ask (not one a guest started). Returns its draft id at once with status waiting; read_invite shows what became of it.",
        args: Args::One(schema_for_type::<InvitePersonArgs>),
        read_only: false,
        destructive: false,
        idempotent: false,
        open_world: true,
        flag: None,
        output: Some(output_schema::<results::InviteDrafted>),
        human: line(
            "ask you to bring someone into this session, saying why",
            "Ask my teammate to watch this session with me, and say why.",
            Needs::Nothing,
        ),
    }
}

fn read_invite_def() -> Def {
    Def {
        name: "read_invite",
        title: "Read an invite draft",
        description: "What became of an invite_person draft: waiting, sent (with its grant and delivery: sent, pending or unreachable, and why), declined (and the owner's reason), dropped (nobody answered in a day) or failed (and why); who settled it and when.",
        args: Args::One(schema_for_type::<ReadInviteArgs>),
        read_only: true,
        destructive: false,
        idempotent: true,
        open_world: false,
        flag: None,
        output: None,
        human: Human::In("invite_person"),
    }
}

fn run_invite_person<'a>(call: &'a Call<'a>, _kind: Option<&'static str>, args: Value) -> BoxFuture<'a, Out> {
    Box::pin(async move {
        match parse(args) {
            Ok(a) => invite_person(call, a).await,
            Err(e) => Err(e),
        }
    })
}

fn run_read_invite<'a>(call: &'a Call<'a>, _kind: Option<&'static str>, args: Value) -> BoxFuture<'a, Out> {
    Box::pin(async move {
        match parse(args) {
            Ok(a) => read_invite(call, a).await,
            Err(e) => Err(e),
        }
    })
}

/// Lists `invite_person` after `forge_draft`, and `read_invite` after it.
pub fn register() {
    registry::tool(registry::Tool { after: Some("forge_draft"), def: invite_person_def, run: run_invite_person });
    registry::tool(registry::Tool { after: Some("invite_person"), def: read_invite_def, run: run_read_invite });
}

/// An invite's panes (#234): the caller's own, and where it opens. An
/// agent's own block, or for a full caller the pane `arugula mcp`
/// runs in, else the one it names.
fn invite_panes(call: &Call<'_>, pane: Option<&PaneArg>) -> Result<(PaneId, PaneId), String> {
    let pane = pane.map(PaneArg::id).transpose()?;
    let mine = call.own_pane();
    match (mine.or(pane), pane.or(mine)) {
        (Some(from), Some(to)) => Ok((from, to)),
        _ => Err("pane: which pane's session to invite them into (arugula mcp in a pane says its own)".into()),
    }
}

/// Whose invite drafts these are: an agent block's, or a full client's
/// in a pane.
fn drafter(call: &Call<'_>, from: PaneId) -> String {
    match call.me() {
        Some(me) => format!("%{me}"),
        None => format!("{}@%{from}", call.by()),
    }
}

/// The invite blocks (#234), with their sessions.
async fn invite_blocks(call: &Call<'_>) -> Vec<(PaneId, arugula_core::SessionId, Arc<dyn crate::block::Block>)> {
    let mut out = vec![];
    for p in call.panes().await.into_iter().filter(|p| p.info.kind == BlockType::Invite) {
        if let Some(b) = call.app.mux.api(|r| Api::Block(p.info.id, r)).await.flatten() {
            out.push((p.info.id, p.session, b));
        }
    }
    out
}

async fn invite_person(call: &Call<'_>, a: InvitePersonArgs) -> Out {
    let role = match a.role.as_deref().map(str::trim) {
        None | Some("" | "viewer") => arugula_core::Role::Viewer,
        Some("editor") => arugula_core::Role::Editor,
        Some(r) => return Err(format!("role {r}: viewer or editor (an invite never makes an owner)")),
    };
    let note = a.note.trim();
    if note.is_empty() {
        return Err("note: say why you want them there (the user reads it on the card)".into());
    }
    if note.chars().count() > crate::card::NOTE_MAX {
        return Err(format!("note: at most {} characters", crate::card::NOTE_MAX));
    }
    let (from, pane) = invite_panes(call, a.pane.as_ref())?;
    let at = call.readable(pane).await?;
    // The owner's agents ask; one a guest started (or an agent of
    // theirs did) learns nothing of who this machine knows.
    let started = started_text(call).await?;
    // Someone this machine knows, or no card at all.
    let person = crate::resolve(call.app, &a.who, None).map_err(|(_, why)| why)?;
    if crate::is_me(call.app, &person.id) {
        return Err(format!("{} owns this machine", person.name));
    }
    let drafter = drafter(call, from);
    let blocks = invite_blocks(call).await;
    let mine: Vec<_> = blocks.iter().filter(|(_, _, b)| b.config()["drafter"] == drafter.as_str()).collect();
    let waiting: u64 = mine.iter().map(|(_, _, b)| b.state()["waiting"].as_u64().unwrap_or(0)).sum();
    if waiting >= INVITES_WAITING {
        return Err(format!("{waiting} of your invites wait for the user already: read_invite until they answer one"));
    }
    let block = match mine.iter().find(|(_, s, _)| *s == at.session) {
        Some((id, _, b)) => (*id, b.clone()),
        None => {
            // A card of its own beside the pane, on this host; not the
            // agent's to drive or close.
            let req = OpenRequest {
                kind: BlockType::Invite,
                config: json!({ "drafter": drafter }),
                session: None,
                split: Some(pane),
                from_pane: Some(pane),
                vm: false,
                image: None,
                host: None,
                local: true,
            };
            let id = match call.app.mux.api(|r| Api::Open(req, None, r)).await {
                Some(r) => r?,
                None => return Err("the daemon is shutting down".into()),
            };
            let by = StartedBy { by: call.by(), block: None };
            call.app.mux.send(crate::mux::Cmd::Api(Api::StartedBy(id, by)));
            let b = call.app.mux.api(|r| Api::Block(id, r)).await.flatten().ok_or("the invite block closed")?;
            (id, b)
        }
    };
    let args = json!({ "who": a.who.trim(), "person": person.id, "name": person.name, "role": role,
        "note": note, "session": at.session, "session_name": at.session_name, "pane": pane, "from": from,
        "started": started, "follow": a.follow });
    let out = block.1.call_by("draft", args, Some(&call.by())).await?;
    let draft = out["draft"].as_str().unwrap_or("?").to_owned();
    done(
        format!(
            "Asked the user to invite {} ({}) into {} at %{pane}: {draft} waits on their card in %{} (read_invite shows what became of it)",
            person.name,
            role.as_str(),
            at.session_name,
            block.0
        ),
        results::InviteDrafted {
            draft,
            status: "waiting",
            block: block.0,
            who: person.id,
            name: person.name,
            role,
            session: at.session,
            pane,
        },
    )
}

/// Who started the agent asking (#234), as its card says it: the
/// owner, or the agent that did. An agent a guest stands behind may
/// not ask at all.
async fn started_text(call: &Call<'_>) -> Result<String, String> {
    // Not in a pane: the person's own client, whose card needs no
    // more than the pane it names (UI audit #178).
    let Some(me) = call.me() else { return Ok(String::new()) };
    if call.app.mux.api(|r| Api::GuestBehind(me, r)).await.flatten().is_some() {
        return Err("only the owner's own agents may ask to invite someone".into());
    }
    Ok(match call.readable(me).await?.info.started_by {
        Some(StartedBy { by, block: Some(b) }) => format!("started by {by} in %{b}"),
        Some(StartedBy { by, block: None }) => format!("started by {by}"),
        None => "you started it".into(),
    })
}

async fn read_invite(call: &Call<'_>, a: ReadInviteArgs) -> Out {
    let drafter = match call.me() {
        Some(me) => format!("%{me}"),
        None => drafter(call, invite_panes(call, a.pane.as_ref())?.0),
    };
    let id = a.draft.trim();
    for (block, _, b) in invite_blocks(call).await {
        if b.config()["drafter"] != drafter.as_str() {
            continue;
        }
        let st = b.state();
        let Some(d) = st["drafts"].as_array().and_then(|ds| ds.iter().find(|d| d["id"] == id)).cloned() else {
            continue;
        };
        let status = d["status"].as_str().unwrap_or("waiting").to_owned();
        let name = d["name"].as_str().unwrap_or("").to_owned();
        let by = d["settled_by"].as_str().unwrap_or("the user");
        let summary = match status.as_str() {
            "waiting" => format!("{id} waits on the user's card in %{block}"),
            "sent" => format!(
                "{by} invited {name}; their notification: {}{}",
                d["delivery"].as_str().unwrap_or("?"),
                d["delivery_reason"].as_str().map(|r| format!(" ({r})")).unwrap_or_default()
            ),
            "declined" => match d["reason"].as_str() {
                Some(r) => format!("{by} declined inviting {name}: {r}"),
                None => format!("{by} declined inviting {name}"),
            },
            "dropped" => format!("Nobody answered {id} in time: nothing was shared"),
            _ => format!("Inviting {name} failed: {}", d["error"].as_str().unwrap_or("?")),
        };
        return done(summary, results::InviteRead { card: d, draft: id.to_owned(), block });
    }
    // Its block was closed: what it said then.
    let root = call.app.mux.store.root().to_owned();
    let kept = crate::task::spawn_blocking(move || crate::card::closed(&root)).await.unwrap_or_default();
    if let Some(c) = kept.into_iter().rev().find(|c| c.drafter == drafter && c.draft.id == id) {
        let status = serde_json::to_value(&c.draft).unwrap_or_default()["status"].as_str().unwrap_or("").to_owned();
        let block = c.block;
        return done(
            format!("{id} is {status}; its invite block %{block} was closed"),
            results::InviteRead { card: c.draft, draft: id.to_owned(), block },
        );
    }
    Err(format!("no invite {id} of yours (invite_person returns one)"))
}
