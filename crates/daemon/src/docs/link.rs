//! Sharing a Doc with a link (#1064, #1066): the owner's snapshot of it,
//! encrypted, pushed to control, and a URL anyone can open.
//!
//! - **The snapshot** (plaintext, before it is cut): `"ADL1"`, a big-endian
//!   `u32` length, the manifest as JSON (`{title, entries, assets: [{name,
//!   mime, size}]}`: what the viewer renders and nothing else), then the
//!   assets' bytes in manifest order.
//! - **The parts:** at most [`PART_PLAIN`] bytes of that, each sealed on its
//!   own as `nonce(12) || AES-256-GCM(ciphertext, tag)`, so a part is at
//!   most 1 MiB on the wire. A nonce is fresh for every part. The additional
//!   data is `ADL1|<id>|<ver>|<n>|<parts>`, which stops a part being moved,
//!   dropped or mixed in from another version.
//! - **The key** is 32 random bytes per link. It lives in
//!   `<state>/doc-links.json` (0600, by link id) and in the URL's fragment,
//!   and nowhere else: not the block's folder, not the Doc's state, not a
//!   log line, not an error. Control only ever holds ciphertext.
//! - **The URL** is `<viewer>/<id>#k=<key, base64url>`.
//!
//! **A link belongs to a Doc in the library** (#1493), not to a block: every
//! lookup goes by the Doc's id (`Entry::doc`), so a Doc that was closed and
//! opened in another block has its menu back. `Entry::block` is the old key:
//! read, so the start-up migration can map it to a Doc, and not written by a
//! new share.
//!
//! A version is `served + 1` on each Update, kept in the store. Control
//! refusing one as `stale` is the daemon's store being behind; it says so
//! once and doesn't loop. A 404 for a link we hold means it was unshared
//! elsewhere: the key is forgotten and the Doc is told ([`Gone`]).
//!
//! The web's `doc-link.ts` opens these; `tests/fixtures/docs/link-vector.json`
//! is made here and checked on both sides.

use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    sync::{Arc, Mutex, OnceLock},
};

use aes_gcm::{
    Aes256Gcm, KeyInit,
    aead::{Aead, Payload},
};
use arugula_core::PaneId;
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use futures_util::future::BoxFuture;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use tracing::info;

use crate::store::{now_ms, write_atomic};

/// The snapshot's first four bytes.
pub const MAGIC: &[u8; 4] = b"ADL1";
/// What a part adds to its plaintext: the nonce and the tag.
const OVERHEAD: usize = 12 + 16;
/// The most plaintext in one part, so that a sealed one is at most 1 MiB.
pub const PART_PLAIN: usize = (1 << 20) - OVERHEAD;
/// Control's limit on parts (a part number is below it).
const MAX_PARTS: usize = 48;

/// What an agent is told, and the owner's CLI when an agent runs it.
pub const AGENT_ASKS: &str = "an agent doesn't share a Doc: ask with the doc tool's share";
/// Said to someone who isn't the owner, and to a team's other owner: the
/// link's key is the sharing account's.
pub const OWNER_ONLY: &str = "sharing a Doc is the machine's own account's";
/// The block methods that publish, change or read back a link.
pub const METHODS: [&str; 4] = ["doc_share", "doc_share_update", "doc_unshare", "doc_shares"];
/// The store, in the state directory.
pub const FILE: &str = "doc-links.json";

pub const UNSHARED_ELSEWHERE: &str = "this Doc was unshared elsewhere; share it again for a new link";
const NOT_JOINED: &str = crate::control::DOC_NOT_JOINED;

// ----------------------------------------------------------------- remote

/// Control, as sharing sees it. `Control` is the real one; tests put a fake
/// that remembers what it was sent.
pub trait Remote: Send + Sync {
    /// The viewer's base URL, or why this control can't keep Docs.
    fn viewer(&self) -> BoxFuture<'_, Result<String, String>>;
    /// A signed call to `/api/doc-links/…`: the status and the JSON.
    fn call(&self, method: &'static str, path: String, body: Vec<u8>) -> BoxFuture<'_, Result<(u16, Value), String>>;
    /// The status of an anonymous GET.
    fn probe(&self, url: String) -> BoxFuture<'_, Result<u16, String>>;
}

impl Remote for crate::control::Control {
    fn viewer(&self) -> BoxFuture<'_, Result<String, String>> {
        Box::pin(self.doc_viewer())
    }

