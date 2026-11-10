//! Telling control who reads each pane's and session's thread (M124,
//! decision 2 of S41).
//!
//! Chat lives in control (M122); the daemon keeps none of it (M125). While
//! control has chat on for this machine's account or team, it sends who
//! may read each pane's and session's thread, as this daemon decides it,
//! whenever that changes (`POST /api/daemon/chat/readers`): after each
//! refresh with control, and as soon as who may read what changes here
//! (a share given or taken away, a pane made private, a roster change;
//! native #684), debounced so a burst of changes is one send. A send names
//! the lists that changed since the last one control took (all of them
//! the first time, and after a send that failed), and ends with every
//! pane and session this machine still has a list for, so control empties
//! the rest. A pane or session that closed keeps its list while the mux
//! remembers it: those who read it when it closed and still may, so a
//! share taken away after it closed reaches control too, and one given
//! after doesn't widen it. A send that fails is tried
//! again on its own, sooner than the next refresh. Whoever
//! gets in as the owner and isn't the owner (a team's other owners, a
//! hosted workspace's editors; native #705) is listed by their own
//! account, as the owner's role.
//!
//! Chat's on/off is control's setting, not a Labs flag here (native #203):
//! it comes with the flags control sends ([`arugula_proto::flags::control_says`]),
//! and this machine's `flags.json` has no say in it. A machine joined to
//! the account alone hears its owner's teams' chat as
//! [`CHAT_HEARD`](arugula_control_wire::CHAT_HEARD) (native #673), which
//! counts here, as the platform would take the lists, and not for copying
//! Docs up. The threads an older
//! daemon kept itself (M61) in `<state>/threads/` are no longer brought
//! into control (the import of M124 is gone, native #203): nothing reads
//! them, and they stay on disk as they are.

use std::{sync::Arc, time::Duration};

use arugula_control_wire::chat::{
    self as wire, CHAT_READERS, ChatReader, ChatReaderList, ChatReaders, ChatReadersStored, ChatTargetKind,
    ChatTargetsHeld,
};
use tokio::time::Instant;
use tracing::{info, warn};

use crate::{
    control::{Answered, Control},
    mux::{Api, Readable},
    server::App,
};

/// Control's chat setting, as it sends it beside the Labs flags (the
/// platform's `chat::FLAG`): the account's or the team's on/off.
const CHAT: &str = "chat";

/// How long after a change the lists go, so the changes of one moment (a
/// share and the refresh it asks for, say) are one send.
const QUIET: Duration = Duration::from_secs(1);

/// The least time between two sends, times the posts the last send took
/// (one per [`wire::MAX_READER_TARGETS`] targets): at most 360 posts an
/// hour however the changes come, well under control's limit of 600 for
/// this route from one machine (`check_daemon`'s `chat-readers`).
const GAP: Duration = Duration::from_secs(10);

/// The most a post's body comes to, well under what control takes (2 MB):
/// a send splits its lists into posts of at most this, and at most
/// [`wire::MAX_READER_TARGETS`] lists.
const POST_BYTES: usize = 1024 * 1024;

/// How long after a send that failed it's tried again, doubled each time
/// it fails again up to [`RETRY_MOST`]. A refresh or a change sooner tries
/// it sooner.
const RETRY: Duration = Duration::from_secs(10);
const RETRY_MOST: Duration = Duration::from_secs(300);

/// Whether control has chat on for this machine: its account's or its
/// team's word, else heard from the owner's teams.
fn chat_on(state_dir: &std::path::Path) -> bool {
    match arugula_proto::flags::control_says(state_dir, CHAT) {
        Some(on) => on,
        None => arugula_proto::flags::control_says(state_dir, arugula_control_wire::CHAT_HEARD) == Some(true),
    }
}

