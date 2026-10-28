//! Doc blocks (#962, #963): a document an agent builds beside its pane, an
//! outline first and then one section at a time, that people watch fill in.
//!
//! Config `{doc, title, share_ask?}`. The block is a view onto one Doc in
//! the library ([`library`], #1493): `doc` is its id, and the files are in
//! `<state>/docs/<doc>/`, not in the block's folder, so closing the block
//! leaves the Doc and any block can open it again. The daemon holds the Doc
//! and the agent only talks to it through MCP's `doc`; disk is persistence,
//! not transport. In the Doc's folder:
//!
//! - `meta.json`: `{id, title, made, updated, by}`, which [`library`] keeps;
//! - `doc.json`: `{title, sections: [{id, heading, intent}]}`, written once
//!   by `doc_outline`;
//! - `sN.md`: the Markdown of each section that was filled.
//!
//! Every file is written with `store::write_atomic`, and one lock per Doc
//! serialises the writes, so parallel fills all land. A `doc.json` that
//! doesn't parse is shown as the block's error and never rewritten; a
//! `sN.tmp` left by a crash is never read.
//!
//! **The state** is `{title, entries, entries_from: 0, rev}`. The sections
//! are `entries` so that clients that already have the first ones get a
//! patch (#713, [`arugula_proto::block_patch`]): every entry from the first
//! one that changed to the end. Filling in order sends the section filled
//! and the pending ones after it. Refilling an early one once later ones
//! are filled sends the tail again; that is accepted.
//!
//! Methods (the owner's, `authz.rs`): `doc_outline {title, sections:
//! [{heading, intent}]}`, once, and `doc_fill {section, markdown}`, which
//! replaces a body. A body may hold ```` ```chart ```` and ```` ```svg ````
//! fences (#964, [`embeds`]): `doc_fill` checks every one before it writes
//! anything, so a bad one leaves the section as it was.
//!
//! **Pictures** (#965, [`assets`]): `doc_asset_put {name, data}` (the
//! owner's) keeps a PNG, JPEG, GIF or WebP in `assets/`, and a body names it
//! as `![alt](assets/<name>)`, which `doc_fill` checks like a fence. The state
//! lists them as `assets: [{name, mime, size, rev}]` and never holds their
//! bytes; `doc_asset {name}` (a viewer's) answers `{mime, data}` for one.
//!
//! **Sharing** (#1064, [`link`]): `doc_share`, `doc_share_update`,
//! `doc_unshare` and `doc_shares` (the owner's, and never an agent's: `api.rs`
//! refuses the agent header) keep an encrypted snapshot on control and give
//! a URL. The state gains `shared: {made, updated} | null` and nothing more:
//! the key is in `<state>/doc-links.json`, never here.
//!
//! Docs left Labs (native #204): no flag gates them, and sharing is still
//! only ever the owner's tap or the card an agent's `share` raises.

mod ask;
mod assets;
pub(crate) mod embeds;
pub mod library;
pub mod link;

use std::{
    path::{Path, PathBuf},
    sync::{Arc, Mutex, Weak},
};

use arugula_proto::{BlockType, fs::READ_MAX};
use futures_util::future::BoxFuture;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

pub use assets::ASSET_MAX;
use assets::{ASSETS_MAX, Asset};
use link::{Fail, Link};

use crate::{
    block::{Block, BlockCtx, BlockKind, Summary, no_method},
    store::{now_ms, private_dir, write_atomic},
};

/// A section's body, in bytes.
pub const SECTION_MAX: usize = 64 * 1024;
/// The sections one Doc has.
pub const SECTIONS_MAX: usize = 40;
/// A title or a heading, in characters.
const HEADING_MAX: usize = 200;
/// A section's intent, in characters.
const INTENT_MAX: usize = 500;

const OUTLINE: &str = "doc.json";

pub struct DocKind;

impl BlockKind for DocKind {
    fn create(&self, ctx: BlockCtx, config: Value) -> Result<Arc<dyn Block>, String> {
        let config: Config = serde_json::from_value(config).map_err(|e| format!("doc config: {e}"))?;
        let id = match config.doc {
            Some(id) => id,
            // Only a layout from before the library has none, and the
            // start-up migration gives it one: if it couldn't, the files
            // are where they were, and the block says so.
            None if ctx.restoring => {
                return Err("this Doc hasn't been moved into the Doc library yet (see the daemon's log); \
                            its files are kept, and the next start tries again"
                    .into());
            }
            // Opened with a title alone (the generic `POST /api/blocks`):
            // its Doc is made now.
            None => library::make(&ctx.state_dir, &config.title, "unknown")?.id,
        };
        let dir = library::dir(&ctx.state_dir, &id)?;
        if !dir.is_dir() {
            return Err(format!("no Doc {id} in the library (docs list has them)"));
        }
        let mut link = Link::none(ctx.state_dir.clone(), id);
        link.remote = link::remote();
        let own = ctx.clone();
        // The Doc's files are read back whenever it is opened, not only on
        // a restart.
        let doc = Doc::with_link(dir, config.title, true, link, Arc::new(move || ctx.changed()));
        doc.attach(own, config.share_ask);
        Ok(doc)
    }

    // No `stored_text`: the library is searched on its own
    // ([`library::search`]), once per Doc however many blocks have shown it.
}

#[derive(Debug, Default, Deserialize)]
struct Config {
    /// The Doc in the library this block shows.
    #[serde(default)]
    doc: Option<String>,
    #[serde(default)]
    title: String,
    /// #1067: an agent's ask to share, kept across a restart.
    #[serde(default)]
    share_ask: Option<ask::Asked>,
}

/// `doc.json`.
#[derive(Debug, Serialize, Deserialize)]
struct Outline {
    title: String,
    sections: Vec<Section>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Section {
    id: String,
    heading: String,
    #[serde(default)]
    intent: String,
}

/// What `doc_outline` is given for each section.
#[derive(Debug, Deserialize)]
struct Planned {
    heading: String,
    #[serde(default)]
    intent: String,
}

#[derive(Debug, Clone, Serialize)]
struct Entry {
    id: String,
    heading: String,
    intent: String,
    /// `None` while pending.
    body: Option<String>,
    /// Bumped when the body is written, so clients can mark a fill (a
    /// refill too).
    rev: u64,
}

#[derive(Debug, Default)]
struct State {
    title: String,
    entries: Vec<Entry>,
    /// The pictures kept in `assets/`, by name.
    assets: Vec<Asset>,
    rev: u64,
    /// When it was shared with a link and last updated; the link is not here.
    shared: Option<SharedAt>,
    /// The folder couldn't be read back: the block shows this and writes
    /// nothing.
    error: Option<String>,
}

/// What everyone who sees the Doc may know of its link.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
struct SharedAt {
    made: u64,
    updated: u64,
}

/// The Doc as the folder holds it.
struct Loaded {
    title: String,
    entries: Vec<Entry>,
}

type Changed = Arc<dyn Fn() + Send + Sync>;

pub struct Doc {
    me: Weak<Doc>,
    dir: PathBuf,
    changed: Changed,
    state: Mutex<State>,
    /// Orders the writes: a fill reads the state, writes its file and then
    /// stores the body, and none of that may interleave with another's.
    io: tokio::sync::Mutex<()>,
    /// Each entry's fingerprint as clients were last sent it.
    sent: Mutex<Vec<u64>>,
    /// Where this Doc's link is kept, and control.
    link: Link,
    /// One share, update or unshare at a time.
    shares: tokio::sync::Mutex<()>,
    /// An agent's ask to share, and its card (#1067).
    card: ask::Card,
}

impl Doc {
    #[cfg(test)]
    pub fn new(dir: PathBuf, title: String, restoring: bool, changed: Changed) -> Arc<Doc> {
        Doc::with_link(dir, title, restoring, Link::none(PathBuf::new(), String::new()), changed)
    }

    pub fn with_link(dir: PathBuf, title: String, restoring: bool, link: Link, changed: Changed) -> Arc<Doc> {
        let mut state = State { title: title.trim().to_owned(), ..Default::default() };
        if restoring && dir.join(OUTLINE).exists() {
            match load(&dir) {
                Ok(l) => (state.title, state.entries) = (l.title, l.entries),
                Err(e) => state.error = Some(e),
            }
        }
        if restoring {
            state.assets = assets::load(&dir.join(assets::FOLDER));
            state.shared = link.current().map(|(_, e)| SharedAt { made: e.made, updated: e.updated });
        }
        Arc::new_cyclic(|me| Doc {
            me: me.clone(),
            dir,
            changed,
            state: Mutex::new(state),
            io: tokio::sync::Mutex::new(()),
            sent: Mutex::new(Vec::new()),
            link,
            shares: tokio::sync::Mutex::new(()),
            card: Default::default(),
        })
    }

    /// The Doc changed on disk: its `meta.json` says when (and its title).
    async fn touched(&self, title: Option<String>) {
        let dir = self.dir.clone();
        let _ = crate::task::spawn_blocking(move || library::touch(&dir, title.as_deref())).await;
    }

