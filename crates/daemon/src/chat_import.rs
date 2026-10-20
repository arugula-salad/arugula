//! Bringing this machine's old threads into control's chat (M124, S41 q6),
//! and telling control who reads each pane's and session's thread.
//!
//! Chat lives in control (M122); the daemon keeps none of it (M125). After
//! each refresh with control, while the `chat` flag is on here:
//! - **Once:** every pane's and session's thread an older daemon kept (M61)
//!   in `threads/`, and every conversation (M81, as a read-only archive,
//!   decision 3), go to control signed as this machine
//!   (`POST /api/daemon/chat/import`), with their numbers, authors (by
//!   account where they had one) and quotes. Then `threads/` becomes
//!   `threads.imported/`, so a second start imports nothing, and an older
//!   daemon after a downgrade starts with no threads rather than a fork.
//!   Control takes each message once, so a try that fails halfway is simply
//!   made again. This stays after the store went (M125, decision 9): a
//!   machine that skipped the release with the import still brings its
//!   threads in.
//! - **From then on:** who may read each pane's and session's thread, as
//!   this daemon decides it (decision 2), whenever that changes
//!   (`POST /api/daemon/chat/readers`).
//!
//! A daemon not in control keeps its old threads on disk (decision 7).

use std::{
    io::{self, BufRead},
    path::{Path, PathBuf},
    sync::Arc,
};

use arugula_control_wire::chat::{
    self as wire, CHAT_IMPORT, CHAT_READERS, ChatImport, ChatImportArchive, ChatImportMessage, ChatImportThread,
    ChatImported, ChatReader, ChatReaderList, ChatReaders, ChatReadersStored, ChatTargetKind,
};
use serde::Deserialize;
use tracing::{info, warn};

use crate::{
    control::{Answered, Control},
    mux::{Api, ChatTarget},
    server::App,
};

/// Where an older daemon kept its threads, under the state dir.
const THREADS: &str = "threads";
/// Where they go once control has them: kept, and never read again. Its
/// being there says the import is done.
const IMPORTED: &str = "threads.imported";

/// Run beside the control loop: after each refresh, import (once) and keep
/// control's reader lists current.
pub fn start(control: Arc<Control>, app: Arc<App>) {
    if !crate::control::imports_chat() {
        return;
    }
    tokio::spawn(async move {
        let mut refreshed = control.on_refresh();
        let mut sync = Sync::default();
        while refreshed.changed().await.is_ok() {
            sync.run(&control, &app).await;
        }
    });
}

#[derive(Default)]
struct Sync {
    /// The reader lists control has, as last sent.
    sent: Option<Vec<ChatReaderList>>,
    /// What was last logged, so a refusal is said once, not every minute.
    said: Option<String>,
}

impl Sync {
    async fn run(&mut self, control: &Control, app: &App) {
        let Some(owner) = control.account() else { return };
        if !arugula_proto::flags::get(control.state_dir(), arugula_proto::flags::CHAT) {
            self.sent = None;
            return;
        }
        match self.once(control, app, &owner).await {
            Ok(Some(done)) => {
                info!(rooms = done.rooms, messages = done.messages, "threads brought into control's chat")
            }
            Ok(None) => {}
            Err(e) => {
                let why = e.to_string();
                // Chat off for this machine in control, or a control from
                // before it: tried again after the next refresh, quietly.
                let quiet = e.downcast_ref::<Answered>().is_some_and(|a| matches!(a.0, 403..=405));
                if self.said.as_deref() != Some(why.as_str()) {
                    if quiet {
                        info!(why, "control doesn't take this machine's threads yet");
                    } else {
                        warn!(why, "can't bring threads into control's chat");
                    }
                    self.said = Some(why);
                }
            }
        }
    }

    /// The import if it's still to do, then the reader lists if they
    /// changed. What the import brought, if it ran.
    async fn once(&mut self, control: &Control, app: &App, owner: &str) -> anyhow::Result<Option<ChatImported>> {
        let readers = readers(app.mux.api(Api::ChatReaders).await.unwrap_or_default(), owner);
        let dir = control.state_dir().to_owned();
        let export = tokio::task::spawn_blocking(move || Export::read(&dir)).await?;
        let mut done = None;
        if !(export.threads.is_empty() && export.convs.is_empty()) {
            let calls = calls(&export, owner, &readers);
            if readers.len() > wire::MAX_READER_TARGETS {
                self.send_readers(control, &readers).await?;
            }
            let mut sum = ChatImported::default();
            for c in &calls {
                let got: ChatImported = control.post_signed(CHAT_IMPORT, c).await?;
                sum.rooms += got.rooms;
                sum.messages += got.messages;
                sum.already += got.already;
            }
            retire(control.state_dir()).map_err(|e| anyhow::anyhow!("can't move the threads aside: {e}"))?;
            self.sent = Some(readers.clone());
            self.said = None;
            done = Some(sum);
        }
        if self.sent.as_ref() != Some(&readers) {
            self.send_readers(control, &readers).await?;
            self.said = None;
        }
        Ok(done)
    }

