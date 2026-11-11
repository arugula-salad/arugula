//! A team's Doc as a block (Arugula #862, aru-139): in a team's context a Doc
//! lives in the team's project on Arugula's cloud, not in the library, and this
//! block shows and fills it through control, as its owner's agent.
//!
//! Config `{doc, project, project_name?, title}`: `doc` is the Doc's `pdc_` id
//! and `project` its project's, chosen once by `doc new` and kept, so the
//! block survives a restart. **Nothing here touches `<state>/docs/`**: that
//! stays the library's. The one thing kept is the last Doc read, in the
//! block's own folder, so a restart with control out of reach still shows it.
//!
//! **The state** is the library Doc's (`title`, `entries`, `assets`, `rev`,
//! `layout`, `made`, `updated`, `by`), read from the platform's `DocView`,
//! plus `team: {project, name, link, offline}` and `platform: true`, which
//! the Doc's bar reads as "has a page the team opens".
//!
//! **Reading:** one GET when the block opens and when a client draws it, then
//! every [`POLL`] while one does. A Doc's thread announces a change to the
//! people on the page, but the daemon holds no subscription to a thread, so
//! it reads again instead; each write of its own is answered with the whole
//! Doc, which it takes as it is, so a run of fills reads nothing.
//!
//! **Writing** goes through the PUT routes, one at a time, with the project
//! and id the block has: never a lookup per fill. A refusal (no `docs.write`,
//! a viewer, a copy) is control's own sentence and the Doc is never written
//! to the machine instead. With control out of reach the block shows what it
//! last read, says "Offline: read-only", and a write fails with that.

use std::{
    path::PathBuf,
    sync::{
        Arc, Mutex, Weak,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    time::Duration,
};

use arugula_control_wire::project_docs::{
    AssetData, DocView, DocWrite, NewDoc, NewSection, agent_project_doc, agent_project_doc_asset, agent_project_docs,
};
use arugula_proto::{BlockType, fs::READ_MAX};
use futures_util::future::BoxFuture;
use serde_json::{Value, json};
use std::sync::OnceLock;

use super::{ASSET_MAX, ASSETS_MAX, Changed, Entry, HEADING_MAX, SECTION_MAX, assets, embeds, hash, markdown};
use crate::{
    block::{Block, BlockCtx, Summary, no_method},
    store::{private_dir, write_atomic},
};

/// What a team Doc says while control is out of reach, and what a write then
/// begins with.
pub const OFFLINE: &str = "Offline: read-only";
/// How often a block a client draws reads its Doc again.
const POLL: Duration = Duration::from_secs(30);
/// Two reads closer than this are one: a client drawing it again at once
/// reads nothing.
const SETTLED: Duration = Duration::from_secs(3);
/// The last Doc read, in the block's own folder.
const CACHE: &str = "team-doc.json";

// ------------------------------------------------------------------ cloud

/// Why a call to control didn't give an answer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Refused {
    /// Control couldn't be reached, or this machine isn't in it: what the
    /// block shows offline. The words say why.
    Offline(String),
    /// What control said (its own sentence, as it is).
    Said(String),
}

impl Refused {
    /// What an agent is told.
    pub fn sentence(&self) -> String {
        match self {
            Refused::Offline(why) => format!("{OFFLINE}: {why}"),
            Refused::Said(said) => said.clone(),
        }
    }
}

/// A call's body.
pub enum Body {
    None,
    Json(Value),
    Bytes(Vec<u8>),
}

