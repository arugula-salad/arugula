//! What an agent's write to a tracker may do (Arugula #423): the connection
//! says, for each kind of write, whether it goes at once (`allow`) or waits
//! for a person (`approve`), and what the footer on what is sent says.
//!
//! - **Five kinds:** a create, a comment, an edit (title or body), a state
//!   change (a close, a reopen, or an edit naming a state) and a field change
//!   (an edit naming a module, parent, type or labels). A create is only a
//!   create, whatever fields it carries. An edit can be several kinds; it
//!   goes at once only if every one of them allows.
//! - **Stored per kind** on the connection (`write_policy`, JSON). The
//!   presets are only this file's constant: a policy that matches none is
//!   "Custom" to a client.
//! - **The footer** is [`Attribution`], applied when a write is sent.

use arugula_control_wire::projects::{TrackerWrite, TrackerWritePolicy};

/// The kinds, in the order a refusal names them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Create,
    Comment,
    Edit,
    State,
    Fields,
}

impl Kind {
    /// Plain plural words: "comments".
    fn words(self) -> &'static str {
        match self {
            Kind::Create => "creates",
            Kind::Comment => "comments",
            Kind::Edit => "edits",
            Kind::State => "state changes",
            Kind::Fields => "field changes",
        }
    }

    fn of(self, p: &TrackerWritePolicy) -> &str {
        match self {
            Kind::Create => &p.create,
            Kind::Comment => &p.comment,
            Kind::Edit => &p.edit,
            Kind::State => &p.state,
            Kind::Fields => &p.fields,
        }
    }
}

/// The three presets, by the names a client shows, with the policy of each:
/// create, comment, edit, state, fields. The web page mirrors this table.
pub const PRESETS: [(&str, [&str; 5]); 3] = [
    ("Approve everything", ["approve", "approve", "approve", "approve", "approve"]),
    ("Allow comments and edits, approve creates and closes", ["approve", "allow", "allow", "approve", "allow"]),
    ("Allow everything", ["allow", "allow", "allow", "allow", "allow"]),
];

/// A connection made now gets this one.
pub const DEFAULT_PRESET: usize = 1;

pub fn preset(r: &[&str; 5]) -> TrackerWritePolicy {
    TrackerWritePolicy {
        create: r[0].into(),
        comment: r[1].into(),
        edit: r[2].into(),
        state: r[3].into(),
        fields: r[4].into(),
    }
}

/// Every kind `what` (`allow` or `approve`).
pub fn all(what: &str) -> TrackerWritePolicy {
    preset(&[what; 5])
}

/// A stored policy; one that can't be read approves everything.
pub fn parse(json: &str) -> TrackerWritePolicy {
    serde_json::from_str(json).ok().filter(valid).unwrap_or_else(|| all("approve"))
}

/// Each kind is `allow` or `approve`.
pub fn valid(p: &TrackerWritePolicy) -> bool {
    [&p.create, &p.comment, &p.edit, &p.state, &p.fields].iter().all(|v| matches!(v.as_str(), "allow" | "approve"))
}

/// `allow` if every kind allows, else `approve`: what a client that predates
/// the policy reads.
pub fn summary(p: &TrackerWritePolicy) -> &'static str {
    if p == &all("allow") { "allow" } else { "approve" }
}

/// The kinds `w` is.
fn kinds(w: &TrackerWrite) -> Vec<Kind> {
    match w {
        TrackerWrite::Create { .. } => vec![Kind::Create],
        TrackerWrite::Comment { .. } => vec![Kind::Comment],
        TrackerWrite::Close { .. } | TrackerWrite::Reopen { .. } => vec![Kind::State],
        TrackerWrite::Edit { title, body, state, module, parent, item_type, labels, labels_remove, .. } => {
            let mut k = vec![];
            if title.is_some() || body.is_some() {
                k.push(Kind::Edit);
            }
            if state.is_some() {
                k.push(Kind::State);
            }
            if module.is_some()
                || parent.is_some()
                || item_type.is_some()
                || !labels.is_empty()
                || !labels_remove.is_empty()
            {
                k.push(Kind::Fields);
            }
            k
        }
    }
}

/// What the policy says of a write.
#[derive(Debug, PartialEq, Eq)]
pub enum Verdict {
    /// It goes at once; the text names the kinds: "comments are allowed".
    Allowed(String),
    /// It waits; the text names the first kind that needs approval:
    /// "state changes need approval".
    Waits(String),
}

pub fn judge(p: &TrackerWritePolicy, w: &TrackerWrite) -> Verdict {
    let kinds = kinds(w);
    if let Some(k) = kinds.iter().find(|k| k.of(p) != "allow") {
        return Verdict::Waits(format!("{} need approval", k.words()));
    }
    let words: Vec<&str> = kinds.iter().map(|k| k.words()).collect();
    Verdict::Allowed(format!("{} are allowed", words.join(" and ")))
}

/// What the footer on a sent body or comment says.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Attribution {
    /// `— via Arugula, by Ada's agent on geek`.
    Full,
    /// `— via Arugula`.
    Short,
    /// Nothing is added.
    None,
}

impl Attribution {
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "full" => Some(Self::Full),
            "short" => Some(Self::Short),
            "none" => Some(Self::None),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_stored_policy_that_cant_be_read_approves_everything() {
        assert_eq!(parse("not json"), all("approve"));
        assert_eq!(
            parse(r#"{"create":"allow","comment":"sometimes","edit":"allow","state":"allow","fields":"allow"}"#),
            all("approve")
        );
        assert_eq!(parse(&serde_json::to_string(&all("allow")).unwrap()), all("allow"));
    }

    #[test]
    fn a_create_is_only_a_create_and_an_edit_is_each_kind_it_names() {
        let create: TrackerWrite =
            serde_json::from_str(r#"{"kind":"create","title":"t","labels":["a"],"module":"m","state":"s"}"#).unwrap();
        assert_eq!(kinds(&create), [Kind::Create]);
        let edit: TrackerWrite =
            serde_json::from_str(r#"{"kind":"edit","item":"fi_1","body":"b","state":"s","parent":"A-1"}"#).unwrap();
        assert_eq!(kinds(&edit), [Kind::Edit, Kind::State, Kind::Fields]);
        let remove: TrackerWrite =
            serde_json::from_str(r#"{"kind":"edit","item":"fi_1","labels_remove":["x"]}"#).unwrap();
        assert_eq!(kinds(&remove), [Kind::Fields]);
    }
}