/// Run beside the control loop: after each refresh, and whenever who may
/// read what changes here, keep control's reader lists current.
pub fn start(control: Arc<Control>, app: Arc<App>) {
    crate::task::spawn(async move {
        let mut refreshed = control.on_refresh();
        let mut access = app.mux.access_changes();
        let mut sync = Sync::default();
        loop {
            let due = sync.due;
            let wait = async {
                match due {
                    Some(at) => tokio::time::sleep_until(at).await,
                    None => std::future::pending().await,
                }
            };
            tokio::select! {
                r = refreshed.changed() => {
                    if r.is_err() { break }
                    sync.changed();
                }
                a = access.changed() => {
                    if a.is_err() { break }
                    sync.changed();
                }
                () = wait => sync.run(&control, &app).await,
            }
        }
    });
}

#[derive(Default)]
struct Sync {
    /// The reader lists control has, as last sent: all of them. `None`
    /// until a send went through, and after one that failed (control may
    /// have taken some of its posts): the next sends them all.
    sent: Option<Vec<ChatReaderList>>,
    /// How long until a send that failed is tried again; `None` after one
    /// that went through.
    retry: Option<Duration>,
    /// What was last logged, so a refusal is said once, not every minute.
    said: Option<String>,
    /// When the next send is due, once something changed.
    due: Option<Instant>,
    /// When the last send went, and how many posts it took.
    last: Option<(Instant, u32)>,
    /// The number of the last post made ([`ChatReaders::seq`]).
    seq: u64,
}

impl Sync {
    /// Something changed (a refresh, or who may read what): a send is due
    /// after [`QUIET`], and no sooner than [`GAP`] after the last one. A
    /// change while one is due doesn't move it, unless what's due is a
    /// retry it comes before.
    fn changed(&mut self) {
        if self.due.is_some() && self.retry.is_none() {
            return;
        }
        let now = Instant::now();
        let mut at = now + QUIET;
        if let Some((last, posts)) = self.last {
            at = at.max(last + GAP * posts.max(1));
        }
        self.due = Some(self.due.map_or(at, |d| d.min(at)));
    }

    /// A send that fails, or is refused, is tried again after [`RETRY`]
    /// (longer each time), or sooner on a change or a refresh, and then
    /// sends every list.
    async fn run(&mut self, control: &Control, app: &App) {
        self.due = None;
        let Some(owner) = control.account() else { return };
        if !chat_on(control.state_dir()) {
            self.sent = None;
            return;
        }
        // No answer from the mux (it's stopping): nothing is sent. An
        // empty list here would tell control the machine holds nothing.
        let Some(lists) = app.mux.api(Api::Readers).await else { return };
        let readers = readers(lists, &owner);
        if self.sent.as_ref() == Some(&readers) {
            return;
        }
        match self.send(control, &readers).await {
            Ok(()) => {
                self.said = None;
                self.retry = None;
            }
            Err(e) => {
                self.failed();
                let why = e.to_string();
                // Chat off for this machine in control after all, or a
                // maintenance window longer than `post_signed` waits
                // (M141): tried again after the next refresh, quietly.
                let quiet = e.downcast_ref::<Answered>().is_some_and(|a| matches!(a.0, 403..=405 | 503));
                if self.said.as_deref() != Some(why.as_str()) {
                    if quiet {
                        info!(why, "control doesn't take this machine's thread readers yet");
                    } else {
                        warn!(why, "can't tell control who reads this machine's threads");
                    }
                    self.said = Some(why);
                }
            }
        }
    }

    /// The next post's number: the time (ms), or one more than the last if
    /// that's later. A restart starts from the time; if the clock went
    /// back, control's answer says so ([`Sync::past`]).
    fn next_seq(&mut self, now_ms: u64) -> u64 {
        self.seq = now_ms.max(self.seq + 1);
        self.seq
    }