/// Control's platform routes as the Doc block calls them. `Control` is the
/// real one; tests put a fake that remembers what it was sent.
pub trait Cloud: Send + Sync {
    fn call(&self, method: &'static str, path: String, body: Body) -> BoxFuture<'_, Result<Value, Refused>>;
}

impl Cloud for crate::control::Control {
    fn call(&self, method: &'static str, path: String, body: Body) -> BoxFuture<'_, Result<Value, Refused>> {
        use crate::control::CloudRefused as C;
        Box::pin(async move {
            let m = reqwest::Method::from_bytes(method.as_bytes()).map_err(|e| Refused::Said(e.to_string()))?;
            let got = match body {
                // A read doesn't wait out a maintenance window: the block
                // shows what it has.
                Body::None if method == "GET" => self.cloud_now(m, &path, None).await,
                Body::None => self.cloud(m, &path, None).await,
                Body::Json(v) => self.cloud(m, &path, Some(&v)).await,
                Body::Bytes(b) => self.cloud_bytes(m, &path, b).await,
            };
            got.map_err(|e| match e {
                C::NotJoined | C::Unreachable(_) | C::Maintenance => Refused::Offline(e.to_string()),
                C::Said { said, .. } => Refused::Said(said),
                other => Refused::Said(other.to_string()),
            })
        })
    }
}

static CLOUD: OnceLock<Arc<dyn Cloud>> = OnceLock::new();

/// The daemon's control, for team Doc blocks made after this.
pub fn init(control: Arc<crate::control::Control>) {
    let _ = CLOUD.set(control);
}

fn cloud() -> Option<Arc<dyn Cloud>> {
    CLOUD.get().cloned()
}

// ------------------------------------------------------------------- make

/// `doc new` in a team's project: the Doc made at once, its sections
/// pending (or written, where the caller had the Markdown), as the owner's
/// agent. Control's own sentence on a refusal.
pub async fn make(
    cloud: &dyn Cloud,
    project: &str,
    title: &str,
    sections: &[(String, String)],
    layout: &str,
) -> Result<DocView, String> {
    let new = NewDoc {
        title: title.to_owned(),
        sections: sections
            .iter()
            .map(|(heading, intent)| NewSection { heading: heading.clone(), intent: intent.clone(), markdown: None })
            .collect(),
        layout: Some(layout.to_owned()).filter(|l| l != "doc"),
    };
    let body = serde_json::to_value(new).map_err(|e| e.to_string())?;
    let got = cloud.call("POST", agent_project_docs(project), Body::Json(body)).await.map_err(|e| e.sentence())?;
    serde_json::from_value(got).map_err(|e| format!("control's answer isn't a Doc: {e}"))
}

/// A team Doc, read: for `doc open` of a `pdc_` id.
pub async fn read(cloud: &dyn Cloud, project: &str, id: &str) -> Result<DocView, String> {
    let got = cloud.call("GET", agent_project_doc(project, id), Body::None).await.map_err(|e| e.sentence())?;
    serde_json::from_value(got).map_err(|e| format!("control's answer isn't a Doc: {e}"))
}

// ------------------------------------------------------------------ block

/// What the block has read.
#[derive(Default)]
struct State {
    title: String,
    view: Option<DocView>,
    /// Why control couldn't be reached at the last try.
    offline: Option<String>,
    /// The Doc can't be read at all (gone, or its owner no longer reads
    /// its project): control's words.
    error: Option<String>,
    /// Counts what clients are shown: it moves with every change, a read
    /// that found nothing new aside.
    rev: u64,
}

pub struct Team {
    me: Weak<Team>,
    project: String,
    name: String,
    id: String,
    cloud: Option<Arc<dyn Cloud>>,
    changed: Changed,
    cache: Option<PathBuf>,
    state: Mutex<State>,
    /// Orders the writes: each is answered with the whole Doc, and the last
    /// answer is what the block holds.
    io: tokio::sync::Mutex<()>,
    sent: Mutex<Vec<u64>>,
    /// Some client draws it now.
    shown: AtomicBool,
    /// When it was last read (ms since it was made, so a clock isn't kept).
    read_at: Mutex<Option<std::time::Instant>>,
    /// Reads made, for a test to say a run of fills made none.
    reads: AtomicU64,
}

impl Team {
    pub fn new(
        project: String,
        name: String,
        id: String,
        title: String,
        cloud: Option<Arc<dyn Cloud>>,
        changed: Changed,
        cache: Option<PathBuf>,
    ) -> Arc<Team> {
        let mut state = State { title: title.trim().to_owned(), ..Default::default() };
        // What it last read, from before a restart.
        if let Some(view) = cache
            .as_ref()
            .and_then(|p| std::fs::read(p).ok())
            .and_then(|b| serde_json::from_slice::<DocView>(&b).ok())
            .filter(|v| v.doc.id == id && v.doc.project == project)
        {
            state.title = view.doc.title.clone();
            state.view = Some(view);
        }
        Arc::new_cyclic(|me| Team {
            me: me.clone(),
            project,
            name,
            id,
            cloud,
            changed,
            cache,
            state: Mutex::new(state),
            io: tokio::sync::Mutex::new(()),
            sent: Mutex::new(Vec::new()),
            shown: AtomicBool::new(false),
            read_at: Mutex::new(None),
            reads: AtomicU64::new(0),
        })
    }

    /// Read it now, and again while a client draws it.
    pub fn start(&self, rt: &tokio::runtime::Handle) {
        let me = self.me.clone();
        rt.spawn(async move {
            if let Some(t) = me.upgrade() {
                let _ = t.refresh().await;
            }
            loop {
                tokio::time::sleep(POLL).await;
                let Some(t) = me.upgrade() else { break };
                if t.shown.load(Ordering::Relaxed) {
                    let _ = t.refresh().await;
                }
            }
        });
    }

    fn cloud(&self) -> Result<&Arc<dyn Cloud>, String> {
        self.cloud
            .as_ref()
            .ok_or_else(|| Refused::Offline("this machine hasn't joined Arugula control".into()).sentence())
    }