    /// `doc_outline`: the sections, once.
    async fn outline(&self, title: &str, sections: Vec<Planned>) -> Result<Value, String> {
        let _io = self.io.lock().await;
        let title = title.trim();
        if title.is_empty() {
            return Err("a Doc needs a title".into());
        }
        if title.chars().count() > HEADING_MAX {
            return Err(format!("the title is over {HEADING_MAX} characters: shorten it"));
        }
        if sections.is_empty() {
            return Err("a Doc needs at least one section".into());
        }
        if sections.len() > SECTIONS_MAX {
            return Err(format!("a Doc has at most {SECTIONS_MAX} sections, not {}", sections.len()));
        }
        let mut planned = Vec::new();
        for (i, s) in sections.into_iter().enumerate() {
            let n = i + 1;
            let (heading, intent) = (s.heading.trim(), s.intent.trim());
            if heading.is_empty() {
                return Err(format!("section {n} needs a heading"));
            }
            if heading.chars().count() > HEADING_MAX {
                return Err(format!("section {n}'s heading is over {HEADING_MAX} characters: shorten it"));
            }
            if intent.chars().count() > INTENT_MAX {
                return Err(format!("section {n}'s intent is over {INTENT_MAX} characters: shorten it"));
            }
            planned.push(Section { id: format!("s{n}"), heading: heading.to_owned(), intent: intent.to_owned() });
        }
        {
            let st = self.state.lock().unwrap();
            if let Some(e) = &st.error {
                return Err(e.clone());
            }
            if !st.entries.is_empty() {
                return Err("this Doc already has its outline: fill its sections".into());
            }
        }
        let outline = Outline { title: title.to_owned(), sections: planned.clone() };
        let dir = self.dir.clone();
        let bytes = serde_json::to_vec_pretty(&outline).map_err(|e| e.to_string())?;
        write(dir, OUTLINE, bytes).await?;
        self.touched(Some(outline.title.clone())).await;
        let ids: Vec<Value> = planned.iter().map(|s| json!({ "id": s.id, "heading": s.heading })).collect();
        {
            let mut st = self.state.lock().unwrap();
            st.title = outline.title;
            st.entries = planned
                .into_iter()
                .map(|s| Entry { id: s.id, heading: s.heading, intent: s.intent, body: None, rev: 0 })
                .collect();
            st.rev += 1;
        }
        (self.changed)();
        Ok(json!({ "sections": ids }))
    }

    /// `doc_fill`: one section's body.
    async fn fill(&self, section: &str, md: &str) -> Result<Value, String> {
        let _io = self.io.lock().await;
        {
            let st = self.state.lock().unwrap();
            if let Some(e) = &st.error {
                return Err(e.clone());
            }
            if st.entries.is_empty() {
                return Err("this Doc has no outline yet: send doc_outline first".into());
            }
            let Some(at) = st.entries.iter().position(|e| e.id == section) else {
                let ids: Vec<&str> = st.entries.iter().map(|e| e.id.as_str()).collect();
                return Err(format!("this Doc has no section {section:?}; it has {}", ids.join(", ")));
            };
            if md.trim().is_empty() {
                return Err(format!("section {section} is empty: give its Markdown"));
            }
            // Before the first write: a bad fence leaves the section, on
            // disk and in the state, exactly as it was.
            embeds::check(md, &st.assets).map_err(|e| format!("section {section}: {e}; the section is unchanged"))?;
            if md.len() > SECTION_MAX {
                return Err(format!(
                    "section {section} is {} bytes; the cap is {SECTION_MAX} (64 KB): split it or shorten it",
                    md.len()
                ));
            }
            let others: usize = st
                .entries
                .iter()
                .enumerate()
                .filter(|(i, _)| *i != at)
                .map(|(_, e)| e.body.as_ref().map_or(0, String::len))
                .sum();
            if (others + md.len()) as u64 > READ_MAX {
                return Err(format!(
                    "the Doc would be {} bytes; the cap is {READ_MAX} (1 MiB): shorten a section",
                    others + md.len()
                ));
            }
        }
        write(self.dir.clone(), &format!("{section}.md"), md.as_bytes().to_vec()).await?;
        self.touched(None).await;
        {
            let mut st = self.state.lock().unwrap();
            let Some(e) = st.entries.iter_mut().find(|e| e.id == section) else { return Err("the Doc changed".into()) };
            e.body = Some(md.to_owned());
            e.rev += 1;
            st.rev += 1;
        }
        (self.changed)();
        Ok(json!({ "section": section, "bytes": md.len() }))
    }

    /// `doc_asset_put`: a picture, by name; a name already kept is replaced.
    async fn asset_put(&self, name: &str, data: &str) -> Result<Value, String> {
        assets::check_name(name)?;
        // The base64 of the cap, and a little over, before it is decoded.
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
            if let Some(e) = &st.error {
                return Err(e.clone());
            }
            // One file on a filesystem that ignores case is one name.
            if let Some(a) = st.assets.iter().find(|a| a.name != name && a.name.eq_ignore_ascii_case(name)) {
                return Err(format!("the Doc has {} already: use that name to replace it", a.name));
            }
            if st.assets.len() >= ASSETS_MAX && !st.assets.iter().any(|a| a.name == name) {
                return Err(format!("a Doc has at most {ASSETS_MAX} assets: replace one by its name"));
            }
        }
        let size = bytes.len() as u64;
        write(self.dir.join(assets::FOLDER), name, bytes).await?;
        self.touched(None).await;
        let rev = {
            let mut st = self.state.lock().unwrap();
            let old = st.assets.iter().position(|a| a.name == name);
            let rev = now_ms().max(old.map_or(0, |i| st.assets[i].rev + 1));
            let asset = Asset { name: name.to_owned(), mime, size, rev };
            match old {
                Some(i) => st.assets[i] = asset,
                None => {
                    st.assets.push(asset);
                    st.assets.sort_by(|a, b| a.name.cmp(&b.name));
                }
            }
            st.rev += 1;
            rev
        };
        (self.changed)();
        Ok(json!({ "name": name, "path": format!("assets/{name}"), "mime": mime, "size": size, "rev": rev }))
    }

    /// `doc_asset`: a kept picture, for a client to show. The type is what
    /// the bytes say.
    async fn asset(&self, name: &str) -> Result<Value, String> {
        use base64::Engine;
        if !self.state.lock().unwrap().assets.iter().any(|a| a.name == name) {
            return Err(format!("this Doc has no asset {name:?}"));
        }
        let path = self.dir.join(assets::FOLDER).join(name);
        let bytes = tokio::fs::read(&path).await.map_err(|_| format!("this Doc has no asset {name:?}"))?;
        let mime = crate::agent::images::sniff(&bytes).ok_or_else(|| format!("{name} isn't an image"))?;
        Ok(json!({ "mime": mime, "data": base64::engine::general_purpose::STANDARD.encode(bytes) }))
    }

    /// The snapshot to share: the Doc as it is now, read under the write
    /// lock so a fill can't land halfway.
    async fn snapshot(&self) -> Result<(String, Vec<u8>), String> {
        let _io = self.io.lock().await;
        let (title, entries, kept) = {
            let st = self.state.lock().unwrap();
            if let Some(e) = &st.error {
                return Err(e.clone());
            }
            if st.entries.is_empty() {
                return Err("this Doc has no outline yet: there is nothing to share".into());
            }
            (st.title.clone(), st.entries.clone(), st.assets.clone())
        };
        let mut blobs = Vec::new();
        let mut assets = Vec::new();
        for a in &kept {
            let bytes = tokio::fs::read(self.dir.join(assets::FOLDER).join(&a.name))
                .await
                .map_err(|e| format!("can't read the picture {}: {e}", a.name))?;
            assets.push(link::ManifestAsset {
                name: a.name.clone(),
                mime: a.mime.to_owned(),
                size: bytes.len() as u64,
            });
            blobs.push(bytes);
        }
        let manifest = link::Manifest {
            title: title.clone(),
            entries: entries
                .into_iter()
                .map(|e| link::ManifestEntry { id: e.id, heading: e.heading, intent: e.intent, body: e.body })
                .collect(),
            assets,
        };
        Ok((title, link::snapshot(&manifest, &blobs)))
    }

    /// Say the Doc is shared (or isn't), and tell clients.
    fn set_shared(&self, shared: Option<SharedAt>) {
        {
            let mut st = self.state.lock().unwrap();
            if st.shared == shared {
                return;
            }
            st.shared = shared;
            st.rev += 1;
        }
        (self.changed)();
    }

    /// `doc_share` and `doc_share_update`: the URL.
    async fn share(&self, update: bool) -> Result<Value, String> {
        let _one = self.shares.lock().await;
        self.share_held(update).await
    }

    /// ...with the share lock held (the card's Share holds it to settle).
    async fn share_held(&self, update: bool) -> Result<Value, String> {
        let (title, plain) = self.snapshot().await?;
        let got = if update { self.link.update(&title, plain).await } else { self.link.share(&title, plain).await };
        match got {
            Ok(s) => {
                self.set_shared(Some(SharedAt { made: s.made, updated: s.updated }));
                Ok(json!({ "url": s.url }))
            }
            Err(Fail::Gone) => {
                self.set_shared(None);
                Err(link::UNSHARED_ELSEWHERE.into())
            }
            Err(Fail::Said(e)) => Err(e),
        }
    }

