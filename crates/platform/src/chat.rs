//! Chat's core, in the platform's base (#314): what the rest of the
//! platform names of chat, whose routes, rooms, messages and agent
//! sessions are `arugula-platform-chat`'s.
//!
//! - **Whether chat is on** for an account ([`chat_on`], [`FLAG`]), and
//!   any Labs flag by the same rule ([`flag_on`]); a person's name
//!   ([`name_of`]); whom a text mentions ([`mentions`]).
//! - **The live bus** ([`Hub`], `Platform::chat`): this machine's open
//!   streams and the events every machine published (`chat_events`), so
//!   issues, boards, the cache and the hub's port can say a room or an
//!   object changed without the chat crate.
//! - **An account's erasure** ([`erase_account`], [`account_gone`]), in
//!   the platform's transaction, and **following the hub's change feed**
//!   ([`start`]).

use std::{
    collections::{HashMap, HashSet},
    sync::{
        Arc, Mutex,
        atomic::{AtomicU64, Ordering},
    },
    time::{Duration, Instant},
};

use arugula_control_common::{ApiError, err};
use arugula_control_wire::chat::{self as wire, ChatLive};
use arugula_e2e::now_ms;
use axum::http::StatusCode;
use tokio::sync::broadcast;

use crate::{
    Platform, cache,
    db::{
        Db,
        chat::NewChatEvent,
        sessions::{Role, Session},
    },
    hub::{Account, Flags},
};

/// Control's chat setting, kept beside an account's and a team's Labs
/// flags under this name (chat left Labs, native #203: no machine has a
/// flag for it).
pub const FLAG: &str = "chat";
/// The reason a refusal gives when chat isn't on for the account.
pub const CHAT_OFF: &str = "chat_off";
/// ...and when a viewer posts in a session's or pane's thread.
pub const WATCHING: &str = "watching";
/// Live streams: open at once, per account and in all (per machine); the
/// most events one is sent again when it resumes, and its channel's
/// backlog.
pub const STREAMS_PER_ACCOUNT: usize = 20;
pub const STREAMS: usize = 5_000;
pub const RING: usize = 1_024;
/// At most one push per room per person in this long.
pub const PUSH_GAP_MS: u64 = 60_000;
/// At most one typing ping per account per room in this long: the page
/// sends one every [`wire::TYPING_PING_MS`], so this only holds back a
/// client that doesn't.
pub const TYPING_GAP_MS: u64 = 1_000;
/// The reserved words: `@arugula` starts an agent session (M256), and
/// `@codex` reviews (M264). Neither names anybody.
pub const SESSION_MENTION: &str = "arugula";
pub const REVIEW_MENTION: &str = "codex";

// ---------------------------------------------------------------- who

pub fn account_owner(account: &str) -> String {
    format!("account:{account}")
}

pub fn team_owner(team: &str) -> String {
    format!("team:{team}")
}

pub fn flag_of(flags: &HashMap<String, Flags>, owner: &str, flag: &str) -> Option<bool> {
    flags.get(owner).and_then(|f| f.get(flag)).copied()
}

/// Whether chat is on for an account: its own flag if it set one, else on
/// when any team it's in turned it on.
pub async fn chat_on(app: &Platform, account: &str) -> anyhow::Result<bool> {
    flag_on(app, account, FLAG).await
}

/// Whether the Labs flag `flag` is on for an account, by the rule chat's
/// is: its own if it set one, else on when any team it's in turned it on.
pub async fn flag_on(app: &Platform, account: &str, flag: &str) -> anyhow::Result<bool> {
    let teams = app.cache.teams_of(&*app.hub, account).await?;
    let mut owners = vec![account_owner(account)];
    owners.extend(teams.iter().map(|t| team_owner(t)));
    let flags = app.cache.flags(&*app.hub, &owners).await?;
    if let Some(on) = flag_of(&flags, &account_owner(account), flag) {
        return Ok(on);
    }
    Ok(teams.iter().any(|t| flag_of(&flags, &team_owner(t), flag) == Some(true)))
}

