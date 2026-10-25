//! The Doc library (#1492, #1493): a Doc is kept in `<state>/docs/<id>/`,
//! not in the folder of the block that shows it, so that closing the block
//! leaves it, and any block can open it again.
//!
//! - `<id>` is 16 lower-case hex digits, random. Nothing else is an id.
//! - `meta.json`: `{id, title, made, updated, by}`. `by` is the `mcp:` agent
//!   or the person's driver name that made it, or `migrated`. `updated`
//!   moves on an outline, a fill and an asset.
//! - `doc.json`, `sN.md` and `assets/`: the Doc's files, as [`super`] says.
//!
//! Nothing here prunes: a Doc stays until the owner deletes it.
//!
//! **The migration** ([`migrate`]) moves the Docs of the old layout, where a
//! Doc's files were in its block's folder (`blocks/<id>/doc/`, or
//! `closed/<id>-<ms>/doc/` once it closed), into the library, once, at
//! start. It copies, checks every file arrived, and only then removes the
//! old folder, so a failure at any step leaves the old files where they were
//! and the next start does it again. `blocks/<id>/doc.lib` holds the id a
//! block's Doc was given, so that a second pass (after a crash) reuses it
//! instead of making a second copy. Running it twice changes nothing; the
//! marker `docs/.migrated-1` says it is done.

use std::{
    collections::HashMap,
    path::{Path, PathBuf},
};

use arugula_core::PaneId;
use arugula_proto::api::SearchHit;
use regex::Regex;
use serde::{Deserialize, Serialize};
use tracing::{info, warn};

use crate::store::{now_ms, private_dir, write_atomic};

/// The library's folder, in the state directory.
pub const DIR: &str = "docs";
const META: &str = "meta.json";
/// Written when the migration has moved every Doc.
const MARKER: &str = ".migrated-1";
/// In a block's folder: the id its Doc was given by the migration.
const POINTER: &str = "doc.lib";
/// A block's folder holds its Doc's files here, in the old layout.
const OLD: &str = "doc";

/// `meta.json`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Meta {
    pub id: String,
    pub title: String,
    pub made: u64,
    pub updated: u64,
    pub by: String,
}

/// Whether `id` is what a Doc is called: 16 lower-case hex digits.
pub fn is_id(id: &str) -> bool {
    id.len() == 16 && id.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
}

pub fn root(state_dir: &Path) -> PathBuf {
    state_dir.join(DIR)
}

/// The Doc's folder. The id is checked, so no path comes from anything else.
pub fn dir(state_dir: &Path, id: &str) -> Result<PathBuf, String> {
    if !is_id(id) {
        return Err(format!("{id:?} isn't a Doc id: a Doc id is 16 lower-case hex digits (docs list has them)"));
    }
    Ok(root(state_dir).join(id))
}

/// A new, empty Doc in the library: its folder and `meta.json`.
pub fn make(state_dir: &Path, title: &str, by: &str) -> Result<Meta, String> {
    let root = root(state_dir);
    private_dir(&root).map_err(|e| format!("can't make the Doc library: {e}"))?;
    for _ in 0..8 {
        let id = hex::encode(crate::push::random::<8>());
        let dir = root.join(&id);
        if dir.exists() {
            continue;
        }
        private_dir(&dir).map_err(|e| format!("can't make the Doc's folder: {e}"))?;
        let now = now_ms();
        let meta = Meta { id, title: title.trim().to_owned(), made: now, updated: now, by: by.to_owned() };
        if let Err(e) = write_meta(&dir, &meta) {
            let _ = std::fs::remove_dir_all(&dir);
            return Err(e);
        }
        return Ok(meta);
    }
    Err("can't find a free Doc id".into())
}

fn write_meta(dir: &Path, meta: &Meta) -> Result<(), String> {
    let bytes = serde_json::to_vec_pretty(meta).map_err(|e| e.to_string())?;
    write_atomic(&dir.join(META), &bytes).map_err(|e| format!("can't write the Doc's meta.json: {e}"))
}

/// The Doc's `meta.json`, if it has one that parses and names this folder.
pub fn read_meta(dir: &Path) -> Option<Meta> {
    let meta: Meta = serde_json::from_slice(&std::fs::read(dir.join(META)).ok()?).ok()?;
    (dir.file_name()?.to_str()? == meta.id).then_some(meta)
}

