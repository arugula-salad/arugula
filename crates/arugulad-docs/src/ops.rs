//! `docs.list`, `doc.open`, `doc.delete` (#1493), `doc.preview` (#588), `doc.read`
//! and `doc.asset` (#608): the Doc library (see
//! [`crate::docs::library`]) listed, a Doc opened in a block, and a Doc
//! deleted, and `doc.move` and `docs.team` (Arugula #863): a Doc given to a
//! team, and the team it would go to. The owner's: `serve` checks that
//! before the handler runs. MCP's
//! `doc` kind `list` is `docs.list`; its kind `open` is `doc.open` with the
//! caller recorded as the block's starter (`mcp/tools.rs`), so the agent can
//! `fill` what it opened. An agent can't delete.
//!
//! **One block per Doc.** The blocks that show a Doc are found by a scan: a
//! Doc block's config names its Doc (`open_blocks`). `doc.open` holds one
//! lock while it looks and while it opens, so two opens of one Doc make one
//! block.
//!
//! **A moved Doc's pointer** isn't a Doc: `docs.list` leaves it out, and a
//! read of it says where it went, but `doc.open` of its old id opens the
//! team Doc (or finds the block that shows it).

use std::{collections::HashMap, sync::Arc};

use arugula_core::PaneId;
use arugula_proto::{
    BlockType,
    api::{
        DocAsset, DocContent, DocMoveRequest, DocMoved, DocOpenRequest, DocOpened, DocPreview, DocRow, DocShared,
        DocTeam, DocTeams, Empty, OpenRequest,
    },
    op::ops::{DocAssetGet, DocDelete, DocMove, DocOpen, DocPreviewGet, DocRead, DocsList, DocsTeam},
};
use axum::http::StatusCode;

use crate::{
    Docs,
    docs::{
        library,
        link::{self, Remote},
        moving::{self, Target},
        team::Cloud,
    },
    mcp::{
        Caller, Scope,
        issues::Chosen,
        ops::{McpOp, Surface},
        tools::{Call, ListArgs, Out, done},
    },
    mux::Api,
    ops::{Cx, Handle, OpError, Via},
    server::App,
};

/// Said to an agent and to a team's other owner who asks to delete a Doc.
pub const DELETE_OWNER_ONLY: &str = "only the owner can delete a Doc";
/// Said to a team's other owner, a guest, or a guest's agent who asks to
/// move one.
pub const MOVE_OWNER_ONLY: &str = "only the owner and their own agents can move a Doc to a project";
/// How much of a Doc its list's preview shows.
const PREVIEW_PARAGRAPHS: usize = 4;

/// The blocks that show a Doc, by the Doc's id: the last, if a Doc has
/// more than one (an agent that can't drive the first is given another,
/// Arugula #1446).
pub(crate) async fn open_blocks(app: &App) -> HashMap<String, PaneId> {
    open_blocks_all(app).await.into_iter().filter_map(|(doc, b)| Some((doc, *b.last()?))).collect()
}

/// Every block that shows a Doc, by the Doc's id, in pane order.
pub(crate) async fn open_blocks_all(app: &App) -> HashMap<String, Vec<PaneId>> {
    let mut out: HashMap<String, Vec<PaneId>> = HashMap::new();
    for p in app.mux.api(Api::Panes).await.unwrap_or_default() {
        if p.info.kind != BlockType::Doc {
            continue;
        }
        if let Some(b) = app.mux.api(|r| Api::Block(p.info.id, r)).await.flatten()
            && let Some(doc) = b.config()["doc"].as_str()
        {
            out.entry(doc.to_owned()).or_default().push(p.info.id);
        }
    }
    out
}

/// `search`'s lines of the Docs in the library (a Doc no block shows too).
pub(crate) async fn search_library(
    app: &App,
    re: regex::Regex,
    since_ms: Option<u64>,
    limit: usize,
) -> Vec<arugula_proto::api::SearchHit> {
    let state = app.control.state_dir().to_owned();
    if limit == 0 {
        return vec![];
    }
    let open = open_blocks(app).await;
    crate::task::spawn_blocking(move || library::search(&state, &open, &re, since_ms, limit)).await.unwrap_or_default()
}