/// An account's name: the one it chose, else its login, else "".
pub async fn name_of(app: &Platform, account: &str) -> anyhow::Result<String> {
    Ok(app.cache.accounts(&*app.hub, &[account.to_owned()]).await?.remove(account).map(|a| a.name).unwrap_or_default())
}

// ---------------------------------------------------------------- mentions

/// The `@words` in a text, lowercased: an `@` at the start or after
/// anything but a letter or digit (not an email address), then letters,
/// digits, `_`, `-` and `.` (not a trailing one).
pub fn at_words(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut prev: Option<char> = None;
    for (i, c) in text.char_indices() {
        if c == '@' && !prev.is_some_and(char::is_alphanumeric) {
            let rest = &text[i + 1..];
            let end = rest
                .char_indices()
                .find(|(_, ch)| !(ch.is_alphanumeric() || matches!(ch, '_' | '-' | '.')))
                .map_or(rest.len(), |(j, _)| j);
            let w = rest[..end].trim_end_matches(['.', '-']).to_lowercase();
            if !w.is_empty() && !out.contains(&w) {
                out.push(w);
            }
        }
        prev = Some(c);
    }
    out
}

/// The one word a roster calls a person (control's `teams::member_name`):
/// the name they chose, its words joined by `-`, else `account-` and the
/// start of their id.
pub fn member_name(id: &str, chosen: &str) -> String {
    let words: Vec<&str> =
        chosen.split(|c: char| c.is_whitespace() || c.is_control()).filter(|w| !w.is_empty()).collect();
    let name: String = words.join("-").chars().take(120).collect();
    if name.is_empty() { format!("account-{}", &id[..6.min(id.len())]) } else { name }
}

/// What an `@word` may name an account by: its login, its name, and the one
/// word a roster calls it. With no name of its own an account goes by its
/// login (`Account::name`), and the roster's word for it is its id's.
pub fn names_of(a: &Account) -> Vec<String> {
    let mut v = vec![a.login.to_lowercase(), a.name.to_lowercase(), member_name(&a.id, &a.name).to_lowercase()];
    if a.name == a.login {
        v.push(member_name(&a.id, "").to_lowercase());
    }
    v
}

/// Whom a text mentions among `audience` (by login, name, or the one word
/// a roster calls them), leaving out the author, and the `@words` that
/// named nobody there.
pub async fn mentions(
    app: &Platform,
    text: &str,
    audience: &[String],
    author: &str,
) -> anyhow::Result<(Vec<String>, Vec<String>)> {
    // `@arugula` is reserved (M256): it names nobody, and starts work; so
    // is `@codex`, which reviews (M264).
    let words: Vec<String> =
        at_words(text).into_iter().filter(|w| w != SESSION_MENTION && w != REVIEW_MENTION).collect();
    if words.is_empty() {
        return Ok((vec![], vec![]));
    }
    let accounts = app.cache.accounts(&*app.hub, audience).await?;
    let mut hit: HashSet<&str> = HashSet::new();
    let mut named = Vec::new();
    for a in audience {
        let Some(acc) = accounts.get(a) else { continue };
        let names = names_of(acc);
        let mut found = false;
        for w in &words {
            if names.iter().any(|n| !n.is_empty() && n == w) {
                hit.insert(w);
                found = true;
            }
        }
        if found && a != author {
            named.push(a.clone());
        }
    }
    let unreached = words.iter().filter(|w| !hit.contains(w.as_str())).cloned().collect();
    Ok((named, unreached))
}

// ---------------------------------------------------------------- live

/// One event for open pages, numbered; `to` limits it to one account (its
/// read marks).
pub struct Out {
    pub n: u64,
    pub ev: ChatLive,
    pub to: Option<String>,
}