    /// Take a Doc control gave: the newer one wins (a read that started
    /// before a write may arrive after it).
    fn adopt(&self, view: DocView) {
        {
            let mut st = self.state.lock().unwrap();
            if st.view.as_ref().is_some_and(|v| v.doc.rev > view.doc.rev) {
                return;
            }
            st.title = view.doc.title.clone();
            st.offline = None;
            st.error = None;
            st.view = Some(view.clone());
            st.rev += 1;
        }
        if let Some(path) = &self.cache {
            let bytes = serde_json::to_vec(&view).unwrap_or_default();
            if let Some(dir) = path.parent() {
                let _ = private_dir(dir);
            }
            let _ = write_atomic(path, &bytes);
        }
        (self.changed)();
    }

    /// Control couldn't be reached: say so, keeping what was read.
    fn offline(&self, why: &str) {
        let mut st = self.state.lock().unwrap();
        if st.offline.as_deref() == Some(why) {
            return;
        }
        st.offline = Some(why.to_owned());
        st.rev += 1;
        drop(st);
        (self.changed)();
    }

    /// Read the Doc again.
    pub async fn refresh(&self) -> Result<(), String> {
        let cloud = self.cloud()?;
        *self.read_at.lock().unwrap() = Some(std::time::Instant::now());
        self.reads.fetch_add(1, Ordering::Relaxed);
        match cloud.call("GET", agent_project_doc(&self.project, &self.id), Body::None).await {
            Ok(v) => {
                self.adopt(serde_json::from_value(v).map_err(|e| format!("control's answer isn't a Doc: {e}"))?);
                Ok(())
            }
            Err(Refused::Offline(why)) => {
                self.offline(&why);
                Err(Refused::Offline(why).sentence())
            }
            Err(Refused::Said(said)) => {
                let mut st = self.state.lock().unwrap();
                st.error = Some(said.clone());
                st.rev += 1;
                drop(st);
                (self.changed)();
                Err(said)
            }
        }
    }

    /// A write's answer: the Doc, or what to tell the agent.
    async fn put(&self, w: DocWrite) -> Result<DocView, String> {
        let cloud = self.cloud()?;
        let body = serde_json::to_value(w).map_err(|e| e.to_string())?;
        match cloud.call("PUT", agent_project_doc(&self.project, &self.id), Body::Json(body)).await {
            Ok(v) => {
                let view: DocView =
                    serde_json::from_value(v).map_err(|e| format!("control's answer isn't a Doc: {e}"))?;
                self.adopt(view.clone());
                Ok(view)
            }
            Err(Refused::Offline(why)) => {
                self.offline(&why);
                Err(Refused::Offline(why).sentence())
            }
            Err(e) => Err(e.sentence()),
        }
    }

    /// The sections' ids and headings, as the Doc has them now: `doc_outline`
    /// of a Doc `doc new` made reads it, so `doc new` answers as it does for a
    /// library Doc.
    async fn outline(&self) -> Result<Value, String> {
        self.refresh().await?;
        let st = self.state.lock().unwrap();
        let sections: Vec<Value> = st
            .view
            .iter()
            .flat_map(|v| v.sections.iter())
            .map(|s| json!({ "id": s.id, "heading": s.heading }))
            .collect();
        Ok(json!({ "sections": sections }))
    }

    /// `doc_fill`: one section's body. The same checks as a library Doc's,
    /// made here on what the block has read, so a bad fence is one line and
    /// nothing is sent; then one PUT.
    async fn fill(&self, section: &str, md: &str) -> Result<Value, String> {
        let _io = self.io.lock().await;
        self.check_fill(section, md)?;
        let w = DocWrite { section: Some(section.to_owned()), markdown: Some(md.to_owned()), ..Default::default() };
        self.put(w).await?;
        Ok(json!({ "section": section, "bytes": md.len() }))
    }

    fn check_fill(&self, section: &str, md: &str) -> Result<(), String> {
        let st = self.state.lock().unwrap();
        if let Some(e) = &st.error {
            return Err(e.clone());
        }
        let Some(view) = &st.view else {
            // Nothing read yet (control was out of reach as it opened).
            return Err(match &st.offline {
                Some(why) => Refused::Offline(why.clone()).sentence(),
                None => "this Doc hasn't been read yet: try again in a moment".into(),
            });
        };
        let Some(at) = view.sections.iter().position(|s| s.id == section) else {
            let ids: Vec<&str> = view.sections.iter().map(|s| s.id.as_str()).collect();
            return Err(format!("this Doc has no section {section:?}; it has {}", ids.join(", ")));
        };
        if md.trim().is_empty() {
            return Err(format!("section {section} is empty: give its Markdown"));
        }
        if view.doc.layout.as_deref() == Some("prototype") {
            embeds::check_prototype_only(md)
                .map_err(|e| format!("section {section}: {e}; the section is unchanged"))?;
        }
        let pictures: Vec<assets::Asset> = view
            .assets
            .iter()
            .map(|a| assets::Asset {
                name: a.name.clone(),
                mime: assets::named_mime(&a.name).unwrap_or("image/png"),
                size: a.size,
                rev: a.rev,
            })
            .collect();
        embeds::check(md, &pictures).map_err(|e| format!("section {section}: {e}; the section is unchanged"))?;
        if md.len() > SECTION_MAX {
            return Err(format!(
                "section {section} is {} bytes; the cap is {SECTION_MAX} (64 KB): split it or shorten it",
                md.len()
            ));
        }
        let others: usize = view
            .sections
            .iter()
            .enumerate()
            .filter(|(i, _)| *i != at)
            .map(|(_, s)| s.body.as_ref().map_or(0, String::len))
            .sum();
        if (others + md.len()) as u64 > READ_MAX {
            return Err(format!(
                "the Doc would be {} bytes; the cap is {READ_MAX} (1 MiB): shorten a section",
                others + md.len()
            ));
        }
        Ok(())
    }

