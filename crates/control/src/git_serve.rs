//! An integration's folder served as a git repository (#1748), so a plugin
//! manager (lazy.nvim, vim-plug, packer, tpm) can install it from a URL:
//! `GET /git/nvim.git/info/refs?service=git-upload-pack` and
//! `POST /git/nvim.git/git-upload-pack`.
//!
//! Control's image has no `git` and no shell, so this speaks the least of
//! git's smart HTTP protocol (v0, `gitprotocol-http` and `gitprotocol-pack`)
//! that clients need, and builds the repository in memory:
//!
//! - The folders are an allowlist in build.rs (`GIT_FOLDERS`). For each,
//!   build.rs reads the folder's tree at every release tag `v*` (up to the
//!   workspace version) with git and embeds the files of each tree that
//!   changed from the release before it.
//! - Each of those trees becomes blobs, trees and one commit by the fixed
//!   author `Arugula <noreply@arugula.io>` at the date of the first tag with
//!   that tree, with the previous commit as its parent. So the ids are the
//!   same on every instance built from the same tags, history is a chain
//!   that fast-forwards (`git pull`, vim-plug), and an old commit stays in
//!   the chain (a lockfile's pin, a `tag =`).
//! - Refs: `HEAD` (a symref to `refs/heads/main`), `refs/heads/main` at the
//!   newest commit, and a `refs/tags/v<version>` for every release, each at
//!   the commit that last changed the folder.
//! - A fetch gets the objects of what it wants as one pack of undeltified
//!   objects: the commits to the `deepen` depth (or all of them, less what
//!   is behind a commit the client says is `shallow`), each with its whole
//!   tree. `have` lines only decide the acknowledgements (git stops asking
//!   when it hears one `ready`). The `filter` capability isn't offered, so
//!   a client drops its filter.
//! - Pushing is refused: only `git-upload-pack` is a service.
//!
//! Protocol v2 isn't offered; a client that asks for it falls back to v0.

use std::{
    collections::{BTreeMap, HashMap},
    io::{Read, Write},
    net::SocketAddr,
    sync::{Arc, LazyLock},
};

use arugula_control_common::error::{ApiError, err};
use axum::{
    body::Bytes,
    extract::{ConnectInfo, Path, Query, State},
    http::{HeaderMap, HeaderValue, StatusCode, header},
    response::{IntoResponse, Response},
};
use sha1::{Digest, Sha1};

use crate::{App, limit};

/// A file of a release: its path from the folder, bytes, whether executable.
pub type File<'a> = (&'a str, &'a [u8], bool);

/// A commit of the chain: the release tags whose folder is `files` (the
/// first is the release that changed it), and that first tag's date.
pub struct Release<'a> {
    pub tags: &'a [&'a str],
    pub epoch: i64,
    pub files: &'a [File<'a>],
}

include!(concat!(env!("OUT_DIR"), "/git_folders.rs"));

/// The largest request body (compressed, as it comes). A clone's is a few
/// hundred bytes; a fetch with many haves is a few KiB.
pub const MAX_BODY: usize = 64 * 1024;
/// And what a gzip body may inflate to.
const MAX_INFLATED: u64 = 1024 * 1024;

const SERVICE: &str = "git-upload-pack";
/// What the server offers. `multi_ack_detailed` and `no-done` let a stateless
/// fetch finish in one round; `side-band-64k` carries the pack; `shallow`
/// answers `--depth`; `ofs-delta` is allowed but never used.
const CAPS: &str =
    "multi_ack_detailed no-done side-band-64k ofs-delta shallow symref=HEAD:refs/heads/main agent=arugula";

type Id = [u8; 20];

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Kind {
    Commit = 1,
    Tree = 2,
    Blob = 3,
}

impl Kind {
    fn name(self) -> &'static str {
        match self {
            Kind::Commit => "commit",
            Kind::Tree => "tree",
            Kind::Blob => "blob",
        }
    }
}

/// An object's id: SHA-1 of `<kind> <len>\0<data>`.
fn object_id(kind: Kind, data: &[u8]) -> Id {
    let mut h = Sha1::new();
    h.update(format!("{} {}\0", kind.name(), data.len()).as_bytes());
    h.update(data);
    h.finalize().into()
}

fn hex(id: &Id) -> String {
    ::hex::encode(id)
}

fn parse_id(s: &str) -> Option<Id> {
    ::hex::decode(s).ok()?.try_into().ok()
}

/// What a fetch is sent, from `Repo::select`.
struct Selection {
    objects: Vec<Id>,
    shallow: Vec<Id>,
    unshallow: Vec<Id>,
}