/// The Doc changed: `updated` moves (and the title, if it was set). A folder
/// with no `meta.json` is left alone.
pub fn touch(dir: &Path, title: Option<&str>) {
    let Some(mut meta) = read_meta(dir) else { return };
    meta.updated = now_ms().max(meta.updated);
    if let Some(t) = title {
        meta.title = t.trim().to_owned();
    }
    if let Err(e) = write_meta(dir, &meta) {
        warn!(doc = meta.id, error = e, "can't update a Doc's meta.json");
    }
}

/// Every Doc in the library, newest `updated` first. A folder without a
/// `meta.json` of its own is left out (and left alone).
pub fn list(state_dir: &Path) -> Vec<Meta> {
    let Ok(entries) = std::fs::read_dir(root(state_dir)) else { return vec![] };
    let mut out: Vec<Meta> = entries.flatten().filter_map(|e| read_meta(&e.path())).collect();
    out.sort_by(|a, b| b.updated.cmp(&a.updated).then(b.made.cmp(&a.made)).then(a.id.cmp(&b.id)));
    out
}

/// The Doc, gone from disk.
pub fn remove(state_dir: &Path, id: &str) -> Result<(), String> {
    let dir = dir(state_dir, id)?;
    std::fs::remove_dir_all(&dir).map_err(|e| format!("can't remove the Doc {id}: {e}"))
}

/// Lines of every Doc in the library matching `re` (a Doc that no block shows
/// too), newest Doc first, for `search`. `open` says which block shows which
/// Doc; `since_ms` leaves out a Doc not changed since.
pub fn search(
    state_dir: &Path,
    open: &HashMap<String, PaneId>,
    re: &Regex,
    since_ms: Option<u64>,
    limit: usize,
) -> Vec<SearchHit> {
    let mut hits = Vec::new();
    for meta in list(state_dir) {
        if hits.len() >= limit {
            break;
        }
        if since_ms.is_some_and(|s| meta.updated < s) {
            continue;
        }
        let Ok(loaded) = super::load(&root(state_dir).join(&meta.id)) else { continue };
        let text = super::markdown(&loaded.title, &loaded.entries);
        let pane = open.get(&meta.id).copied();
        for (n, line) in text.lines().enumerate().filter(|(_, l)| re.is_match(l)) {
            hits.push(SearchHit {
                pane: pane.unwrap_or(0),
                open: pane.is_some(),
                offset: n as u64,
                line: line.to_owned(),
                command: None,
                host: None,
                doc: Some(meta.id.clone()),
            });
            if hits.len() >= limit {
                break;
            }
        }
    }
    hits
}

// -------------------------------------------------------------- migration

/// A Doc folder the old layout holds: whose block it is, and where.
struct Old {
    pane: PaneId,
    /// The block's folder (`blocks/<id>` or `closed/<id>-<ms>`).
    block: PathBuf,
}

/// Move the Docs of the old layout into the library, once. Never panics and
/// never loses a file: whatever fails leaves the old files, logs why, and
/// the next start tries again. Whether it is done.
pub fn migrate(state_dir: &Path) -> bool {
    migrate_with(state_dir, copy_tree)
}

/// ...copying with `copy` (a test's lies about it).
fn migrate_with(state_dir: &Path, copy: Copy) -> bool {
    match migrate_inner(state_dir, copy) {
        Ok(()) => true,
        Err(e) => {
            warn!(
                error = e,
                "can't move the Docs into the library; the old files are kept, and the next start tries again"
            );
            false
        }
    }
}

/// Copies a folder's files into another.
type Copy = fn(&Path, &Path) -> Result<(), String>;