    fn call(&self, method: &'static str, path: String, body: Vec<u8>) -> BoxFuture<'_, Result<(u16, Value), String>> {
        Box::pin(async move { self.doc_call(method, &path, body).await })
    }

    fn probe(&self, url: String) -> BoxFuture<'_, Result<u16, String>> {
        Box::pin(async move { self.doc_probe(&url).await })
    }
}

static REMOTE: OnceLock<Arc<dyn Remote>> = OnceLock::new();

/// The daemon's control, for Doc blocks made after this.
pub fn init(control: Arc<crate::control::Control>) {
    let _ = REMOTE.set(control);
}

pub fn remote() -> Option<Arc<dyn Remote>> {
    REMOTE.get().cloned()
}

// ------------------------------------------------------------------ store

/// One shared Doc: the key, and what the account list can't know.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Entry {
    /// base64url, no padding.
    pub key: String,
    /// The version control serves.
    pub ver: u64,
    /// The Doc in the library that made it (#1493): every lookup goes by
    /// this. Empty only in an entry from before the library that no
    /// migration has mapped yet.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub doc: Option<String>,
    /// The Doc block that made it, in an entry from before the library.
    /// Read so the migration can map it to `doc`; never written.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub block: Option<PaneId>,
    pub title: String,
    pub made: u64,
    pub updated: u64,
    /// Ciphertext served, in bytes.
    pub size: u64,
    /// The viewer's base URL when it was shared.
    pub viewer: String,
}

impl Entry {
    pub fn url(&self, id: &str) -> String {
        format!("{}/{id}#k={}", self.viewer, self.key)
    }
}

/// `<state>/doc-links.json`: a map from link id to [`Entry`]. Read whole and
/// written whole under one lock; a file that doesn't parse is left as it is
/// and reported.
fn store_lock() -> std::sync::MutexGuard<'static, ()> {
    static L: Mutex<()> = Mutex::new(());
    L.lock().unwrap_or_else(|e| e.into_inner())
}

fn read_all(dir: &Path) -> Result<BTreeMap<String, Entry>, String> {
    let path = dir.join(FILE);
    match std::fs::read(&path) {
        Ok(b) => serde_json::from_slice(&b)
            .map_err(|_| format!("{} doesn't parse; it is left as it is: tell the person", path.display())),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(BTreeMap::new()),
        Err(e) => Err(format!("can't read {}: {e}", path.display())),
    }
}

fn write_all(dir: &Path, all: &BTreeMap<String, Entry>) -> Result<(), String> {
    let bytes = serde_json::to_vec_pretty(all).map_err(|e| e.to_string())?;
    write_atomic(&dir.join(FILE), &bytes).map_err(|e| format!("can't save the link's key: {e}"))
}

pub fn get(dir: &Path, id: &str) -> Result<Option<Entry>, String> {
    let _g = store_lock();
    Ok(read_all(dir)?.remove(id))
}

/// The link a Doc in the library has, if it still has one.
pub fn of_doc(dir: &Path, doc: &str) -> Option<(String, Entry)> {
    let _g = store_lock();
    read_all(dir).ok()?.into_iter().find(|(_, e)| e.doc.as_deref() == Some(doc))
}

/// Every link that has a Doc, by Doc id: `(link id, entry)`.
pub fn by_doc(dir: &Path) -> BTreeMap<String, (String, Entry)> {
    let _g = store_lock();
    let all = read_all(dir).unwrap_or_default();
    all.into_iter().filter_map(|(id, e)| Some((e.doc.clone()?, (id, e)))).collect()
}

/// The migration (#1493): give each link an entry from a Doc block a Doc,
/// from `docs` (block to Doc id). Whether the file changed. A link whose
/// block isn't in `docs` is left as it is.
pub fn map_blocks(dir: &Path, docs: &std::collections::HashMap<PaneId, String>) -> Result<bool, String> {
    let _g = store_lock();
    let mut all = read_all(dir)?;
    let mut changed = false;
    for e in all.values_mut().filter(|e| e.doc.is_none()) {
        if let Some(doc) = e.block.and_then(|b| docs.get(&b)) {
            e.doc = Some(doc.clone());
            e.block = None;
            changed = true;
        }
    }
    if changed {
        write_all(dir, &all)?;
    }
    Ok(changed)
}

