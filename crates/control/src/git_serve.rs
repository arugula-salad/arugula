//! An integration's folder served as a git repository (#1748), so a plugin
//! manager (lazy.nvim, vim-plug, packer, tpm) can install it from a URL:
//! `GET /git/nvim.git/info/refs?service=git-upload-pack` and
//! `POST /git/nvim.git/git-upload-pack`.
//!
//! Control's image has no `git` and no shell, so this speaks the least of
//! git's smart HTTP protocol (v0, `gitprotocol-http` and `gitprotocol-pack`)
//! that clients need, and builds the repository in memory:
//!
//! - The folders are an allowlist in build.rs (`GIT_FOLDERS`), embedded
//!   with their files as arugulad-editor's build.rs embeds editors/nvim.
//! - Each becomes blobs, trees and one parentless commit by the fixed
//!   author `Arugula <noreply@arugula.io>` at a fixed time (build.rs'
//!   `EPOCH`: the release tag's date if git could say), so the ids are the
//!   same on every instance and every build of the same tree.
//! - Refs: `HEAD` (a symref to `refs/heads/main`), `refs/heads/main` and
//!   `refs/tags/v<version>`, the current release only.
//! - A fetch always gets the full closure of what it wants, as one pack of
//!   undeltified objects. `have` lines only decide the acknowledgements
//!   (git stops asking when it hears one `ready`), and `deepen` only
//!   answers with an empty shallow list, as the one commit has no parents.
//!   The `filter` capability isn't offered, so a client drops its filter.
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

/// One folder's repository, in memory.
pub struct Repo {
    objects: BTreeMap<Id, (Kind, Vec<u8>)>,
    /// Every ref, by name, sorted, `HEAD` first.
    refs: Vec<(String, Id)>,
}

impl Repo {
    /// `files` are (path with `/`, bytes, executable); `tag` is `v<version>`.
    pub fn build(files: &[(&str, &[u8], bool)], subject: &str, tag: &str, epoch: i64) -> Repo {
        let mut objects = BTreeMap::new();
        let tree = build_tree(files, &mut objects);
        let who = format!("Arugula <noreply@arugula.io> {epoch} +0000");
        let commit = format!("tree {}\nauthor {who}\ncommitter {who}\n\n{subject}\n", hex(&tree)).into_bytes();
        let commit_id = object_id(Kind::Commit, &commit);
        objects.insert(commit_id, (Kind::Commit, commit));
        let mut refs = vec![
            ("HEAD".to_owned(), commit_id),
            ("refs/heads/main".to_owned(), commit_id),
            (format!("refs/tags/{tag}"), commit_id),
        ];
        refs[1..].sort();
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

    /// Ids of `want` and everything it reaches, commits first.
    fn closure(&self, want: &Id) -> Vec<Id> {
        let mut seen = std::collections::BTreeSet::new();
        let mut order = vec![];
        let mut stack = vec![*want];
        while let Some(id) = stack.pop() {
            if !seen.insert(id) {
                continue;
            }
            let Some((kind, data)) = self.objects.get(&id) else { continue };
            order.push(id);
            match kind {
                Kind::Commit => {
                    for line in data.split(|b| *b == b'\n').take_while(|l| !l.is_empty()) {
                        let line = String::from_utf8_lossy(line);
                        if let Some(h) = line.strip_prefix("tree ").or_else(|| line.strip_prefix("parent ")) {
                            stack.extend(parse_id(h));
                        }
                    }
                }
                Kind::Tree => stack.extend(tree_entries(data)),
                Kind::Blob => {}
            }
        }
        order.sort_by_key(|id| self.objects[id].0 as u8);
        order
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
        let mut out = vec![];
        if let Some(depth) = req.deepen {
            // Every round of a depth-limited fetch starts with the shallow
            // list, as upload-pack sends it: the commit at the limit, here
            // the one commit when the depth is 1 (git marks it though it
            // has no parents; these lines have no newline), and a commit the client had cut short that
            // is whole now.
            if depth == 1 {
                for w in req.wants.iter().filter(|w| self.objects[*w].0 == Kind::Commit) {
                    pkt(&mut out, format!("shallow {}", hex(w)).as_bytes());
                }
            } else {
                for s in req.shallows.iter().filter(|s| self.objects.contains_key(*s)) {
                    pkt(&mut out, format!("unshallow {}", hex(s)).as_bytes());
                }
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
        let mut ids: Vec<Id> = vec![];
        for w in &req.wants {
            for id in self.closure(w) {
                if !ids.contains(&id) {
                    ids.push(id);
                }
            }
        }
        let pack = self.pack(&ids);
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
static REPOS: LazyLock<HashMap<&'static str, Repo>> = LazyLock::new(|| {
    let tag = format!("v{}", env!("CARGO_PKG_VERSION"));
    FOLDERS
        .iter()
        .map(|(name, files)| {
            let subject = format!("arugula {name} {tag}");
            (*name, Repo::build(files, &subject, &tag, EPOCH))
        })
        .collect()
});

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
    const COMMIT: &str = "71ecefb92b7ecbee6b5e9ca9ec0e6497bd791b06";

    fn repo() -> Repo {
        Repo::build(FILES, "arugula.nvim v1.2.3", "v1.2.3", 1_700_000_000)
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
        let r = Repo::build(&[("x", b"same", false), ("y", b"same", false)], "s", "v1", 0);
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
        let r = Repo::build(&[("big", &big, false)], "s", "v1", 0);
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

    #[test]
    fn the_embedded_folder_is_a_repository() {
        let r = repo_for("nvim");
        assert!(r.refs.iter().any(|(n, _)| n == &format!("refs/tags/v{}", env!("CARGO_PKG_VERSION"))));
        assert!(repo_for_none("nvim.git") && repo_for_none("nvim") && !repo_for_none("../x") && !repo_for_none("web"));
    }

    fn repo_for(name: &str) -> &'static Repo {
        super::repo(name).unwrap()
    }

    fn repo_for_none(name: &str) -> bool {
        super::repo(name).is_some()
    }
}