fn migrate_inner(state_dir: &Path, copy: Copy) -> Result<(), String> {
    let root = root(state_dir);
    if root.join(MARKER).exists() {
        return Ok(());
    }
    private_dir(&root).map_err(|e| format!("can't make the Doc library: {e}"))?;
    let (olds, mut ids) = scan(state_dir);
    let mut moved = Vec::new();
    // Copy, check, write `meta.json`: for every Doc before anything is removed.
    for old in &olds {
        let title = layout_title(state_dir, old.pane);
        match copy_in(state_dir, old, title.as_deref(), copy) {
            Ok(id) => {
                ids.insert(old.pane, id);
                moved.push(old);
            }
            Err(e) => return Err(format!("block %{}: {e}", old.pane)),
        }
    }
    // The blocks' configs and the links now say which Doc is whose.
    if !ids.is_empty() {
        map_layout(state_dir, &ids)?;
        super::link::map_blocks(state_dir, &ids)?;
    }
    for old in moved {
        if let Err(e) = std::fs::remove_dir_all(old.block.join(OLD)) {
            return Err(format!("can't remove the old Doc folder of %{}: {e}", old.pane));
        }
    }
    write_atomic(&root.join(MARKER), now_ms().to_string().as_bytes())
        .map_err(|e| format!("can't write the migration's marker: {e}"))?;
    info!(docs = ids.len(), "moved the Docs into the library");
    Ok(())
}

/// The block folders with an old-layout Doc, and the Doc ids earlier passes
/// gave blocks (by block).
fn scan(state_dir: &Path) -> (Vec<Old>, HashMap<PaneId, String>) {
    let (mut olds, mut ids) = (Vec::new(), HashMap::new());
    for sub in ["blocks", "closed"] {
        let Ok(entries) = std::fs::read_dir(state_dir.join(sub)) else { continue };
        for e in entries.flatten() {
            let name = e.file_name().to_string_lossy().into_owned();
            let Some(pane) = name.split('-').next().and_then(|n| n.parse::<PaneId>().ok()) else { continue };
            let block = e.path();
            if let Some(id) = std::fs::read_to_string(block.join(POINTER)).ok().map(|s| s.trim().to_owned())
                && is_id(&id)
            {
                ids.insert(pane, id);
            }
            if block.join(OLD).is_dir() {
                olds.push(Old { pane, block });
            }
        }
    }
    olds.sort_by_key(|o| o.pane);
    (olds, ids)
}

/// The title a Doc block's config had in the layout, for a Doc with no
/// `doc.json`.
fn layout_title(state_dir: &Path, pane: PaneId) -> Option<String> {
    let saved = arugula_mux::store::StateDir::open(state_dir.to_owned()).ok()?.load_layout().ok()??;
    saved.panes.get(&pane)?.config.as_ref()?["title"].as_str().map(str::to_owned)
}

/// Copy one old Doc into the library, check it, and write its `meta.json`.
/// Its id.
fn copy_in(state_dir: &Path, old: &Old, layout_title: Option<&str>, copy: Copy) -> Result<String, String> {
    let from = old.block.join(OLD);
    // An id this block's Doc was given already is reused.
    let pointer = old.block.join(POINTER);
    let id = match std::fs::read_to_string(&pointer).ok().map(|s| s.trim().to_owned()).filter(|s| is_id(s)) {
        Some(id) => id,
        None => {
            let id = loop {
                let id = hex::encode(crate::push::random::<8>());
                if !root(state_dir).join(&id).exists() {
                    break id;
                }
            };
            write_atomic(&pointer, id.as_bytes()).map_err(|e| format!("can't note the Doc's id: {e}"))?;
            id
        }
    };
    let to = root(state_dir).join(&id);
    copy(&from, &to)?;
    check_tree(&from, &to)?;
    let (mut made, mut updated) = (u64::MAX, 0);
    times(&from, &mut made, &mut updated);
    let now = now_ms();
    let (made, updated) = if updated == 0 { (now, now) } else { (made, updated) };
    let title = std::fs::read_to_string(from.join(super::OUTLINE))
        .ok()
        .and_then(|t| serde_json::from_str::<serde_json::Value>(&t).ok())
        .and_then(|v| v["title"].as_str().map(str::to_owned))
        .filter(|t| !t.trim().is_empty())
        .or_else(|| layout_title.map(str::to_owned).filter(|t| !t.trim().is_empty()))
        .unwrap_or_else(|| "Untitled".to_owned());
    write_meta(&to, &Meta { id: id.clone(), title, made, updated, by: "migrated".into() })?;
    Ok(id)
}