/// What's open on this machine, in memory: the live streams, and when each
/// person was last pushed for each room.
///
/// **The events aren't kept here (M144).** Each is a row of `chat_events`,
/// numbered across every machine the platform runs on ([`Hub::publish`]).
/// A machine sends its streams the rows after the last one it sent
/// ([`Hub::catch_up`]): when it publishes one itself, when another machine
/// wakes it (`pg_notify`, heard by [`start`]), and every
/// [`CATCH_UP`] besides. So a stream hears what was posted through any
/// machine, and a page that dropped resumes on whichever machine it
/// reaches next (`Last-Event-ID` is `c-<seq>`), from the database, as long
/// as the rows it missed are still kept
/// ([`CHAT_EVENTS_KEPT`](crate::db::chat::CHAT_EVENTS_KEPT)); otherwise it
/// resyncs.
pub struct Hub {
    /// The newest event sent to this machine's streams.
    last: AtomicU64,
    /// Held while events are read in, so each goes out once, in order.
    reading: Mutex<()>,
    tx: broadcast::Sender<Arc<Out>>,
    streams: Mutex<HashMap<String, StreamInfo>>,
    /// Moves when who sees what may have changed (a roster, a reader list,
    /// a flag, read from the hub's change feed): each stream re-reads its
    /// rooms.
    generation: AtomicU64,
    pushed: Mutex<HashMap<(String, String), u64>>,
    /// When each person last pinged typing in each room ([`typing`]).
    typed: Mutex<HashMap<(String, String), u64>>,
}

/// Event ids are `c-<seq>`, `seq` being the event's row in `chat_events`
/// (M144), the same on every machine and across restarts. An id from
/// before (`<epoch>-<n>`, numbered by one process) resyncs.
const EPOCH: &str = "c";

/// How often a machine reads events in even when nothing woke it: what a
/// lost notification can cost.
pub const CATCH_UP: Duration = Duration::from_secs(5);

/// Events read in at a time.
const EVENTS_PAGE: u32 = 500;

pub struct StreamInfo {
    account: String,
    /// The rooms its page shows (more than one with several chat blocks).
    open: Vec<String>,
    /// The thread open beside it (#106).
    thread: Option<i64>,
}

impl Default for Hub {
    fn default() -> Self {
        Self {
            last: AtomicU64::new(0),
            reading: Mutex::new(()),
            tx: broadcast::channel(RING).0,
            streams: Mutex::new(HashMap::new()),
            generation: AtomicU64::new(0),
            pushed: Mutex::new(HashMap::new()),
            typed: Mutex::new(HashMap::new()),
        }
    }
}

impl Hub {
    /// Start after the newest event the database has: a machine that
    /// starts sends its streams only what comes after.
    pub fn start_at(&self, db: &Db) {
        match db.chat_event_bounds() {
            Ok((_, newest)) => self.last.store(newest, Ordering::SeqCst),
            Err(e) => tracing::warn!(error = %e, "chat: reading the newest live event"),
        }
    }

    /// Keep `ev` (for `to` only, if it's someone's) for every machine's
    /// streams, then send this machine's what's new.
    pub fn publish(&self, db: &Db, ev: ChatLive, to: Option<String>) {
        let body = data(&ev);
        let e = NewChatEvent { kind: "live", body: &body, to: to.as_deref(), at: now_ms() };
        if let Err(e) = db.add_chat_event(&e) {
            tracing::warn!(error = %e, "chat: keeping a live event");
        }
        self.catch_up(db);
    }

    /// Send this machine's streams every event after the last one sent,
    /// in order: what any machine published since.
    pub fn catch_up(&self, db: &Db) {
        let _one = self.reading.lock().unwrap();
        loop {
            let page = match db.chat_events_after(self.last.load(Ordering::SeqCst), EVENTS_PAGE) {
                Ok(p) => p,
                Err(e) => return tracing::warn!(error = %e, "chat: reading live events"),
            };
            for e in &page {
                match e.kind.as_str() {
                    "live" => match serde_json::from_str::<ChatLive>(&e.body) {
                        Ok(ev) => {
                            if matches!(ev, ChatLive::Room { .. }) {
                                self.changed();
                            }
                            let _ = self.tx.send(Arc::new(Out { n: e.seq, ev, to: e.to.clone() }));
                        }
                        Err(err) => {
                            tracing::warn!(seq = e.seq, error = %err, "chat: a live event this build can't read")
                        }
                    },
                    // A ping of #111: sent while it's fresh, not replayed
                    // after (a machine that was away, a stream that resumes).
                    "typing" => {
                        if now_ms().saturating_sub(e.at) <= wire::TYPING_MS
                            && let Ok(ev) = serde_json::from_str::<ChatLive>(&e.body)
                        {
                            // `n: 0`: not numbered, so no stream keeps its place by it.
                            let _ = self.tx.send(Arc::new(Out { n: 0, ev, to: None }));
                        }
                    }
                    // A project's warm mark (M262): nothing to send.
                    "warm" => {}
                    _ => self.changed(),
                }
                self.last.store(e.seq, Ordering::SeqCst);
            }
            if page.len() < EVENTS_PAGE as usize {
                return;
            }
        }
    }