/// A commit's tree and parents.
fn commit_links(data: &[u8]) -> (Option<Id>, Vec<Id>) {
    let (mut tree, mut parents) = (None, vec![]);
    for line in data.split(|b| *b == b'\n').take_while(|l| !l.is_empty()) {
        let line = String::from_utf8_lossy(line);
        if let Some(h) = line.strip_prefix("tree ") {
            tree = parse_id(h);
        } else if let Some(h) = line.strip_prefix("parent ") {
            parents.extend(parse_id(h));
        }
    }
    (tree, parents)
}

/// One folder's repository, in memory.
pub struct Repo {
    objects: BTreeMap<Id, (Kind, Vec<u8>)>,
    /// Every ref, by name, sorted, `HEAD` first.
    refs: Vec<(String, Id)>,
}

impl Repo {
    /// The chain of `releases`, oldest first, for the folder `name`.
    pub fn build(name: &str, releases: &[Release]) -> Repo {
        let mut objects = BTreeMap::new();
        let mut tags = vec![];
        let mut parent: Option<Id> = None;
        for r in releases {
            let tree = build_tree(r.files, &mut objects);
            let who = format!("Arugula <noreply@arugula.io> {} +0000", r.epoch);
            let parent_line = parent.map(|p| format!("parent {}\n", hex(&p))).unwrap_or_default();
            let subject = format!("arugula {name} {}", r.tags.first().copied().unwrap_or("release"));
            let commit =
                format!("tree {}\n{parent_line}author {who}\ncommitter {who}\n\n{subject}\n", hex(&tree)).into_bytes();
            let id = object_id(Kind::Commit, &commit);
            objects.insert(id, (Kind::Commit, commit));
            tags.extend(r.tags.iter().map(|t| (format!("refs/tags/{t}"), id)));
            parent = Some(id);
        }
        let head = parent.expect("a folder has at least one release");
        tags.sort();
        let mut refs = vec![("HEAD".to_owned(), head), ("refs/heads/main".to_owned(), head)];
        refs.extend(tags);
        Repo { objects, refs }
    }

    fn advertisement(&self) -> Vec<u8> {
        let mut out = vec![];
        pkt(&mut out, format!("# service={SERVICE}\n").as_bytes());
        out.extend_from_slice(b"0000");
        for (i, (name, id)) in self.refs.iter().enumerate() {
            let line = if i == 0 { format!("{} {name}\0{CAPS}\n", hex(id)) } else { format!("{} {name}\n", hex(id)) };
            pkt(&mut out, line.as_bytes());
        }
        out.extend_from_slice(b"0000");
        out
    }

    /// What a fetch is sent. From the wanted commits, the commits within
    /// `depth` (all, if none), counting a want as 1: the commits at the
    /// limit are `shallow`. Without a depth, the walk stops at a commit the
    /// client says is shallow, and with one, the client's shallow commits
    /// it now has the parents of are `unshallow`. Then each commit's tree,
    /// and anything else wanted whole: commits first, then trees, then
    /// blobs.
    fn select(&self, wants: &[Id], depth: Option<u32>, client_shallow: &[Id]) -> Selection {
        let mut seen = std::collections::BTreeSet::new();
        let mut commits = vec![];
        let (mut shallow, mut unshallow) = (vec![], vec![]);
        let mut queue: std::collections::VecDeque<(Id, u32)> = wants.iter().map(|w| (*w, 1)).collect();
        let mut rest = vec![];
        while let Some((id, d)) = queue.pop_front() {
            let Some((kind, data)) = self.objects.get(&id) else { continue };
            if *kind != Kind::Commit {
                rest.push(id);
                continue;
            }
            if !seen.insert(id) {
                continue;
            }
            commits.push(id);
            let (_, parents) = commit_links(data);
            if depth.is_some_and(|n| d >= n.max(1)) {
                shallow.push(id);
            } else if depth.is_none() && client_shallow.contains(&id) {
                // The client has it without its parents, and wants it so.
            } else {
                if client_shallow.contains(&id) {
                    unshallow.push(id);
                }
                queue.extend(parents.into_iter().map(|p| (p, d + 1)));
            }
        }
        let mut objects = commits.clone();
        let mut stack: Vec<Id> = commits.iter().filter_map(|c| commit_links(&self.objects[c].1).0).collect();
        stack.extend(rest);
        let mut have: std::collections::BTreeSet<Id> = objects.iter().copied().collect();
        let (mut trees, mut blobs) = (vec![], vec![]);
        while let Some(id) = stack.pop() {
            let Some((kind, data)) = self.objects.get(&id) else { continue };
            if !have.insert(id) {
                continue;
            }
            match kind {
                Kind::Tree => {
                    trees.push(id);
                    stack.extend(tree_entries(data));
                }
                Kind::Blob => blobs.push(id),
                Kind::Commit => objects.push(id),
            }
        }
        objects.extend(trees);
        objects.extend(blobs);
        Selection { objects, shallow, unshallow }
    }