    async fn send_readers(&mut self, control: &Control, readers: &[ChatReaderList]) -> anyhow::Result<()> {
        for part in readers.chunks(wire::MAX_READER_TARGETS) {
            let _: ChatReadersStored =
                control.post_signed(CHAT_READERS, &ChatReaders { targets: part.to_vec() }).await?;
        }
        self.sent = Some(readers.to_vec());
        Ok(())
    }
}

/// One message as an older daemon kept it (M61's `ThreadMsg`), one JSON
/// object per line of `<target>.jsonl`.
#[derive(Debug, Clone, Deserialize)]
struct OldMsg {
    id: u64,
    at: u64,
    /// A principal id: `owner`, `account:…`, `tailnet:…`.
    who: String,
    name: String,
    text: String,
    #[serde(default)]
    quote: Option<OldQuote>,
    /// Principal ids it @mentioned.
    #[serde(default)]
    mentions: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
struct OldQuote {
    pane: u32,
    text: String,
}

/// A conversation's record (M81), `conv-<id>.json`.
#[derive(Debug, Clone, Deserialize)]
struct OldConv {
    /// `c-` and 32 hex digits.
    id: String,
    title: String,
    owner: String,
    #[serde(default)]
    members: Vec<OldMember>,
    created: u64,
}

#[derive(Debug, Clone, Deserialize)]
struct OldMember {
    who: String,
    joined: u64,
    /// Added "with history": they read all of it.
    #[serde(default)]
    history: bool,
}

/// What `threads/` holds: each pane's and session's thread with messages,
/// and each conversation with its timeline, oldest first.
#[derive(Debug, Default)]
struct Export {
    threads: Vec<(ChatTarget, Vec<OldMsg>)>,
    convs: Vec<(OldConv, Vec<OldMsg>)>,
}

impl Export {
    /// What an older daemon left in `<state>/threads/`; nothing once it's
    /// imported (or where there never were any). A line that doesn't parse
    /// is skipped (a crash mid-append left at most one).
    fn read(state: &Path) -> Self {
        let dir = state.join(THREADS);
        if state.join(IMPORTED).exists() {
            return Self::default();
        }
        let mut out = Self::default();
        let mut timelines: Vec<(String, Vec<OldMsg>)> = Vec::new();
        let mut records: Vec<OldConv> = Vec::new();
        for path in std::fs::read_dir(&dir).into_iter().flatten().flatten().map(|e| e.path()) {
            let Some(name) = path.file_name().and_then(|n| n.to_str()) else { continue };
            if let Some(key) = name.strip_suffix(".jsonl") {
                let msgs = lines(&path);
                if msgs.is_empty() {
                    continue;
                }
                match target(key) {
                    Some(t) => out.threads.push((t, msgs)),
                    None => {
                        if let Some(id) = key.strip_prefix("conv-") {
                            timelines.push((id.to_owned(), msgs));
                        }
                    }
                }
            } else if let Some(id) = name.strip_prefix("conv-").and_then(|n| n.strip_suffix(".json")) {
                match std::fs::read(&path)
                    .map_err(io::Error::other)
                    .and_then(|b| serde_json::from_slice::<OldConv>(&b).map_err(io::Error::other))
                {
                    Ok(c) if c.id == id => records.push(c),
                    Ok(_) => warn!(path = %path.display(), "a conversation's record names another id"),
                    Err(e) => warn!(?e, path = %path.display(), "can't read a conversation's record"),
                }
            }
        }
        out.threads.sort_by_key(|(t, _)| *t);
        for c in records {
            let msgs = timelines.iter().find(|(id, _)| *id == c.id).map(|(_, m)| m.clone()).unwrap_or_default();
            out.convs.push((c, msgs));
        }
        out.convs.sort_by(|(a, _), (b, _)| (a.created, &a.id).cmp(&(b.created, &b.id)));
        out
    }
}

/// `pane-7` or `session-2`.
fn target(key: &str) -> Option<ChatTarget> {
    if let Some(p) = key.strip_prefix("pane-") {
        return p.parse().ok().map(ChatTarget::Pane);
    }
    key.strip_prefix("session-")?.parse().ok().map(ChatTarget::Session)
}

fn lines(path: &Path) -> Vec<OldMsg> {
    let Ok(f) = std::fs::File::open(path) else { return Vec::new() };
    io::BufReader::new(f).lines().map_while(Result::ok).filter_map(|l| serde_json::from_str(&l).ok()).collect()
}

/// Control has them: `threads/` becomes `threads.imported/`.
fn retire(state: &Path) -> io::Result<()> {
    let (from, to): (PathBuf, PathBuf) = (state.join(THREADS), state.join(IMPORTED));
    if to.exists() {
        return Err(io::Error::new(io::ErrorKind::AlreadyExists, format!("{} is there already", to.display())));
    }
    std::fs::rename(from, to)
}

/// A principal here as control's account: the machine's owner, or an
/// `account:` one; nobody else has one.
fn account_of(who: &str, owner: &str) -> Option<String> {
    if who == "owner" {
        return Some(owner.to_owned());
    }
    who.strip_prefix("account:").filter(|a| !a.is_empty()).map(str::to_owned)
}

fn kind_of(t: ChatTarget) -> (ChatTargetKind, String) {
    match t {
        ChatTarget::Pane(p) => (ChatTargetKind::Pane, p.to_string()),
        ChatTarget::Session(s) => (ChatTargetKind::Session, s.to_string()),
    }
}

/// The mux's reader lists as control takes them: accounts only, the owner
/// left out (control lets them read everything anyway).
fn readers(lists: crate::mux::ChatReaders, owner: &str) -> Vec<ChatReaderList> {
    lists
        .into_iter()
        .map(|(t, people)| {
            let (kind, target) = kind_of(t);
            let mut readers: Vec<ChatReader> = Vec::new();
            for (who, from, watching) in people {
                let Some(account) = account_of(&who, owner).filter(|a| a != owner) else { continue };
                if !readers.iter().any(|r| r.account == account) {
                    readers.push(ChatReader { account, from: (from > 0).then_some(from), watching });
                }
            }
            ChatReaderList { kind, target, readers }
        })
        .collect()
}

fn message(m: &OldMsg, owner: &str) -> ChatImportMessage {
    let mut mentions: Vec<String> = Vec::new();
    for who in &m.mentions {
        if let Some(a) = account_of(who, owner)
            && !mentions.contains(&a)
        {
            mentions.push(a);
        }
    }
    ChatImportMessage {
        id: m.id,
        at: m.at,
        account: account_of(&m.who, owner),
        name: m.name.clone(),
        text: m.text.clone(),
        quote: m.quote.as_ref().map(|q| q.text.clone()),
        pane: m.quote.as_ref().map(|q| q.pane.to_string()),
        mentions,
    }
}

/// A conversation's people as control's archive takes them: its owner,
/// and each member with an account, from when they joined it unless they
/// were added with its history.
fn members(c: &OldConv, owner: &str) -> Vec<ChatReader> {
    let mut out: Vec<ChatReader> = Vec::new();
    let first = account_of(&c.owner, owner).into_iter().map(|a| ChatReader { account: a, from: None, watching: false });
    let rest = c.members.iter().filter_map(|m| {
        let account = account_of(&m.who, owner)?;
        Some(ChatReader { account, from: (!m.history && m.joined > 0).then_some(m.joined), watching: false })
    });
    for r in first.chain(rest) {
        if !out.iter().any(|x| x.account == r.account) {
            out.push(r);
        }
    }
    out
}

/// The calls that carry an export, each under control's limits: at most
/// [`wire::MAX_IMPORT_ROOMS`] threads and archives, and about
/// [`wire::MAX_IMPORT_BODY`] bytes. A thread too long for one goes in
/// parts, oldest first. The reader lists go with the first, when they fit
/// in one call (otherwise they're sent on their own before).
fn calls(e: &Export, owner: &str, readers: &[ChatReaderList]) -> Vec<ChatImport> {
    enum Part {
        Thread(ChatImportThread),
        Archive(ChatImportArchive),
    }
    let size = |m: &ChatImportMessage| serde_json::to_vec(m).map_or(0, |b| b.len()) + 1;
    let mut parts: Vec<(Part, usize)> = Vec::new();
    // Split a timeline into runs that fit.
    let runs = |msgs: Vec<ChatImportMessage>| -> Vec<(Vec<ChatImportMessage>, usize)> {
        let mut out: Vec<(Vec<ChatImportMessage>, usize)> = vec![(Vec::new(), 0)];
        for m in msgs {
            let n = size(&m);
            let cur = out.last_mut().expect("never empty");
            if !cur.0.is_empty() && cur.1 + n > wire::MAX_IMPORT_BODY / 2 {
                out.push((vec![m], n));
            } else {
                cur.0.push(m);
                cur.1 += n;
            }
        }
        out
    };
    for (t, msgs) in &e.threads {
        let (kind, target) = kind_of(*t);
        for (messages, n) in runs(msgs.iter().map(|m| message(m, owner)).collect()) {
            parts.push((Part::Thread(ChatImportThread { kind, target: target.clone(), messages }), n + 64));
        }
    }
    for (c, msgs) in &e.convs {
        for (messages, n) in runs(msgs.iter().map(|m| message(m, owner)).collect()) {
            let a = ChatImportArchive {
                id: c.id.clone(),
                title: c.title.clone(),
                created: c.created,
                members: members(c, owner),
                messages,
            };
            let head = serde_json::to_vec(&a.members).map_or(0, |b| b.len()) + a.title.len() + 128;
            parts.push((Part::Archive(a), n + head));
        }
    }
    let mut out: Vec<(ChatImport, usize)> = vec![(ChatImport::default(), 0)];
    if readers.len() <= wire::MAX_READER_TARGETS {
        out[0].0.readers = readers.to_vec();
        out[0].1 = serde_json::to_vec(readers).map_or(0, |b| b.len());
    }
    for (p, n) in parts {
        let cur = out.last_mut().expect("never empty");
        let rooms = cur.0.threads.len() + cur.0.archives.len();
        let fits = rooms == 0 || (rooms < wire::MAX_IMPORT_ROOMS && cur.1 + n <= wire::MAX_IMPORT_BODY);
        if !fits {
            out.push((ChatImport::default(), 0));
        }
        let cur = out.last_mut().expect("never empty");
        cur.1 += n;
        match p {
            Part::Thread(t) => cur.0.threads.push(t),
            Part::Archive(a) => cur.0.archives.push(a),
        }
    }
    out.into_iter().map(|(c, _)| c).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// From JSON, as an older daemon wrote its files.
    fn msg(id: u64, who: &str, text: &str) -> OldMsg {
        serde_json::from_value(json!({
            "id": id, "at": 1000 + id, "who": who, "name": who.trim_start_matches("account:"), "text": text,
        }))
        .unwrap()
    }

    #[test]
    fn authors_mentions_and_quotes_become_accounts_and_text() {
        let mut m = msg(3, "owner", "@alex look");
        m.quote = Some(OldQuote { pane: 7, text: "FAILED mean".into() });
        m.mentions = vec!["account:a2".into(), "tailnet:sam@x".into(), "owner".into(), "account:a2".into()];
        let got = message(&m, "a1");
        assert_eq!(got.account.as_deref(), Some("a1"), "the owner is the machine's account");
        assert_eq!((got.quote.as_deref(), got.pane.as_deref()), (Some("FAILED mean"), Some("7")));
        assert_eq!(got.mentions, ["a2", "a1"], "accounts only, once each");
        assert_eq!(message(&msg(1, "tailnet:sam@x", "hi"), "a1").account, None, "no account: kept by name");
        assert_eq!(message(&msg(1, "mcp:claude", "hi"), "a1").account, None);
        assert_eq!(message(&msg(1, "account:a2", "hi"), "a1").account.as_deref(), Some("a2"));
    }

    #[test]
    fn reader_lists_name_accounts_but_not_the_owner() {
        let lists = vec![
            (
                ChatTarget::Pane(2),
                vec![
                    ("account:a2".into(), 0, false),
                    ("tailnet:x".into(), 0, false),
                    ("account:a1".into(), 0, false),
                    ("account:a3".into(), 55, true),
                ],
            ),
            (ChatTarget::Session(1), vec![]),
        ];
        let got = readers(lists, "a1");
        assert_eq!(got.len(), 2);
        assert_eq!((got[0].kind, got[0].target.as_str()), (ChatTargetKind::Pane, "2"));
        assert_eq!(
            got[0].readers,
            [
                ChatReader { account: "a2".into(), from: None, watching: false },
                ChatReader { account: "a3".into(), from: Some(55), watching: true }
            ]
        );
        assert_eq!((got[1].kind, got[1].target.as_str(), got[1].readers.len()), (ChatTargetKind::Session, "1", 0));
    }

    #[test]
    fn a_conversation_keeps_its_members_and_their_floors() {
        let c: OldConv = serde_json::from_value(json!({
            "id": "c-00000000000000000000000000000001", "title": "calc", "owner": "owner", "created": 5,
            "members": [
                { "who": "account:a2", "name": "alex", "joined": 100 },
                { "who": "account:a3", "name": "carol", "joined": 200, "history": true },
                { "who": "tailnet:x", "name": "x", "joined": 300 },
            ],
        }))
        .unwrap();
        assert_eq!(
            members(&c, "a1"),
            [
                ChatReader { account: "a1".into(), from: None, watching: false },
                ChatReader { account: "a2".into(), from: Some(100), watching: false },
                ChatReader { account: "a3".into(), from: None, watching: false },
            ]
        );
    }

    #[test]
    fn a_big_export_goes_in_calls_that_fit_with_long_threads_in_parts() {
        let big = "x".repeat(8 * 1024);
        let long: Vec<OldMsg> = (1..=200).map(|i| msg(i, "owner", &big)).collect();
        let e = Export {
            threads: vec![(ChatTarget::Pane(1), long), (ChatTarget::Session(1), vec![msg(1, "owner", "hi")])],
            convs: vec![],
        };
        let readers = vec![ChatReaderList { kind: ChatTargetKind::Pane, target: "1".into(), readers: vec![] }];
        let calls = calls(&e, "a1", &readers);
        assert!(calls.len() > 1);
        assert_eq!(calls[0].readers, readers, "the reader lists go first");
        assert!(calls[1..].iter().all(|c| c.readers.is_empty()));
        for c in &calls {
            assert!(serde_json::to_vec(c).unwrap().len() <= wire::MAX_IMPORT_BODY + 4096, "each call fits");
        }
        // Pane 1's parts, oldest first, and every message once.
        let ids: Vec<u64> = calls
            .iter()
            .flat_map(|c| {
                c.threads
                    .iter()
                    .filter(|t| t.kind == ChatTargetKind::Pane)
                    .flat_map(|t| t.messages.iter().map(|m| m.id))
            })
            .collect();
        assert_eq!(ids, (1..=200).collect::<Vec<_>>());
        assert_eq!(calls.iter().flat_map(|c| &c.threads).filter(|t| t.kind == ChatTargetKind::Session).count(), 1);
    }

    /// What an older daemon left on disk, read as it wrote it: threads,
    /// a conversation with its timeline, a torn last line skipped; nothing
    /// once moved aside.
    #[test]
    fn the_files_an_older_daemon_left_are_read_once() {
        let state = std::env::temp_dir().join(format!("arugula-chat-import-{}-{}", std::process::id(), line!()));
        let dir = state.join(THREADS);
        std::fs::create_dir_all(&dir).unwrap();
        let line = |v: serde_json::Value| format!("{v}\n");
        std::fs::write(
            dir.join("pane-4.jsonl"),
            line(json!({ "id": 1, "at": 1, "who": "owner", "name": "Jake", "text": "hi", "pic": "p", "landed": ["x"],
                         "quote": { "pane": 4, "text": "ls" } }))
                + "{\"id\": 2, \"at\"",
        )
        .unwrap();
        std::fs::write(
            dir.join("session-2.jsonl"),
            line(json!({ "id": 1, "at": 2, "who": "owner", "name": "J", "text": "s" })),
        )
        .unwrap();
        std::fs::write(dir.join("reads.json"), "{}").unwrap();
        let id = "c-0000000000000000000000000000000a";
        std::fs::write(
            dir.join(format!("conv-{id}.json")),
            json!({ "id": id, "title": "calc", "owner": "owner", "owner_name": "Jake", "created": 5,
                    "members": [{ "who": "account:a2", "name": "alex", "joined": 6, "root": "r" }] })
            .to_string(),
        )
        .unwrap();
        std::fs::write(
            dir.join(format!("conv-{id}.jsonl")),
            line(json!({ "id": 1, "at": 7, "who": "owner", "name": "J", "text": "c" })),
        )
        .unwrap();
        let e = Export::read(&state);
        let got: Vec<(ChatTarget, usize)> = e.threads.iter().map(|(t, m)| (*t, m.len())).collect();
        assert_eq!(got, [(ChatTarget::Pane(4), 1), (ChatTarget::Session(2), 1)]);
        assert_eq!(e.threads[0].1[0].quote.as_ref().map(|q| q.pane), Some(4));
        assert_eq!((e.convs.len(), e.convs[0].0.title.as_str(), e.convs[0].1.len()), (1, "calc", 1));
        retire(&state).unwrap();
        assert!(state.join(IMPORTED).join("pane-4.jsonl").exists() && !dir.exists());
        let again = Export::read(&state);
        assert!(again.threads.is_empty() && again.convs.is_empty(), "imported: nothing to read");
        assert!(retire(&state).is_err(), "never over what was moved aside");
        std::fs::remove_dir_all(&state).unwrap();
    }
}