    /// Control holds lists from a post numbered `highest`, above this one
    /// (`seq`): this machine's clock went back, and some of what the post
    /// said was ignored as older. The next posts are numbered above it,
    /// and, as after any failure, send every list.
    fn past(&mut self, seq: u64, highest: Option<u64>) -> anyhow::Result<()> {
        match highest {
            Some(h) if h > seq => {
                self.seq = self.seq.max(h);
                anyhow::bail!("control holds this machine's thread readers from post {h}, past this one ({seq})")
            }
            _ => Ok(()),
        }
    }

    /// A send failed: control has some of it, or none. The next sends
    /// every list, after a retry's wait unless something sooner asks.
    fn failed(&mut self) {
        self.sent = None;
        let wait = self.retry.map_or(RETRY, |w| (w * 2).min(RETRY_MOST));
        self.retry = Some(wait);
        self.due = Some(Instant::now() + wait);
    }

    async fn send(&mut self, control: &Control, readers: &[ChatReaderList]) -> anyhow::Result<()> {
        let mut posts = 0;
        self.last = Some((Instant::now(), 1));
        for mut body in posts_for(self.sent.as_deref(), readers) {
            posts += 1;
            self.last = Some((Instant::now(), posts));
            let seq = self.next_seq(arugula_e2e::now_ms());
            body.seq = Some(seq);
            let stored: ChatReadersStored = control.post_signed(CHAT_READERS, &body).await?;
            self.past(seq, stored.seq)?;
        }
        self.sent = Some(readers.to_vec());
        Ok(())
    }
}

/// What a send posts to make control's lists `now` when it has `sent`
/// (`None`: not known, so all of them): the lists that changed, in posts
/// of at most [`wire::MAX_READER_TARGETS`] lists and [`POST_BYTES`], the
/// last naming every target held. Numbered as they go.
/// Past [`wire::MAX_HELD`] of either kind, nothing is said of what's held,
/// and control keeps the lists it isn't sent.
fn posts_for(sent: Option<&[ChatReaderList]>, now: &[ChatReaderList]) -> Vec<ChatReaders> {
    let key = |l: &ChatReaderList| (l.kind == ChatTargetKind::Pane, l.target.clone());
    let had: std::collections::HashMap<(bool, String), &Vec<ChatReader>> =
        sent.unwrap_or_default().iter().map(|l| (key(l), &l.readers)).collect();
    let changed: Vec<ChatReaderList> =
        now.iter().filter(|l| sent.is_none() || had.get(&key(l)) != Some(&&l.readers)).cloned().collect();
    let mut held = ChatTargetsHeld::default();
    for l in now {
        match l.kind {
            ChatTargetKind::Pane => held.panes.push(l.target.clone()),
            ChatTargetKind::Session => held.sessions.push(l.target.clone()),
            ChatTargetKind::Object => {}
        }
    }
    let held = (held.panes.len() <= wire::MAX_HELD && held.sessions.len() <= wire::MAX_HELD).then_some(held);
    let mut posts: Vec<ChatReaders> = Vec::new();
    let mut bytes = 0;
    for l in changed {
        let n = json_bytes(&l);
        let full = posts.last().is_none_or(|p| p.targets.len() >= wire::MAX_READER_TARGETS || bytes + n > POST_BYTES);
        if full {
            posts.push(ChatReaders::default());
            bytes = 0;
        }
        bytes += n;
        posts.last_mut().expect("a post").targets.push(l);
    }
    let held_bytes = held.as_ref().map_or(0, json_bytes);
    match posts.last_mut() {
        Some(last) if bytes + held_bytes <= POST_BYTES => last.held = held,
        _ => posts.push(ChatReaders { held, ..Default::default() }),
    }
    posts
}

/// How many bytes `v` comes to in a post's body, with the comma after it.
fn json_bytes(v: &impl serde::Serialize) -> usize {
    serde_json::to_vec(v).map_or(0, |b| b.len()) + 1
}