    /// The response to a fetch request body.
    fn upload_pack(&self, body: &[u8]) -> Result<Vec<u8>, String> {
        let req = Request::parse(body)?;
        if req.wants.is_empty() {
            return Err("no want".into());
        }
        for w in &req.wants {
            if !self.objects.contains_key(w) {
                return Err(format!("upload-pack: not our ref {}", hex(w)));
            }
        }
        let sel = self.select(&req.wants, req.deepen, &req.shallows);
        let mut out = vec![];
        if req.deepen.is_some() {
            // Every round of a depth-limited fetch starts with the shallow
            // list, as upload-pack sends it (these lines have no newline):
            // the commits at the limit, even a root commit, and the
            // client's shallow commits that are whole now.
            for s in &sel.shallow {
                pkt(&mut out, format!("shallow {}", hex(s)).as_bytes());
            }
            for s in &sel.unshallow {
                pkt(&mut out, format!("unshallow {}", hex(s)).as_bytes());
            }
            out.extend_from_slice(b"0000");
            if req.haves.is_empty() && !req.done {
                // The depth's own round (wants, depth, a flush): the list
                // alone. The request with `done` follows.
                return Ok(out);
            }
        }
        let common: Vec<Id> = req.haves.iter().filter(|h| self.objects.contains_key(*h)).copied().collect();
        if req.done {
            match common.last() {
                Some(last) => {
                    for c in &common {
                        pkt(&mut out, format!("ACK {} common\n", hex(c)).as_bytes());
                    }
                    pkt(&mut out, format!("ACK {}\n", hex(last)).as_bytes());
                }
                None => pkt(&mut out, b"NAK\n"),
            }
        } else if let Some(last) = common.last() {
            // The end of a round of haves with something in common: say
            // it's enough, and (no-done) send the pack without waiting.
            for c in &common {
                pkt(&mut out, format!("ACK {} common\n", hex(c)).as_bytes());
            }
            pkt(&mut out, format!("ACK {} ready\n", hex(last)).as_bytes());
            pkt(&mut out, b"NAK\n");
            pkt(&mut out, format!("ACK {}\n", hex(last)).as_bytes());
        } else {
            // Nothing in common yet: the client sends more haves, or done.
            pkt(&mut out, b"NAK\n");
            return Ok(out);
        }
        let pack = self.pack(&sel.objects);
        for chunk in pack.chunks(65515) {
            let mut c = vec![1u8];
            c.extend_from_slice(chunk);
            pkt(&mut out, &c);
        }
        out.extend_from_slice(b"0000");
        Ok(out)
    }

    /// A version 2 pack of these objects, each whole.
    fn pack(&self, ids: &[Id]) -> Vec<u8> {
        let mut out = b"PACK".to_vec();
        out.extend_from_slice(&2u32.to_be_bytes());
        out.extend_from_slice(&(ids.len() as u32).to_be_bytes());
        for id in ids {
            let (kind, data) = &self.objects[id];
            pack_object(&mut out, *kind, data);
        }
        let sum = Sha1::digest(&out);
        out.extend_from_slice(&sum);
        out
    }
}

/// An object in a pack: type and size as a varint (3 bits of type and 4 of
/// size in the first byte, then 7 bits of size a byte), then the data
/// deflated as zlib.
fn pack_object(out: &mut Vec<u8>, kind: Kind, data: &[u8]) {
    let mut size = data.len();
    let mut byte = ((kind as u8) << 4) | (size & 0x0f) as u8;
    size >>= 4;
    while size > 0 {
        out.push(byte | 0x80);
        byte = (size & 0x7f) as u8;
        size >>= 7;
    }
    out.push(byte);
    let mut z = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
    z.write_all(data).expect("writing to a Vec");
    out.extend(z.finish().expect("writing to a Vec"));
}

/// A tree of `files`, and its entries' objects, into `objects`.
fn build_tree(files: &[(&str, &[u8], bool)], objects: &mut BTreeMap<Id, (Kind, Vec<u8>)>) -> Id {
    // (name, mode, id, is_dir)
    let mut entries: Vec<(String, &str, Id, bool)> = vec![];
    type Files<'a> = Vec<(&'a str, &'a [u8], bool)>;
    let mut dirs: BTreeMap<&str, Files> = BTreeMap::new();
    for (path, data, exec) in files {
        match path.split_once('/') {
            Some((dir, rest)) => dirs.entry(dir).or_default().push((rest, data, *exec)),
            None => {
                let id = object_id(Kind::Blob, data);
                objects.insert(id, (Kind::Blob, data.to_vec()));
                entries.push((path.to_string(), if *exec { "100755" } else { "100644" }, id, false));
            }
        }
    }
    for (dir, files) in dirs {
        entries.push((dir.to_owned(), "40000", build_tree(&files, objects), true));
    }
    // Git sorts as if a directory's name ended in `/`.
    entries.sort_by(|a, b| {
        let key = |e: &(String, &str, Id, bool)| {
            let mut k = e.0.clone().into_bytes();
            if e.3 {
                k.push(b'/');
            }
            k
        };
        key(a).cmp(&key(b))
    });
    let mut data = vec![];
    for (name, mode, id, _) in entries {
        data.extend_from_slice(format!("{mode} {name}\0").as_bytes());
        data.extend_from_slice(&id);
    }
    let id = object_id(Kind::Tree, &data);
    objects.insert(id, (Kind::Tree, data));
    id
}

