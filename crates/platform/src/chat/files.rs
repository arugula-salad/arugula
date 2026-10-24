//! Files and images in chat (#107). A person (or an agent, with
//! `threads.post`) uploads to a room they may post in; the file waits,
//! theirs alone, until a message is posted with its id, and is swept after
//! an hour if none is. Once attached it is served by control, to whoever
//! reads the room and to no one else: there is no public address, and
//! someone outside the room gets the same 404 as for a room that isn't
//! there. It goes with its message when that's deleted, and with its room
//! or its uploader's account (`db::chat_files`).
//!
//! What a file is, is what its bytes say, never what its upload claimed:
//! [`sniff`] decides. Only PNG, JPEG, GIF and WebP are images, and only
//! a few containers are video or audio; those show inline. Everything else,
//! SVG and HTML included (they sniff as plain text), is a card that
//! downloads: `Content-Disposition: attachment`, with `nosniff` and a CSP
//! that lets the response run nothing.
//!
//! The bytes live in the platform's own database, as link previews' images
//! do. The run-log bucket isn't used: its rule deletes objects after a
//! month, which a chat's files must outlive.

use super::*;
use crate::db::chat_files::{ChatFileRow, NewChatFile};
use arugula_control_wire::chat::{ChatFile, ChatFileKind, ChatUploadQuery, MAX_FILE_BYTES, MAX_FILES};
use axum::{
    body::Bytes,
    http::{HeaderValue, header},
    response::{IntoResponse as _, Response},
};

/// Uploads, per account per minute.
const CHAT_UPLOADS: (&str, usize) = ("chat-upload", 30);

/// What the bytes are.
#[derive(Debug, PartialEq, Eq)]
pub(super) struct Sniffed {
    pub mime: &'static str,
    pub kind: ChatFileKind,
    pub width: Option<u32>,
    pub height: Option<u32>,
}

fn is(mime: &'static str, kind: ChatFileKind) -> Sniffed {
    Sniffed { mime, kind, width: None, height: None }
}

fn be16(b: &[u8], at: usize) -> Option<u32> {
    Some(u32::from(u16::from_be_bytes([*b.get(at)?, *b.get(at + 1)?])))
}

fn le16(b: &[u8], at: usize) -> Option<u32> {
    Some(u32::from(u16::from_le_bytes([*b.get(at)?, *b.get(at + 1)?])))
}

fn le24(b: &[u8], at: usize) -> Option<u32> {
    Some(u32::from(*b.get(at)?) | u32::from(*b.get(at + 1)?) << 8 | u32::from(*b.get(at + 2)?) << 16)
}

/// A JPEG's size, from its first start-of-frame segment.
fn jpeg_size(b: &[u8]) -> Option<(u32, u32)> {
    let mut i = 2;
    while i + 4 <= b.len() {
        if b[i] != 0xFF {
            return None;
        }
        let marker = b[i + 1];
        if marker == 0xFF {
            i += 1;
            continue;
        }
        if matches!(marker, 0xC0..=0xCF) && !matches!(marker, 0xC4 | 0xC8 | 0xCC) {
            return Some((be16(b, i + 7)?, be16(b, i + 5)?));
        }
        if marker == 0xD8 || marker == 0x01 || matches!(marker, 0xD0..=0xD7) {
            i += 2;
        } else {
            i += 2 + be16(b, i + 2)? as usize;
        }
    }
    None
}