fn not_found(id: &str) -> OpError {
    OpError::Status(StatusCode::NOT_FOUND, format!("no Doc {id} in the library (docs list has them)"))
}

fn checked(app: &App, id: &str) -> Result<library::Meta, OpError> {
    let dir = library::dir(app.control.state_dir(), id).map_err(|e| OpError::Status(StatusCode::BAD_REQUEST, e))?;
    library::read_meta(&dir).ok_or_else(|| not_found(id))
}

/// [`checked`], and a Doc, not a moved one's pointer: that says where it went.
fn readable(app: &App, id: &str) -> Result<library::Meta, OpError> {
    let meta = checked(app, id)?;
    match &meta.moved {
        Some(to) => Err(OpError::Status(StatusCode::GONE, moved_words(to))),
        None => Ok(meta),
    }
}

/// Where a moved Doc went, for a call on its old id.
pub(crate) fn moved_words(to: &library::MovedTo) -> String {
    let team = if to.name.is_empty() { &to.project } else { &to.name };
    match &to.link {
        Some(link) => format!("this Doc moved to {team} ({}): {link}", to.id),
        None => format!("this Doc moved to {team} ({})", to.id),
    }
}

impl Handle<Docs> for DocsList {
    async fn handle(cx: &Cx<'_>, _: (), _: Empty) -> Result<Vec<DocRow>, OpError> {
        let open = open_blocks(cx.app).await;
        let state = cx.app.control.state_dir().to_owned();
        let (metas, links) = crate::task::spawn_blocking(move || (library::list(&state), link::by_doc(&state)))
            .await
            .map_err(|e| OpError::Failed(e.to_string()))?;
        Ok(metas
            .into_iter()
            .map(|m| {
                let shape = m.shape.unwrap_or_default();
                DocRow {
                    open: open.get(&m.id).copied(),
                    shared: links.get(&m.id).map(|(link, e)| DocShared {
                        made: e.made,
                        updated: e.updated,
                        link: link.clone(),
                    }),
                    id: m.id,
                    title: m.title,
                    made: m.made,
                    updated: m.updated,
                    by: m.by,
                    summary: shape.summary,
                    sections: shape.sections,
                    layout: m.layout,
                }
            })
            .collect())
    }
}

impl McpOp<Docs> for DocsList {
    const SURFACE: Surface = Surface::Kind {
        tool: "doc",
        kind: "list",
        description: "The kept Docs, newest first: id, title, made, updated, author, open (its block), shared.",
    };
    type Args = ListArgs;
    type Answer = serde_json::Value;

    async fn args(_: &Call<'_>, _: &ListArgs) -> Result<((), Empty), String> {
        Ok(((), Empty {}))
    }

    fn answer(_: &Call<'_>, _: &ListArgs, _: &(), docs: Vec<DocRow>) -> Out {
        let n = docs.len();
        // An agent has `doc open` for a Doc's outline: the list stays short.
        let docs: Vec<_> = docs.into_iter().map(|d| DocRow { sections: vec![], ..d }).collect();
        done(format!("{n} Docs; doc open shows one in a block"), serde_json::json!({ "docs": docs }))
    }
}

/// Open a Doc in a block, or find the block that has it: the block, and
/// whether it was made now. `who` is the person who asks, over HTTP.
/// `fit` is the blocks the asker may use, best first: a Doc whose blocks
/// aren't among them gets another (Arugula #1446); `None` takes any.
pub(crate) async fn open_doc(
    app: &App,
    who: Option<crate::acl::Principal>,
    fit: Option<&[PaneId]>,
    req: DocOpenRequest,
) -> Result<(PaneId, bool), OpError> {
    // One at a time, so two opens of one Doc can't both find none.
    static ONE: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());
    let _one = ONE.lock().await;
    let id =
        match (req.id, req.link) {
            (Some(id), None) => id,
            // A link names its Doc; one from before the library may not.
            (None, Some(link)) => link::get(app.control.state_dir(), &link)
                .map_err(OpError::Failed)?
                .and_then(|e| e.doc)
                .ok_or_else(|| OpError::Status(StatusCode::NOT_FOUND, "this link's Doc isn't in the library".into()))?,
            _ => return Err(OpError::Status(StatusCode::BAD_REQUEST, "give exactly one of id and link".into())),
        };
    let meta = checked(app, &id)?;
    // A moved Doc's old id is the team Doc's (Arugula #863).
    let (doc, config) = match &meta.moved {
        Some(to) => (
            to.id.clone(),
            serde_json::json!({ "doc": to.id, "project": to.project, "project_name": to.name, "title": meta.title }),
        ),
        None => (meta.id.clone(), serde_json::json!({ "doc": meta.id, "title": meta.title })),
    };
    if let Some(block) = reusable(open_blocks_all(app).await.get(&doc), fit) {
        return Ok((block, false));
    }
    let open = OpenRequest {
        kind: BlockType::Doc,
        config,
        session: req.session,
        split: req.split,
        from_pane: req.split,
        vm: false,
        image: None,
        host: None,
        local: true,
        project: None,
        issue: None,
    };
    match app.mux.api(|r| Api::Open(open, who, r)).await {
        Some(Ok(b)) => Ok((b, true)),
        Some(Err(e)) => Err(OpError::Status(StatusCode::BAD_REQUEST, e)),
        None => Err(OpError::Status(StatusCode::SERVICE_UNAVAILABLE, "daemon is shutting down".into())),
    }
}