/// A principal here as control's account: the machine's owner, or an
/// `account:` one; nobody else has one.
fn account_of(who: &str, owner: &str) -> Option<String> {
    if who == "owner" {
        return Some(owner.to_owned());
    }
    who.strip_prefix("account:").filter(|a| !a.is_empty()).map(str::to_owned)
}

/// A pane or a session here as a thread's target in control's chat.
fn kind_of(t: Readable) -> (ChatTargetKind, String) {
    match t {
        Readable::Pane(p) => (ChatTargetKind::Pane, p.to_string()),
        Readable::Session(s) => (ChatTargetKind::Session, s.to_string()),
    }
}

/// Who may read each pane and session, as the mux says it, as control's
/// thread reader lists: accounts only, the owner left out (control lets
/// them read everything anyway).
fn readers(lists: crate::mux::Readers, owner: &str) -> Vec<ChatReaderList> {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reader_lists_name_accounts_but_not_the_owner() {
        let lists = vec![
            (
                Readable::Pane(2),
                vec![
                    ("account:a2".into(), 0, false),
                    ("tailnet:x".into(), 0, false),
                    ("account:a1".into(), 0, false),
                    ("account:a3".into(), 55, true),
                ],
            ),
            (Readable::Session(1), vec![]),
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

    /// A burst of changes is one send after a quiet moment, the next waits
    /// out the gap (longer when the last send took several posts), and a
    /// change while one is due doesn't move it.
    #[tokio::test]
    async fn changes_are_debounced_and_sends_keep_their_gap() {
        let mut s = Sync::default();
        let t0 = Instant::now();
        s.changed();
        s.changed();
        let due = s.due.expect("due");
        assert!(due >= t0 + QUIET && due < t0 + QUIET + Duration::from_secs(1), "the first send waits only for quiet");
        s.due = None;
        s.last = Some((t0, 1));
        s.changed();
        assert_eq!(s.due, Some(t0 + GAP), "the gap after a send");
        s.changed();
        assert_eq!(s.due, Some(t0 + GAP), "a burst doesn't push it out");
        s.due = None;
        s.last = Some((t0, 3));
        s.changed();
        assert_eq!(s.due, Some(t0 + GAP * 3), "three posts last time: three gaps");
        s.due = None;
        s.last = Some((t0.checked_sub(GAP * 4).unwrap_or(t0), 1));
        s.changed();
        let due = s.due.expect("due");
        assert!(due < t0 + GAP, "long after the last send: only quiet ({:?})", due - t0);
    }

    fn list(kind: ChatTargetKind, target: &str, accounts: &[&str]) -> ChatReaderList {
        ChatReaderList {
            kind,
            target: target.into(),
            readers: accounts.iter().map(|a| ChatReader { account: (*a).into(), ..Default::default() }).collect(),
        }
    }

    /// The first send names every list; later ones only those that
    /// changed. Each ends with every pane and session still held, so a
    /// list taken away, or a target forgotten, reaches control as well.
    #[test]
    fn a_send_names_what_changed_and_ends_with_what_is_held() {
        use ChatTargetKind::{Pane, Session};
        let held = |panes: &[&str], sessions: &[&str]| {
            Some(ChatTargetsHeld {
                panes: panes.iter().map(|p| (*p).into()).collect(),
                sessions: sessions.iter().map(|s| (*s).into()).collect(),
            })
        };
        let first = vec![list(Pane, "1", &["a2"]), list(Pane, "2", &["a2"]), list(Session, "1", &["a2"])];
        let posts = posts_for(None, &first);
        assert_eq!(posts, [ChatReaders { targets: first.clone(), held: held(&["1", "2"], &["1"]), seq: None }]);

        // Pane 2 (closed, still remembered) and the session lose a2; pane
        // 1 doesn't change, so it isn't sent again.
        let mut next = first.clone();
        next[1].readers.clear();
        next[2].readers.clear();
        let posts = posts_for(Some(&first), &next);
        assert_eq!(posts, [ChatReaders { targets: next[1..].to_vec(), held: held(&["1", "2"], &["1"]), seq: None }]);

        // Pane 2 forgotten: nothing changed to name, and it's not held.
        let forgot = vec![next[0].clone(), next[2].clone()];
        let posts = posts_for(Some(&next), &forgot);
        assert_eq!(posts, [ChatReaders { targets: vec![], held: held(&["1"], &["1"]), seq: None }]);

        // Many lists: in posts of the most one takes, the last saying
        // what's held.
        let many: Vec<ChatReaderList> =
            (0..wire::MAX_READER_TARGETS + 3).map(|i| list(Pane, &i.to_string(), &[])).collect();
        let posts = posts_for(None, &many);
        assert_eq!(posts.len(), 2);
        assert_eq!((posts[0].targets.len(), posts[0].held.is_none()), (wire::MAX_READER_TARGETS, true));
        assert_eq!((posts[1].targets.len(), posts[1].held.as_ref().map(|h| h.panes.len())), (3, Some(many.len())));

        // Long lists: in posts well under control's limit on a body, the
        // whole resend still going through.
        let accounts: Vec<String> = (0..200).map(|i| format!("account-{i:04}")).collect();
        let names: Vec<&str> = accounts.iter().map(String::as_str).collect();
        let long: Vec<ChatReaderList> = (0..200).map(|i| list(Pane, &i.to_string(), &names)).collect();
        let posts = posts_for(None, &long);
        assert!(posts.len() > 1, "one post of {} bytes", json_bytes(&posts[0]));
        for p in &posts {
            assert!(json_bytes(p) <= POST_BYTES + 1024, "a post of {} bytes", json_bytes(p));
        }
        assert_eq!(posts.iter().map(|p| p.targets.len()).sum::<usize>(), long.len());
        assert!(posts.last().unwrap().held.is_some() && posts[..posts.len() - 1].iter().all(|p| p.held.is_none()));
    }

    /// A post control says it holds newer lists than: the send fails (so
    /// the next sends every list), numbered above what control holds.
    #[test]
    fn a_clock_that_went_back_numbers_above_what_control_holds() {
        let mut s = Sync::default();
        let seq = s.next_seq(1000);
        assert!(s.past(seq, Some(seq)).is_ok() && s.past(seq, None).is_ok());
        assert!(s.past(seq, Some(9000)).is_err());
        assert_eq!(s.next_seq(1001), 9001);
    }

    /// Posts are numbered from the time, and always upwards.
    #[test]
    fn posts_are_numbered_upwards() {
        let mut s = Sync::default();
        assert_eq!(s.next_seq(1000), 1000);
        assert_eq!(s.next_seq(1000), 1001);
        assert_eq!(s.next_seq(900), 1002, "a clock that went back");
        assert_eq!(s.next_seq(5000), 5000);
    }

    /// A send that fails forgets what control has, so the next sends every
    /// list; it's tried again on its own, later each time it fails, and a
    /// change brings it sooner.
    #[tokio::test]
    async fn a_failed_send_is_tried_again_with_every_list() {
        let mut s = Sync { sent: Some(vec![list(ChatTargetKind::Pane, "1", &["a2"])]), ..Default::default() };
        let t0 = Instant::now();
        s.failed();
        assert_eq!(s.sent, None, "the next send names every list");
        let due = s.due.expect("a retry");
        assert!(due >= t0 + RETRY && due < t0 + RETRY + Duration::from_secs(1));
        s.failed();
        assert_eq!(s.retry, Some(RETRY * 2));
        for _ in 0..10 {
            s.failed();
        }
        assert_eq!(s.retry, Some(RETRY_MOST));
        // A change comes before the retry: it's due after the quiet moment.
        s.changed();
        assert!(s.due.expect("due") < Instant::now() + QUIET + Duration::from_secs(1));
    }
}