    /// `doc_asset_put`: a picture, by name; a name already kept is replaced.
    async fn asset_put(&self, name: &str, data: &str) -> Result<Value, String> {
        assets::check_name(name)?;
        if data.len() > ASSET_MAX / 3 * 4 + 8 {
            return Err(format!("{name} is too big; the cap is {ASSET_MAX} bytes (2 MB): shrink it"));
        }
        let Some(bytes) = crate::agent::images::decode(data) else { return Err(assets::not_a_picture(name, data)) };
        if bytes.len() > ASSET_MAX {
            return Err(format!("{name} is {} bytes; the cap is {ASSET_MAX} (2 MB): shrink it", bytes.len()));
        }
        let mime = assets::kind_of(name, &bytes)?;
        let _io = self.io.lock().await;
        {
            let st = self.state.lock().unwrap();
            if let Some(view) = &st.view {
                if let Some(a) = view.assets.iter().find(|a| a.name != name && a.name.eq_ignore_ascii_case(name)) {
                    return Err(format!("the Doc has {} already: use that name to replace it", a.name));
                }
                if view.assets.len() >= ASSETS_MAX && !view.assets.iter().any(|a| a.name == name) {
                    return Err(format!("a Doc has at most {ASSETS_MAX} assets: replace one by its name"));
                }
            }
        }
        let size = bytes.len() as u64;
        let path = agent_project_doc_asset(&self.project, &self.id, name);
        let got = match self.cloud()?.call("PUT", path, Body::Bytes(bytes)).await {
            Ok(v) => v,
            Err(Refused::Offline(why)) => {
                self.offline(&why);
                return Err(Refused::Offline(why).sentence());
            }
            Err(e) => return Err(e.sentence()),
        };
        let put: arugula_control_wire::project_docs::Asset =
            serde_json::from_value(got).map_err(|e| format!("control's answer isn't a picture: {e}"))?;
        // The Doc has it now: take that without another read.
        let view = {
            let st = self.state.lock().unwrap();
            st.view.clone()
        };
        if let Some(mut view) = view {
            match view.assets.iter().position(|a| a.name == put.name) {
                Some(i) => view.assets[i] = put.clone(),
                None => {
                    view.assets.push(put.clone());
                    view.assets.sort_by(|a, b| a.name.cmp(&b.name));
                }
            }
            view.doc.rev += 1;
            self.adopt(view);
        }
        Ok(json!({ "name": name, "path": format!("assets/{name}"), "mime": mime, "size": size, "rev": put.rev }))
    }

    /// `doc_asset`: a picture's type and bytes, for a client to show.
    async fn asset(&self, name: &str) -> Result<Value, String> {
        let known = self.state.lock().unwrap().view.as_ref().is_some_and(|v| v.assets.iter().any(|a| a.name == name));
        if !known {
            return Err(format!("this Doc has no asset {name:?}"));
        }
        let path = agent_project_doc_asset(&self.project, &self.id, name);
        let got = self.cloud()?.call("GET", path, Body::None).await.map_err(|e| e.sentence())?;
        let data: AssetData =
            serde_json::from_value(got).map_err(|e| format!("control's answer isn't a picture: {e}"))?;
        Ok(json!({ "mime": data.mime, "data": data.data }))
    }

    /// The Doc's page on the platform's page, once it has been read.
    fn link(&self) -> Option<String> {
        self.state.lock().unwrap().view.as_ref().and_then(|v| v.link.clone())
    }

    fn entries(view: &DocView) -> Vec<Entry> {
        view.sections
            .iter()
            .map(|s| Entry {
                id: s.id.clone(),
                heading: s.heading.clone(),
                intent: s.intent.clone(),
                body: s.body.clone(),
                rev: s.rev,
            })
            .collect()
    }