fn put(dir: &Path, id: &str, e: Entry) -> Result<(), String> {
    let _g = store_lock();
    let mut all = read_all(dir)?;
    all.insert(id.to_owned(), e);
    write_all(dir, &all)
}

fn forget(dir: &Path, id: &str) -> Result<(), String> {
    let _g = store_lock();
    let mut all = read_all(dir)?;
    if all.remove(id).is_some() {
        write_all(dir, &all)?;
    }
    Ok(())
}

// --------------------------------------------------------------- snapshot

#[derive(Debug, Serialize, Deserialize)]
pub struct Manifest {
    pub title: String,
    pub entries: Vec<ManifestEntry>,
    pub assets: Vec<ManifestAsset>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ManifestEntry {
    pub id: String,
    pub heading: String,
    pub intent: String,
    pub body: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ManifestAsset {
    pub name: String,
    pub mime: String,
    pub size: u64,
}

/// The plaintext snapshot: the manifest and each asset's bytes, in order.
pub fn snapshot(manifest: &Manifest, blobs: &[Vec<u8>]) -> Vec<u8> {
    let json = serde_json::to_vec(manifest).expect("a manifest is JSON");
    let mut out = Vec::with_capacity(8 + json.len() + blobs.iter().map(Vec::len).sum::<usize>());
    out.extend_from_slice(MAGIC);
    out.extend_from_slice(&(json.len() as u32).to_be_bytes());
    out.extend_from_slice(&json);
    for b in blobs {
        out.extend_from_slice(b);
    }
    out
}

/// The additional data of part `n` of `parts`.
fn aad(id: &str, ver: u64, n: usize, parts: usize) -> String {
    format!("ADL1|{id}|{ver}|{n}|{parts}")
}

/// Cut `plain` into parts of at most `part_max` and seal each with a nonce
/// from `nonce`. At least one part, even for nothing.
pub fn seal(
    key: &[u8; 32],
    id: &str,
    ver: u64,
    plain: &[u8],
    part_max: usize,
    nonce: &mut dyn FnMut() -> [u8; 12],
) -> Result<Vec<Vec<u8>>, String> {
    let cipher = Aes256Gcm::new_from_slice(key).map_err(|_| "a link's key is 32 bytes".to_owned())?;
    let chunks: Vec<&[u8]> = if plain.is_empty() { vec![plain] } else { plain.chunks(part_max).collect() };
    let total = chunks.len();
    chunks
        .into_iter()
        .enumerate()
        .map(|(n, chunk)| {
            let iv = nonce();
            let ct = cipher
                .encrypt(&iv.into(), Payload { msg: chunk, aad: aad(id, ver, n, total).as_bytes() })
                .map_err(|_| "can't seal a part of the Doc".to_owned())?;
            let mut part = Vec::with_capacity(OVERHEAD + chunk.len());
            part.extend_from_slice(&iv);
            part.extend_from_slice(&ct);
            Ok(part)
        })
        .collect()
}

/// The plaintext of `parts` sealed by [`seal`], or `None` if any part
/// doesn't open. The web's `openSnapshot` does the same.
#[cfg(test)]
pub fn open(key: &[u8; 32], id: &str, ver: u64, parts: &[Vec<u8>]) -> Option<Vec<u8>> {
    let cipher = Aes256Gcm::new_from_slice(key).ok()?;
    let mut out = Vec::new();
    for (n, p) in parts.iter().enumerate() {
        let (iv, ct) = p.split_at_checked(12)?;
        let iv: [u8; 12] = iv.try_into().ok()?;
        out.extend(cipher.decrypt(&iv.into(), Payload { msg: ct, aad: aad(id, ver, n, parts.len()).as_bytes() }).ok()?);
    }
    Some(out)
}

fn random<const N: usize>() -> [u8; N] {
    crate::push::random()
}

// ------------------------------------------------------------------- link

/// A Doc's way to share: where the keys are, which Doc in the library it
/// is, and control.
#[derive(Clone)]
pub struct Link {
    pub state_dir: PathBuf,
    pub doc: String,
    pub remote: Option<Arc<dyn Remote>>,
    /// Tests fix the key, the id, the nonces and the size of a part.
    #[cfg(test)]
    pub seam: Option<Seam>,
}

#[cfg(test)]
#[derive(Clone, Default)]
pub struct Seam {
    pub key: Option<[u8; 32]>,
    pub id: Option<String>,
    pub part_max: Option<usize>,
    /// Nonces are `0, 1, 2…` big-endian from here instead of random.
    pub counted_nonces: bool,
}

/// What sharing, updating or reading a link answers.
#[derive(Debug, PartialEq, Eq)]
pub struct Shared {
    pub url: String,
    pub made: u64,
    pub updated: u64,
}

/// Why a share call failed.
#[derive(Debug, PartialEq, Eq)]
pub enum Fail {
    /// Said in one line.
    Said(String),
    /// Control doesn't have the link any more: the key is forgotten, and the
    /// Doc clears `shared`.
    Gone,
}

impl From<String> for Fail {
    fn from(s: String) -> Self {
        Fail::Said(s)
    }
}

/// One line for a refusal of control's: its `reason`, with what to do.
fn refused(status: u16, v: &Value) -> String {
    let said = |fallback: &str| {
        v["error"].as_str().filter(|s| !s.is_empty() && !s.contains('\n')).unwrap_or(fallback).to_owned()
    };
    match v["reason"].as_str() {
        Some("stale") => "control already serves a newer copy of this link: Unshare it and share the Doc again".into(),
        Some("incomplete") => "control didn't get every part: try again".into(),
        Some("part") => "a part of the Doc was over 1 MB for control: try again".into(),
        Some("too_big") => said("this Doc is too big to share: shorten it or drop a picture"),
        Some("quota") => said("your shared Docs are over the account's limit: unshare one first"),
        Some("links") => said("this account has all the shared Docs it may: unshare one first"),
        Some("disk") => said("control's disk is nearly full: try again later"),
        _ => match status {
            401 | 403 => "control didn't accept this machine's signature: rejoin control and try again".into(),
            410 => "control says this machine was removed: join control again".into(),
            _ => format!("control said {status}: try again"),
        },
    }
}

impl Link {
    pub fn none(state_dir: PathBuf, doc: String) -> Self {
        Link {
            state_dir,
            doc,
            remote: None,
            #[cfg(test)]
            seam: None,
        }
    }

    fn remote(&self) -> Result<&Arc<dyn Remote>, String> {
        self.remote.as_ref().ok_or_else(|| NOT_JOINED.to_owned())
    }

    fn key(&self) -> [u8; 32] {
        #[cfg(test)]
        if let Some(k) = self.seam.as_ref().and_then(|s| s.key) {
            return k;
        }
        random()
    }

    fn new_id(&self) -> String {
        #[cfg(test)]
        if let Some(id) = self.seam.as_ref().and_then(|s| s.id.clone()) {
            return id;
        }
        hex::encode(random::<16>())
    }

    fn part_max(&self) -> usize {
        #[cfg(test)]
        if let Some(m) = self.seam.as_ref().and_then(|s| s.part_max) {
            return m;
        }
        PART_PLAIN
    }

    fn nonces(&self) -> Box<dyn FnMut() -> [u8; 12] + Send> {
        #[cfg(test)]
        if self.seam.as_ref().is_some_and(|s| s.counted_nonces) {
            let mut n = 0u64;
            return Box::new(move || {
                let mut iv = [0u8; 12];
                iv[4..].copy_from_slice(&n.to_be_bytes());
                n += 1;
                iv
            });
        }
        Box::new(random::<12>)
    }

    /// The link this Doc has, if it has one.
    pub fn current(&self) -> Option<(String, Entry)> {
        of_doc(&self.state_dir, &self.doc)
    }

    /// Whether this control can keep Docs, or why not, for a menu to say.
    pub async fn available(&self) -> Result<(), String> {
        self.remote()?.viewer().await.map(|_| ())
    }

    /// Make a link for `plain`. Refuses a Doc that has one.
    pub async fn share(&self, title: &str, plain: Vec<u8>) -> Result<Shared, Fail> {
        if self.current().is_some() {
            return Err("this Doc is already shared: use Update to change what its link shows".to_owned().into());
        }
        let remote = self.remote()?;
        let viewer = remote.viewer().await?;
        let (id, key) = (self.new_id(), self.key());
        let now = now_ms();
        let mut entry = Entry {
            key: URL_SAFE_NO_PAD.encode(key),
            ver: 0,
            doc: Some(self.doc.clone()),
            block: None,
            title: title.to_owned(),
            made: now,
            updated: now,
            size: 0,
            viewer,
        };
        self.push(&id, &mut entry, &key, plain, None).await?;
        if let Err(e) = put(&self.state_dir, &id, entry.clone()) {
            // Served, but its key is lost: take it down rather than leave
            // a link nobody can open.
            let _ = remote.call("DELETE", format!("/api/doc-links/{id}"), vec![]).await;
            return Err(e.into());
        }
        Ok(Shared { url: entry.url(&id), made: entry.made, updated: entry.updated })
    }

    /// A new version under the same id and key.
    pub async fn update(&self, title: &str, plain: Vec<u8>) -> Result<Shared, Fail> {
        let Some((id, mut entry)) = self.current() else {
            return Err("this Doc isn't shared: use Share link first".to_owned().into());
        };
        let key = decode_key(&entry.key)?;
        entry.title = title.to_owned();
        entry.updated = now_ms();
        let old = entry.size;
        self.push(&id, &mut entry, &key, plain, Some(old)).await.inspect_err(|e| {
            if *e == Fail::Gone {
                let _ = forget(&self.state_dir, &id);
            }
        })?;
        put(&self.state_dir, &id, entry.clone())?;
        Ok(Shared { url: entry.url(&id), made: entry.made, updated: entry.updated })
    }

    /// Check the quota, seal `plain` as `entry.ver + 1`, upload the parts and
    /// commit; on success `entry` has the new version and size. The store is
    /// the caller's to write.
    async fn push(
        &self,
        id: &str,
        entry: &mut Entry,
        key: &[u8; 32],
        plain: Vec<u8>,
        replacing: Option<u64>,
    ) -> Result<(), Fail> {
        let remote = self.remote()?;
        let ver = entry.ver + 1;
        let mb = |n: u64| n >> 20;
        let parts_n = plain.len().div_ceil(self.part_max()).max(1);
        let size = (plain.len() + parts_n * OVERHEAD) as u64;
        // Before anything is sealed or sent.
        let (status, q) = remote.call("GET", "/api/doc-links/quota".into(), vec![]).await?;
        if status != 200 {
            return Err(refused(status, &q).into());
        }
        let n = |k: &str| q[k].as_u64().unwrap_or(0);
        if parts_n > MAX_PARTS || size > n("max_link").max(1) {
            return Err(format!(
                "this Doc is {} MB once encrypted and a shared one is at most {} MB: shorten it or drop a picture",
                mb(size).max(1),
                mb(n("max_link"))
            )
            .into());
        }
        if replacing.is_none() && n("links") >= n("max_links") {
            return Err(
                format!("this account has {} shared Docs, the most it may: unshare one first", n("links")).into()
            );
        }
        if n("used").saturating_sub(replacing.unwrap_or(0)) + size > n("limit") {
            return Err(format!(
                "this Doc would take the account past its {} MB of shared Docs: unshare one first",
                mb(n("limit"))
            )
            .into());
        }
        let sealed = seal(key, id, ver, &plain, self.part_max(), &mut *self.nonces())?;
        for (i, part) in sealed.iter().enumerate() {
            let (status, v) =
                remote.call("PUT", format!("/api/doc-links/{id}/v/{ver}/parts/{i}"), part.clone()).await?;
            self.check(status, &v, replacing.is_some())?;
        }
        let body = serde_json::to_vec(&json!({ "parts": sealed.len(), "size": size })).expect("JSON");
        let (status, v) = remote.call("POST", format!("/api/doc-links/{id}/v/{ver}/commit"), body).await?;
        self.check(status, &v, replacing.is_some())?;
        (entry.ver, entry.size) = (ver, size);
        info!(link = id, ver, parts = sealed.len(), size, "shared a Doc");
        Ok(())
    }

    fn check(&self, status: u16, v: &Value, known: bool) -> Result<(), Fail> {
        match status {
            200 => Ok(()),
            404 if known => Err(Fail::Gone),
            _ => Err(refused(status, v).into()),
        }
    }

    /// What this Doc's link is, asking control (anonymously, as a visitor
    /// would) whether it still serves it.
    pub async fn shares(&self) -> Result<Option<Shared>, Fail> {
        let Some((id, entry)) = self.current() else { return Ok(None) };
        if let Some(remote) = &self.remote
            && let Ok(404) = remote.probe(format!("{}/{id}/data/meta", entry.viewer)).await
        {
            forget(&self.state_dir, &id)?;
            return Err(Fail::Gone);
        }
        Ok(Some(Shared { url: entry.url(&id), made: entry.made, updated: entry.updated }))
    }

    /// Unshare `id` (default this Doc's): control deletes it (a 404 is as
    /// good), then the key goes.
    pub async fn unshare(&self, id: Option<&str>) -> Result<(), String> {
        let id = match id {
            Some(id) => id.to_owned(),
            None => self.current().ok_or("this Doc isn't shared")?.0,
        };
        unshare_id(&self.state_dir, self.remote()?, &id).await
    }
}

fn decode_key(k: &str) -> Result<[u8; 32], String> {
    URL_SAFE_NO_PAD
        .decode(k)
        .ok()
        .and_then(|b| b.try_into().ok())
        .ok_or_else(|| format!("the key in {FILE} isn't one: Unshare this Doc and share it again"))
}

/// Unshare a link by its id, through control: what the account list does,
/// and what a Doc's Unshare is.
pub async fn unshare_id(dir: &Path, remote: &Arc<dyn Remote>, id: &str) -> Result<(), String> {
    if get(dir, id)?.is_none() {
        return Err("no such shared Doc on this machine".into());
    }
    let (status, v) = remote.call("DELETE", format!("/api/doc-links/{id}"), vec![]).await?;
    if status != 200 && status != 404 {
        return Err(refused(status, &v));
    }
    info!(link = id, "unshared a Doc");
    forget(dir, id)
}

/// The URL of a link this machine made, key and all, for the account list.
pub fn url_of(dir: &Path, id: &str) -> Result<Option<String>, String> {
    Ok(get(dir, id)?.map(|e| e.url(id)))
}

/// Whether `id` is what a link is called: 32 lower-case hex digits.
pub fn is_id(id: &str) -> bool {
    id.len() == 32 && id.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
}

#[cfg(test)]
pub mod fake {
    //! A control that keeps what it is sent, for tests of sharing.
    use super::*;

    #[derive(Default)]
    pub struct Fake {
        pub viewer: Mutex<Option<String>>,
        /// `(method, path)` of every signed call, in order.
        pub calls: Mutex<Vec<(String, String)>>,
        /// The parts held, by `(id, ver, n)`.
        pub parts: Mutex<BTreeMap<(String, u64, usize), Vec<u8>>>,
        /// The served `(ver, parts, size)`, by id.
        pub served: Mutex<BTreeMap<String, (u64, usize, u64)>>,
        /// Answer the next calls (any path) with this status and reason.
        pub refuse: Mutex<Option<(u16, &'static str)>>,
        /// Answer 404 to calls about a link, and to the anonymous probe.
        pub gone: Mutex<bool>,
        pub quota: Mutex<Option<Value>>,
    }

    impl Fake {
        pub fn new() -> Arc<Fake> {
            Arc::new(Fake { viewer: Mutex::new(Some("https://docs.test".into())), ..Default::default() })
        }

        pub fn paths(&self) -> Vec<String> {
            self.calls.lock().unwrap().iter().map(|(m, p)| format!("{m} {p}")).collect()
        }
    }

    impl Remote for Fake {
        fn viewer(&self) -> BoxFuture<'_, Result<String, String>> {
            let v = self.viewer.lock().unwrap().clone();
            Box::pin(async move { v.ok_or_else(|| crate::control::DOC_NO_LINKS.to_owned()) })
        }

        fn call(
            &self,
            method: &'static str,
            path: String,
            body: Vec<u8>,
        ) -> BoxFuture<'_, Result<(u16, Value), String>> {
            self.calls.lock().unwrap().push((method.into(), path.clone()));
            let gone = *self.gone.lock().unwrap();
            let refuse = *self.refuse.lock().unwrap();
            let segs: Vec<&str> = path.trim_start_matches('/').split('/').collect();
            let out = match (method, segs.as_slice()) {
                (_, _) if refuse.is_some() && !path.ends_with("/quota") => {
                    let (s, r) = refuse.unwrap();
                    (s, json!({ "error": format!("control says {r}"), "reason": r }))
                }
                ("GET", ["api", "doc-links", "quota"]) => (
                    200,
                    self.quota.lock().unwrap().clone().unwrap_or_else(
                        || json!({ "used": 0, "limit": 200 << 20, "links": 0, "max_links": 100, "max_link": 48 << 20 }),
                    ),
                ),
                (_, ["api", "doc-links", ..]) if gone => (404, json!({ "error": "no such link" })),
                ("PUT", ["api", "doc-links", id, "v", ver, "parts", n]) => {
                    let key = (id.to_string(), ver.parse().unwrap(), n.parse().unwrap());
                    self.parts.lock().unwrap().insert(key, body);
                    (200, json!({}))
                }
                ("POST", ["api", "doc-links", id, "v", ver, "commit"]) => {
                    let c: Value = serde_json::from_slice(&body).unwrap();
                    self.served.lock().unwrap().insert(
                        id.to_string(),
                        (ver.parse().unwrap(), c["parts"].as_u64().unwrap() as usize, c["size"].as_u64().unwrap()),
                    );
                    (200, json!({}))
                }
                ("DELETE", ["api", "doc-links", id]) => {
                    self.served.lock().unwrap().remove(*id);
                    (200, json!({}))
                }
                _ => (404, json!({})),
            };
            Box::pin(async move { Ok(out) })
        }

        fn probe(&self, _url: String) -> BoxFuture<'_, Result<u16, String>> {
            let s = if *self.gone.lock().unwrap() { 404 } else { 200 };
            Box::pin(async move { Ok(s) })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ID: &str = "00112233445566778899aabbccddeeff";
    const VER: u64 = 3;

    fn key() -> [u8; 32] {
        std::array::from_fn(|i| i as u8)
    }

    /// Nonces `0, 1, 2…`, as `Seam::counted_nonces`.
    fn nonces() -> impl FnMut() -> [u8; 12] {
        let mut n = 0u64;
        move || {
            let mut iv = [0u8; 12];
            iv[4..].copy_from_slice(&n.to_be_bytes());
            n += 1;
            iv
        }
    }

    fn vector_doc() -> (Manifest, Vec<Vec<u8>>) {
        let png = vec![137, 80, 78, 71, 13, 10, 26, 10, 1, 2, 3];
        let manifest = Manifest {
            title: "Vector".into(),
            entries: vec![
                ManifestEntry {
                    id: "s1".into(),
                    heading: "Intro".into(),
                    intent: "what it says".into(),
                    body: Some("Hello, **world**. \u{1F96C}\n\n![dot](assets/dot.png)".into()),
                },
                ManifestEntry { id: "s2".into(), heading: "Later".into(), intent: String::new(), body: None },
            ],
            assets: vec![ManifestAsset { name: "dot.png".into(), mime: "image/png".into(), size: png.len() as u64 }],
        };
        (manifest, vec![png])
    }

    /// Small parts, so the vector has several to swap.
    const VECTOR_PART: usize = 64;

    fn vector() -> Value {
        use base64::engine::general_purpose::STANDARD;
        let (manifest, blobs) = vector_doc();
        let plain = snapshot(&manifest, &blobs);
        let parts = seal(&key(), ID, VER, &plain, VECTOR_PART, &mut nonces()).unwrap();
        json!({
            "id": ID,
            "ver": VER,
            "key": URL_SAFE_NO_PAD.encode(key()),
            "parts": parts.iter().map(|p| STANDARD.encode(p)).collect::<Vec<_>>(),
            "manifest": manifest,
            "assets": manifest.assets.iter().zip(&blobs)
                .map(|(a, b)| json!({ "name": a.name, "b64": STANDARD.encode(b) })).collect::<Vec<_>>(),
        })
    }

    fn fixture_path() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/docs/link-vector.json")
    }

    /// The committed vector is what the encrypt path makes, byte for byte,
    /// so it can't drift from the code; `UPDATE_LINK_VECTOR=1` rewrites it.
    /// The web's spec (`doc-link.spec.ts`) opens the same file.
    #[test]
    fn the_test_vector_is_what_sealing_makes_and_it_opens() {
        let made = vector();
        if std::env::var_os("UPDATE_LINK_VECTOR").is_some() {
            std::fs::write(fixture_path(), serde_json::to_string_pretty(&made).unwrap() + "\n").unwrap();
        }
        let on_disk: Value = serde_json::from_slice(&std::fs::read(fixture_path()).unwrap()).unwrap();
        assert_eq!(on_disk, made, "link-vector.json is stale: run with UPDATE_LINK_VECTOR=1");
        assert!(made["parts"].as_array().unwrap().len() >= 4, "several parts to swap");

        use base64::engine::general_purpose::STANDARD;
        let parts: Vec<Vec<u8>> = on_disk["parts"]
            .as_array()
            .unwrap()
            .iter()
            .map(|p| STANDARD.decode(p.as_str().unwrap()).unwrap())
            .collect();
        let (manifest, blobs) = vector_doc();
        assert_eq!(open(&key(), ID, VER, &parts).unwrap(), snapshot(&manifest, &blobs));
        assert!(parts.iter().all(|p| p.len() <= (1 << 20)));
        // Every part has its own nonce.
        let mut ivs: Vec<&[u8]> = parts.iter().map(|p| &p[..12]).collect();
        ivs.sort();
        ivs.dedup();
        assert_eq!(ivs.len(), parts.len());
    }

    #[test]
    fn a_swapped_part_another_version_or_id_or_a_cut_off_end_or_a_wrong_key_doesnt_open() {
        let (manifest, blobs) = vector_doc();
        let plain = snapshot(&manifest, &blobs);
        let parts = seal(&key(), ID, VER, &plain, VECTOR_PART, &mut nonces()).unwrap();
        assert!(open(&key(), ID, VER, &parts).is_some());
        let mut swapped = parts.clone();
        swapped.swap(1, 2);
        assert!(open(&key(), ID, VER, &swapped).is_none(), "swapped");
        assert!(open(&key(), ID, VER, &parts[..parts.len() - 1]).is_none(), "truncated");
        let mut extra = parts.clone();
        extra.push(parts[0].clone());
        assert!(open(&key(), ID, VER, &extra).is_none(), "one more");
        assert!(open(&key(), ID, VER + 1, &parts).is_none(), "another version");
        assert!(open(&key(), &ID.replace('0', "1"), VER, &parts).is_none(), "another link");
        let other = seal(&key(), ID, VER + 1, &plain, VECTOR_PART, &mut nonces()).unwrap();
        let mut mixed = parts.clone();
        mixed[0] = other[0].clone();
        assert!(open(&key(), ID, VER, &mixed).is_none(), "part 0 of another version");
        let mut bad = key();
        bad[0] ^= 1;
        assert!(open(&bad, ID, VER, &parts).is_none(), "wrong key");
        let mut flipped = parts.clone();
        flipped[3][20] ^= 1;
        assert!(open(&key(), ID, VER, &flipped).is_none(), "a bit flipped");
    }

    #[test]
    fn a_part_is_at_most_a_megabyte_and_a_doc_is_cut_there() {
        let plain = vec![7u8; PART_PLAIN * 2 + 5];
        let parts = seal(&key(), ID, 1, &plain, PART_PLAIN, &mut nonces()).unwrap();
        assert_eq!(parts.len(), 3);
        assert_eq!(parts[0].len(), 1 << 20, "a full part is exactly 1 MiB");
        assert!(parts.iter().all(|p| p.len() <= 1 << 20));
        assert_eq!(open(&key(), ID, 1, &parts).unwrap(), plain);
        // Nothing still makes a part (an empty Doc is never shared, but the
        // format has no zero-part snapshot).
        assert_eq!(seal(&key(), ID, 1, &[], PART_PLAIN, &mut nonces()).unwrap().len(), 1);
    }

    #[test]
    fn real_nonces_are_fresh_for_every_part() {
        let plain = vec![1u8; 4 * 100];
        let parts = seal(&key(), ID, 1, &plain, 100, &mut random::<12>).unwrap();
        let mut ivs: Vec<[u8; 12]> = parts.iter().map(|p| p[..12].try_into().unwrap()).collect();
        ivs.sort();
        ivs.dedup();
        assert_eq!(ivs.len(), 4);
    }
}
