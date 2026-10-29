//! Whether a hosted machine is working (M180, #1241): busy from the start
//! of an agent turn or a foreground command to its end, idle otherwise.
//!
//! A hosted daemon (`arugulad --hosted`) says so on its relay socket, as a
//! text frame beside the others (`trust`, `forge.watch`, `ops`, ...), when
//! it connects and whenever it changes. The hub keeps the latest per
//! machine while its socket is up and hands it to the platform
//! (`HubPort::activity`), which holds the box's timeout while it's busy
//! (M181). A machine whose socket is down has no activity: the platform
//! treats it as idle, so nothing holds a box forever.

use serde::{Deserialize, Deserializer, Serialize, Serializer};

/// `"activity"`: serializes as it, and parses only from it.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ActivityTag;

impl Serialize for ActivityTag {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str("activity")
    }
}

impl<'de> Deserialize<'de> for ActivityTag {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let t = String::deserialize(d)?;
        if t == "activity" { Ok(ActivityTag) } else { Err(serde::de::Error::custom("not an activity")) }
    }
}

#[cfg(feature = "schema")]
crate::const_schema!(ActivityTag = "activity");

/// `{"t": "activity", "busy": true, "since_ms": ...}`, daemon to control.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct Activity {
    pub t: ActivityTag,
    pub busy: bool,
    /// When it became busy or idle (the daemon's clock, ms).
    #[serde(default)]
    pub since_ms: u64,
}

impl Activity {
    pub fn new(busy: bool, since_ms: u64) -> Self {
        Activity { t: ActivityTag, busy, since_ms }
    }
}

/// A machine's latest [`Activity`], as the hub keeps it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct MachineActivity {
    /// The machine (its device id).
    pub machine: String,
    pub busy: bool,
    /// When it became busy or idle, as the machine said (ms).
    pub since_ms: u64,
    /// When the hub heard it (the hub's clock, ms).
    pub heard_ms: u64,
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn activity_round_trips_and_other_frames_dont_parse() {
        let v = json!({ "t": "activity", "busy": true, "since_ms": 5 });
        let a: Activity = serde_json::from_value(v.clone()).unwrap();
        assert_eq!(a, Activity::new(true, 5));
        assert_eq!(serde_json::to_value(a).unwrap(), v);
        // Lenient: `since_ms` may be missing.
        let a: Activity = serde_json::from_value(json!({ "t": "activity", "busy": false })).unwrap();
        assert!(!a.busy);
        assert!(serde_json::from_value::<Activity>(json!({ "t": "ops", "busy": true })).is_err());
        assert!(serde_json::from_str::<Activity>("trust").is_err());
    }
}