/// The block of `blocks` the asker may use: the first of `fit` that
/// shows the Doc, or any if `fit` is `None`.
pub(crate) fn reusable(blocks: Option<&Vec<PaneId>>, fit: Option<&[PaneId]>) -> Option<PaneId> {
    let blocks = blocks?;
    match fit {
        None => blocks.last().copied(),
        Some(fit) => fit.iter().copied().find(|b| blocks.contains(b)),
    }
}

impl Handle<Docs> for DocOpen {
    async fn handle(cx: &Cx<'_>, _: (), req: DocOpenRequest) -> Result<DocOpened, OpError> {
        let who = match &cx.via {
            Via::Http { who, .. } => Some(who.clone()),
            Via::Mcp(_) => None,
        };
        Ok(DocOpened { pane: open_doc(cx.app, who, None, req).await?.0 })
    }
}

impl Handle<Docs> for DocPreviewGet {
    async fn handle(cx: &Cx<'_>, id: String, _: Empty) -> Result<DocPreview, OpError> {
        let meta = readable(cx.app, &id)?;
        let dir = library::dir(cx.app.control.state_dir(), &meta.id).map_err(OpError::Failed)?;
        let opening = crate::task::spawn_blocking(move || crate::docs::shape::opening(&dir, PREVIEW_PARAGRAPHS))
            .await
            .map_err(|e| OpError::Failed(e.to_string()))?;
        Ok(DocPreview { opening })
    }
}

/// `doc.read` and `doc.asset` (#608) read the Doc from its files with the
/// block's own loader and ask `open_blocks` nothing: a read never opens a
/// block, so `docs.list` still has the Doc closed.
impl Handle<Docs> for DocRead {
    async fn handle(cx: &Cx<'_>, id: String, _: Empty) -> Result<DocContent, OpError> {
        let meta = readable(cx.app, &id)?;
        let state = crate::docs::read(cx.app.control.state_dir().to_owned(), meta.id).await.map_err(OpError::Failed)?;
        serde_json::from_value(state).map_err(|e| OpError::Failed(e.to_string()))
    }
}

impl Handle<Docs> for DocAssetGet {
    async fn handle(cx: &Cx<'_>, (id, name): (String, String), _: Empty) -> Result<DocAsset, OpError> {
        let meta = readable(cx.app, &id)?;
        let got = crate::docs::read_asset(cx.app.control.state_dir().to_owned(), meta.id, name)
            .await
            .map_err(|e| OpError::Status(StatusCode::NOT_FOUND, e))?;
        serde_json::from_value(got).map_err(|e| OpError::Failed(e.to_string()))
    }
}