    /// `doc_shares`: this Doc's URL again, or that it has none (and whether
    /// it could have).
    async fn shares(&self) -> Result<Value, String> {
        let _one = self.shares.lock().await;
        let got = match self.link.shares().await {
            Ok(Some(s)) => {
                self.set_shared(Some(SharedAt { made: s.made, updated: s.updated }));
                return Ok(json!({ "url": s.url, "made": s.made, "updated": s.updated }));
            }
            Ok(None) | Err(Fail::Gone) => self.link.available().await,
            Err(Fail::Said(e)) => return Err(e),
        };
        self.set_shared(None);
        Ok(match got {
            Ok(()) => json!({ "shared": null, "can": true }),
            Err(why) => json!({ "shared": null, "can": false, "why": why }),
        })
    }

    /// `doc_unshare`: with an `id`, only if that is this Doc's link.
    async fn unshare(&self, id: Option<String>) -> Result<Value, String> {
        let _one = self.shares.lock().await;
        if let Some(id) = &id
            && self.link.current().is_none_or(|(mine, _)| mine != *id)
        {
            return Ok(json!({ "other": true }));
        }
        self.link.unshare(None).await?;
        self.set_shared(None);
        Ok(json!({}))
    }

    fn state_json(&self) -> Value {
        let st = self.state.lock().unwrap();
        let mut v = json!({
            "title": st.title, "entries": st.entries, "entries_from": 0, "assets": st.assets, "rev": st.rev,
            "shared": st.shared,
        });
        if let Some(e) = &st.error {
            v["error"] = json!(e);
        }
        v
    }
}

/// Write `name` in the Doc's folder, off the async threads.
async fn write(dir: PathBuf, name: &str, bytes: Vec<u8>) -> Result<(), String> {
    let name = name.to_owned();
    crate::task::spawn_blocking(move || {
        private_dir(&dir).map_err(|e| format!("can't make the Doc's folder: {e}"))?;
        write_atomic(&dir.join(&name), &bytes).map_err(|e| format!("can't write {name}: {e}"))
    })
    .await
    .map_err(|e| e.to_string())?
}

/// An id `doc_outline` made. Read back from `doc.json` only if it is one,
/// so no file name comes from anything else.
fn is_id(id: &str) -> bool {
    id.strip_prefix('s').is_some_and(|n| !n.is_empty() && n.len() <= 3 && n.bytes().all(|b| b.is_ascii_digit()))
}

/// The folder read back. An error says what to tell the person.
fn load(dir: &Path) -> Result<Loaded, String> {
    let file = dir.join(OUTLINE);
    let text = std::fs::read_to_string(&file).map_err(|e| format!("can't read {}: {e}", file.display()))?;
    let outline: Outline = serde_json::from_str(&text)
        .map_err(|e| format!("{} doesn't parse ({e}); it is left as it is: tell the person", file.display()))?;
    let mut entries = Vec::new();
    for s in outline.sections {
        if !is_id(&s.id) || entries.iter().any(|e: &Entry| e.id == s.id) {
            return Err(format!("{} names a section {:?} that isn't one; it is left as it is", file.display(), s.id));
        }
        let path = dir.join(format!("{}.md", s.id));
        let body = match std::fs::read_to_string(&path) {
            Ok(b) => Some(b),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
            Err(e) => return Err(format!("can't read {}: {e}", path.display())),
        };
        let rev = u64::from(body.is_some());
        entries.push(Entry { id: s.id, heading: s.heading, intent: s.intent, body, rev });
    }
    Ok(Loaded { title: outline.title, entries })
}

/// The Doc as Markdown: a pending section is its heading and what it
/// will hold, a chart one line and a drawing `[drawing]`.
fn markdown(title: &str, entries: &[Entry]) -> String {
    let mut out = format!("# {title}\n");
    for e in entries {
        out.push_str(&format!("\n## {}\n\n", e.heading));
        match &e.body {
            Some(b) => {
                out.push_str(embeds::plain(b).trim_end());
                out.push('\n');
            }
            None if e.intent.is_empty() => out.push_str("_(coming)_\n"),
            None => out.push_str(&format!("_(coming: {})_\n", e.intent)),
        }
    }
    out
}

/// An entry's fingerprint, to tell what changed since clients were sent it.
fn hash(s: &str) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    s.hash(&mut h);
    h.finish()
}

impl Block for Doc {
    fn kind(&self) -> BlockType {
        BlockType::Doc
    }

    fn config(&self) -> Value {
        let mut v = json!({ "title": self.state.lock().unwrap().title });
        // A Doc made without the library (a test's) has no id.
        if !self.link.doc.is_empty() {
            v["doc"] = json!(self.link.doc);
        }
        if let Some(asked) = self.asked_json() {
            v["share_ask"] = asked;
        }
        v
    }

    fn state(&self) -> Value {
        self.state_json()
    }

