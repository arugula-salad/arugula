//! Telling control who reads each pane's and session's thread (M124,
//! decision 2 of S41).
//!
//! Chat lives in control (M122); the daemon keeps none of it (M125). After
//! each refresh with control, while control has chat on for this machine's
//! account or team, it sends who may read each pane's and session's
//! thread, as this daemon decides it, whenever that changes
//! (`POST /api/daemon/chat/readers`).
//!
//! Chat's on/off is control's setting, not a Labs flag here (native #203):
//! it comes with the flags control sends ([`arugula_proto::flags::control_says`]),
//! and this machine's `flags.json` has no say in it. The threads an older
//! daemon kept itself (M61) in `<state>/threads/` are no longer brought
//! into control (the import of M124 is gone, native #203): nothing reads
//! them, and they stay on disk as they are.

use std::sync::Arc;

use arugula_control_wire::chat::{
    self as wire, CHAT_READERS, ChatReader, ChatReaderList, ChatReaders, ChatReadersStored, ChatTargetKind,
};
use tracing::{info, warn};

use crate::{
    control::{Answered, Control},
    mux::{Api, ChatTarget},
    server::App,
};

/// Control's chat setting, as it sends it beside the Labs flags (the
/// platform's `chat::FLAG`): the account's or the team's on/off.
const CHAT: &str = "chat";

/// Run beside the control loop: after each refresh, keep control's reader
/// lists current.
pub fn start(control: Arc<Control>, app: Arc<App>) {
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
        if arugula_proto::flags::control_says(control.state_dir(), CHAT) != Some(true) {
            self.sent = None;
            return;
        }
        let readers = readers(app.mux.api(Api::ChatReaders).await.unwrap_or_default(), &owner);
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
        for part in readers.chunks(wire::MAX_READER_TARGETS) {
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

#[cfg(test)]
mod tests {
    use super::*;

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
}