impl Handle<Docs> for DocDelete {
    async fn handle(cx: &Cx<'_>, id: String, _: Empty) -> Result<Empty, OpError> {
        // The link's key is the machine's own account's, and a Doc that is
        // gone can't be got back: not an agent's, nor a team's other owner's.
        if cx.agent_or_guest() || cx.co_owner() {
            return Err(OpError::Forbidden(DELETE_OWNER_ONLY.into()));
        }
        let meta = checked(cx.app, &id)?;
        let state = cx.app.control.state_dir().to_owned();
        let block = open_blocks(cx.app).await.get(&meta.id).copied();
        // Unshared first. A key must never be left behind a live link, so
        // with control out of reach everything is kept.
        if let Some((link_id, _)) = link::of_doc(&state, &meta.id) {
            let refuse = |e: String| {
                OpError::Status(
                    StatusCode::SERVICE_UNAVAILABLE,
                    format!("can't unshare it at control, so the Doc is kept: {e}"),
                )
            };
            // The open block, if it holds this link, does it all and clears
            // its `shared`; otherwise only the key goes.
            let mut done = false;
            if let Some(b) = match block {
                Some(b) => cx.app.mux.api(|r| Api::Block(b, r)).await.flatten(),
                None => None,
            } {
                let got = b.call("doc_unshare", serde_json::json!({ "id": link_id })).await.map_err(refuse)?;
                done = got["other"] != true;
            }
            if !done {
                let remote: Arc<dyn Remote> = cx.app.control.clone();
                link::unshare_id(&state, &remote, &link_id).await.map_err(refuse)?;
            }
        }
        // The block first, so nothing writes into the folder as it goes.
        // Every block that shows it: a Doc may have one an agent can't drive
        // beside the one it can.
        for b in open_blocks_all(cx.app).await.remove(&meta.id).unwrap_or_default() {
            cx.app.mux.api(|r| Api::Close(b, r)).await;
        }
        let id = meta.id;
        crate::task::spawn_blocking(move || library::remove(&state, &id))
            .await
            .map_err(|e| OpError::Failed(e.to_string()))?
            .map_err(OpError::Failed)?;
        Ok(Empty {})
    }
}

/// The project a Doc moves to: the one named, else the current one, by the
/// one rule MCP's tools choose by (`Call::chosen_project`); for an agent,
/// with its own pane's session and clone. A Doc goes where its project goes
/// (Arugula #1026): a team's, or the machine's owner's personal one, not
/// another account's.
async fn target(cx: &Cx<'_>, project: Option<&str>) -> Result<Target, OpError> {
    let chosen = chosen(cx, project).await?;
    let owner = cx.app.control.account();
    may_take(chosen, owner.as_deref()).map_err(|e| OpError::Status(StatusCode::BAD_REQUEST, e))
}

async fn chosen(cx: &Cx<'_>, project: Option<&str>) -> Result<Chosen, OpError> {
    let chosen = match &cx.via {
        Via::Mcp(call) => {
            call.owners_own("moving a Doc to a project").await.map_err(OpError::Forbidden)?;
            call.chosen_project(project).await
        }
        Via::Http { .. } => {
            let caller = Caller { scope: Scope::Full, token: None, pane: None };
            Call::detached(cx.app, caller, "the owner".into()).chosen_project(project).await
        }
    };
    chosen.map_err(|e| OpError::Status(StatusCode::BAD_GATEWAY, e))
}

/// [`target`]'s rule: a team's project, or `owner`'s own personal one.
fn may_take(chosen: Chosen, owner: Option<&str>) -> Result<Target, String> {
    if chosen.team || (owner.is_some() && chosen.person.as_deref() == owner) {
        return Ok(Target { project: chosen.id, name: chosen.name });
    }
    let name = if chosen.name.is_empty() { chosen.id } else { chosen.name };
    Err(match chosen.person {
        None => format!("Arugula cloud didn't say whose personal project {name} is, so the Doc stays here"),
        Some(_) => format!("{name} is someone else's personal project: a Doc moves to a team's project or your own"),
    })
}

impl Handle<Docs> for DocMove {
    async fn handle(cx: &Cx<'_>, id: String, req: DocMoveRequest) -> Result<DocMoved, OpError> {
        // The Doc is the machine's own account's to give away.
        if cx.co_owner() || matches!(cx.via, Via::Http { owner: false, .. }) {
            return Err(OpError::Forbidden(MOVE_OWNER_ONLY.into()));
        }
        let meta = checked(cx.app, &id)?;
        let to = match &meta.moved {
            // Moved already: where it went.
            Some(to) => Target { project: to.project.clone(), name: to.name.clone() },
            None => target(cx, req.project.as_deref()).await?,
        };
        let machine = cx.app.control.enrolled().map(|e| e.saved.cert.device.clone()).ok_or_else(|| {
            OpError::Status(
                StatusCode::SERVICE_UNAVAILABLE,
                "this machine hasn't joined Arugula cloud: a project's Docs are there".into(),
            )
        })?;
        let cloud: Arc<dyn Cloud> = cx.app.control.clone();
        let state = cx.app.control.state_dir().to_owned();
        let at = moving::move_doc(&state, cloud, &machine, &meta.id, &to)
            .await
            .map_err(|e| OpError::Status(StatusCode::BAD_GATEWAY, format!("can't move it to {}: {e}", to.name)))?;
        Ok(DocMoved { project: at.project, name: at.name, id: at.id, link: at.link })
    }
}

