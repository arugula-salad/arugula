//! Whether hosted machines are working (M180, #1241): the latest
//! [`Activity`] each daemon said on its relay socket, kept while that
//! socket is up, for the platform to read (`HubPort::activity`, M181).

use std::{collections::HashMap, sync::Mutex};

use arugula_control_wire::activity::{Activity, MachineActivity};
use arugula_e2e::now_ms;
use tracing::debug;

/// Each connected daemon's latest activity, with the relay socket's
/// generation it came on.
#[derive(Default)]
pub struct Activities(Mutex<HashMap<String, (u64, MachineActivity)>>);

impl Activities {
    /// A daemon's text, if it's an activity: kept, and true.
    pub fn heard(&self, daemon: &str, generation: u64, text: &str) -> bool {
        let Ok(a) = serde_json::from_str::<Activity>(text) else { return false };
        debug!(%daemon, busy = a.busy, "machine activity");
        let m = MachineActivity { machine: daemon.to_owned(), busy: a.busy, since_ms: a.since_ms, heard_ms: now_ms() };
        self.0.lock().unwrap().insert(daemon.to_owned(), (generation, m));
        true
    }

    /// The daemon's socket closed: what it said goes with it, unless a newer
    /// socket of the same daemon said something since.
    pub fn drop_daemon(&self, daemon: &str, generation: u64) {
        let mut map = self.0.lock().unwrap();
        if map.get(daemon).is_some_and(|(g, _)| *g == generation) {
            map.remove(daemon);
        }
    }

    /// The activity of these machines, in the order asked: those that
    /// aren't connected, or never said, are left out.
    pub fn of(&self, machines: &[String]) -> Vec<MachineActivity> {
        let map = self.0.lock().unwrap();
        machines.iter().filter_map(|m| map.get(m).map(|(_, a)| a.clone())).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kept_per_socket_and_dropped_with_it() {
        let a = Activities::default();
        assert!(!a.heard("d1", 1, "trust"));
        assert!(!a.heard("d1", 1, r#"{"t":"ops","ops":[]}"#));
        assert!(a.heard("d1", 1, r#"{"t":"activity","busy":true,"since_ms":7}"#));
        assert!(a.heard("d2", 2, r#"{"t":"activity","busy":false,"since_ms":8}"#));
        let got = a.of(&["d2".into(), "d1".into(), "d3".into()]);
        assert_eq!(got.iter().map(|m| (m.machine.as_str(), m.busy)).collect::<Vec<_>>(), [("d2", false), ("d1", true)]);
        assert_eq!(got[1].since_ms, 7);
        // d1 reconnected (generation 3) before its old socket's end was seen.
        assert!(a.heard("d1", 3, r#"{"t":"activity","busy":false,"since_ms":9}"#));
        a.drop_daemon("d1", 1);
        assert_eq!(a.of(&["d1".into()]).len(), 1);
        a.drop_daemon("d1", 3);
        assert!(a.of(&["d1".into()]).is_empty());
    }
}