/// The oldest and newest modification time under `dir`, in ms.
fn times(dir: &Path, oldest: &mut u64, newest: &mut u64) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    for e in entries.flatten() {
        let Ok(m) = e.metadata() else { continue };
        if m.is_dir() {
            times(&e.path(), oldest, newest);
        } else if let Some(ms) =
            m.modified().ok().and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map(|d| d.as_millis() as u64)
        {
            *oldest = (*oldest).min(ms);
            *newest = (*newest).max(ms);
        }
    }
}

fn copy_tree(from: &Path, to: &Path) -> Result<(), String> {
    private_dir(to).map_err(|e| format!("can't make {}: {e}", to.display()))?;
    for e in std::fs::read_dir(from).map_err(|e| format!("can't read {}: {e}", from.display()))?.flatten() {
        let (src, dst) = (e.path(), to.join(e.file_name()));
        if e.file_type().map_err(|e| e.to_string())?.is_dir() {
            copy_tree(&src, &dst)?;
        } else {
            let bytes = std::fs::read(&src).map_err(|e| format!("can't read {}: {e}", src.display()))?;
            write_atomic(&dst, &bytes).map_err(|e| format!("can't write {}: {e}", dst.display()))?;
        }
    }
    Ok(())
}

/// Every file under `from` is under `to`, byte for byte.
fn check_tree(from: &Path, to: &Path) -> Result<(), String> {
    for e in std::fs::read_dir(from).map_err(|e| format!("can't read {}: {e}", from.display()))?.flatten() {
        let (src, dst) = (e.path(), to.join(e.file_name()));
        if e.file_type().map_err(|e| e.to_string())?.is_dir() {
            check_tree(&src, &dst)?;
        } else if std::fs::read(&src).map_err(|e| e.to_string())? != std::fs::read(&dst).map_err(|e| e.to_string())? {
            return Err(format!("{} didn't arrive intact", dst.display()));
        }
    }
    Ok(())
}