/// What `bytes` are. Anything not recognised as a safe picture, video or
/// audio is a `File`, and plain UTF-8 text (which is what SVG and HTML are)
/// is `text/plain`, never its own type.
pub(super) fn sniff(b: &[u8]) -> Sniffed {
    use ChatFileKind::*;
    let sized = |mime, (width, height): (Option<u32>, Option<u32>)| Sniffed { mime, kind: Image, width, height };
    if b.starts_with(b"\x89PNG\r\n\x1a\n") {
        return sized("image/png", (be32(b, 16), be32(b, 20)));
    }
    if b.starts_with(&[0xFF, 0xD8, 0xFF]) {
        let (w, h) = jpeg_size(b).unzip();
        return sized("image/jpeg", (w, h));
    }
    if b.starts_with(b"GIF87a") || b.starts_with(b"GIF89a") {
        return sized("image/gif", (le16(b, 6), le16(b, 8)));
    }
    if b.len() >= 12 && &b[..4] == b"RIFF" {
        match &b[8..12] {
            b"WEBP" => {
                let dims = if b.get(12..16) == Some(b"VP8X") {
                    (le24(b, 24).map(|w| w + 1), le24(b, 27).map(|h| h + 1))
                } else {
                    (None, None)
                };
                return sized("image/webp", dims);
            }
            b"WAVE" => return is("audio/wav", Audio),
            _ => {}
        }
    }
    if b.len() >= 12 && &b[4..8] == b"ftyp" {
        return match &b[8..12] {
            b"qt  " => is("video/quicktime", Video),
            b"M4A " | b"M4B " => is("audio/mp4", Audio),
            // Pictures in a video container (HEIC, AVIF) aren't shown:
            // browsers differ, so they download.
            b"heic" | b"heix" | b"hevc" | b"mif1" | b"msf1" | b"avif" | b"avis" => is("application/octet-stream", File),
            _ => is("video/mp4", Video),
        };
    }
    if b.starts_with(&[0x1A, 0x45, 0xDF, 0xA3]) {
        return is("video/webm", Video);
    }
    if b.starts_with(b"OggS") {
        return is("audio/ogg", Audio);
    }
    if b.starts_with(b"fLaC") {
        return is("audio/flac", Audio);
    }
    if b.starts_with(b"ID3") || (b.len() > 2 && b[0] == 0xFF && b[1] & 0xE0 == 0xE0 && b[1] & 0x06 != 0) {
        return is("audio/mpeg", Audio);
    }
    if b.starts_with(b"%PDF-") {
        return is("application/pdf", File);
    }
    if b.starts_with(b"PK\x03\x04") {
        return is("application/zip", File);
    }
    if b.starts_with(&[0x1F, 0x8B]) {
        return is("application/gzip", File);
    }
    let head = &b[..b.len().min(8192)];
    let text = !head.contains(&0)
        && match std::str::from_utf8(head) {
            Ok(_) => true,
            // A multi-byte character cut by the window.
            Err(e) => e.error_len().is_none() && e.valid_up_to() + 4 > head.len(),
        };
    is(if text { "text/plain" } else { "application/octet-stream" }, File)
}

fn be32(b: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_be_bytes([*b.get(at)?, *b.get(at + 1)?, *b.get(at + 2)?, *b.get(at + 3)?]))
}

/// A name a file is kept and offered under: no folders, no control
/// characters, at most 120 bytes, and never empty.
pub(super) fn clean_name(name: &str) -> String {
    let base = name.rsplit(['/', '\\']).next().unwrap_or("");
    let mut out: String =
        base.chars().filter(|c| !c.is_control() && !matches!(c, '"' | '<' | '>' | '|' | ':' | '*' | '?')).collect();
    let out_trim = out.trim().trim_start_matches('.').to_owned();
    out = out_trim;
    while out.len() > 120 {
        out.pop();
    }
    if out.is_empty() { "file".to_owned() } else { out }
}

fn kind_name(k: ChatFileKind) -> &'static str {
    match k {
        ChatFileKind::Image => "image",
        ChatFileKind::Video => "video",
        ChatFileKind::Audio => "audio",
        ChatFileKind::File => "file",
    }
}

fn kind_from(k: &str) -> ChatFileKind {
    match k {
        "image" => ChatFileKind::Image,
        "video" => ChatFileKind::Video,
        "audio" => ChatFileKind::Audio,
        _ => ChatFileKind::File,
    }
}

/// A file's address for a reader: a person's, or (`agent`) a machine's
/// agents', who sign as their machine.
pub(super) fn url(room: &str, id: &str, agent: bool) -> String {
    let base = if agent { "/api/daemon/chat/rooms" } else { "/api/chat/rooms" };
    format!("{base}/{room}/files/{id}")
}

pub(super) fn shown(f: &ChatFileRow, agent: bool) -> ChatFile {
    ChatFile {
        id: f.id.clone(),
        name: f.name.clone(),
        mime: f.mime.clone(),
        kind: kind_from(&f.kind),
        size: f.size,
        width: f.width,
        height: f.height,
        url: url(&f.room, &f.id, agent),
    }
}

