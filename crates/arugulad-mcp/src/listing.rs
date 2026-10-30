//! What each open MCP session was last told it has, and telling it when
//! that changes (#370). The server declares `tools.listChanged`; a Labs
//! flag turned on after a session started (from control's refresh, or the
//! `labs` file) would otherwise need the client restarted to show its
//! tools.
//!
//! A ticker compares, for each open session, the tools a new one would
//! list with what the session was last given or told, by name (and kind),
//! and sends `notifications/tools/list_changed` only when they differ: a
//! refresh that reads the same flags sends nothing. What the client last
//! *listed* is kept apart ([`Listed`]), for `capabilities` to say which
//! tools it hasn't got yet.

use std::{
    path::PathBuf,
    sync::{Arc, Mutex, Once, Weak},
    time::Duration,
};

use arugula_proto::flags::On;
use rmcp::{RoleServer, service::Peer};

use super::tools;
use crate::mcp::{Scope, tools::Listed};

/// How often the open sessions are compared: flags change on control's
/// refresh, every few seconds.
const TICK: Duration = Duration::from_secs(2);

static OPEN: Mutex<Vec<Weak<Session>>> = Mutex::new(Vec::new());
static TICKER: Once = Once::new();

/// One MCP session of the HTTP server (or a relayed pipe).
pub struct Session {
    /// What its client last listed.
    pub listed: Arc<Listed>,
    state_dir: PathBuf,
    /// Its client's peer and scope, known from its first `tools/list`.
    client: Mutex<Option<(Peer<RoleServer>, Scope)>>,
    /// What the client is up to date with: the set it last listed or was
    /// last told of. `None` until it lists: a session that never listed has
    /// nothing to be told about.
    current: Mutex<Option<Vec<String>>>,
}

impl Session {
    /// A session, watched until it is dropped.
    pub fn open(state_dir: PathBuf) -> Arc<Self> {
        let s =
            Arc::new(Self { listed: Arc::default(), state_dir, client: Mutex::new(None), current: Mutex::new(None) });
        {
            let mut open = OPEN.lock().unwrap();
            open.retain(|w| w.strong_count() > 0);
            open.push(Arc::downgrade(&s));
        }
        TICKER.call_once(|| {
            crate::task::spawn(async {
                loop {
                    tokio::time::sleep(TICK).await;
                    sweep().await;
                }
            });
        });
        s
    }

    /// The client listed `names` as `scope` (from `peer`).
    pub fn listing(&self, peer: Peer<RoleServer>, scope: Scope, names: Vec<String>) {
        *self.client.lock().unwrap() = Some((peer, scope));
        *self.current.lock().unwrap() = Some(names.clone());
        self.listed.listed(names);
    }

    /// Whether the client should be told now that a new session would list
    /// `now`; if so, it counts as told.
    fn due(&self, now: Vec<String>) -> bool {
        let mut current = self.current.lock().unwrap();
        match current.as_ref() {
            Some(have) if *have != now => {
                *current = Some(now);
                true
            }
            _ => false,
        }
    }

    /// Tells the client if what it would list with the flags in `on` is not
    /// what it has.
    async fn check(&self, on: On) {
        let Some((peer, scope)) = self.client.lock().unwrap().clone() else { return };
        if self.due(tools::listed_names(scope, on)) {
            self.listed.tell();
            let _ = peer.notify_tool_list_changed().await;
        }
    }
}

/// Every open session, against the flags on now.
async fn sweep() {
    let open: Vec<Arc<Session>> = {
        let mut open = OPEN.lock().unwrap();
        open.retain(|w| w.strong_count() > 0);
        open.iter().filter_map(Weak::upgrade).collect()
    };
    for s in open {
        let on = crate::lab_flags::flags(&s.state_dir);
        s.check(on).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn session(current: Option<&[&str]>) -> Session {
        Session {
            listed: Arc::default(),
            state_dir: PathBuf::new(),
            client: Mutex::new(None),
            current: Mutex::new(current.map(|c| c.iter().map(|n| (*n).to_owned()).collect())),
        }
    }

    fn names(n: &[&str]) -> Vec<String> {
        n.iter().map(|n| (*n).to_owned()).collect()
    }

    /// A session is told when what it would list differs from what it has,
    /// once: the same flags read again send nothing.
    #[test]
    fn a_session_is_told_once_per_change_not_per_refresh() {
        let s = session(Some(&["run", "show"]));
        assert!(!s.due(names(&["run", "show"])), "the same set");
        assert!(s.due(names(&["board_read", "run", "show"])), "a tool came");
        assert!(!s.due(names(&["board_read", "run", "show"])), "the same flags, read again");
        assert!(!s.due(names(&["board_read", "run", "show"])), "and again");
        assert!(s.due(names(&["run", "show"])), "and went");
    }

    /// A session whose client never listed has nothing to be told.
    #[test]
    fn a_session_that_never_listed_is_not_told() {
        assert!(!session(None).due(names(&["board_read"])));
    }
}