    fn state_json(&self) -> Value {
        let st = self.state.lock().unwrap();
        let team = |link: Option<String>, writer: bool| json!({ "project": self.project, "name": self.name, "link": link, "offline": st.offline, "writer": writer });
        let Some(view) = &st.view else {
            let mut v = json!({
                "title": st.title, "entries": [], "entries_from": 0, "assets": [], "rev": st.rev, "doc": self.id,
                "team": team(None, false),
            });
            if let Some(e) = &st.error {
                v["error"] = json!(e);
            }
            return v;
        };
        let mut v = json!({
            "title": st.title, "entries": Self::entries(view), "entries_from": 0, "assets": view.assets,
            "rev": st.rev, "shared": null, "doc": self.id,
            "made": view.doc.created_at, "updated": view.doc.updated_at,
            "by": view.doc.agent.as_ref().map(|a| a.machine_name.clone()).unwrap_or_else(|| view.doc.author.clone()),
            "team": team(view.link.clone(), view.writer),
        });
        // The team's page is what the bar's Copy link and Open use.
        if view.link.is_some() {
            v["platform"] = json!(true);
        }
        if let Some(l) = view.doc.layout.as_deref().filter(|l| *l != "doc") {
            v["layout"] = json!(l);
        }
        if let Some(e) = &st.error {
            v["error"] = json!(e);
        }
        v
    }
}

/// What `doc share` and the bar's Share say of a team Doc: a team's is the
/// team's already, and its page is the link. A public link for one is
/// a follow-up.
const TEAM_SHARED: &str =
    "this is the team's Doc: the team opens it on its page; a public link for a team Doc isn't offered yet";

impl Block for Team {
    fn kind(&self) -> BlockType {
        BlockType::Doc
    }

    fn config(&self) -> Value {
        json!({
            "doc": self.id, "project": self.project, "project_name": self.name,
            "title": self.state.lock().unwrap().title,
        })
    }

    fn state(&self) -> Value {
        self.state_json()
    }

    fn state_and_changed(&self) -> Option<(Value, u64)> {
        let state = self.state_json();
        let hashes: Vec<u64> =
            state["entries"].as_array().map(|es| es.iter().map(|e| hash(&e.to_string())).collect()).unwrap_or_default();
        let mut sent = self.sent.lock().unwrap();
        let same = hashes.iter().zip(sent.iter()).take_while(|(a, b)| a == b).count();
        *sent = hashes;
        Some((state, same as u64))
    }

    fn text(&self) -> String {
        let st = self.state.lock().unwrap();
        match (&st.error, &st.view) {
            (Some(e), _) => format!("{e}\n"),
            (None, Some(v)) => markdown(&st.title, &Self::entries(v)),
            (None, None) => markdown(&st.title, &[]),
        }
    }

    fn call(&self, method: &str, args: Value) -> BoxFuture<'static, Result<Value, String>> {
        self.call_by(method, args, None)
    }

    fn call_by(&self, method: &str, args: Value, _by: Option<&str>) -> BoxFuture<'static, Result<Value, String>> {
        let me = self.me.upgrade();
        match method {
            // `doc new` made it through control: read what it made.
            "doc_outline" => Box::pin(async move { me.ok_or("closed")?.outline().await }),
            "doc_fill" => {
                let section = args["section"].as_str().unwrap_or_default().to_owned();
                let markdown = args["markdown"].as_str().map(str::to_owned);
                Box::pin(async move {
                    let me = me.ok_or("closed")?;
                    let markdown = markdown.ok_or("doc_fill needs {section, markdown}")?;
                    me.fill(&section, &markdown).await
                })
            }
            "doc_asset_put" => {
                let name = args["name"].as_str().map(str::to_owned);
                let data = args["data"].as_str().map(str::to_owned);
                Box::pin(async move {
                    let me = me.ok_or("closed")?;
                    let (Some(name), Some(data)) = (name, data) else {
                        return Err("doc_asset_put needs {name, data}".into());
                    };
                    me.asset_put(&name, &data).await
                })
            }
            "doc_asset" => {
                let name = args["name"].as_str().map(str::to_owned);
                Box::pin(async move {
                    let me = me.ok_or("closed")?;
                    me.asset(&name.ok_or("doc_asset needs {name}")?).await
                })
            }
            // `doc share`: the team's page, which the team opens.
            "doc_share_ask" => Box::pin(async move {
                let link = me.ok_or("closed")?.link();
                Ok(json!({ "status": "team", "url": link, "why": TEAM_SHARED }))
            }),
            "doc_share" | "doc_share_update" | "doc_unshare" => Box::pin(async move { Err(TEAM_SHARED.to_owned()) }),
            // What the bar asks: no public link, and the team's page.
            "doc_shares" => Box::pin(async move {
                let link = me.ok_or("closed")?.link();
                Ok(json!({ "shared": null, "can": false, "why": TEAM_SHARED, "platform": link }))
            }),
            "state" => {
                let s = self.state();
                Box::pin(async move { Ok(s) })
            }
            m => {
                let e = no_method(BlockType::Doc, m);
                Box::pin(async move { Err(e) })
            }
        }
    }