/// The files on `ms`, by (room, seq): one query a room over the numbers it
/// spans, as reactions are read.
pub(super) fn files_of(db: &Db, ms: &[&Message]) -> anyhow::Result<HashMap<(String, i64), Vec<ChatFileRow>>> {
    let mut span: HashMap<&str, (i64, i64)> = HashMap::new();
    for m in ms.iter().filter(|m| m.deleted.is_none() && m.imported.is_none()) {
        let e = span.entry(m.room.as_str()).or_insert((m.seq, m.seq));
        *e = (e.0.min(m.seq), e.1.max(m.seq));
    }
    let mut out = HashMap::new();
    for (room, (from, to)) in span {
        out.extend(db.chat_files_in(room, from, to)?);
    }
    Ok(out)
}

/// Keep `bytes` as `name` in `room` for `account` (an agent's, `agent`):
/// the caller has found the room readable. Posting is checked here.
pub(super) async fn store(
    app: &Platform,
    account: &str,
    (room, facts): (&Room, &Facts),
    name: &str,
    bytes: &[u8],
) -> Result<ChatFile, ApiError> {
    if watching(app, facts, account, room)? {
        return Err(refusal(
            StatusCode::FORBIDDEN,
            WATCHING,
            "you're watching this session: you read its threads, and its owner can let you post",
        ));
    }
    if !writable(app, facts, room, account)? {
        return Err(err(StatusCode::CONFLICT, "this room is read-only"));
    }
    if bytes.is_empty() {
        return Err(err(StatusCode::BAD_REQUEST, "that file is empty"));
    }
    if bytes.len() > MAX_FILE_BYTES {
        return Err(err(StatusCode::PAYLOAD_TOO_LARGE, "a file is at most 10 MB"));
    }
    app.limits.check_account_per_minute(CHAT_UPLOADS, account)?;
    let sniffed = sniff(bytes);
    let id = new_id("cf_");
    let name = clean_name(name);
    let row = NewChatFile {
        id: &id,
        room: &room.id,
        account,
        scope: &room.scope,
        name: &name,
        mime: sniffed.mime,
        kind: kind_name(sniffed.kind),
        width: sniffed.width,
        height: sniffed.height,
        at: now_ms(),
        data: bytes,
    };
    if !app.db.add_chat_file(&row, wire::MAX_SCOPE_FILE_BYTES)? {
        return Err(err(
            StatusCode::INSUFFICIENT_STORAGE,
            "this team's chat files are full (256 MB): delete some first",
        ));
    }
    let f = app.db.chat_file(&id)?.ok_or_else(|| anyhow::anyhow!("a file just kept isn't there"))?;
    Ok(shown(&f, false))
}

/// `POST /api/chat/rooms/{id}/files?name=`: a file's bytes, kept for the
/// next message the uploader posts with it.
pub async fn upload(
    State(app): State<Arc<Platform>>,
    s: Person,
    Path(id): Path<String>,
    Query(q): Query<ChatUploadQuery>,
    body: Bytes,
) -> J<ChatFile> {
    gate(&app, &s.account).await?;
    let (room, _, facts) = room_for(&app, &s.account, &id).await?;
    Ok(Json(store(&app, &s.account, (&room, &facts), &q.name, &body).await?))
}

/// The files a post names, found and checked: the poster's own uploads to
/// this room that no message has. Refused as a whole otherwise.
pub(super) fn to_attach(
    app: &Platform,
    room: &Room,
    account: &str,
    ids: &[String],
) -> Result<Vec<ChatFileRow>, ApiError> {
    if ids.len() > MAX_FILES {
        return Err(err(StatusCode::BAD_REQUEST, "a message carries at most 10 files"));
    }
    let mut seen = std::collections::HashSet::new();
    if !ids.iter().all(|i| seen.insert(i)) {
        return Err(err(StatusCode::BAD_REQUEST, "a file is attached once"));
    }
    let found = app.db.chat_files_to_attach(&room.id, account, ids)?;
    if found.len() != ids.len() {
        return Err(err(StatusCode::BAD_REQUEST, "a file you attached isn't yours, or was used already"));
    }
    Ok(found)
}