/// The ids a tree's data names.
fn tree_entries(data: &[u8]) -> Vec<Id> {
    let mut ids = vec![];
    let mut rest = data;
    while let Some(nul) = rest.iter().position(|b| *b == 0) {
        let Some(id) = rest.get(nul + 1..nul + 21) else { break };
        ids.push(id.try_into().unwrap());
        rest = &rest[nul + 21..];
    }
    ids
}

/// A pkt-line of `data`.
fn pkt(out: &mut Vec<u8>, data: &[u8]) {
    out.extend_from_slice(format!("{:04x}", data.len() + 4).as_bytes());
    out.extend_from_slice(data);
}

#[derive(Debug, PartialEq, Eq)]
enum Pkt<'a> {
    Data(&'a [u8]),
    Flush,
    Delim,
}

fn pkts(mut buf: &[u8]) -> Result<Vec<Pkt<'_>>, String> {
    let mut out = vec![];
    while !buf.is_empty() {
        let head = buf.get(..4).ok_or("short pkt-line")?;
        let len = usize::from_str_radix(std::str::from_utf8(head).map_err(|_| "bad pkt-line length")?, 16)
            .map_err(|_| "bad pkt-line length")?;
        match len {
            0 => out.push(Pkt::Flush),
            1 => out.push(Pkt::Delim),
            2 => {}
            3 => return Err("bad pkt-line length".into()),
            n => out.push(Pkt::Data(buf.get(4..n).ok_or("short pkt-line")?)),
        }
        buf = &buf[len.max(4)..];
    }
    Ok(out)
}

/// A fetch request: `want`s (with the first one's capabilities), the
/// client's `shallow`s and `deepen`, then after a flush its `have`s and
/// perhaps `done`. Other lines (`filter`, `deepen-since`, ...) are ignored.
#[derive(Debug, Default)]
struct Request {
    wants: Vec<Id>,
    shallows: Vec<Id>,
    deepen: Option<u32>,
    haves: Vec<Id>,
    done: bool,
}

impl Request {
    fn parse(body: &[u8]) -> Result<Request, String> {
        let mut r = Request::default();
        for p in pkts(body)? {
            let Pkt::Data(line) = p else { continue };
            let line = String::from_utf8_lossy(line);
            let line = line.trim_end_matches('\n');
            let id = |s: &str| {
                parse_id(s.split(' ').next().unwrap_or("")).ok_or_else(|| format!("bad object id in {line:?}"))
            };
            if let Some(v) = line.strip_prefix("want ") {
                r.wants.push(id(v)?);
            } else if let Some(v) = line.strip_prefix("shallow ") {
                r.shallows.push(id(v)?);
            } else if let Some(v) = line.strip_prefix("deepen ") {
                r.deepen = Some(v.parse().map_err(|_| format!("bad deepen {v:?}"))?);
            } else if let Some(v) = line.strip_prefix("have ") {
                r.haves.push(id(v)?);
            } else if line == "done" {
                r.done = true;
            }
        }
        Ok(r)
    }
}

/// Every allowlisted folder's repository, built once.
static REPOS: LazyLock<HashMap<&'static str, Repo>> =
    LazyLock::new(|| FOLDERS.iter().map(|(name, releases)| (*name, Repo::build(name, releases))).collect());

/// `nvim.git` and `nvim` are the same folder.
fn repo(folder: &str) -> Option<&'static Repo> {
    REPOS.get(folder.strip_suffix(".git").unwrap_or(folder))
}

fn no_cache(mut res: Response) -> Response {
    let h = res.headers_mut();
    h.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-cache, max-age=0, must-revalidate"));
    h.insert(header::PRAGMA, HeaderValue::from_static("no-cache"));
    h.insert(header::EXPIRES, HeaderValue::from_static("Fri, 01 Jan 1980 00:00:00 GMT"));
    res
}

fn check_rate(app: &App, peer: SocketAddr, headers: &HeaderMap) -> Result<(), ApiError> {
    app.limits.check(limit::GIT, app.limits.client_ip(peer, headers))
}