/// Give each Doc block in the saved layout its Doc (`config.doc`).
fn map_layout(state_dir: &Path, ids: &HashMap<PaneId, String>) -> Result<(), String> {
    let store = arugula_mux::store::StateDir::open(state_dir.to_owned()).map_err(|e| e.to_string())?;
    let Some(mut saved) = store.load_layout().map_err(|e| format!("can't read layout.json: {e}"))? else {
        return Ok(());
    };
    let mut changed = false;
    for (pane, meta) in saved.panes.iter_mut().filter(|(_, m)| m.kind == arugula_proto::BlockType::Doc) {
        let Some(id) = ids.get(pane) else { continue };
        let config = meta.config.get_or_insert_with(|| serde_json::json!({}));
        if config["doc"].as_str() != Some(id) {
            config["doc"] = serde_json::json!(id);
            changed = true;
        }
    }
    if changed {
        store.save_layout(&saved).map_err(|e| format!("can't write layout.json: {e}"))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use arugula_mux::store::{LAYOUT_VERSION, PaneMeta, Saved, StateDir};
    use serde_json::json;

    use super::*;

    fn temp(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("arugula-library-{name}-{}-{}", std::process::id(), now_ms()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// A Doc as the old layout kept it, in a block's folder: an outline, a
    /// filled section and a picture.
    fn old_doc(block: &Path, title: &str) {
        let doc = block.join(OLD);
        std::fs::create_dir_all(doc.join("assets")).unwrap();
        let outline = json!({ "title": title, "sections": [
            { "id": "s1", "heading": "One", "intent": "first" }, { "id": "s2", "heading": "Two", "intent": "" }] });
        std::fs::write(doc.join("doc.json"), outline.to_string()).unwrap();
        std::fs::write(doc.join("s1.md"), format!("The words of {title}.")).unwrap();
        std::fs::write(doc.join("assets/dot.png"), b"\x89PNG\r\n\x1a\nbytes").unwrap();
    }

    fn link(block: PaneId) -> serde_json::Value {
        json!({ "key": "k", "ver": 2, "block": block, "title": "t", "made": 5, "updated": 6, "size": 7,
                "viewer": "https://docs.test" })
    }

    /// A state dir as the old daemon left it: block 3 is open and shared,
    /// block 4 is open and not shared, 5 is a Doc that closed (shared too),
    /// 6 is a closed terminal; a link for block 9, whose folder is long gone.
    fn old_state(name: &str) -> PathBuf {
        let state = temp(name);
        let store = StateDir::open(state.clone()).unwrap();
        for (dir, title) in [("blocks/3", "Plan"), ("blocks/4", "Notes"), ("closed/5-1700000000000", "Old")] {
            std::fs::create_dir_all(state.join(dir)).unwrap();
            old_doc(&state.join(dir), title);
        }
        std::fs::create_dir_all(state.join("closed/6-1700000000000")).unwrap();
        std::fs::write(state.join("closed/6-1700000000000/log"), "a terminal").unwrap();
        let doc_pane = |title: &str| PaneMeta {
            kind: arugula_proto::BlockType::Doc,
            config: Some(json!({ "title": title })),
            ..Default::default()
        };
        let saved = Saved {
            version: LAYOUT_VERSION,
            saved_at_ms: 1,
            mux: arugula_core::Mux::new(),
            panes: BTreeMap::from([(3, doc_pane("Plan")), (4, doc_pane("Notes"))]),
            machines: BTreeMap::new(),
            next_machine: 1,
        };
        store.save_layout(&saved).unwrap();
        let links = json!({ "aaaaaaaabbbbbbbbccccccccdddddddd": link(3), "11111111222222223333333344444444": link(5),
                            "99999999999999999999999999999999": link(9) });
        std::fs::write(state.join(super::super::link::FILE), links.to_string()).unwrap();
        state
    }

    /// Every file under `dir`, with its bytes.
    fn tree(dir: &Path) -> BTreeMap<String, Vec<u8>> {
        fn walk(root: &Path, at: &Path, out: &mut BTreeMap<String, Vec<u8>>) {
            for e in std::fs::read_dir(at).into_iter().flatten().flatten() {
                let p = e.path();
                if p.is_dir() {
                    walk(root, &p, out);
                } else {
                    // Keyed by `/` paths, whatever the platform's separator.
                    let key = p.strip_prefix(root).unwrap().display().to_string().replace('\\', "/");
                    out.insert(key, std::fs::read(&p).unwrap());
                }
            }
        }
        let mut out = BTreeMap::new();
        walk(dir, dir, &mut out);
        out
    }

    fn doc_of(state: &Path, pane: PaneId) -> Option<String> {
        let saved = StateDir::open(state.to_owned()).unwrap().load_layout().unwrap().unwrap();
        saved.panes.get(&pane)?.config.as_ref()?["doc"].as_str().map(str::to_owned)
    }

    #[test]
    fn an_id_is_sixteen_lower_case_hex_digits_and_nothing_else_makes_a_path() {
        assert!(is_id("0123456789abcdef"));
        for bad in
            ["", "0123456789ABCDEF", "0123456789abcde", "0123456789abcdef0", "../../../etc/pass", "0123456789abcdeg"]
        {
            assert!(!is_id(bad), "{bad}");
            assert!(dir(Path::new("/state"), bad).unwrap_err().contains("isn't a Doc id"), "{bad}");
        }
        let state = temp("make");
        let a = make(&state, " A title ", "mcp:claude-code").unwrap();
        let b = make(&state, "A title", "mcp:claude-code").unwrap();
        assert!(is_id(&a.id) && a.id != b.id, "two make two");
        assert_eq!((a.title.as_str(), a.by.as_str()), ("A title", "mcp:claude-code"));
        assert_eq!(read_meta(&dir(&state, &a.id).unwrap()), Some(a.clone()));
        // `updated` moves; the newest is first.
        std::thread::sleep(std::time::Duration::from_millis(5));
        touch(&dir(&state, &a.id).unwrap(), Some("Renamed"));
        let ids: Vec<_> = list(&state).into_iter().map(|m| (m.id, m.title)).collect();
        assert_eq!(ids, [(a.id.clone(), "Renamed".into()), (b.id, "A title".into())]);
        // A folder with no meta.json of its own is left out and left alone.
        std::fs::create_dir_all(root(&state).join("0000000000000000")).unwrap();
        assert_eq!(list(&state).len(), 2);
        assert!(root(&state).join("0000000000000000").is_dir());
        std::fs::remove_dir_all(state).unwrap();
    }

    /// The trap: a Doc lost in the move. An open one, a shared one and a
    /// closed one all arrive whole, their blocks and links point at them,
    /// and the old folders go only then.
    #[test]
    fn migration_moves_an_open_a_shared_and_a_closed_doc_whole() {
        let state = old_state("migrate");
        let before = tree(&state.join("blocks/3/doc"));
        assert!(migrate(&state));

        let docs = list(&state);
        assert_eq!(docs.len(), 3, "{docs:?}");
        assert!(docs.iter().all(|m| m.by == "migrated" && is_id(&m.id)));
        let by_title = |t: &str| docs.iter().find(|m| m.title == t).unwrap_or_else(|| panic!("no {t}")).clone();
        // Byte for byte, picture and all.
        assert_eq!(tree(&root(&state).join(&by_title("Plan").id)).remove("s1.md").unwrap(), b"The words of Plan.");
        for (k, v) in &before {
            assert_eq!(tree(&root(&state).join(&by_title("Plan").id)).get(k), Some(v), "{k}");
        }
        assert!(tree(&root(&state).join(&by_title("Old").id)).contains_key("assets/dot.png"));
        // `made` and `updated` are the files' times, not now.
        let plan = by_title("Plan");
        assert!(plan.made <= plan.updated && plan.updated <= now_ms());

        // The open blocks' configs name their Doc.
        assert_eq!(doc_of(&state, 3), Some(by_title("Plan").id));
        assert_eq!(doc_of(&state, 4), Some(by_title("Notes").id));
        // The links follow the Doc: block 3's and the closed block 5's. The
        // one whose block is gone keeps what it had.
        let links = super::super::link::by_doc(&state);
        assert_eq!(links[&by_title("Plan").id].0, "aaaaaaaabbbbbbbbccccccccdddddddd");
        assert_eq!(links[&by_title("Old").id].0, "11111111222222223333333344444444");
        assert_eq!(links.len(), 2);
        let kept = super::super::link::get(&state, "99999999999999999999999999999999").unwrap().unwrap();
        assert_eq!((kept.doc, kept.block), (None, Some(9)));

        // The old folders are gone, the block folders (with their logs) not.
        for dir in ["blocks/3", "blocks/4", "closed/5-1700000000000"] {
            assert!(!state.join(dir).join(OLD).exists(), "{dir}");
            assert!(state.join(dir).is_dir(), "{dir}");
        }
        assert_eq!(std::fs::read_to_string(state.join("closed/6-1700000000000/log")).unwrap(), "a terminal");
        assert!(root(&state).join(MARKER).is_file());
        std::fs::remove_dir_all(state).unwrap();
    }

    /// Run twice, it changes nothing: not a file, not a byte, not an id.
    #[test]
    fn running_migration_twice_changes_nothing() {
        let state = old_state("twice");
        assert!(migrate(&state));
        let once = (tree(&state), list(&state));
        assert!(migrate(&state));
        assert_eq!((tree(&state), list(&state)), once);
        // The marker means done: an old-layout folder that turns up later is
        // left where it is, not moved.
        old_doc(&state.join("blocks/77"), "Late");
        assert!(migrate(&state));
        assert!(state.join("blocks/77/doc/doc.json").is_file());
        assert_eq!(list(&state), once.1);
        std::fs::remove_dir_all(state.join("blocks/77")).unwrap();
        // Even with the marker gone (a crash before it was written), a
        // second pass finds nothing left to move and leaves the library as it is.
        std::fs::remove_file(root(&state).join(MARKER)).unwrap();
        assert!(migrate(&state));
        let mut again = tree(&state);
        again.retain(|k, _| !k.ends_with(MARKER));
        let mut first = once.0.clone();
        first.retain(|k, _| !k.ends_with(MARKER));
        assert_eq!(again, first);
        assert_eq!(list(&state), once.1);
        std::fs::remove_dir_all(state).unwrap();
    }

    /// The trap: copy, check, then remove. A step that fails leaves every
    /// old file where it was, and the next start finishes the job with the
    /// same ids, not a second copy.
    #[test]
    fn a_failed_migration_keeps_every_old_file_and_the_next_start_finishes_it() {
        let state = old_state("fails");
        let old: BTreeMap<_, _> = ["blocks/3", "blocks/4", "closed/5-1700000000000"]
            .into_iter()
            .map(|d| (d, tree(&state.join(d).join(OLD))))
            .collect();
        // The layout can't be read: the blocks' configs can't be mapped.
        let layout = std::fs::read(state.join("layout.json")).unwrap();
        std::fs::write(state.join("layout.json"), "{ not json").unwrap();
        assert!(!migrate(&state), "it says it isn't done");
        for (d, files) in &old {
            assert_eq!(&tree(&state.join(d).join(OLD)), files, "{d}: nothing was removed");
        }
        assert!(!root(&state).join(MARKER).exists());
        let copied = list(&state);

        std::fs::write(state.join("layout.json"), layout).unwrap();
        assert!(migrate(&state));
        let done = list(&state);
        assert_eq!(done.len(), 3, "the same three, not six: {done:?}");
        assert_eq!(
            done.iter().map(|m| &m.id).collect::<std::collections::BTreeSet<_>>(),
            copied.iter().map(|m| &m.id).collect()
        );
        assert!(doc_of(&state, 3).is_some() && root(&state).join(MARKER).is_file());
        for d in old.keys() {
            assert!(!state.join(d).join(OLD).exists(), "{d}");
        }
        std::fs::remove_dir_all(state).unwrap();
    }

    /// A copy that says it worked and didn't (a short write, a full disk) is
    /// found by the check, before anything is removed.
    #[test]
    fn a_copy_that_lost_bytes_is_caught_and_every_old_file_stays() {
        fn lossy(from: &Path, to: &Path) -> Result<(), String> {
            copy_tree(from, to)?;
            std::fs::write(to.join("s1.md"), b"short").map_err(|e| e.to_string())
        }
        let state = old_state("lossy");
        let before = tree(&state.join("blocks/3/doc"));
        assert!(!migrate_with(&state, lossy));
        assert_eq!(tree(&state.join("blocks/3/doc")), before);
        assert!(!root(&state).join(MARKER).exists());
        assert_eq!(doc_of(&state, 3), None, "no block was given a Doc that isn't whole");
        // The next start copies it again, whole.
        assert!(migrate(&state));
        let plan = list(&state).into_iter().find(|m| m.title == "Plan").unwrap();
        assert_eq!(std::fs::read(root(&state).join(plan.id).join("s1.md")).unwrap(), b"The words of Plan.");
        std::fs::remove_dir_all(state).unwrap();
    }

    #[test]
    fn a_fresh_state_dir_is_marked_and_a_library_doc_is_never_touched() {
        let state = temp("fresh");
        let made = make(&state, "Mine", "me").unwrap();
        std::fs::write(root(&state).join(&made.id).join("s1.md"), "mine").unwrap();
        assert!(migrate(&state));
        assert!(root(&state).join(MARKER).is_file());
        assert_eq!(std::fs::read_to_string(root(&state).join(&made.id).join("s1.md")).unwrap(), "mine");
        std::fs::remove_dir_all(state).unwrap();
    }

    /// Search reads the library: a Doc that no block shows, and one that
    /// does, with the block named.
    #[test]
    fn search_finds_a_doc_no_block_shows() {
        let state = temp("search");
        let (a, b) = (make(&state, "Alpha", "x").unwrap(), make(&state, "Beta", "x").unwrap());
        for (m, text) in [(&a, "the needle is here"), (&b, "nothing")] {
            let d = root(&state).join(&m.id);
            std::fs::write(
                d.join("doc.json"),
                json!({ "title": m.title, "sections": [{ "id": "s1", "heading": "H" }] }).to_string(),
            )
            .unwrap();
            std::fs::write(d.join("s1.md"), text).unwrap();
        }
        let re = Regex::new("needle").unwrap();
        let hits = search(&state, &HashMap::new(), &re, None, 10);
        assert_eq!(hits.len(), 1);
        assert_eq!((hits[0].doc.as_deref(), hits[0].open, hits[0].pane), (Some(a.id.as_str()), false, 0));
        assert!(hits[0].line.contains("the needle is here"));
        let shown = HashMap::from([(a.id.clone(), 7)]);
        let hits = search(&state, &shown, &re, None, 10);
        assert_eq!((hits[0].open, hits[0].pane), (true, 7));
        // `since` leaves out a Doc not changed since.
        assert!(search(&state, &shown, &re, Some(now_ms() + 60_000), 10).is_empty());
        assert!(search(&state, &shown, &re, None, 0).is_empty());
        std::fs::remove_dir_all(state).unwrap();
    }
}