    /// A client draws it: read it again unless that was just done, and
    /// read it every [`POLL`] while one does.
    fn drawn(&self, on: bool) {
        self.shown.store(on, Ordering::Relaxed);
        let settled = self.read_at.lock().unwrap().is_some_and(|t| t.elapsed() < SETTLED);
        if on
            && !settled
            && let Some(me) = self.me.upgrade()
        {
            tokio::spawn(async move {
                let _ = me.refresh().await;
            });
        }
    }

    fn close(&self) {}

    fn summary(&self) -> Summary {
        let title = self.state.lock().unwrap().title.clone();
        Summary { title: Some(title).filter(|t| !t.is_empty()), ..Summary::default() }
    }
}

/// The block a Doc of the team's project makes. A title or heading over
/// [`HEADING_MAX`] is the platform's to refuse; the daemon says it first for
/// the outline only.
pub(super) fn open(ctx: BlockCtx, project: String, name: String, id: String, title: String) -> Arc<dyn Block> {
    let (rt, cache) = (ctx.rt.clone(), ctx.dir.join(CACHE));
    let team = Team::new(project, name, id, title, cloud(), Arc::new(move || ctx.changed()), Some(cache));
    team.start(&rt);
    team
}

/// The longest title: control's, so `doc new` can say it before asking.
pub fn title_ok(title: &str) -> Result<(), String> {
    if title.trim().is_empty() {
        return Err("a Doc needs a title".into());
    }
    if title.trim().chars().count() > HEADING_MAX {
        return Err(format!("the title is over {HEADING_MAX} characters: shorten it"));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::AtomicUsize;

    use arugula_control_wire::chat::ChatAgent;
    use arugula_control_wire::project_docs::{Asset, Doc, Section};

    use super::*;

    /// A control that holds one team Doc and counts what it's asked.
    #[derive(Default)]
    struct Fake {
        view: Mutex<Option<DocView>>,
        /// `(method, path)` of each call, in order.
        calls: Mutex<Vec<(String, String)>>,
        /// Answered instead of the route's, for each call, while set.
        refuse: Mutex<Option<Refused>>,
        n: AtomicUsize,
    }

    impl Fake {
        fn calls(&self) -> Vec<(String, String)> {
            self.calls.lock().unwrap().clone()
        }

        fn count(&self, method: &str) -> usize {
            self.calls().iter().filter(|(m, _)| m == method).count()
        }
    }

    fn doc_view(project: &str, id: &str, rev: u64, sections: &[(&str, Option<&str>)]) -> DocView {
        DocView {
            doc: Doc {
                id: id.into(),
                project: project.into(),
                title: "Plan".into(),
                author: "a".into(),
                agent: Some(ChatAgent { machine: "m".into(), machine_name: "a's agent on geek".into(), session: None }),
                sections: sections.len() as u32,
                written: sections.iter().filter(|(_, b)| b.is_some()).count() as u32,
                rev,
                created_at: 10,
                updated_at: 20 + rev,
                machine: None,
                machine_name: None,
                made_from: None,
                layout: None,
            },
            sections: sections
                .iter()
                .enumerate()
                .map(|(i, (h, b))| Section {
                    id: format!("s{}", i + 1),
                    heading: (*h).into(),
                    intent: String::new(),
                    body: b.map(str::to_owned),
                    rev: u64::from(b.is_some()),
                })
                .collect(),
            assets: vec![],
            writer: true,
            deleter: false,
            link: Some(format!("https://app.test/p/{project}/docs/{id}")),
        }
    }

    impl Cloud for Fake {
        fn call(&self, method: &'static str, path: String, body: Body) -> BoxFuture<'_, Result<Value, Refused>> {
            Box::pin(async move {
                self.calls.lock().unwrap().push((method.to_owned(), path.clone()));
                self.n.fetch_add(1, Ordering::Relaxed);
                if let Some(r) = self.refuse.lock().unwrap().clone() {
                    return Err(r);
                }
                let mut held = self.view.lock().unwrap();
                match (method, body) {
                    ("POST", Body::Json(v)) => {
                        let new: NewDoc = serde_json::from_value(v).unwrap();
                        let sections: Vec<(&str, Option<&str>)> =
                            new.sections.iter().map(|s| (s.heading.as_str(), s.markdown.as_deref())).collect();
                        let view = doc_view("prj_t", "pdc_1", 1, &sections);
                        *held = Some(view.clone());
                        Ok(serde_json::to_value(view).unwrap())
                    }
                    ("GET", _) => Ok(serde_json::to_value(held.clone().unwrap()).unwrap()),
                    ("PUT", Body::Json(v)) => {
                        let w: DocWrite = serde_json::from_value(v).unwrap();
                        let view = held.as_mut().unwrap();
                        let s = view.sections.iter_mut().find(|s| Some(&s.id) == w.section.as_ref()).unwrap();
                        s.body = w.markdown;
                        s.rev += 1;
                        view.doc.rev += 1;
                        Ok(serde_json::to_value(view.clone()).unwrap())
                    }
                    ("PUT", Body::Bytes(b)) => {
                        let view = held.as_mut().unwrap();
                        let name = path.rsplit('/').next().unwrap().to_owned();
                        let a = Asset { name, mime: "image/png".into(), size: b.len() as u64, rev: 1 };
                        view.assets.push(a.clone());
                        Ok(serde_json::to_value(a).unwrap())
                    }
                    other => panic!("unexpected {} {path}", other.0),
                }
            })
        }
    }

    fn temp(name: &str) -> PathBuf {
        let p = std::env::temp_dir().join(format!("arugula-team-doc-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&p);
        std::fs::create_dir_all(&p).unwrap();
        p
    }

    fn block(fake: &Arc<Fake>, cache: Option<PathBuf>) -> (Arc<Team>, Arc<AtomicUsize>) {
        let changed = Arc::new(AtomicUsize::new(0));
        let n = changed.clone();
        let team = Team::new(
            "prj_t".into(),
            "Acme".into(),
            "pdc_1".into(),
            "Plan".into(),
            Some(fake.clone() as Arc<dyn Cloud>),
            Arc::new(move || {
                n.fetch_add(1, Ordering::Relaxed);
            }),
            cache,
        );
        (team, changed)
    }

    fn seeded() -> Arc<Fake> {
        let fake = Arc::new(Fake::default());
        *fake.view.lock().unwrap() =
            Some(doc_view("prj_t", "pdc_1", 1, &[("Goal", None), ("Risks", None), ("Plan", None)]));
        fake
    }

    /// Thirty fills are thirty PUTs and nothing else: the block holds the
    /// project and the Doc, and takes each answer as the Doc.
    #[tokio::test]
    async fn a_run_of_fills_is_one_put_each_and_reads_nothing() {
        let fake = seeded();
        let (team, _) = block(&fake, None);
        team.refresh().await.unwrap();
        let gets = fake.count("GET");
        for (i, id) in ["s1", "s2", "s3"].iter().enumerate() {
            team.fill(id, &format!("Body {i}")).await.unwrap();
        }
        assert_eq!((fake.count("PUT"), fake.count("GET"), fake.count("POST")), (3, gets, 0), "{:?}", fake.calls());
        assert!(fake.calls().iter().skip(1).all(|(_, p)| p == "/api/daemon/projects/prj_t/docs/pdc_1"));
        let s = team.state();
        assert_eq!(s["entries"][2]["body"], "Body 2");
        assert_eq!(s["entries"][0]["rev"], 1);
        team.fill("s1", "Again").await.unwrap();
        assert_eq!(team.state()["entries"][0]["rev"], 2, "a section filled again has a new rev");
        assert_eq!(s["team"]["project"], "prj_t");
        assert_eq!(s["doc"], "pdc_1");
        assert_eq!(s["by"], "a's agent on geek");
        assert_eq!(s["platform"], true);
    }

    /// A bad fence or a section that isn't there is one line, and nothing is sent.
    #[tokio::test]
    async fn a_bad_fill_is_refused_before_it_is_sent() {
        let fake = seeded();
        let (team, _) = block(&fake, None);
        team.refresh().await.unwrap();
        let before = fake.calls().len();
        let e = team.fill("s9", "x").await.unwrap_err();
        assert!(e.contains("no section") && e.contains("s1, s2, s3"), "{e}");
        let e = team.fill("s1", "```chart\n{}\n```").await.unwrap_err();
        assert!(e.contains("section s1") && e.contains("unchanged"), "{e}");
        assert_eq!(fake.calls().len(), before, "nothing was sent");
    }

    /// Control out of reach: the last Doc read stays, with the note, and a
    /// write fails saying why; it comes back when control does.
    #[tokio::test]
    async fn offline_the_block_keeps_what_it_read_and_a_fill_fails_with_why() {
        let fake = seeded();
        let (team, changed) = block(&fake, None);
        team.refresh().await.unwrap();
        team.fill("s1", "One").await.unwrap();
        *fake.refuse.lock().unwrap() = Some(Refused::Offline("can't reach Arugula control: down".into()));
        let was = changed.load(Ordering::Relaxed);
        assert!(team.refresh().await.is_err());
        assert!(changed.load(Ordering::Relaxed) > was, "clients are told");
        let s = team.state();
        assert_eq!(s["team"]["offline"], "can't reach Arugula control: down");
        assert_eq!(s["entries"][0]["body"], "One", "the last state read");
        let e = team.fill("s2", "Two").await.unwrap_err();
        assert!(e.starts_with("Offline: read-only") && e.contains("can't reach Arugula control"), "{e}");
        assert_eq!(team.state()["entries"][1]["body"], serde_json::Value::Null, "nothing was written here");
        *fake.refuse.lock().unwrap() = None;
        team.refresh().await.unwrap();
        assert_eq!(team.state()["team"]["offline"], serde_json::Value::Null);
    }

    /// Control's refusal (no `docs.write`) comes back as it is, and is never
    /// answered by keeping the Doc on this machine.
    #[tokio::test]
    async fn a_refusal_goes_back_as_it_is_and_nothing_is_kept_on_the_machine() {
        let fake = seeded();
        let state = temp("refused");
        let (team, _) = block(&fake, Some(state.join("blocks/7").join(CACHE)));
        team.refresh().await.unwrap();
        let said = "this machine isn't granted docs.write over this project: give it in Settings".to_owned();
        *fake.refuse.lock().unwrap() = Some(Refused::Said(said.clone()));
        assert_eq!(team.fill("s1", "One").await.unwrap_err(), said);
        assert!(!state.join("docs").exists(), "no library under the state");
        assert_eq!(team.state()["entries"][0]["body"], serde_json::Value::Null);
        let _ = std::fs::remove_dir_all(&state);
    }

    /// The last Doc read is in the block's folder, and a restart shows it
    /// with control out of reach. The library's folder is never made.
    #[tokio::test]
    async fn a_restart_offline_shows_the_last_doc_and_writes_no_library() {
        let fake = seeded();
        let state = temp("restart");
        let cache = state.join("blocks/7").join(CACHE);
        let (team, _) = block(&fake, Some(cache.clone()));
        team.refresh().await.unwrap();
        team.fill("s1", "Kept").await.unwrap();
        drop(team);
        let down = Arc::new(Fake::default());
        *down.refuse.lock().unwrap() = Some(Refused::Offline("down".into()));
        let (again, _) = block(&down, Some(cache));
        assert!(again.refresh().await.is_err());
        let s = again.state();
        assert_eq!((s["entries"][0]["body"].as_str(), s["team"]["offline"].as_str()), (Some("Kept"), Some("down")));
        assert!(!state.join("docs").exists(), "ls <state>/docs stays library-only");
        let _ = std::fs::remove_dir_all(&state);
    }

    /// A read that started before a write may arrive after it: the older Doc
    /// doesn't replace the newer.
    #[tokio::test]
    async fn an_older_read_arriving_late_doesnt_replace_a_newer_write() {
        let fake = seeded();
        let (team, _) = block(&fake, None);
        team.refresh().await.unwrap();
        let stale = fake.view.lock().unwrap().clone().unwrap();
        team.fill("s1", "Newer").await.unwrap();
        team.adopt(stale);
        assert_eq!(team.state()["entries"][0]["body"], "Newer");
    }

    /// A picture goes through the PUT and joins the state without a read.
    #[tokio::test]
    async fn a_picture_is_put_and_listed_without_a_read() {
        use base64::Engine;
        let fake = seeded();
        let (team, _) = block(&fake, None);
        team.refresh().await.unwrap();
        let gets = fake.count("GET");
        let mut png = b"\x89PNG\r\n\x1a\n".to_vec();
        png.extend_from_slice(b"pixels");
        let data = base64::engine::general_purpose::STANDARD.encode(&png);
        let out = team.asset_put("pic.png", &data).await.unwrap();
        assert_eq!(out["path"], "assets/pic.png");
        assert_eq!(fake.count("GET"), gets);
        assert_eq!(team.state()["assets"][0]["name"], "pic.png");
        team.fill("s1", "![a](assets/pic.png)").await.unwrap();
        assert!(team.asset_put("pic.jpg", &data).await.is_err(), "named for another type");
    }

    /// `doc new` in a team: one POST with the outline pending and the layout
    /// said only off `doc`; its sections are the platform's ids.
    #[tokio::test]
    async fn making_a_team_doc_is_one_post_with_pending_sections() {
        let fake = Arc::new(Fake::default());
        let sections = vec![("Goal".to_owned(), "why".to_owned()), ("Risks".to_owned(), String::new())];
        let view = make(&*fake, "prj_t", "Plan", &sections, "tabs").await.unwrap();
        assert_eq!(fake.calls(), [("POST".to_owned(), "/api/daemon/projects/prj_t/docs".to_owned())]);
        assert_eq!(view.sections.iter().map(|s| s.id.as_str()).collect::<Vec<_>>(), ["s1", "s2"]);
        *fake.refuse.lock().unwrap() = Some(Refused::Said("no docs.write".into()));
        assert_eq!(make(&*fake, "prj_t", "Plan", &sections, "doc").await.unwrap_err(), "no docs.write");
    }

    /// `doc share` of a team Doc is its page, and a public link isn't offered.
    #[tokio::test]
    async fn sharing_a_team_doc_gives_its_page() {
        let fake = seeded();
        let (team, _) = block(&fake, None);
        team.refresh().await.unwrap();
        let out = team.call("doc_share_ask", json!({})).await.unwrap();
        assert_eq!(
            (out["status"].as_str(), out["url"].as_str()),
            (Some("team"), Some("https://app.test/p/prj_t/docs/pdc_1"))
        );
        assert!(team.call("doc_share", json!({})).await.unwrap_err().contains("team's Doc"));
    }
}