/// `GET /git/{folder}/info/refs?service=git-upload-pack`: the ref
/// advertisement. Any other service is refused, as is asking without one
/// (the dumb protocol).
pub async fn info_refs(
    State(app): State<Arc<App>>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Path(folder): Path<String>,
    Query(q): Query<HashMap<String, String>>,
) -> Response {
    if let Err(e) = check_rate(&app, peer, &headers) {
        return e.into_response();
    }
    let Some(repo) = repo(&folder) else { return err(StatusCode::NOT_FOUND, "no such repository").into_response() };
    if q.get("service").map(String::as_str) != Some(SERVICE) {
        return err(StatusCode::FORBIDDEN, "only git-upload-pack is served here").into_response();
    }
    no_cache(
        ([(header::CONTENT_TYPE, "application/x-git-upload-pack-advertisement")], repo.advertisement()).into_response(),
    )
}

/// `POST /git/{folder}/git-upload-pack`: wants in, a pack out.
pub async fn upload_pack(
    State(app): State<Arc<App>>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Path(folder): Path<String>,
    body: Bytes,
) -> Response {
    if let Err(e) = check_rate(&app, peer, &headers) {
        return e.into_response();
    }
    let Some(repo) = repo(&folder) else { return err(StatusCode::NOT_FOUND, "no such repository").into_response() };
    if headers.get(header::CONTENT_TYPE).and_then(|v| v.to_str().ok()) != Some("application/x-git-upload-pack-request")
    {
        return err(StatusCode::UNSUPPORTED_MEDIA_TYPE, "not a git-upload-pack request").into_response();
    }
    let body = match headers.get(header::CONTENT_ENCODING).and_then(|v| v.to_str().ok()) {
        None | Some("identity") => body.to_vec(),
        Some("gzip") | Some("x-gzip") => {
            let mut inflated = vec![];
            let mut d = flate2::read::GzDecoder::new(&body[..]).take(MAX_INFLATED + 1);
            if d.read_to_end(&mut inflated).is_err() || inflated.len() as u64 > MAX_INFLATED {
                return err(StatusCode::BAD_REQUEST, "bad or too large gzip body").into_response();
            }
            inflated
        }
        Some(_) => return err(StatusCode::UNSUPPORTED_MEDIA_TYPE, "unknown content encoding").into_response(),
    };
    let reply = match repo.upload_pack(&body) {
        Ok(r) => r,
        Err(e) => {
            // An ERR line is how upload-pack tells git why.
            let mut out = vec![];
            pkt(&mut out, format!("ERR {e}\n").as_bytes());
            out
        }
    };
    no_cache(([(header::CONTENT_TYPE, "application/x-git-upload-pack-result")], reply).into_response())
}

#[cfg(test)]
mod tests {
    use super::*;

    // The ids below are `git hash-object`'s, `git write-tree`'s and
    // `git commit-tree`'s for this folder, made with the same author and time:
    //   a-b "dash\n", a.txt "dot\n", a/z "zed\n", run.sh "#!/bin/sh\n" (executable)
    const FILES: &[(&str, &[u8], bool)] = &[
        ("run.sh", b"#!/bin/sh\n", true),
        ("a/z", b"zed\n", false),
        ("a.txt", b"dot\n", false),
        ("a-b", b"dash\n", false),
    ];
    const TREE: &str = "ad9d8ae6c6b6acac3de2d606b17092a56826e345";
    const SUBTREE: &str = "f1b17637203119fb37607aeb2ce9fee81acd415a";
    const COMMIT: &str = "102e4bcaa6169eb8bb796d0846d60e57d60cd72a";
    // The same folder with a.txt "dot2\n", a release later, its commit's parent COMMIT.
    const FILES2: &[(&str, &[u8], bool)] = &[
        ("run.sh", b"#!/bin/sh\n", true),
        ("a/z", b"zed\n", false),
        ("a.txt", b"dot2\n", false),
        ("a-b", b"dash\n", false),
    ];
    const TREE2: &str = "09d608ca663b67fb1e2d6dc8c7d11c52129fb0b6";
    const COMMIT2: &str = "03121371a2156a3db69460373c76c10b01ec9168";