    /// Who sees what may have changed, here: streams re-read their rooms.
    /// The hub's change feed moves it on every machine, each following it.
    pub fn changed(&self) {
        self.generation.fetch_add(1, Ordering::SeqCst);
    }

    /// Who sees what may have changed, through this machine (a reader list,
    /// an import): every machine's streams re-read their rooms.
    pub fn changed_everywhere(&self, db: &Db) {
        self.changed();
        let e = NewChatEvent { kind: "changed", body: "", to: None, at: now_ms() };
        if let Err(e) = db.add_chat_event(&e) {
            tracing::warn!(error = %e, "chat: keeping a live event");
        }
        self.catch_up(db);
    }

    /// A room appeared, changed or went.
    pub fn room(&self, db: &Db, room: &str) {
        self.changed();
        self.publish(db, ChatLive::Room { room: room.to_owned() }, None);
    }

    /// The object `target`'s thread is on changed (M203): pages showing
    /// it read it again. Nothing when nobody has made its thread.
    pub fn object_changed(&self, db: &Db, scope: &str, target: &str) {
        match db.chat_room_on(scope, target) {
            Ok(Some(room)) => {
                self.publish(db, ChatLive::Object { room: room.id, target: target.to_owned() }, None);
            }
            Ok(None) => {}
            Err(e) => tracing::warn!(error = %e, "chat: finding an object's thread"),
        }
    }

    pub fn id(&self, n: u64) -> String {
        format!("{EPOCH}-{n}")
    }

    /// The events a page that last had `id` missed, from the database, to
    /// send again; `None` when they can't all be: an id from before M144
    /// or from another database, or more missed than are kept or than
    /// [`RING`].
    pub fn missed(&self, db: &Db, id: &str) -> Option<(u64, Vec<Arc<Out>>)> {
        let n: u64 = id.strip_prefix(EPOCH)?.strip_prefix('-')?.parse().ok()?;
        self.catch_up(db);
        if n > self.last.load(Ordering::SeqCst) {
            return None;
        }
        // Pruned past it: what it missed may be gone.
        let (oldest, _) = db.chat_event_bounds().ok()?;
        if oldest > n + 1 {
            return None;
        }
        let rows = db.chat_events_after(n, RING as u32 + 1).ok()?;
        if rows.len() > RING {
            return None;
        }
        let outs = rows
            .into_iter()
            .filter(|r| r.kind == "live")
            .filter_map(|r| Some(Arc::new(Out { n: r.seq, ev: serde_json::from_str(&r.body).ok()?, to: r.to })))
            .collect();
        Some((n, outs))
    }

    pub fn has_open(&self, account: &str, room: &str) -> bool {
        self.streams.lock().unwrap().values().any(|s| s.account == account && s.open.iter().any(|r| r == room))
    }

    /// Whether `account` has the thread `parent` of `room` open (#106).
    pub fn has_open_thread(&self, account: &str, room: &str, parent: i64) -> bool {
        self.streams.lock().unwrap().values().any(|s| {
            s.account == account && s.open.first().map(String::as_str) == Some(room) && s.thread == Some(parent)
        })
    }

    /// Whether `account` may be pushed for `room` now (and note it). Per
    /// machine (M144): with two, a room's pushes are at most one a
    /// [`PUSH_GAP_MS`] from each.
    pub fn may_push(&self, account: &str, room: &str, now: u64) -> bool {
        let mut p = self.pushed.lock().unwrap();
        if p.len() > 50_000 {
            p.retain(|_, at| now.saturating_sub(*at) < PUSH_GAP_MS);
        }
        let key = (account.to_owned(), room.to_owned());
        if p.get(&key).is_some_and(|at| now.saturating_sub(*at) < PUSH_GAP_MS) {
            return false;
        }
        p.insert(key, now);
        true
    }

