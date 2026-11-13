//! A Doc moved to a team (Arugula #863, spike #832's model: one home, no
//! copies, a hidden pointer left behind). The machine makes the move, not
//! the platform, in two steps:
//!
//! 1. **The team Doc is made** from the Doc as it is now, through the team
//!    block's routes ([`super::team`]): its title, outline and layout with
//!    `moved_from` (this machine and the Doc's id), then its pictures, then
//!    each section that was written.
//! 2. **The Doc becomes a pointer** ([`library::point`]): `meta.json` keeps
//!    its `id` and `title` and gains `moved`, and its other files go.
//!
//! If making the team Doc fails at any step, the half-made team Doc is
//! deleted and the Doc here is left as it was: never both half there. Once
//! the team Doc exists, and before it's filled, the Doc's `meta.json` names
//! it (`moving`, [`library::set_moving`]), so a move cut off by a crash isn't
//! lost: the next move of the Doc deletes that team Doc first, and so does
//! the daemon's next start ([`clear_unfinished`]), which moves nothing.
//!
//! **A Doc being filled as it moves:** each block showing it holds its write
//! lock through the move, so a fill waits; then the block shows the team Doc
//! ([`super::Doc::follow`]), and the fill that waited lands there. A block
//! opened on the old id afterwards, `doc open` of it, and a restart open the
//! team block instead ([`library::moved_to`]).

use std::{path::Path, sync::Arc};

use arugula_control_wire::project_docs::{
    DocView, DocWrite, MovedFrom, NewDoc, NewSection, agent_project_doc, agent_project_doc_asset, agent_project_docs,
};
use arugula_proto::api::DocLayout;
use serde_json::Value;
use tracing::{info, warn};

use super::{
    Entry, assets,
    library::{self, MovedTo, Moving},
    team::{self, Body, Cloud, Refused},
};
use crate::store::now_ms;

/// The team's project a Doc moves to: its id, and its name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Target {
    pub project: String,
    pub name: String,
}

/// The Doc as it is when it moves.
struct Content {
    title: String,
    layout: DocLayout,
    entries: Vec<Entry>,
    /// Each picture's name and bytes.
    pictures: Vec<(String, Vec<u8>)>,
}

/// One move at a time, so two moves of one Doc make one team Doc, and a
/// start's clearing never meets a move.
static ONE: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

/// Move the library Doc `id` to `to`, as the machine `machine`. Where it is
/// now; a Doc moved already answers where it went. On an error the Doc
/// here is as it was, and no team Doc is left (or, with control out of
/// reach, one is named in `moving` to delete on the next try).
pub async fn move_doc(
    state_dir: &Path,
    cloud: Arc<dyn Cloud>,
    machine: &str,
    id: &str,
    to: &Target,
) -> Result<MovedTo, String> {
    let _one = ONE.lock().await;
    let dir = library::dir(state_dir, id)?;
    let meta = {
        let dir = dir.clone();
        crate::task::spawn_blocking(move || library::read_meta(&dir)).await.map_err(|e| e.to_string())?
    };
    let meta = meta.ok_or_else(|| format!("no Doc {id} in the library (docs list has them)"))?;
    if let Some(there) = meta.moved {
        return Ok(there);
    }
    // A move cut off before: its team Doc goes first.
    if let Some(was) = &meta.moving {
        forget(&*cloud, &dir, was).await.map_err(|e| {
            format!("can't delete the team Doc {} a move cut off left: {e}; the Doc is as it was", was.id)
        })?;
    }
    // The blocks showing it write nothing while it moves.
    let blocks = super::showing(id);
    let mut held = Vec::new();
    for b in &blocks {
        held.push(b.io.lock().await);
    }
    let content = {
        let dir = dir.clone();
        crate::task::spawn_blocking(move || read(&dir)).await.map_err(|e| e.to_string())??
    };
    let got = make(&*cloud, machine, id, to, &content).await.map_err(|e| format!("{e}; the Doc is as it was"))?;
    let made = got["doc"]["id"].as_str().unwrap_or_default().to_owned();
    if made.is_empty() {
        return Err("control's answer names no Doc; the Doc is as it was".into());
    }
    // Before anything is put to it: a crash from here on leaves its name.
    let moving = Moving { project: to.project.clone(), id: made.clone(), at: now_ms() };
    let noted = {
        let (dir, moving) = (dir.clone(), moving.clone());
        crate::task::spawn_blocking(move || library::set_moving(&dir, Some(moving))).await.map_err(|e| e.to_string())
    };
    if let Err(e) = noted.and_then(|r| r) {
        let _ = cloud.call("DELETE", agent_project_doc(&to.project, &made), Body::None).await;
        return Err(format!("can't note the move here ({e}); the Doc is as it was"));
    }
    let view = match fill(&*cloud, to, &content, got).await {
        Ok(view) => view,
        Err(e) => return Err(format!("{e}{}; the Doc is as it was", undo(&*cloud, &dir, &moving).await)),
    };
    let at = MovedTo {
        project: to.project.clone(),
        name: to.name.clone(),
        id: view.doc.id.clone(),
        link: view.link.clone(),
        at: now_ms(),
    };
    let pointed = {
        let (dir, at) = (dir.clone(), at.clone());
        crate::task::spawn_blocking(move || library::point(&dir, at)).await.map_err(|e| e.to_string())
    };
    if let Err(e) = pointed.and_then(|r| r) {
        let undone = undo(&*cloud, &dir, &moving).await;
        return Err(format!("can't keep the Doc's pointer here ({e}){undone}; the Doc is as it was"));
    }
    for b in &blocks {
        b.follow(Some(cloud.clone()), to, view.clone());
    }
    drop(held);
    info!(doc = id, project = to.project, team_doc = at.id, "moved a Doc to a team");
    Ok(at)
}

