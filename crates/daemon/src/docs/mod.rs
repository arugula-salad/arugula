//! Doc blocks (#962, #963): a document an agent builds beside its pane, an
//! outline first and then one section at a time, that people watch fill in.
//!
//! Config `{title}`. The daemon holds the Doc and the agent only talks to
//! it through MCP's `doc`; disk is persistence, not transport. In the
//! block's own directory, `doc/`:
//!
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
//! The Doc is Labs' while `docs` is a flag, but its code is core: it checks
//! the flag itself when made ([`DocKind`]), and a restored one comes back
//! with the flag off (#347: turning a flag off hides and refuses, it
//! doesn't throw away).

mod assets;
mod embeds;

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

/// The folder inside the block's directory.
const FOLDER: &str = "doc";
const OUTLINE: &str = "doc.json";

pub struct DocKind;

impl BlockKind for DocKind {
    fn create(&self, ctx: BlockCtx, config: Value) -> Result<Arc<dyn Block>, String> {
        if !ctx.restoring {
            arugula_mux::labs::flag(&ctx.state_dir, arugula_proto::flags::DOCS)?;
        }
        let config: Config = serde_json::from_value(config).map_err(|e| format!("doc config: {e}"))?;
        let dir = ctx.dir.join(FOLDER);
        Ok(Doc::new(dir, config.title, ctx.restoring, Arc::new(move || ctx.changed())))
    }

    fn stored_text(&self, dir: &Path) -> Option<String> {
        let folder = dir.join(FOLDER);
        if !folder.join(OUTLINE).is_file() {
            return None;
        }
        let loaded = load(&folder).ok()?;
        Some(markdown(&loaded.title, &loaded.entries))
    }
}

#[derive(Debug, Default, Deserialize)]
struct Config {
    #[serde(default)]
    title: String,
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
    /// The folder couldn't be read back: the block shows this and writes
    /// nothing.
    error: Option<String>,
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
}

impl Doc {
    pub fn new(dir: PathBuf, title: String, restoring: bool, changed: Changed) -> Arc<Doc> {
        let mut state = State { title: title.trim().to_owned(), ..Default::default() };
        if restoring && dir.join(OUTLINE).exists() {
            match load(&dir) {
                Ok(l) => (state.title, state.entries) = (l.title, l.entries),
                Err(e) => state.error = Some(e),
            }
        }
        if restoring {
            state.assets = assets::load(&dir.join(assets::FOLDER));
        }
        Arc::new_cyclic(|me| Doc {
            me: me.clone(),
            dir,
            changed,
            state: Mutex::new(state),
            io: tokio::sync::Mutex::new(()),
            sent: Mutex::new(Vec::new()),
        })
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

    fn state_json(&self) -> Value {
        let st = self.state.lock().unwrap();
        let mut v = json!({
            "title": st.title, "entries": st.entries, "entries_from": 0, "assets": st.assets, "rev": st.rev,
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
    tokio::task::spawn_blocking(move || {
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
        json!({ "title": self.state.lock().unwrap().title })
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
        let me = self.me.upgrade();
        match method {
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

    fn close(&self) {}

    fn summary(&self) -> Summary {
        let title = self.state.lock().unwrap().title.clone();
        Summary { title: Some(title).filter(|t| !t.is_empty()), ..Summary::default() }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};

    use super::*;

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
        // A closed block's folder: history and search read the same.
        assert_eq!(DocKind.stored_text(&dir).as_deref(), Some(want));
        assert_eq!(DocKind.stored_text(&temp("none")), None);
        std::fs::remove_dir_all(dir).unwrap();
    }

    /// A block's context in `state_dir`, as the multiplexer makes one.
    fn ctx(state_dir: &Path, restoring: bool) -> (BlockCtx, tokio::sync::mpsc::UnboundedReceiver<crate::pane::Notice>) {
        let (notices, seen) = tokio::sync::mpsc::unbounded_channel();
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
            cmds: None,
            ids: Default::default(),
            state_dir: state_dir.to_path_buf(),
        };
        let dir = state_dir.join("blocks/7");
        (BlockCtx::new(7, dir, base, None, restoring, Default::default(), Default::default()), seen)
    }

    /// Made only where the machine has `docs` on; brought back whatever the
    /// flag says (#347).
    #[tokio::test]
    async fn the_kind_is_made_with_the_flag_on_and_restored_without_it() {
        let state = temp("kind");
        let config = json!({ "title": "Report" });
        let (new, _seen) = ctx(&state, false);
        let refused = DocKind.create(new, config.clone());
        // A build without Labs has no flags to be off (`labs::flag`).
        if crate::labs::BUILT {
            let e = refused.err().expect("refused while the flag is off");
            assert!(e.contains("arugulad flags docs on"), "{e}");
        }
        arugula_proto::flags::set(&state, arugula_proto::flags::DOCS, true).unwrap();
        let (new, _seen) = ctx(&state, false);
        assert_eq!(DocKind.create(new, config.clone()).map(|b| b.kind()).unwrap(), BlockType::Doc);
        arugula_proto::flags::set(&state, arugula_proto::flags::DOCS, false).unwrap();
        let (back, _seen) = ctx(&state, true);
        assert_eq!(DocKind.create(back, config).map(|b| b.kind()).unwrap(), BlockType::Doc, "restored with it off");
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
        assert_eq!(DocKind.stored_text(&dir).as_deref(), Some(want));
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