    /// A page's stream opens on this machine (`GET /api/chat/live`), unless
    /// too many are open here, or for its account.
    pub fn add_stream(&self, id: &str, account: &str) -> Result<(), ApiError> {
        let mut streams = self.streams.lock().unwrap();
        if streams.len() >= STREAMS {
            return Err(err(StatusCode::SERVICE_UNAVAILABLE, "too many open pages here right now; try again soon"));
        }
        if streams.values().filter(|x| x.account == account).count() >= STREAMS_PER_ACCOUNT {
            return Err(err(StatusCode::TOO_MANY_REQUESTS, "too many open chat pages; close some"));
        }
        streams.insert(id.to_owned(), StreamInfo { account: account.to_owned(), open: Vec::new(), thread: None });
        Ok(())
    }

    /// The stream `id` ended: its account, when it was that account's last
    /// one here.
    pub fn drop_stream(&self, id: &str) -> Option<String> {
        let mut streams = self.streams.lock().unwrap();
        let account = streams.remove(id).map(|s| s.account);
        account.filter(|a| !streams.values().any(|s| s.account == *a))
    }

    /// The rooms (and the thread beside the first) the stream `id`'s page
    /// has open; `false` when it isn't `account`'s stream.
    pub fn open_in(&self, id: &str, account: &str, open: Vec<String>, thread: Option<i64>) -> bool {
        let mut streams = self.streams.lock().unwrap();
        let Some(st) = streams.get_mut(id).filter(|x| x.account == account) else { return false };
        st.open = open;
        st.thread = thread;
        true
    }

    /// The last event sent here, and this machine's events from now:
    /// what a new stream starts from.
    pub fn subscribe(&self) -> (u64, broadcast::Receiver<Arc<Out>>) {
        let start = self.last.load(Ordering::SeqCst);
        (start, self.tx.subscribe())
    }

    /// The newest event sent to this machine's streams.
    pub fn last(&self) -> u64 {
        self.last.load(Ordering::SeqCst)
    }

    /// Moves when who sees what may have changed ([`Hub::changed`]).
    pub fn generation(&self) -> u64 {
        self.generation.load(Ordering::SeqCst)
    }

    /// Tell every machine's streams that `account` is typing in `room`
    /// (#111). Whether it was sent (a ping inside the gap isn't).
    pub fn typing(&self, db: &Db, room: &str, account: &str, name: &str, now: u64) -> bool {
        {
            let mut t = self.typed.lock().unwrap();
            if t.len() > 50_000 {
                t.retain(|_, at| now.saturating_sub(*at) < TYPING_GAP_MS);
            }
            let key = (account.to_owned(), room.to_owned());
            if t.get(&key).is_some_and(|at| now.saturating_sub(*at) < TYPING_GAP_MS) {
                return false;
            }
            t.insert(key, now);
        }
        let ev = ChatLive::Typing { room: room.to_owned(), account: account.to_owned(), name: name.to_owned() };
        let body = data(&ev);
        let e = NewChatEvent { kind: "typing", body: &body, to: None, at: now };
        if let Err(e) = db.add_chat_event(&e) {
            tracing::warn!(error = %e, "chat: keeping a typing ping");
        }
        self.catch_up(db);
        true
    }

    /// Whether the stream `id` has `room` open (it hears typing there only).
    pub fn stream_has_open(&self, id: &str, room: &str) -> bool {
        self.streams.lock().unwrap().get(id).is_some_and(|s| s.open.iter().any(|r| r == room))
    }

    /// Session `s` changed (M257): pages showing its card read it again, on
    /// every machine (through `chat_events`). A Try 3's sibling's parent
    /// card shows it in its compare view (M264), so it's told too.
    pub fn session(&self, db: &Db, s: &Session) {
        self.publish(db, ChatLive::Session { room: s.room.clone(), ses: s.id.clone() }, None);
        if let (Role::Try, Some(parent)) = (s.role, &s.parent_session) {
            self.publish(db, ChatLive::Session { room: s.room.clone(), ses: parent.clone() }, None);
        }
    }