/// At start: each move a crash cut off has its half-made team Doc deleted
/// and its `moving` cleared. The Doc stays here, unmoved; one control can't
/// be asked about now waits for its next move.
pub async fn clear_unfinished(state_dir: &Path, cloud: Arc<dyn Cloud>) {
    let _one = ONE.lock().await;
    let state = state_dir.to_owned();
    let cut = crate::task::spawn_blocking(move || library::unfinished(&state)).await.unwrap_or_default();
    for (id, was) in cut {
        let Ok(dir) = library::dir(state_dir, &id) else { continue };
        match forget(&*cloud, &dir, &was).await {
            Ok(()) => info!(doc = id, team_doc = was.id, "deleted the team Doc of a move cut off"),
            Err(e) => warn!(doc = id, team_doc = was.id, error = e, "can't delete the team Doc of a move cut off yet"),
        }
    }
}

/// The Doc's outline, sections and pictures, from its files.
fn read(dir: &Path) -> Result<Content, String> {
    let meta = library::read_meta(dir).ok_or("the Doc's meta.json can't be read")?;
    let (title, entries) = if dir.join(super::OUTLINE).exists() {
        let loaded = super::load(dir)?;
        (loaded.title, loaded.entries)
    } else {
        (meta.title.clone(), Vec::new())
    };
    let mut pictures = Vec::new();
    for a in assets::load(&dir.join(assets::FOLDER)) {
        let bytes = std::fs::read(dir.join(assets::FOLDER).join(&a.name))
            .map_err(|e| format!("can't read the picture {}: {e}", a.name))?;
        pictures.push((a.name, bytes));
    }
    Ok(Content { title, layout: meta.layout, entries, pictures })
}

/// The team Doc, made with its outline and where it's from: control's
/// answer.
async fn make(cloud: &dyn Cloud, machine: &str, id: &str, to: &Target, c: &Content) -> Result<Value, String> {
    let new = NewDoc {
        title: c.title.clone(),
        sections: c
            .entries
            .iter()
            .map(|e| NewSection { heading: e.heading.clone(), intent: e.intent.clone(), markdown: None })
            .collect(),
        layout: (!c.layout.is_doc()).then(|| c.layout.as_str().to_owned()),
        moved_from: Some(MovedFrom { machine: machine.to_owned(), doc: id.to_owned() }),
    };
    let body = serde_json::to_value(new).map_err(|e| e.to_string())?;
    cloud.call("POST", agent_project_docs(&to.project), Body::Json(body)).await.map_err(|e| e.sentence())
}