/// How a message with no words reads in a push.
pub(super) fn label(files: &[ChatFileRow]) -> String {
    match files {
        [] => String::new(),
        [one] => format!("📎 {}", one.name),
        many => format!("📎 {} files", many.len()),
    }
}

/// `Range: bytes=a-b` (one range, the only kind a media element sends),
/// as the first and last byte of `len`; `None` for anything else.
fn range(h: &HeaderMap, len: usize) -> Option<(usize, usize)> {
    let v = h.get(header::RANGE)?.to_str().ok()?.strip_prefix("bytes=")?;
    if v.contains(',') || len == 0 {
        return None;
    }
    let (a, b) = v.split_once('-')?;
    let (from, to) = if a.is_empty() {
        let n: usize = b.parse().ok()?;
        (len.saturating_sub(n), len - 1)
    } else {
        (a.parse().ok()?, if b.is_empty() { len - 1 } else { b.parse::<usize>().ok()?.min(len - 1) })
    };
    (from <= to && from < len).then_some((from, to))
}

#[derive(serde::Deserialize, Default)]
pub struct Download {
    /// Any value: offer it for saving, even a picture.
    pub download: Option<String>,
}

/// The file's response: inline only for sniffed pictures, video and audio.
fn serve(f: &ChatFileRow, data: Vec<u8>, headers: &HeaderMap, download: bool) -> Result<Response, ApiError> {
    let inline = !download && f.kind != "file";
    let bad = |_| err(StatusCode::INTERNAL_SERVER_ERROR, "a file's name can't be sent");
    let encoded: String = f
        .name
        .bytes()
        .map(|b| {
            if b.is_ascii_alphanumeric() || b"-_.~".contains(&b) {
                (b as char).to_string()
            } else {
                format!("%{b:02X}")
            }
        })
        .collect();
    let ascii: String = f.name.chars().map(|c| if c.is_ascii_graphic() || c == ' ' { c } else { '_' }).collect();
    let disposition =
        format!("{}; filename=\"{}\"; filename*=UTF-8''{encoded}", if inline { "inline" } else { "attachment" }, ascii);
    let total = data.len();
    let (status, body, content_range) = match range(headers, total) {
        Some((a, b)) => (StatusCode::PARTIAL_CONTENT, data[a..=b].to_vec(), Some(format!("bytes {a}-{b}/{total}"))),
        None => (StatusCode::OK, data, None),
    };
    let mut r = (status, body).into_response();
    let h = r.headers_mut();
    h.insert(header::CONTENT_TYPE, HeaderValue::from_str(&f.mime).map_err(bad)?);
    h.insert(header::CONTENT_DISPOSITION, HeaderValue::from_str(&disposition).map_err(bad)?);
    h.insert(header::ACCEPT_RANGES, HeaderValue::from_static("bytes"));
    if let Some(cr) = content_range {
        h.insert(header::CONTENT_RANGE, HeaderValue::from_str(&cr).map_err(bad)?);
    }
    h.insert(header::CACHE_CONTROL, HeaderValue::from_static("private, max-age=300"));
    h.insert(header::X_CONTENT_TYPE_OPTIONS, HeaderValue::from_static("nosniff"));
    h.insert(header::CONTENT_SECURITY_POLICY, HeaderValue::from_static("default-src 'none'; sandbox"));
    h.insert("cross-origin-resource-policy", HeaderValue::from_static("same-origin"));
    Ok(r)
}

/// The file `file` of a room `account` reads, or 404: the same answer for
/// a room that isn't theirs, a file that isn't in it, one before they
/// joined, and one nobody posted yet that someone else uploaded.
pub(super) async fn fetch(
    app: &Platform,
    (room, from): (&Room, u64),
    account: &str,
    file: &str,
    headers: &HeaderMap,
    download: bool,
) -> Result<Response, ApiError> {
    let gone = || err(StatusCode::NOT_FOUND, "no such file");
    let f = app.db.chat_file(file)?.filter(|f| f.room == room.id).ok_or_else(gone)?;
    match f.seq {
        Some(seq) => {
            let m = app.db.chat_message(&room.id, seq)?.ok_or_else(gone)?;
            if m.at < from || m.deleted.is_some() {
                return Err(gone());
            }
        }
        None if f.account == account => {}
        None => return Err(gone()),
    }
    let data = app.db.chat_file_data(file)?.ok_or_else(gone)?;
    serve(&f, data, headers, download)
}