/// `doc move`'s arguments.
#[derive(serde::Deserialize, schemars::JsonSchema)]
pub struct DocMoveArgs {
    /// The Doc's id (doc list has them). Move one only when the user asks: its block, doc fill on it and doc open of this id go on with the project's Doc.
    pub doc: String,
    /// The project (a prj_ id): a team's, or the user's personal one. Default: the current project.
    #[serde(default)]
    pub project: Option<String>,
}

impl McpOp<Docs> for DocMove {
    const SURFACE: Surface = Surface::Kind {
        tool: "doc",
        kind: "move",
        description: "Give a Doc to a project (a team's, or the user's own): it leaves this list.",
    };
    type Args = DocMoveArgs;
    type Answer = serde_json::Value;

    async fn args(_: &Call<'_>, a: &DocMoveArgs) -> Result<(String, DocMoveRequest), String> {
        Ok((a.doc.clone(), DocMoveRequest { project: a.project.clone() }))
    }

    fn answer(_: &Call<'_>, a: &DocMoveArgs, _: &String, m: DocMoved) -> Out {
        done(
            format!(
                "Moved Doc {} to {}: it is the project's Doc {} now; doc open {} shows it",
                a.doc, m.name, m.id, m.id
            ),
            serde_json::json!({ "doc": a.doc, "project": m.project, "project_name": m.name, "id": m.id, "link": m.link }),
        )
    }
}

impl Handle<Docs> for DocsTeam {
    /// What the Docs page offers Move to: the current project when it's a
    /// team's (a personal one is named with `project`). Control out of reach is no team, with why.
    async fn handle(cx: &Cx<'_>, _: (), _: Empty) -> Result<DocTeams, OpError> {
        if cx.co_owner() {
            return Ok(DocTeams::default());
        }
        Ok(match chosen(cx, None).await {
            Ok(c) if c.team => DocTeams { team: Some(DocTeam { project: c.id, name: c.name }), why: None },
            Ok(c) => {
                let name = if c.name.is_empty() { c.id } else { c.name };
                DocTeams { team: None, why: Some(format!("{name} is a personal project, not a team's")) }
            }
            Err(e) => DocTeams { team: None, why: Some(e.sentence()) },
        })
    }
}

#[cfg(test)]
mod tests {
    use arugulad_base::mcp::issues::Rule;

    use super::*;

    fn chosen(team: bool, person: Option<&str>) -> Chosen {
        Chosen { id: "prj_1".into(), name: "Rhythm".into(), team, person: person.map(Into::into), rule: Rule::Asked }
    }

    /// Arugula #1026: a Doc goes where its project goes, a team's or the
    /// machine's owner's personal one, and never another account's.
    #[test]
    fn a_doc_moves_to_a_team_or_its_owners_personal_project() {
        let to = Target { project: "prj_1".into(), name: "Rhythm".into() };
        assert_eq!(may_take(chosen(true, None), Some("acct_a")), Ok(to.clone()));
        assert_eq!(may_take(chosen(false, Some("acct_a")), Some("acct_a")), Ok(to));
        assert_eq!(
            may_take(chosen(false, Some("acct_b")), Some("acct_a")),
            Err("Rhythm is someone else's personal project: a Doc moves to a team's project or your own".into())
        );
        assert_eq!(
            may_take(chosen(false, None), Some("acct_a")),
            Err("Arugula cloud didn't say whose personal project Rhythm is, so the Doc stays here".into())
        );
        assert!(
            may_take(chosen(false, Some("acct_a")), None).is_err(),
            "a machine not in the cloud has no owner to match"
        );
    }
}