/// Its pictures, then each written section.
async fn fill(cloud: &dyn Cloud, to: &Target, c: &Content, got: Value) -> Result<DocView, String> {
    let parse =
        |v: Value| serde_json::from_value::<DocView>(v).map_err(|e| format!("control's answer isn't a Doc: {e}"));
    let mut view = parse(got)?;
    // A block's fills name sections by the ids it had, so the team Doc's
    // must be the same.
    let ids: Vec<&str> = view.sections.iter().map(|s| s.id.as_str()).collect();
    if ids != c.entries.iter().map(|e| e.id.as_str()).collect::<Vec<_>>() {
        return Err(format!("control gave the sections other ids ({})", ids.join(", ")));
    }
    let doc = view.doc.id.clone();
    for (name, bytes) in &c.pictures {
        let path = agent_project_doc_asset(&to.project, &doc, name);
        cloud.call("PUT", path, Body::Bytes(bytes.clone())).await.map_err(|e| format!("{name}: {}", e.sentence()))?;
    }
    for e in &c.entries {
        let Some(md) = &e.body else { continue };
        let w = DocWrite { section: Some(e.id.clone()), markdown: Some(md.clone()), ..Default::default() };
        let body = serde_json::to_value(w).map_err(|e| e.to_string())?;
        let got = cloud
            .call("PUT", agent_project_doc(&to.project, &doc), Body::Json(body))
            .await
            .map_err(|err| format!("section {}: {}", e.id, err.sentence()))?;
        view = parse(got)?;
    }
    // The pictures are in the answer to a later write; with none, read it.
    if !c.pictures.is_empty() && c.entries.iter().all(|e| e.body.is_none()) {
        view = team::read(cloud, &to.project, &doc).await?;
    }
    Ok(view)
}

/// The half-made team Doc, deleted, and `moving` cleared: nothing, or what
/// the person is told when control couldn't delete it (the next try does).
async fn undo(cloud: &dyn Cloud, dir: &Path, moving: &Moving) -> String {
    match forget(cloud, dir, moving).await {
        Ok(()) => String::new(),
        Err(e) => {
            warn!(team_doc = moving.id, error = e, "can't delete a half-made team Doc");
            format!(" (the half-made team Doc {} couldn't be deleted yet: the next move deletes it)", moving.id)
        }
    }
}

/// Delete the team Doc `moving` names (one control hasn't got is as good),
/// then clear `moving`.
async fn forget(cloud: &dyn Cloud, dir: &Path, moving: &Moving) -> Result<(), String> {
    match cloud.call("DELETE", agent_project_doc(&moving.project, &moving.id), Body::None).await {
        Ok(_) | Err(Refused::Gone(_)) => {}
        Err(e) => return Err(e.sentence()),
    }
    let dir = dir.to_owned();
    crate::task::spawn_blocking(move || library::set_moving(&dir, None)).await.map_err(|e| e.to_string())?
}

#[cfg(test)]
mod tests {
    use std::{
        collections::BTreeMap,
        path::PathBuf,
        sync::{
            Mutex,
            atomic::{AtomicBool, Ordering},
        },
    };

    use arugula_control_wire::{
        chat::ChatAgent,
        project_docs::{Asset, Doc as Head, Section},
    };
    use futures_util::future::BoxFuture;
    use serde_json::json;
    use tokio::sync::Notify;

    use super::*;
    use crate::{
        block::Block,
        docs::{Doc, LIVE, link::Link, team::Refused},
    };

    /// A control that holds the one team Doc a move makes, and remembers
    /// what it was sent.
    #[derive(Default)]
    struct Fake {
        view: Mutex<Option<DocView>>,
        new: Mutex<Option<NewDoc>>,
        calls: Mutex<Vec<(String, String)>>,
        /// A call refused while set: its method and how its path ends.
        refuse: Mutex<Option<(&'static str, &'static str)>>,
        /// The POST waits for this while set, so the move is under way.
        gate: Mutex<Option<Arc<Notify>>>,
        /// It got as far as the POST.
        posted: Notify,
        deleted: AtomicBool,
        /// Each PUT waits forever while set: a move cut off there.
        hang: AtomicBool,
        /// The team Docs made, so each gets its own id.
        made: std::sync::atomic::AtomicUsize,
    }

    impl Fake {
        fn calls(&self) -> Vec<(String, String)> {
            self.calls.lock().unwrap().clone()
        }

        fn body(&self, section: usize) -> Option<String> {
            self.view.lock().unwrap().as_ref().and_then(|v| v.sections[section].body.clone())
        }
    }

