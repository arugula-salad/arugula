//! Telling control who reads each pane's and session's thread (M124,
//! decision 2 of S41).
//!
//! Chat lives in control (M122); the daemon keeps none of it (M125). While
//! control has chat on for this machine's account or team, it sends who
//! may read each pane's and session's thread, as this daemon decides it,
//! whenever that changes (`POST /api/daemon/chat/readers`): after each
//! refresh with control, and as soon as who may read what changes here
//! (a share given or taken away, a pane made private, a roster change;
//! native #684), debounced so a burst of changes is one send.
//!
//! Chat's on/off is control's setting, not a Labs flag here (native #203):
//! it comes with the flags control sends ([`arugula_proto::flags::control_says`]),
//! and this machine's `flags.json` has no say in it. The threads an older
//! daemon kept itself (M61) in `<state>/threads/` are no longer brought
//! into control (the import of M124 is gone, native #203): nothing reads
//! them, and they stay on disk as they are.

use std::{sync::Arc, time::Duration};

use arugula_control_wire::chat::{
    self as wire, CHAT_READERS, ChatReader, ChatReaderList, ChatReaders, ChatReadersStored, ChatTargetKind,
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
    /// The reader lists control has, as last sent.
    sent: Option<Vec<ChatReaderList>>,
    /// What was last logged, so a refusal is said once, not every minute.
    said: Option<String>,
    /// When the next send is due, once something changed.
    due: Option<Instant>,
    /// When the last send went, and how many posts it took.
    last: Option<(Instant, u32)>,
}

impl Sync {
    /// Something changed (a refresh, or who may read what): a send is due
    /// after [`QUIET`], and no sooner than [`GAP`] after the last one. A
    /// change while one is due doesn't move it.
    fn changed(&mut self) {
        if self.due.is_some() {
            return;
        }
        let now = Instant::now();
        let mut at = now + QUIET;
        if let Some((last, posts)) = self.last {
            at = at.max(last + GAP * posts.max(1));
        }
        self.due = Some(at);
    }

    /// A send that fails, or is refused, isn't tried again on its own: the
    /// next change or refresh sends the whole list, which carries it.
    async fn run(&mut self, control: &Control, app: &App) {
        self.due = None;
        let Some(owner) = control.account() else { return };
        if arugula_proto::flags::control_says(control.state_dir(), CHAT) != Some(true) {
            self.sent = None;
            return;
        }
        let readers = readers(app.mux.api(Api::Readers).await.unwrap_or_default(), &owner);
        if self.sent.as_ref() == Some(&readers) {
            return;
        }
        match self.send(control, &readers).await {
            Ok(()) => self.said = None,
            Err(e) => {
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

    async fn send(&mut self, control: &Control, readers: &[ChatReaderList]) -> anyhow::Result<()> {
        let mut posts = 0;
        self.last = Some((Instant::now(), 1));
        for part in readers.chunks(wire::MAX_READER_TARGETS) {
            posts += 1;
            self.last = Some((Instant::now(), posts));
            let _: ChatReadersStored =
                control.post_signed(CHAT_READERS, &ChatReaders { targets: part.to_vec() }).await?;
        }
        self.sent = Some(readers.to_vec());
        Ok(())
    }
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
}