    fn one(files: &'static [File<'static>], tag: &'static str) -> Repo {
        let tags: &'static [&'static str] = Box::leak(Box::new([tag]));
        Repo::build("nvim", &[Release { tags, epoch: 0, files }])
    }

    /// COMMIT (v1.2.3, and v1.2.4 which changed nothing), then COMMIT2 (v1.3.0).
    fn chain() -> Repo {
        Repo::build(
            "nvim",
            &[
                Release { tags: &["v1.2.3", "v1.2.4"], epoch: 1_700_000_000, files: FILES },
                Release { tags: &["v1.3.0"], epoch: 1_700_100_000, files: FILES2 },
            ],
        )
    }

    fn repo() -> Repo {
        Repo::build("nvim", &[Release { tags: &["v1.2.3"], epoch: 1_700_000_000, files: FILES }])
    }

    #[test]
    fn ids_are_gits() {
        assert_eq!(hex(&object_id(Kind::Blob, b"dash\n")), "a2544f7ec3007899167de1fef481a5a0fd63fa41");
        assert_eq!(hex(&object_id(Kind::Blob, b"")), "e69de29bb2d1d6434b8b29ae775ad8c2e48c5391");
        let r = repo();
        // The sorting: `a-b`, `a.txt`, then the directory `a` (as `a/`), `run.sh`.
        assert!(r.objects.contains_key(&parse_id(TREE).unwrap()));
        assert!(r.objects.contains_key(&parse_id(SUBTREE).unwrap()));
        assert_eq!(hex(&r.refs[0].1), COMMIT);
        assert_eq!(r.objects.len(), 7);
        let tree = &r.objects[&parse_id(TREE).unwrap()].1;
        let text = String::from_utf8_lossy(tree);
        let at = |s: &str| text.find(s).unwrap();
        assert!(
            at("100644 a-b") < at("100644 a.txt")
                && at("100644 a.txt") < at("40000 a\0")
                && at("40000 a\0") < at("100755 run.sh")
        );
    }

    #[test]
    fn empty_blob_and_same_content_are_one_object() {
        let r = one(&[("x", b"same", false), ("y", b"same", false)], "v1");
        // blob, tree, commit
        assert_eq!(r.objects.len(), 3);
    }

    #[test]
    fn advertisement_lists_head_main_and_the_tag() {
        let adv = String::from_utf8(repo().advertisement()).unwrap();
        assert!(adv.starts_with("001e# service=git-upload-pack\n0000"));
        assert!(adv.contains(&format!("{COMMIT} HEAD\0multi_ack_detailed")));
        assert!(adv.contains("symref=HEAD:refs/heads/main"));
        assert!(!adv.contains("filter"));
        assert!(adv.contains(&format!("{COMMIT} refs/heads/main\n")));
        assert!(adv.contains(&format!("{COMMIT} refs/tags/v1.2.3\n")));
        assert!(adv.ends_with("0000"));
    }

    #[test]
    fn pkt_lines_parse() {
        let mut b = vec![];
        pkt(&mut b, b"want x\n");
        b.extend_from_slice(b"0000");
        pkt(&mut b, b"done\n");
        assert_eq!(pkts(&b).unwrap(), vec![Pkt::Data(b"want x\n"), Pkt::Flush, Pkt::Data(b"done\n")]);
        assert!(pkts(b"00").is_err());
        assert!(pkts(b"zzzz").is_err());
        assert!(pkts(b"0003").is_err());
        assert!(pkts(b"0010abc").is_err());
    }

    fn request(lines: &[&str]) -> Vec<u8> {
        let mut b = vec![];
        for l in lines {
            if l.is_empty() {
                b.extend_from_slice(b"0000");
            } else {
                pkt(&mut b, format!("{l}\n").as_bytes());
            }
        }
        b
    }

    /// The pack in a reply, after the acknowledgements: the side-band
    /// channel 1 bytes.
    fn pack_of(reply: &[u8]) -> Vec<u8> {
        let mut pack = vec![];
        for p in pkts(reply).unwrap() {
            if let Pkt::Data(d) = p
                && d.first() == Some(&1)
            {
                pack.extend_from_slice(&d[1..]);
            }
        }
        pack
    }

    /// Reads a pack back: (kind, data) of each object.
    fn unpack(pack: &[u8]) -> Vec<(u8, Vec<u8>)> {
        assert_eq!(&pack[..4], b"PACK");
        assert_eq!(&pack[4..8], &2u32.to_be_bytes());
        let n = u32::from_be_bytes(pack[8..12].try_into().unwrap());
        let (body, sum) = pack.split_at(pack.len() - 20);
        assert_eq!(Sha1::digest(body).as_slice(), sum);
        let mut at = 12;
        let mut out = vec![];
        for _ in 0..n {
            let mut b = body[at];
            at += 1;
            let kind = (b >> 4) & 7;
            let mut size = (b & 15) as usize;
            let mut shift = 4;
            while b & 0x80 != 0 {
                b = body[at];
                at += 1;
                size |= ((b & 0x7f) as usize) << shift;
                shift += 7;
            }
            let mut z = flate2::bufread::ZlibDecoder::new(&body[at..]);
            let mut data = vec![];
            z.read_to_end(&mut data).unwrap();
            assert_eq!(data.len(), size);
            at += z.total_in() as usize;
            out.push((kind, data));
        }
        assert_eq!(at, body.len());
        out
    }

    #[test]
    fn a_clone_gets_nak_and_the_whole_pack() {
        let r = repo();
        let req = request(&[&format!("want {COMMIT} multi_ack_detailed no-done side-band-64k"), "", "done"]);
        let reply = r.upload_pack(&req).unwrap();
        assert!(reply.starts_with(b"0008NAK\n"));
        let objs = unpack(&pack_of(&reply));
        assert_eq!(objs.len(), 7);
        assert_eq!(objs.iter().filter(|o| o.0 == 1).count(), 1);
        assert_eq!(objs.iter().filter(|o| o.0 == 2).count(), 2);
        assert_eq!(objs.iter().filter(|o| o.0 == 3).count(), 4);
        assert_eq!(objs[0].0, 1, "the commit comes first");
        assert!(reply.ends_with(b"0000"));
    }

    #[test]
    fn a_depth_starts_each_round_with_the_shallow_list() {
        let r = repo();
        let want = format!("want {COMMIT} multi_ack_detailed no-done side-band-64k");
        // What `git upload-pack --stateless-rpc` answers, byte for byte.
        let reply = r.upload_pack(&request(&[&want, "deepen 1", ""])).unwrap();
        assert_eq!(reply, format!("0034shallow {COMMIT}0000").into_bytes());
        let reply = r
            .upload_pack(&request(&[&want, "deepen 1", "", "have 1111111111111111111111111111111111111111", ""]))
            .unwrap();
        assert_eq!(reply, format!("0034shallow {COMMIT}00000008NAK\n").into_bytes());
        let known = format!("shallow {COMMIT}");
        let reply = r.upload_pack(&request(&[&want, &known, "deepen 2147483647", ""])).unwrap();
        assert_eq!(reply, format!("0036unshallow {COMMIT}0000").into_bytes());
        let reply = r.upload_pack(&request(&[&want, "deepen 1", "", "done"])).unwrap();
        assert!(reply.starts_with(format!("0034shallow {COMMIT}00000008NAK\n").as_bytes()));
    }

    #[test]
    fn a_large_object_is_split_across_side_band_packets_and_a_big_size_varint() {
        let mut x = 0x2545_f491_4f6c_dd1du64;
        let big: Vec<u8> = (0..300_000)
            .map(|_| {
                x ^= x << 13;
                x ^= x >> 7;
                x ^= x << 17;
                (x >> 32) as u8
            })
            .collect();
        let big: &'static [u8] = Box::leak(big.into_boxed_slice());
        let files: &'static [File<'static>] = Box::leak(Box::new([("big", big, false)]));
        let r = one(files, "v1");
        let want = hex(&r.refs[0].1);
        let reply = r.upload_pack(&request(&[&format!("want {want}"), "", "done"])).unwrap();
        let objs = unpack(&pack_of(&reply));
        assert!(objs.iter().any(|o| o.0 == 3 && o.1 == big));
        assert!(pkts(&reply).unwrap().iter().filter(|p| matches!(p, Pkt::Data(d) if d[0] == 1)).count() > 1);
    }

    #[test]
    fn haves_in_common_are_acknowledged_and_the_pack_follows_without_done() {
        let r = repo();
        let req = request(&[
            &format!("want {COMMIT}"),
            "",
            &format!("have {COMMIT}"),
            &format!("have {}", "0".repeat(40)),
            "",
        ]);
        let reply = r.upload_pack(&req).unwrap();
        let text = String::from_utf8_lossy(&reply);
        let at = |s: &str| text.find(s).unwrap();
        assert!(at(&format!("ACK {COMMIT} common")) < at(&format!("ACK {COMMIT} ready")));
        assert!(at("NAK") < at(&format!("ACK {COMMIT}\n")));
        assert_eq!(unpack(&pack_of(&reply)).len(), 7);
    }

    #[test]
    fn unknown_haves_alone_get_nak_and_no_pack() {
        let r = repo();
        let req = request(&[&format!("want {COMMIT}"), "", &format!("have {}", "1".repeat(40)), ""]);
        assert_eq!(r.upload_pack(&req).unwrap(), b"0008NAK\n");
    }

    #[test]
    fn unknown_wants_and_junk_are_errors() {
        let r = repo();
        let e = r.upload_pack(&request(&[&format!("want {}", "2".repeat(40)), "", "done"])).unwrap_err();
        assert!(e.contains("not our ref"));
        assert!(r.upload_pack(&request(&["want nonsense", "", "done"])).is_err());
        assert!(r.upload_pack(&request(&["", "done"])).is_err());
        assert!(r.upload_pack(b"zz").is_err());
    }

    fn count(objs: &[(u8, Vec<u8>)], kind: u8) -> usize {
        objs.iter().filter(|o| o.0 == kind).count()
    }

    #[test]
    fn releases_chain_with_the_previous_commit_as_parent() {
        let r = chain();
        let at = |name: &str| hex(&r.refs.iter().find(|(n, _)| n == name).unwrap().1);
        // The ids are `git commit-tree`'s, made with the same parent, author and dates.
        assert_eq!(at("HEAD"), COMMIT2);
        assert_eq!(at("refs/heads/main"), COMMIT2);
        assert_eq!(at("refs/tags/v1.3.0"), COMMIT2);
        assert_eq!(at("refs/tags/v1.2.3"), COMMIT);
        assert_eq!(at("refs/tags/v1.2.4"), COMMIT, "a release that changed nothing shares the commit");
        assert_eq!(r.refs.len(), 5);
        let c2 = String::from_utf8_lossy(&r.objects[&parse_id(COMMIT2).unwrap()].1).into_owned();
        assert!(c2.starts_with(&format!(
            "tree {TREE2}\nparent {COMMIT}\nauthor Arugula <noreply@arugula.io> 1700100000 +0000\n"
        )));
        let adv = String::from_utf8(r.advertisement()).unwrap();
        assert!(
            adv.contains(&format!("{COMMIT} refs/tags/v1.2.4\n"))
                && adv.contains(&format!("{COMMIT2} refs/heads/main\n"))
        );
    }

    #[test]
    fn a_full_fetch_of_the_chain_sends_every_commit_and_tree() {
        let r = chain();
        let reply = r.upload_pack(&request(&[&format!("want {COMMIT2}"), "", "done"])).unwrap();
        let objs = unpack(&pack_of(&reply));
        assert_eq!((count(&objs, 1), count(&objs, 2), count(&objs, 3)), (2, 3, 5));
        assert_eq!(objs[0].0, 1);
    }

    #[test]
    fn a_depth_cuts_the_chain_and_names_the_shallow_commit() {
        let r = chain();
        let want = format!("want {COMMIT2} multi_ack_detailed no-done side-band-64k");
        let reply = r.upload_pack(&request(&[&want, "deepen 1", "", "done"])).unwrap();
        assert!(reply.starts_with(format!("0034shallow {COMMIT2}00000008NAK\n").as_bytes()));
        let objs = unpack(&pack_of(&reply));
        assert_eq!((count(&objs, 1), count(&objs, 2), count(&objs, 3)), (1, 2, 4));
        // Depth 2 reaches the root commit, which is the limit.
        let reply = r.upload_pack(&request(&[&want, "deepen 2", ""])).unwrap();
        assert_eq!(reply, format!("0034shallow {COMMIT}0000").into_bytes());
        // Deeper than the chain: nothing is shallow.
        let reply = r.upload_pack(&request(&[&want, "deepen 3", ""])).unwrap();
        assert_eq!(reply, b"0000");
    }

    #[test]
    fn a_shallow_client_is_not_sent_what_is_behind_its_shallow_commit() {
        let r = chain();
        let (want, shallow) = (format!("want {COMMIT2}"), format!("shallow {COMMIT2}"));
        let reply = r.upload_pack(&request(&[&want, &shallow, "", "done"])).unwrap();
        assert_eq!(count(&unpack(&pack_of(&reply)), 1), 1);
        // Deepening it: the commit gets its parent, so it is no longer shallow; the parent is the new limit.
        let reply = r.upload_pack(&request(&[&want, &shallow, "deepen 2", ""])).unwrap();
        assert_eq!(reply, format!("0034shallow {COMMIT}0036unshallow {COMMIT2}0000").into_bytes());
    }

    #[test]
    fn the_embedded_folder_is_a_repository() {
        let r = repo_for("nvim");
        let name = |n: &str| r.refs.iter().any(|(r, _)| r == n);
        assert!(name("HEAD") && name("refs/heads/main"));
        assert!(r.refs.iter().any(|(n, _)| n.starts_with("refs/tags/v")));
        // Every ref names a commit we have, and the history is whole.
        for (_, id) in &r.refs {
            assert_eq!(r.objects[id].0, Kind::Commit);
            let sel = r.select(&[*id], None, &[]);
            assert!(sel.shallow.is_empty() && r.pack(&sel.objects).len() > 32);
        }
        assert!(CHECKOUT_NVIM.iter().any(|f| f.0 == "README.md"));
        assert!(repo_for_none("nvim.git") && repo_for_none("nvim") && !repo_for_none("../x") && !repo_for_none("web"));
    }

    fn repo_for(name: &str) -> &'static Repo {
        super::repo(name).unwrap()
    }

    fn repo_for_none(name: &str) -> bool {
        super::repo(name).is_some()
    }
}