    /// The first entry whose fingerprint differs from what clients were
    /// last sent (the Doc's sections are all its `entries`, `entries_from`
    /// 0): a patch is that entry and every one after it.
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
        match &st.error {
            Some(e) => format!("{e}\n"),
            None => markdown(&st.title, &st.entries),
        }
    }

    fn call(&self, method: &str, args: Value) -> BoxFuture<'static, Result<Value, String>> {
        self.call_by(method, args, None)
    }

    fn call_by(&self, method: &str, args: Value, by: Option<&str>) -> BoxFuture<'static, Result<Value, String>> {
        let me = self.me.upgrade();
        match method {
            // #1067: an agent's ask. Not in `link::METHODS`: it raises a
            // card and reads one, and uploads nothing.
            "doc_share_ask" => {
                let by = by.map(str::to_owned);
                Box::pin(async move { me.ok_or("closed")?.share_ask(by).await })
            }
            "doc_outline" => {
                let title = args["title"].as_str().unwrap_or_default().to_owned();
                let sections = serde_json::from_value::<Vec<Planned>>(args["sections"].clone())
                    .map_err(|e| format!("doc_outline needs {{title, sections: [{{heading, intent}}]}}: {e}"));
                Box::pin(async move {
                    let me = me.ok_or("closed")?;
                    me.outline(&title, sections?).await
                })
            }
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
            "doc_share" | "doc_share_update" => {
                let update = method == "doc_share_update";
                Box::pin(async move { me.ok_or("closed")?.share(update).await })
            }
            "doc_shares" => Box::pin(async move { me.ok_or("closed")?.shares().await }),
            "doc_unshare" => {
                let id = args["id"].as_str().map(str::to_owned);
                Box::pin(async move { me.ok_or("closed")?.unshare(id).await })
            }
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

    fn close(&self) {
        self.drop_card();
    }

    fn summary(&self) -> Summary {
        let title = self.state.lock().unwrap().title.clone();
        Summary { title: Some(title).filter(|t| !t.is_empty()), ..Summary::default() }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};

    use super::*;

    /// Where a test's Doc keeps its files under its temp dir.
    const FOLDER: &str = "doc";

    fn temp(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("arugula-doc-{name}-{}-{}", std::process::id(), crate::store::now_ms()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn doc(dir: &Path, restoring: bool) -> (Arc<Doc>, Arc<AtomicUsize>) {
        let pushes = Arc::new(AtomicUsize::new(0));
        let n = pushes.clone();
        let made = Doc::new(
            dir.join(FOLDER),
            "Plan".into(),
            restoring,
            Arc::new(move || {
                n.fetch_add(1, Ordering::SeqCst);
            }),
        );
        (made, pushes)
    }

    fn plan(n: usize) -> Value {
        json!({ "title": "Plan", "sections": (1..=n).map(|i| json!({ "heading": format!("H{i}"), "intent": format!("what {i} holds") })).collect::<Vec<_>>() })
    }

    async fn outline(d: &Doc, n: usize) -> Result<Value, String> {
        d.call("doc_outline", plan(n)).await
    }

    async fn fill(d: &Doc, id: &str, md: &str) -> Result<Value, String> {
        d.call("doc_fill", json!({ "section": id, "markdown": md })).await
    }

    fn entries(d: &Doc) -> Vec<Value> {
        d.state()["entries"].as_array().unwrap().clone()
    }

    #[tokio::test]
    async fn fills_out_of_order_and_in_parallel_all_land() {
        let dir = temp("parallel");
        let (d, pushes) = doc(&dir, false);
        let made = outline(&d, 6).await.unwrap();
        assert_eq!(made["sections"][5]["id"], "s6");
        // Out of order, then at once.
        fill(&d, "s3", "three").await.unwrap();
        let ids = ["s1", "s2", "s4", "s5", "s6"];
        let bodies: Vec<String> = ids.iter().map(|id| format!("body {id}")).collect();
        let fills: Vec<_> = ids.iter().zip(&bodies).map(|(id, b)| fill(&d, id, b)).collect();
        for r in futures_util::future::join_all(fills).await {
            r.unwrap();
        }
        assert!(entries(&d).iter().all(|e| e["body"].is_string()));
        assert_eq!(pushes.load(Ordering::SeqCst), 7, "one push for the outline and one per fill");
        // doc.json is intact and the sections are on disk.
        let (again, _) = doc(&dir, true);
        let st = again.state();
        assert_eq!(st["title"], "Plan");
        assert_eq!(st["entries"][2]["body"], "three");
        assert_eq!(st["entries"][5]["body"], "body s6");
        assert_eq!(st["entries"][0]["intent"], "what 1 holds");
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[tokio::test]
    async fn every_cap_is_refused_in_one_line() {
        let dir = temp("caps");
        let (d, pushes) = doc(&dir, false);
        for (args, says) in [
            (json!({ "title": " ", "sections": [{ "heading": "A" }] }), "needs a title"),
            (json!({ "title": "T", "sections": [] }), "at least one section"),
            (
                json!({ "title": "T", "sections": (0..41).map(|_| json!({ "heading": "A" })).collect::<Vec<_>>() }),
                "at most 40",
            ),
            (json!({ "title": "T", "sections": [{ "heading": " " }] }), "section 1 needs a heading"),
            (
                json!({ "title": "T", "sections": [{ "heading": "A", "intent": "x".repeat(501) }] }),
                "intent is over 500",
            ),
            (json!({ "title": "T", "sections": [{ "heading": "A".repeat(201) }] }), "heading is over 200"),
            (json!({ "title": "T", "sections": "nope" }), "doc_outline needs"),
        ] {
            let e = d.call("doc_outline", args).await.unwrap_err();
            assert!(e.contains(says) && !e.contains('\n'), "{e}");
        }
        assert!(d.state()["entries"].as_array().unwrap().is_empty());
        assert!(fill(&d, "s1", "x").await.unwrap_err().contains("no outline yet"));
        outline(&d, 20).await.unwrap();
        let pushed = pushes.load(Ordering::SeqCst);
        // An empty body, a body over 64 KB, an unknown section.
        assert!(fill(&d, "s1", "  \n").await.unwrap_err().contains("empty"));
        let e = fill(&d, "s1", &"x".repeat(SECTION_MAX + 1)).await.unwrap_err();
        assert!(e.contains("65537 bytes") && e.contains("64 KB"), "{e}");
        let e = fill(&d, "s21", "x").await.unwrap_err();
        assert!(e.contains("s1, s2, s3") && e.contains("s20"), "{e}");
        // Exactly 64 KB is fine; the Doc as a whole stops at 1 MiB.
        for i in 1..=16 {
            fill(&d, &format!("s{i}"), &"x".repeat(SECTION_MAX)).await.unwrap();
        }
        let e = fill(&d, "s17", "x").await.unwrap_err();
        assert!(e.contains("1 MiB") && !e.contains('\n'), "{e}");
        // Refilling a section within the cap still works.
        fill(&d, "s1", "short").await.unwrap();
        fill(&d, "s17", "x").await.unwrap();
        assert_eq!(pushes.load(Ordering::SeqCst), pushed + 18, "a refusal pushes nothing");
        // The outline is once.
        assert!(outline(&d, 3).await.unwrap_err().contains("already has its outline"));
        std::fs::remove_dir_all(dir).unwrap();
    }

    fn example(name: &str) -> String {
        std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/docs").join(name)).unwrap()
    }

    /// A bad chart or drawing is refused before the first write: the
    /// section is as it was on disk and in the state, filled or pending,
    /// and nothing is pushed.
    #[tokio::test]
    async fn a_bad_fence_leaves_the_section_as_it_was() {
        let dir = temp("fences");
        let (d, pushes) = doc(&dir, false);
        outline(&d, 2).await.unwrap();
        fill(&d, "s1", &example("bar-chart.md")).await.unwrap();
        let (state, pushed) = (entries(&d), pushes.load(Ordering::SeqCst));
        let folder = dir.join(FOLDER);
        let files = || {
            let mut names: Vec<_> = std::fs::read_dir(&folder).unwrap().map(|e| e.unwrap().file_name()).collect();
            names.sort();
            names
        };
        let on_disk = (files(), std::fs::read_to_string(folder.join("s1.md")).unwrap());
        for name in ["bad/unordered-line.md", "bad/bad-json.md", "bad/svg-no-root.md", "bad/no-rows.md"] {
            for section in ["s1", "s2"] {
                let e = fill(&d, section, &example(name)).await.unwrap_err();
                assert!(e.starts_with(&format!("section {section}: ")) && !e.contains('\n'), "{name}: {e}");
            }
        }
        // Past the checks of the body as a whole too: a good chart then a bad one.
        let both = format!("{}\n{}", example("line-chart.md"), example("bad/string-value.md"));
        assert!(fill(&d, "s2", &both).await.unwrap_err().contains("must be a number"));
        assert_eq!(entries(&d), state);
        assert_eq!(pushes.load(Ordering::SeqCst), pushed, "a refusal pushes nothing");
        assert_eq!((files(), std::fs::read_to_string(folder.join("s1.md")).unwrap()), on_disk);
        // A drawing over the cap is refused on its own account.
        let big = format!("```svg\n<svg xmlns=\"http://www.w3.org/2000/svg\">{}</svg>\n```", "x".repeat(SECTION_MAX));
        assert!(fill(&d, "s2", &big).await.unwrap_err().contains("svg: "));
        // A drawing with a script in it is stored as written.
        let script = example("script-drawing.md");
        fill(&d, "s2", &script).await.unwrap();
        assert_eq!(std::fs::read_to_string(folder.join("s2.md")).unwrap(), script);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[tokio::test]
    async fn ids_are_the_outlines_and_never_reused() {
        let dir = temp("ids");
        let (d, _) = doc(&dir, false);
        let made = outline(&d, 3).await.unwrap();
        let ids: Vec<&str> = made["sections"].as_array().unwrap().iter().map(|s| s["id"].as_str().unwrap()).collect();
        assert_eq!(ids, ["s1", "s2", "s3"]);
        // A second outline can't renumber them; a refill keeps the id.
        assert!(outline(&d, 5).await.is_err());
        fill(&d, "s2", "a").await.unwrap();
        fill(&d, "s2", "b").await.unwrap();
        let ids: Vec<String> = entries(&d).iter().map(|e| e["id"].as_str().unwrap().to_owned()).collect();
        assert_eq!(ids, ["s1", "s2", "s3"]);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[tokio::test]
    async fn an_unparsable_outline_is_reported_and_not_rewritten() {
        let dir = temp("bad");
        let folder = dir.join(FOLDER);
        std::fs::create_dir_all(&folder).unwrap();
        std::fs::write(folder.join(OUTLINE), "{ not json").unwrap();
        let (d, _) = doc(&dir, true);
        let st = d.state();
        assert!(st["error"].as_str().unwrap().contains("doesn't parse"), "{st}");
        assert!(d.text().contains("doesn't parse"));
        assert!(outline(&d, 2).await.unwrap_err().contains("doesn't parse"));
        assert!(fill(&d, "s1", "x").await.unwrap_err().contains("doesn't parse"));
        assert_eq!(std::fs::read_to_string(folder.join(OUTLINE)).unwrap(), "{ not json");
        assert!(!folder.join("s1.md").exists());
        // A section id that would leave the folder is no outline either.
        std::fs::write(folder.join(OUTLINE), r#"{"title":"T","sections":[{"id":"../x","heading":"H"}]}"#).unwrap();
        let (d, _) = doc(&dir, true);
        assert!(d.state()["error"].as_str().unwrap().contains("isn't one"));
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[tokio::test]
    async fn a_stray_tmp_is_ignored_on_restore() {
        let dir = temp("tmp");
        let (d, _) = doc(&dir, false);
        outline(&d, 2).await.unwrap();
        fill(&d, "s1", "good").await.unwrap();
        // What a crash mid-write leaves: beside a good file, and for a
        // section that was never filled.
        let folder = dir.join(FOLDER);
        std::fs::write(folder.join("s1.tmp"), "half a fil").unwrap();
        std::fs::write(folder.join("s2.tmp"), "half").unwrap();
        let (again, _) = doc(&dir, true);
        let st = again.state();
        assert!(st.get("error").is_none(), "{st}");
        assert_eq!(st["entries"][0]["body"], "good");
        assert!(st["entries"][1]["body"].is_null());
        std::fs::remove_dir_all(dir).unwrap();
    }

    /// What a client already holding the previous state is sent.
    fn sent(d: &Doc) -> (u64, Value) {
        let (state, at) = d.state_and_changed().unwrap();
        (at, arugula_proto::block_patch::to_patch(&state, at))
    }

    #[tokio::test]
    async fn a_patch_starts_at_the_first_entry_that_changed() {
        let dir = temp("patch");
        let (d, _) = doc(&dir, false);
        outline(&d, 6).await.unwrap();
        assert_eq!(sent(&d).0, 0, "the first state is all new");
        assert_eq!(sent(&d).0, 6, "nothing changed");
        let body = "word ".repeat(2000);
        for (i, id) in ["s1", "s2", "s3", "s4"].iter().enumerate() {
            fill(&d, id, &body).await.unwrap();
            let (at, patch) = sent(&d);
            // The next pending section in order: the patch is it and the
            // pending ones after it.
            assert_eq!(at, i as u64, "{id}");
            assert!(patch.to_string().len() <= 2 * body.len(), "{id}: {} bytes", patch.to_string().len());
            assert_eq!(patch["entries"].as_array().unwrap().len(), 6 - i);
            assert_eq!(patch["entries_at"], i);
        }
        // An early refill sends the filled ones after it too.
        fill(&d, "s2", "changed").await.unwrap();
        let (at, patch) = sent(&d);
        assert_eq!(at, 1);
        assert_eq!(patch["entries"].as_array().unwrap().len(), 5);
        // A refill of the last filled one is itself and the pending.
        fill(&d, "s4", "again").await.unwrap();
        assert_eq!(sent(&d).0, 3);
        // s1 refilled with later ones filled: everything.
        fill(&d, "s1", "first").await.unwrap();
        assert_eq!(sent(&d).0, 0);
        // Two fills land before the next send (the notices coalesce): the
        // patch starts at the earlier one, or a client would miss it.
        fill(&d, "s5", "five").await.unwrap();
        fill(&d, "s2", "two, again").await.unwrap();
        let (at, patch) = sent(&d);
        assert_eq!(at, 1);
        assert_eq!(patch["entries"][0]["body"], "two, again", "{patch}");
        assert_eq!(patch["entries"].as_array().unwrap().len(), 5);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[tokio::test]
    async fn text_is_the_doc_as_markdown() {
        let dir = temp("text");
        let (d, _) = doc(&dir, false);
        outline(&d, 2).await.unwrap();
        fill(&d, "s1", "Body one.\n").await.unwrap();
        let want = "# Plan\n\n## H1\n\nBody one.\n\n## H2\n\n_(coming: what 2 holds)_\n";
        assert_eq!(d.text(), want);
        std::fs::remove_dir_all(dir).unwrap();
    }

    /// A block's context in `state_dir`, as the multiplexer makes one.
    fn ctx(state_dir: &Path, restoring: bool) -> (BlockCtx, tokio::sync::mpsc::UnboundedReceiver<crate::pane::Notice>) {
        let (ctx, seen, _) = ctx_with_cmds(state_dir, restoring);
        (ctx, seen)
    }

    /// ...and the commands it sends the multiplexer (a block's asks).
    fn ctx_with_cmds(
        state_dir: &Path,
        restoring: bool,
    ) -> (
        BlockCtx,
        tokio::sync::mpsc::UnboundedReceiver<crate::pane::Notice>,
        tokio::sync::mpsc::UnboundedReceiver<crate::mux::Cmd>,
    ) {
        let (notices, seen) = tokio::sync::mpsc::unbounded_channel();
        let (cmds, sent) = tokio::sync::mpsc::unbounded_channel();
        let home = state_dir.to_path_buf();
        let base = crate::block::BlockEnv {
            notices,
            provider: None,
            launch: crate::pane::Launcher {
                exe: "arugulad".into(),
                scopes: false,
                no_expand: false,
                service: None,
                fd_store: false,
                hold: false,
                #[cfg(windows)]
                host: "arugulad.exe".into(),
            },
            env: vec![],
            home: home.clone(),
            fs: Arc::new(crate::fs::Scope::new(home.clone(), vec![])),
            shell_env: crate::shellenv::ShellEnv::new(
                "sh".into(),
                vec![],
                home,
                vec![],
                std::time::Duration::from_secs(1),
            ),
            cmds: Some(cmds),
            ids: Default::default(),
            state_dir: state_dir.to_path_buf(),
        };
        let dir = state_dir.join("blocks/7");
        (BlockCtx::new(7, dir, base, None, restoring, Default::default(), Default::default()), seen, sent)
    }

    /// Made anywhere (native #204: Docs left Labs), and brought back.
    #[tokio::test]
    async fn the_kind_is_made_and_restored() {
        let state = temp("kind");
        let config = json!({ "title": "Report" });
        let kept = library::list(&state).len();
        let (new, _seen) = ctx(&state, false);
        let made = DocKind.create(new, config.clone()).unwrap();
        assert_eq!(made.kind(), BlockType::Doc);
        // Opened with a title alone, its Doc is made in the library now.
        let doc = made.config()["doc"].as_str().unwrap().to_owned();
        let listed = library::list(&state);
        assert_eq!(listed.len(), kept + 1);
        assert!(listed.iter().any(|m| m.id == doc));
        let (back, _seen) = ctx(&state, true);
        let back = DocKind.create(back, json!({ "doc": doc, "title": "Report" })).unwrap();
        assert_eq!(back.kind(), BlockType::Doc, "restored");
        assert_eq!(back.config()["doc"], doc.as_str());
        // A saved block with no Doc is from before the library: the
        // migration gives it one, and until it has, the block says so.
        let (old, _seen) = ctx(&state, true);
        let e = DocKind.create(old, config).err().expect("no Doc to show");
        assert!(e.contains("hasn't been moved"), "{e}");
        std::fs::remove_dir_all(state).unwrap();
    }

    /// #1067: an ask restored within its day puts its card up; one past its
    /// day does not (a card shown and taken down at once would still notify
    /// the owner), and is left for `expire` to lapse.
    #[tokio::test]
    async fn a_restored_ask_is_raised_only_within_its_day() {
        let state = temp("restore-ask");
        let asks = |mut sent: tokio::sync::mpsc::UnboundedReceiver<crate::mux::Cmd>| {
            let mut n = 0;
            while let Ok(c) = sent.try_recv() {
                n += usize::from(matches!(c, crate::mux::Cmd::Api(crate::mux::Api::Ask(..))));
            }
            n
        };
        let id = library::make(&state, "Plan", "test").unwrap().id;
        for (at_ms, raised) in [(now_ms(), 1), (1, 0)] {
            let (ctx, _seen, sent) = ctx_with_cmds(&state, true);
            let config =
                json!({ "doc": id, "title": "Plan", "share_ask": { "at_ms": at_ms, "by": "mcp:claude-code" } });
            let doc = DocKind.create(ctx, config).unwrap();
            tokio::time::sleep(std::time::Duration::from_millis(400)).await;
            assert_eq!(asks(sent), raised, "an ask made at {at_ms}");
            if raised == 0 {
                // ...and it lapsed: the config says so.
                assert_eq!(doc.config()["share_ask"]["outcome"], "lapsed", "{}", doc.config());
            }
        }
        std::fs::remove_dir_all(state).unwrap();
    }

    /// What a PNG starts with: the daemon tells a file by its first bytes.
    fn png(extra: &str) -> Vec<u8> {
        [b"\x89PNG\r\n\x1a\n".as_slice(), extra.as_bytes()].concat()
    }

    fn b64(bytes: &[u8]) -> String {
        use base64::Engine;
        base64::engine::general_purpose::STANDARD.encode(bytes)
    }

    async fn put(d: &Doc, name: &str, bytes: &[u8]) -> Result<Value, String> {
        d.call("doc_asset_put", json!({ "name": name, "data": b64(bytes) })).await
    }

    fn assets(d: &Doc) -> Vec<Value> {
        d.state()["assets"].as_array().unwrap().clone()
    }

    fn files(folder: &Path) -> Vec<String> {
        let mut names: Vec<String> =
            std::fs::read_dir(folder).unwrap().map(|e| e.unwrap().file_name().into_string().unwrap()).collect();
        names.sort();
        names
    }

    #[tokio::test]
    async fn a_picture_is_kept_by_name_and_a_replaced_name_has_a_new_rev() {
        let dir = temp("asset");
        let (d, pushes) = doc(&dir, false);
        let made = put(&d, "chart.png", &png("one")).await.unwrap();
        assert_eq!(
            (made["path"].as_str(), made["mime"].as_str(), made["size"].as_u64()),
            (Some("assets/chart.png"), Some("image/png"), Some(11))
        );
        assert_eq!(pushes.load(Ordering::SeqCst), 1);
        let first = assets(&d);
        assert_eq!(first.len(), 1);
        assert_eq!(first[0]["name"], "chart.png");
        assert_eq!(first[0].as_object().unwrap().len(), 4, "name, mime, size and rev, and nothing else: {first:?}");
        // The same name again replaces it, with a new rev and the new size.
        put(&d, "chart.png", &png("two, longer")).await.unwrap();
        let second = assets(&d);
        assert_eq!(second.len(), 1);
        assert!(second[0]["rev"].as_u64() > first[0]["rev"].as_u64(), "{first:?} {second:?}");
        assert_eq!(second[0]["size"], 19);
        let read = d.call("doc_asset", json!({ "name": "chart.png" })).await.unwrap();
        assert_eq!(read["data"], b64(&png("two, longer")));
        assert_eq!(pushes.load(Ordering::SeqCst), 2);
        // A picture is kept on disk and found again after a restart.
        std::fs::write(dir.join(FOLDER).join(assets::FOLDER).join("chart.tmp"), "half").unwrap();
        let (again, _) = doc(&dir, true);
        assert_eq!(assets(&again).len(), 1);
        assert_eq!(assets(&again)[0]["name"], "chart.png");
        assert_eq!(assets(&again)[0]["size"], 19);
        let read = again.call("doc_asset", json!({ "name": "chart.png" })).await.unwrap();
        assert_eq!(read["mime"], "image/png");
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[tokio::test]
    async fn a_picture_that_breaks_a_rule_is_refused_and_nothing_changes() {
        let dir = temp("asset-rules");
        let (d, pushes) = doc(&dir, false);
        put(&d, "keep.png", &png("keep")).await.unwrap();
        let before = (assets(&d), pushes.load(Ordering::SeqCst), files(&dir.join(FOLDER).join(assets::FOLDER)));
        let svg = b"<svg xmlns=\"http://www.w3.org/2000/svg\"/>".as_slice();
        let jpeg = b"\xff\xd8\xff\xe0 jfif".as_slice();
        let big = png(&"x".repeat(ASSET_MAX));
        let fits = png(&"x".repeat(ASSET_MAX - 8));
        for (name, bytes, says) in [
            // Names.
            ("../evil.png", png("x"), "1 to 100"),
            ("sub/a.png", png("x"), "1 to 100"),
            ("a.tmp", png("x"), ".tmp"),
            ("a.TMP", png("x"), ".tmp"),
            ("", png("x"), "1 to 100"),
            ("Keep.png", png("x"), "has keep.png already"),
            // Types: the bytes decide and the name has to agree.
            ("a.svg", png("x"), "must end in .png"),
            ("a.jpg", png("x"), "holds a png image: name it a.png"),
            ("page.png", b"<html><body>hi</body></html>".to_vec(), "isn't a PNG, JPEG, GIF or WebP"),
            ("drawing.png", svg.to_vec(), "```svg fence"),
            ("drawing.svg", svg.to_vec(), "must end in .png"),
            ("text.png", b"just words".to_vec(), "isn't a PNG, JPEG, GIF or WebP"),
            ("photo.png", jpeg.to_vec(), "holds a jpeg image: name it photo.jpg"),
            // Size: 2 MB fits, one byte more doesn't.
            ("big.png", big.clone(), "the cap is 2097152"),
            ("over.png", png(&"x".repeat(ASSET_MAX - 7)), "is 2097153 bytes; the cap is 2097152"),
        ] {
            let e = put(&d, name, &bytes).await.unwrap_err();
            assert!(e.contains(says) && !e.contains('\n'), "{name}: {e}");
        }
        put(&d, "fits.png", &fits).await.unwrap();
        put(&d, "exact.png", &png(&"x".repeat(ASSET_MAX - 8))).await.unwrap();
        let e = d.call("doc_asset_put", json!({ "name": "a.png", "data": "!!not base64!!" })).await.unwrap_err();
        assert!(e.contains("isn't base64"), "{e}");
        let e =
            d.call("doc_asset_put", json!({ "name": "a.png", "data": "x".repeat(ASSET_MAX * 2) })).await.unwrap_err();
        assert!(e.contains("too big; the cap is 2097152"), "{e}");
        assert!(d.call("doc_asset_put", json!({ "name": "a.png" })).await.unwrap_err().contains("needs {name, data}"));
        // Nothing else was kept or sent.
        let kept = assets(&d);
        assert_eq!(
            kept.iter().map(|a| a["name"].as_str().unwrap()).collect::<Vec<_>>(),
            ["exact.png", "fits.png", "keep.png"]
        );
        assert_eq!(pushes.load(Ordering::SeqCst), before.1 + 2);
        assert_eq!(files(&dir.join(FOLDER).join(assets::FOLDER)), ["exact.png", "fits.png", "keep.png"]);
        assert_eq!(kept.iter().find(|a| a["name"] == "keep.png"), before.0.first());
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[tokio::test]
    async fn a_doc_keeps_twenty_pictures() {
        let dir = temp("asset-count");
        let (d, pushes) = doc(&dir, false);
        for i in 0..ASSETS_MAX {
            put(&d, &format!("p{i:02}.png"), &png(&i.to_string())).await.unwrap();
        }
        let pushed = pushes.load(Ordering::SeqCst);
        let e = put(&d, "one-more.png", &png("x")).await.unwrap_err();
        assert!(e.contains("at most 20 assets") && !e.contains('\n'), "{e}");
        assert_eq!((assets(&d).len(), pushes.load(Ordering::SeqCst)), (20, pushed));
        assert!(!dir.join(FOLDER).join(assets::FOLDER).join("one-more.png").exists());
        // A name it has is replaced, not counted twice.
        put(&d, "p07.png", &png("new")).await.unwrap();
        assert_eq!(assets(&d).len(), 20);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[tokio::test]
    async fn fill_names_only_pictures_the_doc_has_and_the_text_says_image() {
        let dir = temp("asset-fill");
        let (d, pushes) = doc(&dir, false);
        outline(&d, 2).await.unwrap();
        fill(&d, "s1", "Before.").await.unwrap();
        let state = (entries(&d), pushes.load(Ordering::SeqCst), files(&dir.join(FOLDER)));
        // A picture that isn't there, on a filled section and a pending one:
        // refused, and nothing is written.
        for section in ["s1", "s2"] {
            let e = fill(&d, section, "![The flow](assets/flow.png)").await.unwrap_err();
            assert!(e.starts_with(&format!("section {section}: image assets/flow.png: no such asset")), "{e}");
            assert!(e.ends_with("the section is unchanged") && !e.contains('\n'), "{e}");
        }
        for md in ["![x](https://example.com/x.png)", "![x](../x.png)", "![x](assets/flow.svg)"] {
            assert!(fill(&d, "s2", md).await.unwrap_err().contains("section s2: image "), "{md}");
        }
        assert_eq!((entries(&d), pushes.load(Ordering::SeqCst), files(&dir.join(FOLDER))), state);
        // Put, then it fills, and `text()` says what it is.
        put(&d, "flow.png", &png("flow")).await.unwrap();
        fill(&d, "s2", "The flow:\n\n![The flow, in boxes](assets/flow.png)\n").await.unwrap();
        let want = "# Plan\n\n## H1\n\nBefore.\n\n## H2\n\nThe flow:\n\n[image: The flow, in boxes]\n";
        assert_eq!(d.text(), want);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[tokio::test]
    async fn doc_asset_answers_with_the_bytes_type_and_only_for_a_kept_name() {
        let dir = temp("asset-read");
        let (d, _) = doc(&dir, false);
        outline(&d, 1).await.unwrap();
        put(&d, "a.png", &png("pic")).await.unwrap();
        let read = d.call("doc_asset", json!({ "name": "a.png" })).await.unwrap();
        assert_eq!(
            (read["mime"].as_str(), read["data"].as_str()),
            (Some("image/png"), Some(b64(&png("pic")).as_str()))
        );
        // The type is the bytes', never the name's: a file that changed
        // under its name (hand-edited) is read for what it is.
        let jpeg = b"\xff\xd8\xff\xe0 jfif";
        std::fs::write(dir.join(FOLDER).join(assets::FOLDER).join("a.png"), jpeg).unwrap();
        assert_eq!(d.call("doc_asset", json!({ "name": "a.png" })).await.unwrap()["mime"], "image/jpeg");
        std::fs::write(dir.join(FOLDER).join(assets::FOLDER).join("a.png"), "<html>").unwrap();
        assert!(d.call("doc_asset", json!({ "name": "a.png" })).await.unwrap_err().contains("isn't an image"));
        // Nothing but a kept name is read: not the Doc's own files, not a path.
        for name in ["doc.json", "../doc.json", "../s1.md", "b.png", "", "a.png/../../doc.json"] {
            let e = d.call("doc_asset", json!({ "name": name })).await.unwrap_err();
            assert!(e.contains("no asset"), "{name:?}: {e}");
        }
        assert!(d.call("doc_asset", json!({})).await.unwrap_err().contains("needs {name}"));
        std::fs::remove_dir_all(dir).unwrap();
    }

    /// The trap: the bytes must never be in the state every client is sent.
    #[tokio::test]
    async fn no_state_holds_a_pictures_bytes() {
        let dir = temp("asset-state");
        let (d, _) = doc(&dir, false);
        outline(&d, 2).await.unwrap();
        let bytes = png(&"UNIQUE-PICTURE-BYTES ".repeat(50));
        put(&d, "big.png", &bytes).await.unwrap();
        fill(&d, "s1", "![alt](assets/big.png)").await.unwrap();
        let (state, at) = d.state_and_changed().unwrap();
        let patch = arugula_proto::block_patch::to_patch(&state, at);
        for sent in [d.state(), state, patch] {
            let text = sent.to_string();
            assert!(!text.contains("UNIQUE-PICTURE") && !text.contains(&b64(&bytes)[..40]), "{text}");
            assert!(text.len() < 1000, "{} bytes of state for a {} byte picture", text.len(), bytes.len());
            assert_eq!(sent["assets"][0]["name"], "big.png");
        }
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[tokio::test]
    async fn the_config_and_summary_carry_the_title() {
        let dir = temp("title");
        let (d, _) = doc(&dir, false);
        outline(&d, 1).await.unwrap();
        assert_eq!(d.config(), json!({ "title": "Plan" }));
        assert_eq!(d.summary().title.as_deref(), Some("Plan"));
        std::fs::remove_dir_all(dir).unwrap();
    }
}

/// Sharing a Doc (#1066), against a control that keeps what it is sent
/// (`link::fake`). The format is `link.rs`'s own tests'.
#[cfg(test)]
mod share_tests {
    use std::sync::atomic::AtomicUsize;

    use base64::{
        Engine,
        engine::general_purpose::{STANDARD, URL_SAFE_NO_PAD},
    };

    use super::{
        link::{self, Link, Seam, fake::Fake},
        *,
    };

    const FOLDER: &str = "doc";

    /// A key that can be found by eye in anything it leaks into.
    const MARKER: [u8; 32] = *b"ARUGULA-MARKER-KEY-0123456789ABC";
    const ID: &str = "abcdefabcdefabcdefabcdefabcdefab";
    /// A Doc id.
    const ID16: &str = "0123456789abcdef";

    struct Setup {
        dir: PathBuf,
        state_dir: PathBuf,
        fake: Arc<Fake>,
        doc: Arc<Doc>,
    }

    impl Drop for Setup {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.dir);
        }
    }

    fn temp(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "arugula-doc-share-{name}-{}-{}",
            std::process::id(),
            hex::encode(link_random())
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn link_random() -> [u8; 4] {
        crate::push::random()
    }

    fn at(dir: &Path, fake: Option<Arc<Fake>>, restoring: bool) -> (Arc<Doc>, Arc<AtomicUsize>) {
        let state_dir = dir.join("state");
        std::fs::create_dir_all(&state_dir).unwrap();
        let mut link = Link::none(state_dir, ID16.into());
        link.remote = fake.map(|f| f as Arc<dyn link::Remote>);
        link.seam = Some(Seam { key: Some(MARKER), id: Some(ID.into()), part_max: Some(512), counted_nonces: false });
        let pushes = Arc::new(AtomicUsize::new(0));
        let n = pushes.clone();
        let doc = Doc::with_link(
            dir.join("block").join(FOLDER),
            "Plan".into(),
            restoring,
            link,
            Arc::new(move || {
                n.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            }),
        );
        (doc, pushes)
    }

    /// A Doc with an outline and two sections (one with a picture).
    async fn setup(name: &str) -> Setup {
        let dir = temp(name);
        let fake = Fake::new();
        let (doc, _) = at(&dir, Some(fake.clone()), false);
        let state_dir = dir.join("state");
        doc.call(
            "doc_outline",
            json!({ "title": "Plan", "sections": [
            { "heading": "One", "intent": "first" }, { "heading": "Two", "intent": "second" }, { "heading": "Three" }
        ] }),
        )
        .await
        .unwrap();
        let png = [b"\x89PNG\r\n\x1a\n".as_slice(), b"pretend pixels"].concat();
        doc.call("doc_asset_put", json!({ "name": "dot.png", "data": STANDARD.encode(&png) })).await.unwrap();
        doc.call("doc_fill", json!({ "section": "s1", "markdown": "Hello, secret words.\n\n![dot](assets/dot.png)" }))
            .await
            .unwrap();
        doc.call("doc_fill", json!({ "section": "s2", "markdown": "x".repeat(2000) })).await.unwrap();
        Setup { dir, state_dir, fake, doc }
    }

    fn key_forms() -> Vec<String> {
        vec![
            String::from_utf8(MARKER.to_vec()).unwrap(),
            URL_SAFE_NO_PAD.encode(MARKER),
            STANDARD.encode(MARKER),
            hex::encode(MARKER),
        ]
    }

    /// Every file under `dir`, with its text.
    fn files(dir: &Path, out: &mut Vec<(PathBuf, Vec<u8>)>) {
        for e in std::fs::read_dir(dir).unwrap() {
            let p = e.unwrap().path();
            if p.is_dir() {
                files(&p, out);
            } else {
                out.push((p.clone(), std::fs::read(&p).unwrap()));
            }
        }
    }

    fn contains(hay: &[u8], needle: &str) -> bool {
        hay.windows(needle.len()).any(|w| w == needle.as_bytes())
    }

    #[tokio::test]
    async fn sharing_checks_the_quota_then_uploads_in_parts_and_the_url_opens() {
        let s = setup("share").await;
        let got = s.doc.call("doc_share", json!({})).await.unwrap();
        let url = got["url"].as_str().unwrap();
        assert_eq!(url, format!("https://docs.test/{ID}#k={}", URL_SAFE_NO_PAD.encode(MARKER)));

        // The quota first, then each part, then the commit.
        let paths = s.fake.paths();
        assert_eq!(paths[0], "GET /api/doc-links/quota");
        let parts = s.fake.served.lock().unwrap()[ID].1;
        assert!(parts >= 3, "{parts}");
        for n in 0..parts {
            assert_eq!(paths[1 + n], format!("PUT /api/doc-links/{ID}/v/1/parts/{n}"));
        }
        assert_eq!(paths[1 + parts], format!("POST /api/doc-links/{ID}/v/1/commit"));
        assert_eq!(paths.len(), parts + 2);

        // What control holds opens with the key, as the manifest says.
        let held: Vec<Vec<u8>> =
            (0..parts).map(|n| s.fake.parts.lock().unwrap()[&(ID.to_string(), 1, n)].clone()).collect();
        assert!(held.iter().all(|p| p.len() <= 512 + 28));
        // Each part has a nonce of its own (random ones: the seam doesn't count them).
        let mut ivs: Vec<&[u8]> = held.iter().map(|p| &p[..12]).collect();
        ivs.sort();
        ivs.dedup();
        assert_eq!(ivs.len(), held.len(), "a nonce was reused");
        let plain = link::open(&MARKER, ID, 1, &held).unwrap();
        assert_eq!(&plain[..4], b"ADL1");
        let len = u32::from_be_bytes(plain[4..8].try_into().unwrap()) as usize;
        let manifest: Value = serde_json::from_slice(&plain[8..8 + len]).unwrap();
        assert_eq!(manifest["title"], "Plan");
        assert_eq!(manifest["entries"][0]["body"], "Hello, secret words.\n\n![dot](assets/dot.png)");
        assert!(manifest["entries"][2]["body"].is_null());
        assert_eq!(manifest["assets"][0]["name"], "dot.png");
        assert_eq!(manifest["assets"][0]["mime"], "image/png");
        // ...and nothing the viewer doesn't render.
        assert_eq!(manifest["assets"][0].as_object().unwrap().len(), 3);
        assert_eq!(manifest["entries"][0].as_object().unwrap().len(), 4);
        assert_eq!(manifest.as_object().unwrap().len(), 3);
        assert_eq!(plain.len(), 8 + len + manifest["assets"][0]["size"].as_u64().unwrap() as usize);

        // Control holds ciphertext: not the words, and not the key.
        for p in &held {
            assert!(!contains(p, "secret words"));
        }
        // The state says only when.
        let st = s.doc.state();
        assert_eq!(st["shared"].as_object().unwrap().len(), 2);
        assert!(st["shared"]["made"].as_u64().unwrap() > 0);
        // A second share is refused, saying what to use.
        let e = s.doc.call("doc_share", json!({})).await.unwrap_err();
        assert!(e.contains("Update") && !e.contains('\n'), "{e}");
        assert_eq!(s.fake.served.lock().unwrap().len(), 1);
    }

    #[tokio::test]
    async fn an_update_is_a_new_version_under_the_same_id_and_key_and_unshare_takes_it_down() {
        let s = setup("update").await;
        let first = s.doc.call("doc_share", json!({})).await.unwrap()["url"].as_str().unwrap().to_owned();
        let made = s.doc.state()["shared"]["made"].clone();
        s.doc.call("doc_fill", json!({ "section": "s3", "markdown": "A new section." })).await.unwrap();
        let again = s.doc.call("doc_share_update", json!({})).await.unwrap()["url"].as_str().unwrap().to_owned();
        assert_eq!(first, again, "the same URL");
        assert_eq!(s.fake.served.lock().unwrap()[ID].0, 2);
        assert!(s.fake.paths().iter().any(|p| p.starts_with(&format!("PUT /api/doc-links/{ID}/v/2/parts/0"))));
        assert_eq!(s.doc.state()["shared"]["made"], made, "made stays");
        let n = s.fake.served.lock().unwrap()[ID].1;
        let held: Vec<Vec<u8>> =
            (0..n).map(|n| s.fake.parts.lock().unwrap()[&(ID.to_string(), 2, n)].clone()).collect();
        let plain = link::open(&MARKER, ID, 2, &held).unwrap();
        assert!(contains(&plain, "A new section."));
        // Version 2's parts don't open as version 1.
        assert!(link::open(&MARKER, ID, 1, &held).is_none());

        // Update on a Doc that isn't shared says so.
        let other = setup("update-other").await;
        let e = other.doc.call("doc_share_update", json!({})).await.unwrap_err();
        assert!(e.contains("isn't shared"), "{e}");

        // Another link's id leaves this one alone.
        let r = s.doc.call("doc_unshare", json!({ "id": "f".repeat(32) })).await.unwrap();
        assert_eq!(r["other"], true);
        assert!(s.doc.state()["shared"].is_object());
        s.doc.call("doc_unshare", json!({})).await.unwrap();
        assert!(s.doc.state()["shared"].is_null());
        assert!(s.fake.served.lock().unwrap().is_empty());
        assert_eq!(s.fake.paths().last().unwrap(), &format!("DELETE /api/doc-links/{ID}"));
        assert!(link::get(&s.state_dir, ID).unwrap().is_none(), "the key is forgotten");
        let e = s.doc.call("doc_unshare", json!({})).await.unwrap_err();
        assert!(e.contains("isn't shared"), "{e}");
        // Shared again, it is a new link (the fixed id here; a new key and id in life).
        s.doc.call("doc_share", json!({})).await.unwrap();
        assert_eq!(s.fake.served.lock().unwrap()[ID].0, 1);
    }

    /// The trap: the key reaches the state, the Doc's folder, a log line or
    /// an error. Every path a share takes, including the ones that fail, with
    /// a key that can be found by eye.
    #[tokio::test]
    async fn the_key_is_in_the_store_and_nowhere_else() {
        let logs = Arc::new(Mutex::new(Vec::<u8>::new()));
        #[derive(Clone)]
        struct Sink(Arc<Mutex<Vec<u8>>>);
        impl std::io::Write for Sink {
            fn write(&mut self, b: &[u8]) -> std::io::Result<usize> {
                self.0.lock().unwrap().extend_from_slice(b);
                Ok(b.len())
            }
            fn flush(&mut self) -> std::io::Result<()> {
                Ok(())
            }
        }
        let sink = Sink(logs.clone());
        // With one dispatcher in the process, tracing works out a new
        // callsite's interest from the registering thread's default alone:
        // another test's thread that reaches "shared a Doc" first would
        // switch it off for this one. A second dispatcher makes it ask all.
        let _both = tracing::Dispatch::new(tracing::subscriber::NoSubscriber::default());
        let _log = tracing::subscriber::set_default(
            tracing_subscriber::fmt()
                .with_max_level(tracing::Level::TRACE)
                .with_ansi(false)
                .with_writer(move || sink.clone())
                .finish(),
        );
        let s = setup("key").await;
        let mut said = Vec::new();
        said.push(s.doc.call("doc_share", json!({})).await.unwrap().to_string()); // the URL has it, once
        said.push(s.doc.call("doc_shares", json!({})).await.unwrap().to_string());
        s.doc.call("doc_fill", json!({ "section": "s3", "markdown": "More." })).await.unwrap();
        said.push(s.doc.call("doc_share_update", json!({})).await.unwrap().to_string());
        // Errors: a refusal from control, an unshare elsewhere, a second share.
        *s.fake.refuse.lock().unwrap() = Some((413, "quota"));
        said.push(s.doc.call("doc_share_update", json!({})).await.unwrap_err());
        *s.fake.refuse.lock().unwrap() = Some((409, "stale"));
        said.push(s.doc.call("doc_share_update", json!({})).await.unwrap_err());
        *s.fake.refuse.lock().unwrap() = None;
        said.push(s.doc.call("doc_share", json!({})).await.unwrap_err());
        *s.fake.gone.lock().unwrap() = true;
        said.push(s.doc.call("doc_share_update", json!({})).await.unwrap_err());
        *s.fake.gone.lock().unwrap() = false;
        said.push(s.doc.call("doc_share", json!({})).await.unwrap().to_string());
        s.doc.call("doc_unshare", json!({})).await.unwrap();
        said.push(s.doc.call("doc_unshare", json!({})).await.unwrap_err());

        let state = serde_json::to_string(&s.doc.state()).unwrap();
        let mut folder = Vec::new();
        files(&s.dir.join("block"), &mut folder);
        assert!(!folder.is_empty());
        let log = logs.lock().unwrap().clone();
        assert!(!log.is_empty(), "the logs were captured");
        for form in key_forms() {
            assert!(!state.contains(&form), "state holds the key as {form}");
            for (p, b) in &folder {
                assert!(!contains(b, &form), "{} holds the key as {form}", p.display());
            }
            assert!(!contains(&log, &form), "a log line holds the key as {form}");
            // The answers that are errors (the URLs, 0 to 2 and 7, are the key's way out).
            for (i, e) in said.iter().enumerate().filter(|(i, _)| ![0, 1, 2, 7].contains(i)) {
                assert!(!e.contains(&form), "error {i} holds the key: {e}");
            }
        }
        // The store is where it lives (a share that was unshared has nothing; share once more).
        s.doc.call("doc_share", json!({})).await.unwrap();
        let store = std::fs::read(s.state_dir.join(link::FILE)).unwrap();
        assert!(contains(&store, &URL_SAFE_NO_PAD.encode(MARKER)));
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(std::fs::metadata(s.state_dir.join(link::FILE)).unwrap().permissions().mode() & 0o777, 0o600);
        }
    }

    #[tokio::test]
    async fn the_quota_is_checked_before_anything_is_uploaded() {
        let s = setup("quota").await;
        // Over the account's bytes.
        *s.fake.quota.lock().unwrap() = Some(
            json!({ "used": (200u64 << 20) - 100, "limit": 200u64 << 20, "links": 3, "max_links": 100, "max_link": 48u64 << 20 }),
        );
        let e = s.doc.call("doc_share", json!({})).await.unwrap_err();
        assert!(e.contains("past its 200 MB") && !e.contains('\n'), "{e}");
        // Over the account's links.
        *s.fake.quota.lock().unwrap() =
            Some(json!({ "used": 0, "limit": 200u64 << 20, "links": 100, "max_links": 100, "max_link": 48u64 << 20 }));
        let e = s.doc.call("doc_share", json!({})).await.unwrap_err();
        assert!(e.contains("100 shared Docs") && e.contains("unshare one"), "{e}");
        // Too big for a link.
        *s.fake.quota.lock().unwrap() =
            Some(json!({ "used": 0, "limit": 200u64 << 20, "links": 0, "max_links": 100, "max_link": 1000 }));
        let e = s.doc.call("doc_share", json!({})).await.unwrap_err();
        assert!(e.contains("too big") || e.contains("at most"), "{e}");
        // Nothing but the quota was asked, and nothing was kept or shown.
        assert!(s.fake.paths().iter().all(|p| p == "GET /api/doc-links/quota"), "{:?}", s.fake.paths());
        assert!(s.fake.parts.lock().unwrap().is_empty());
        assert!(link::get(&s.state_dir, ID).unwrap().is_none());
        assert!(s.doc.state()["shared"].is_null());
    }

    #[tokio::test]
    async fn a_404_for_a_link_we_hold_clears_shared_and_the_key() {
        for how in ["update", "shares"] {
            let s = setup(&format!("gone-{how}")).await;
            s.doc.call("doc_share", json!({})).await.unwrap();
            assert!(s.doc.state()["shared"].is_object());
            *s.fake.gone.lock().unwrap() = true;
            if how == "update" {
                let e = s.doc.call("doc_share_update", json!({})).await.unwrap_err();
                assert!(e.contains("unshared elsewhere") && e.contains("share it again"), "{e}");
            } else {
                let r = s.doc.call("doc_shares", json!({})).await.unwrap();
                assert!(r["shared"].is_null(), "{r}");
            }
            assert!(s.doc.state()["shared"].is_null(), "{how}");
            assert!(link::get(&s.state_dir, ID).unwrap().is_none(), "{how}: the key is forgotten");
        }
    }

    #[tokio::test]
    async fn each_refusal_of_control_is_one_line_and_the_daemon_doesnt_loop() {
        let s = setup("refusals").await;
        for (status, reason, says) in [
            (413, "too_big", "control says too_big"),
            (413, "links", "control says links"),
            (507, "disk", "control says disk"),
            (409, "stale", "newer copy"),
            (409, "incomplete", "didn't get every part"),
            (413, "part", "over 1 MB"),
        ] {
            s.fake.calls.lock().unwrap().clear();
            *s.fake.refuse.lock().unwrap() = Some((status, reason));
            let e = s.doc.call("doc_share", json!({})).await.unwrap_err();
            assert!(e.contains(says) && !e.contains('\n'), "{reason}: {e}");
            // Quota, the first part, and that's it: no retry.
            assert_eq!(s.fake.paths().len(), 2, "{reason}: {:?}", s.fake.paths());
            assert!(s.doc.state()["shared"].is_null());
            assert!(link::get(&s.state_dir, ID).unwrap().is_none());
        }
    }

    #[tokio::test]
    async fn no_control_or_a_control_without_doc_links_says_what_to_do() {
        let dir = temp("nocontrol");
        let (doc, _) = at(&dir, None, false);
        doc.call("doc_outline", json!({ "title": "T", "sections": [{ "heading": "H" }] })).await.unwrap();
        for m in ["doc_share"] {
            assert_eq!(
                doc.call(m, json!({})).await.unwrap_err(),
                "this machine hasn't joined control: sharing needs control"
            );
        }
        let r = doc.call("doc_shares", json!({})).await.unwrap();
        assert_eq!((r["shared"].clone(), r["can"].clone()), (Value::Null, json!(false)));
        assert_eq!(r["why"], "this machine hasn't joined control: sharing needs control");
        std::fs::remove_dir_all(&dir).unwrap();

        let s = setup("old-control").await;
        *s.fake.viewer.lock().unwrap() = None;
        assert_eq!(s.doc.call("doc_share", json!({})).await.unwrap_err(), "this control can't keep shared Docs yet");
        let r = s.doc.call("doc_shares", json!({})).await.unwrap();
        assert_eq!(r["why"], "this control can't keep shared Docs yet");
        assert!(s.fake.paths().is_empty(), "nothing was sent");
    }

    #[tokio::test]
    async fn a_restart_keeps_the_doc_shared_and_a_closed_block_keeps_its_key() {
        let s = setup("restart").await;
        let url = s.doc.call("doc_share", json!({})).await.unwrap()["url"].as_str().unwrap().to_owned();
        let shared = s.doc.state()["shared"].clone();
        let (again, _) = at(&s.dir, Some(s.fake.clone()), true);
        assert_eq!(again.state()["shared"], shared);
        let r = again.call("doc_shares", json!({})).await.unwrap();
        assert_eq!(r["url"], url.as_str());
        // The block is gone; the key isn't (the account list still copies it).
        drop(again);
        assert_eq!(link::url_of(&s.state_dir, ID).unwrap().as_deref(), Some(url.as_str()));
    }
}