    /// Hear sessions change on this machine's streams, from now: what a
    /// wait on them wakes on (Fleet 5, the chat crate's `sessions::wave`).
    pub fn session_moves(&self) -> SessionMoves {
        SessionMoves(self.tx.subscribe())
    }
}

/// [`Hub::session_moves`].
pub struct SessionMoves(broadcast::Receiver<Arc<Out>>);

impl SessionMoves {
    /// Once one of `ids` changed, or this missed some events (so it may
    /// have).
    pub async fn changed(&mut self, ids: &[String]) {
        use tokio::sync::broadcast::error::RecvError;
        loop {
            match self.0.recv().await {
                Ok(o) => {
                    if let ChatLive::Session { ses, .. } = &o.ev
                        && ids.contains(ses)
                    {
                        return;
                    }
                }
                Err(RecvError::Lagged(_)) => return,
                Err(RecvError::Closed) => std::future::pending().await,
            }
        }
    }
}

pub fn data(ev: &ChatLive) -> String {
    serde_json::to_string(ev).unwrap_or_default()
}

/// What deleting an account takes from chat, in the platform's
/// transaction (`PlatformPort::account_deleted`): its DMs (both sides),
/// the threads on its machines, the rooms of the teams it disbanded, and
/// its name on everything else. The rooms that go, for open pages.
pub fn erase_account(tx: &impl crate::db::Sql, deleted: &crate::Deleted) -> anyhow::Result<Vec<String>> {
    let dms: Vec<String> = tx.query_map(
        "SELECT room FROM chat_members WHERE account = ?1",
        crate::db::sql::params![deleted.account],
        |r| r.get(0),
    )?;
    crate::db::chat::erase_account(tx, &deleted.account, &deleted.machines, &deleted.disbanded)?;
    Ok(dms)
}

/// After an account went: open pages hear of its rooms, and nothing kept of
/// the hub's answers outlives it.
pub fn account_gone(app: &Platform, rooms: &[String]) {
    app.cache.drop_all();
    for r in rooms {
        app.chat.room(&app.db, r);
    }
    app.chat.changed_everywhere(&app.db);
}