    impl Cloud for Fake {
        fn call(&self, method: &'static str, path: String, body: Body) -> BoxFuture<'_, Result<Value, Refused>> {
            Box::pin(async move {
                self.calls.lock().unwrap().push((method.to_owned(), path.clone()));
                if let Some((m, end)) = *self.refuse.lock().unwrap()
                    && m == method
                    && path.ends_with(end)
                {
                    return Err(Refused::Said("the project is full".into()));
                }
                if method == "PUT" && self.hang.load(Ordering::Relaxed) {
                    std::future::pending::<()>().await;
                }
                if method == "POST" {
                    self.posted.notify_one();
                    let gate = self.gate.lock().unwrap().clone();
                    if let Some(g) = gate {
                        g.notified().await;
                    }
                }
                let mut held = self.view.lock().unwrap();
                match (method, body) {
                    ("POST", Body::Json(v)) => {
                        let new: NewDoc = serde_json::from_value(v).unwrap();
                        let n = self.made.fetch_add(1, Ordering::Relaxed);
                        let view = DocView {
                            doc: Head {
                                id: if n == 0 { "pdc_9".into() } else { format!("pdc_9{n}") },
                                project: "prj_t".into(),
                                title: new.title.clone(),
                                author: "a".into(),
                                agent: Some(ChatAgent {
                                    machine: "m".into(),
                                    machine_name: "geek".into(),
                                    session: None,
                                }),
                                sections: new.sections.len() as u32,
                                written: 0,
                                rev: 1,
                                created_at: 1,
                                updated_at: 1,
                                layout: new.layout.clone(),
                                moved_from: new.moved_from.clone(),
                            },
                            sections: new
                                .sections
                                .iter()
                                .enumerate()
                                .map(|(i, s)| Section {
                                    id: format!("s{}", i + 1),
                                    heading: s.heading.clone(),
                                    intent: s.intent.clone(),
                                    body: None,
                                    rev: 0,
                                })
                                .collect(),
                            assets: vec![],
                            writer: true,
                            deleter: false,
                            link: Some("https://app.test/p/prj_t/docs/pdc_9".into()),
                            public: None,
                        };
                        *self.new.lock().unwrap() = Some(new);
                        *held = Some(view.clone());
                        Ok(serde_json::to_value(view).unwrap())
                    }
                    ("PUT", Body::Json(v)) => {
                        let w: DocWrite = serde_json::from_value(v).unwrap();
                        let view = held.as_mut().unwrap();
                        let s = view.sections.iter_mut().find(|s| Some(&s.id) == w.section.as_ref()).unwrap();
                        (s.body, s.rev) = (w.markdown, s.rev + 1);
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
                    ("GET", _) => Ok(serde_json::to_value(held.clone().unwrap()).unwrap()),
                    ("DELETE", _) => {
                        // Only the Doc it holds; another is gone already.
                        if held.as_ref().is_none_or(|v| !path.ends_with(&format!("/{}", v.doc.id))) {
                            return Err(Refused::Gone("no such Doc".into()));
                        }
                        *held = None;
                        self.deleted.store(true, Ordering::Relaxed);
                        Ok(json!({}))
                    }
                    other => panic!("unexpected {} {path}", other.0),
                }
            })
        }
    }

    fn temp(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("arugula-move-{name}-{}-{}", std::process::id(), now_ms()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// A library Doc with an outline of two sections, the first filled
    /// with a picture, shown in a block as `doc new` leaves it.
    async fn shown(state: &Path) -> (String, Arc<Doc>) {
        use base64::Engine;
        let meta = library::make(state, "Plan", "mcp:claude-code").unwrap();
        let dir = library::dir(state, &meta.id).unwrap();
        let link = Link::none(state.to_owned(), meta.id.clone());
        let doc = Doc::with_link(dir, "Plan".into(), true, link, Arc::new(|| {}));
        LIVE.lock().unwrap().push(Arc::downgrade(&doc));
        let sections = json!([{ "heading": "Goal", "intent": "why" }, { "heading": "Risks" }]);
        doc.call("doc_outline", json!({ "title": "Plan", "sections": sections })).await.unwrap();
        let png = base64::engine::general_purpose::STANDARD.encode(b"\x89PNG\r\n\x1a\npixels");
        doc.call("doc_asset_put", json!({ "name": "dot.png", "data": png })).await.unwrap();
        doc.call("doc_fill", json!({ "section": "s1", "markdown": "First ![a dot](assets/dot.png)" })).await.unwrap();
        (meta.id, doc)
    }

    fn acme() -> Target {
        Target { project: "prj_t".into(), name: "Acme".into() }
    }

    /// Every file in a folder, with its bytes.
    fn tree(dir: &Path) -> BTreeMap<String, Vec<u8>> {
        let mut out = BTreeMap::new();
        for e in walkdir(dir) {
            out.insert(e.strip_prefix(dir).unwrap().display().to_string(), std::fs::read(&e).unwrap());
        }
        out
    }

    fn walkdir(dir: &Path) -> Vec<PathBuf> {
        let mut out = vec![];
        for e in std::fs::read_dir(dir).into_iter().flatten().flatten() {
            if e.path().is_dir() {
                out.extend(walkdir(&e.path()));
            } else {
                out.push(e.path());
            }
        }
        out
    }

    /// The trap: an agent fills the Doc while it moves. The fill that waits
    /// for the move, and the next one on the old block, land in the team
    /// Doc: never a library Doc made again, never "no such Doc".
    #[tokio::test]
    async fn a_fill_on_the_old_block_during_and_after_the_move_lands_in_the_team_doc() {
        let state = temp("trap");
        let (id, doc) = shown(&state).await;
        let fake = Arc::new(Fake::default());
        let gate = Arc::new(Notify::new());
        *fake.gate.lock().unwrap() = Some(gate.clone());

        let moving = {
            let (state, fake, id) = (state.clone(), fake.clone(), id.clone());
            tokio::spawn(async move { move_doc(&state, fake, "dev_m", &id, &acme()).await })
        };
        // The move holds the block: a fill now waits for it.
        fake.posted.notified().await;
        let filling = {
            let doc = doc.clone();
            tokio::spawn(async move { doc.call("doc_fill", json!({ "section": "s2", "markdown": "Second" })).await })
        };
        tokio::task::yield_now().await;
        gate.notify_one();
        let at = moving.await.unwrap().unwrap();
        filling.await.unwrap().expect("the fill that waited lands");
        assert_eq!((at.project.as_str(), at.name.as_str(), at.id.as_str()), ("prj_t", "Acme", "pdc_9"));
        assert_eq!(fake.body(1).as_deref(), Some("Second"), "in the team Doc");

        // The next fill on the old block goes there too.
        doc.call("doc_fill", json!({ "section": "s1", "markdown": "Again" })).await.unwrap();
        assert_eq!(fake.body(0).as_deref(), Some("Again"));
        assert_eq!(doc.state()["entries"][0]["body"], "Again", "the block shows the team Doc");
        let config = doc.config();
        assert_eq!(
            (config["doc"].as_str(), config["project"].as_str()),
            (Some("pdc_9"), Some("prj_t")),
            "a restart opens the team block"
        );

        // The team Doc was made whole, with where it came from.
        let new = fake.new.lock().unwrap().clone().unwrap();
        assert_eq!(new.moved_from, Some(MovedFrom { machine: "dev_m".into(), doc: id.clone() }));
        assert_eq!(fake.view.lock().unwrap().as_ref().unwrap().assets[0].name, "dot.png");
        // The library keeps a pointer, and nothing else: no fill wrote there.
        let dir = library::dir(&state, &id).unwrap();
        assert_eq!(tree(&dir).into_keys().collect::<Vec<_>>(), ["meta.json"]);
        let meta = library::read_meta(&dir).unwrap();
        assert_eq!((meta.id.as_str(), meta.title.as_str()), (id.as_str(), "Plan"));
        assert_eq!(
            meta.moved.as_ref().map(|m| m.link.clone()),
            Some(Some("https://app.test/p/prj_t/docs/pdc_9".into()))
        );
        assert!(library::list(&state).is_empty(), "the pointer isn't a Doc");
        assert_eq!(library::moved_to(&state, &id), meta.moved);
        // Moving it again is where it went, and asks control nothing.
        let n = fake.calls().len();
        assert_eq!(move_doc(&state, fake.clone(), "dev_m", &id, &acme()).await.unwrap(), at);
        assert_eq!(fake.calls().len(), n);
        std::fs::remove_dir_all(state).unwrap();
    }

    /// A step that fails partway takes the half-made team Doc back down, and
    /// leaves the Doc here byte for byte as it was, its block too.
    #[tokio::test]
    async fn a_move_that_fails_midway_deletes_the_team_doc_and_leaves_the_doc() {
        for (method, end) in [("PUT", "/assets/dot.png"), ("PUT", "/docs/pdc_9")] {
            let state = temp("rollback");
            let (id, doc) = shown(&state).await;
            let dir = library::dir(&state, &id).unwrap();
            let before = tree(&dir);
            let fake = Arc::new(Fake::default());
            *fake.refuse.lock().unwrap() = Some((method, end));

            let e = move_doc(&state, fake.clone(), "dev_m", &id, &acme()).await.unwrap_err();
            assert!(e.contains("the project is full") && e.ends_with("the Doc is as it was"), "{e}");
            assert!(fake.deleted.load(Ordering::Relaxed), "{end}: the team Doc was deleted");
            assert_eq!(fake.calls().last().map(|c| c.0.as_str()), Some("DELETE"));
            assert_eq!(tree(&dir), before, "{end}: the Doc is untouched");
            assert_eq!(library::list(&state).len(), 1);
            assert_eq!(doc.config()["doc"], id.as_str(), "the block still shows it");
            doc.call("doc_fill", json!({ "section": "s2", "markdown": "Here" })).await.unwrap();
            assert_eq!(std::fs::read_to_string(dir.join("s2.md")).unwrap(), "Here");
            std::fs::remove_dir_all(state).unwrap();
        }
    }

    /// A move cut off after the POST (the daemon stopped there) leaves the
    /// team Doc's name in `meta.json`. A start deletes that team Doc and moves
    /// nothing; and a move tried again after a cut deletes it first, so one
    /// team Doc is left.
    #[tokio::test]
    async fn a_move_cut_off_after_the_post_then_tried_again_leaves_one_team_doc() {
        let state = temp("cut");
        let (id, doc) = shown(&state).await;
        let dir = library::dir(&state, &id).unwrap();
        let fake = Arc::new(Fake::default());
        let cut = |fake: &Arc<Fake>| {
            fake.hang.store(true, Ordering::Relaxed);
            let (state, fake, id) = (state.clone(), fake.clone(), id.clone());
            tokio::spawn(async move { move_doc(&state, fake, "dev_m", &id, &acme()).await })
        };

        // Cut off, then a start: the team Doc goes, and the Doc stays here.
        let moving = cut(&fake);
        fake.posted.notified().await;
        while library::read_meta(&dir).unwrap().moving.is_none() {
            tokio::task::yield_now().await;
        }
        moving.abort();
        let _ = moving.await;
        assert_eq!(library::read_meta(&dir).unwrap().moving.map(|m| m.id), Some("pdc_9".into()));
        fake.hang.store(false, Ordering::Relaxed);
        clear_unfinished(&state, fake.clone()).await;
        assert!(fake.view.lock().unwrap().is_none(), "the half-made team Doc is deleted");
        let meta = library::read_meta(&dir).unwrap();
        assert!(meta.moving.is_none() && meta.moved.is_none(), "cleared, and not moved");
        assert_eq!(library::list(&state).len(), 1);
        assert_eq!(doc.config()["doc"], id.as_str());

        // Cut off again, then moved again: the first try's team Doc is
        // deleted before the second is made.
        let moving = cut(&fake);
        fake.posted.notified().await;
        while library::read_meta(&dir).unwrap().moving.is_none() {
            tokio::task::yield_now().await;
        }
        moving.abort();
        let _ = moving.await;
        let first = library::read_meta(&dir).unwrap().moving.unwrap().id;
        fake.hang.store(false, Ordering::Relaxed);
        let at = move_doc(&state, fake.clone(), "dev_m", &id, &acme()).await.unwrap();
        assert_ne!(at.id, first);
        let deletes: Vec<_> = fake.calls().into_iter().filter(|(m, _)| m == "DELETE").map(|(_, p)| p).collect();
        assert_eq!(
            deletes.last().map(String::as_str),
            Some(format!("/api/daemon/projects/prj_t/docs/{first}").as_str())
        );
        assert_eq!(fake.view.lock().unwrap().as_ref().map(|v| v.doc.id.clone()), Some(at.id.clone()), "one team Doc");
        assert_eq!(fake.body(0).as_deref(), Some("First ![a dot](assets/dot.png)"));
        let meta = library::read_meta(&dir).unwrap();
        assert!(meta.moving.is_none() && meta.moved.is_some());
        std::fs::remove_dir_all(state).unwrap();
    }
}