/// `GET /api/chat/rooms/{id}/files/{file}[?download=1]`.
pub async fn get(
    State(app): State<Arc<Platform>>,
    s: Person,
    Path((id, file)): Path<(String, String)>,
    Query(q): Query<Download>,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    gate(&app, &s.account).await?;
    let (room, from, _) = room_for(&app, &s.account, &id).await?;
    fetch(&app, (&room, from), &s.account, &file, &headers, q.download.is_some()).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bytes_decide_what_a_file_is() {
        let png = [b"\x89PNG\r\n\x1a\n".as_slice(), &[0, 0, 0, 13], b"IHDR", &[0, 0, 1, 0, 0, 0, 0, 200]].concat();
        let s = sniff(&png);
        assert_eq!((s.mime, s.kind, s.width, s.height), ("image/png", ChatFileKind::Image, Some(256), Some(200)));
        let jpeg = [&[0xFF, 0xD8, 0xFF, 0xE0, 0, 4, 0, 0][..], &[0xFF, 0xC0, 0, 11, 8, 0, 20, 0, 30, 3]].concat();
        let s = sniff(&jpeg);
        assert_eq!((s.mime, s.width, s.height), ("image/jpeg", Some(30), Some(20)));
        assert_eq!(sniff(b"GIF89a\x05\x00\x06\x00").height, Some(6));
        assert_eq!(sniff(b"\0\0\0\x18ftypmp42\0\0\0\0").kind, ChatFileKind::Video);
        assert_eq!(sniff(b"\0\0\0\x18ftypM4A \0\0\0\0").kind, ChatFileKind::Audio);
        assert_eq!(sniff(b"\0\0\0\x18ftypheic\0\0\0\0").kind, ChatFileKind::File);
        assert_eq!(sniff(b"%PDF-1.7\n").mime, "application/pdf");
        assert_eq!(sniff(b"RIFF\0\0\0\0WAVEfmt ").mime, "audio/wav");
    }

    #[test]
    fn svg_and_html_are_text_and_never_shown() {
        for body in [
            &b"<svg xmlns='http://www.w3.org/2000/svg' onload='alert(1)'/>"[..],
            b"<!doctype html><script>alert(1)</script>",
            b"<?xml version='1.0'?><svg/>",
        ] {
            let s = sniff(body);
            assert_eq!((s.mime, s.kind), ("text/plain", ChatFileKind::File));
        }
        // A name saying .png doesn't make text a picture; there is no way to say a type.
        assert_eq!(sniff(b"not a png at all").kind, ChatFileKind::File);
        assert_eq!(sniff(&[0xC0, 0x00, 0xFF, 0x00]).mime, "application/octet-stream");
    }

    #[test]
    fn names_are_plain() {
        assert_eq!(clean_name("../../etc/passwd"), "passwd");
        assert_eq!(clean_name("C:\\Users\\a\\shot \"1\".png"), "shot 1.png");
        assert_eq!(clean_name(".."), "file");
        assert_eq!(clean_name("  .hidden "), "hidden");
        assert_eq!(clean_name(&"é".repeat(100)).len(), 120);
        assert_eq!(clean_name("line\nbreak.txt"), "linebreak.txt");
    }

    #[test]
    fn ranges() {
        let h = |v: &str| {
            let mut m = HeaderMap::new();
            m.insert(header::RANGE, v.parse().unwrap());
            m
        };
        assert_eq!(range(&h("bytes=0-99"), 1000), Some((0, 99)));
        assert_eq!(range(&h("bytes=900-"), 1000), Some((900, 999)));
        assert_eq!(range(&h("bytes=-100"), 1000), Some((900, 999)));
        assert_eq!(range(&h("bytes=0-5000"), 1000), Some((0, 999)));
        assert_eq!(range(&h("bytes=2000-"), 1000), None);
        assert_eq!(range(&h("bytes=0-1,5-6"), 1000), None);
    }
}