/// Chat's background work: following the hub's change feed
/// ([`cache::start`]), which moves who sees what; sending this machine's
/// streams the events other machines published (M144: woken by their
/// `pg_notify`, else every [`CATCH_UP`]); and pruning the oldest events
/// kept, every minute unless paused (any machine: it's the same delete).
pub fn start(app: &Arc<Platform>) {
    cache::start(app);
    let app = Arc::downgrade(app);
    tokio::spawn(async move {
        let mut pruned = Instant::now();
        loop {
            let Some(p) = app.upgrade() else { return };
            p.chat.catch_up(&p.db);
            if let Err(e) = p.db.prune_typing(now_ms()) {
                tracing::warn!(error = %e, "chat: pruning typing");
            }
            if pruned.elapsed() >= Duration::from_secs(60) && !p.is_paused() {
                if let Err(e) = p.db.prune_chat_events() {
                    tracing::warn!(error = %e, "chat: pruning live events");
                }
                if let Err(e) = p.db.sweep_chat_files(now_ms()) {
                    tracing::warn!(error = %e, "chat: sweeping files nobody posted");
                }
                pruned = Instant::now();
            }
            let heard = p.listening.as_ref().map(|l| l.heard.clone());
            drop(p);
            match heard {
                Some(h) => drop(tokio::time::timeout(CATCH_UP, h.notified()).await),
                None => tokio::time::sleep(CATCH_UP).await,
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn at_words_are_names_not_emails() {
        assert_eq!(at_words("@alex the mean test fails, cc @Sam-Stranger."), ["alex", "sam-stranger"]);
        assert_eq!(at_words("mail jake@example.com"), Vec::<String>::new());
        assert_eq!(at_words("(@jhgaylor) and @jhgaylor again, @"), ["jhgaylor"]);
        assert_eq!(at_words("@ünïcode @a.b."), ["ünïcode", "a.b"]);
    }

    #[test]
    fn an_account_is_named_by_its_login_name_or_roster_word() {
        let a = |id: &str, login: &str, name: &str| Account { id: id.into(), login: login.into(), name: name.into() };
        assert_eq!(names_of(&a("a1jake", "jhgaylor", "Jake Gaylor")), ["jhgaylor", "jake gaylor", "jake-gaylor"]);
        // With no name of its own: its login, and the roster's word for it.
        assert_eq!(names_of(&a("a2alex", "alex", "alex")), ["alex", "alex", "alex", "account-a2alex"]);
        assert_eq!(names_of(&a("a3pass", "", "")), ["", "", "account-a3pass", "account-a3pass"]);
    }

    #[test]
    fn a_ping_goes_to_other_streams_and_not_twice_in_a_second() {
        let db = Db::test();
        let hub = Hub::default();
        let mut rx = hub.tx.subscribe();
        let now = now_ms();
        assert!(hub.typing(&db, "ch_1", "a1", "Alex", now));
        assert!(!hub.typing(&db, "ch_1", "a1", "Alex", now + 500), "inside the gap");
        assert!(hub.typing(&db, "ch_2", "a1", "Alex", now + 500), "another room");
        assert!(hub.typing(&db, "ch_1", "a1", "Alex", now + 1_000));
        let first = rx.try_recv().unwrap();
        assert_eq!(first.n, 0, "not numbered: nothing replays it");
        assert!(matches!(&first.ev, ChatLive::Typing { room, account, .. } if room == "ch_1" && account == "a1"));
    }

    #[test]
    fn a_machine_that_was_away_hears_only_fresh_pings_and_old_ones_are_pruned() {
        let db = Db::test();
        let a = Hub::default();
        let b = Hub::default();
        let mut rx = b.tx.subscribe();
        let now = now_ms();
        let ping = |room: &str, at: u64| {
            let ev = ChatLive::Typing { room: room.into(), account: "a1".into(), name: "Alex".into() };
            db.add_chat_event(&NewChatEvent { kind: "typing", body: &data(&ev), to: None, at }).unwrap();
        };
        ping("ch_old", now - crate::db::chat::TYPING_KEPT_MS - 1_000);
        // Another machine's ping reaches this one through the table.
        assert!(a.typing(&db, "ch_new", "a1", "Alex", now));
        b.catch_up(&db);
        let heard = rx.try_recv().unwrap();
        assert!(matches!(&heard.ev, ChatLive::Typing { room, .. } if room == "ch_new"));
        assert!(rx.try_recv().is_err(), "a stale ping isn't sent");
        // A resume reads `live` rows only.
        let (oldest, _) = db.chat_event_bounds().unwrap();
        assert!(a.missed(&db, &format!("c-{}", oldest - 1)).is_none_or(|(_, outs)| outs.is_empty()));
        // The stale one goes; the fresh one stays for a while.
        assert_eq!(db.prune_typing(now + 1).unwrap(), 1);
        assert_eq!(db.chat_events_after(0, 10).unwrap().len(), 1);
        assert_eq!(db.prune_typing(now + crate::db::chat::TYPING_KEPT_MS + 1).unwrap(), 1);
    }

    #[test]
    fn only_a_stream_with_the_room_open_hears_it() {
        let hub = Hub::default();
        hub.streams
            .lock()
            .unwrap()
            .insert("s1".into(), StreamInfo { account: "b".into(), open: vec!["ch_1".into()], thread: None });
        hub.streams
            .lock()
            .unwrap()
            .insert("s2".into(), StreamInfo { account: "c".into(), open: Vec::new(), thread: None });
        assert!(hub.stream_has_open("s1", "ch_1"));
        assert!(!hub.stream_has_open("s1", "ch_2"));
        assert!(!hub.stream_has_open("s2", "ch_1"));
    }

    #[test]
    fn a_page_with_two_chat_blocks_has_both_rooms_open() {
        let hub = Hub::default();
        hub.streams.lock().unwrap().insert(
            "s1".into(),
            StreamInfo { account: "b".into(), open: vec!["ch_1".into(), "ch_2".into()], thread: None },
        );
        assert!(hub.stream_has_open("s1", "ch_1") && hub.stream_has_open("s1", "ch_2"));
        assert!(hub.has_open("b", "ch_2"), "no push for either");
        assert!(!hub.has_open("c", "ch_2"));
    }
}
