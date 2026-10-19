//! Conversations (#855): threads of their own, beside the pane and session
//! threads, kept on their home machine (`threads.rs` keeps the record and
//! the timeline). These are their rules, apart from the mux so they can be
//! tested alone: who may change one, and what a change leaves.
//!
//! - Only its owner renames or deletes it, and adds or removes members.
//! - A member may leave. The owner can't: deleting it is how it ends.
//! - A member reads from when they joined, or all of it when added with
//!   history (`Conversation::floor`). The owner reads all of it.
//!
//! Whom a member may be (someone who can reach this machine) is the mux's
//! call: it knows who that is.

use arugula_proto::{ConvId, ConvMember, Conversation};

/// The longest title kept, in characters.
pub const MAX_TITLE: usize = 100;

/// Why a change was refused: an HTTP status and what to say.
pub type Refusal = (u16, String);

/// What to do to a conversation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Change {
    Rename(String),
    /// Add someone (their principal id and name), from now on or with
    /// history, at `at` (ms).
    Add {
        who: String,
        name: String,
        history: bool,
        at: u64,
        /// Their account's root, when being a member is how they reach
        /// this machine (#856).
        root: Option<String>,
    },
    /// Take someone out, or leave.
    Remove(String),
}

/// A title as kept: trimmed, one line, not empty, and not too long.
pub fn title(t: &str) -> Result<String, Refusal> {
    let t: String = t.split_whitespace().collect::<Vec<_>>().join(" ");
    if t.is_empty() {
        return Err((400, "a conversation needs a title".into()));
    }
    Ok(t.chars().take(MAX_TITLE).collect())
}

/// A new conversation, owned by `owner`, with a fresh id.
pub fn create(owner: &str, owner_name: &str, title_: &str, at: u64) -> Result<Conversation, Refusal> {
    let mut id = [0u8; 16];
    getrandom::fill(&mut id).map_err(|e| (500, format!("no randomness for its id: {e}")))?;
    Ok(Conversation {
        id: ConvId(id),
        title: title(title_)?,
        owner: owner.to_owned(),
        owner_name: owner_name.to_owned(),
        members: Vec::new(),
        created: at,
    })
}

/// Whether `by` may delete it: its owner alone.
pub fn may_delete(c: &Conversation, by: &str) -> Result<(), Refusal> {
    if by == c.owner { Ok(()) } else { Err((403, "only the conversation's owner deletes it".into())) }
}

/// `change` made by `by`, or why not. Nothing changes on a refusal.
pub fn apply(c: &mut Conversation, by: &str, change: Change) -> Result<(), Refusal> {
    let owner = by == c.owner;
    match change {
        Change::Rename(t) => {
            if !owner {
                return Err((403, "only the conversation's owner renames it".into()));
            }
            c.title = title(&t)?;
        }
        Change::Add { who, name, history, at, root } => {
            if !owner {
                return Err((403, "only the conversation's owner adds people to it".into()));
            }
            if who == c.owner {
                return Err((409, "that's its owner".into()));
            }
            if c.members.iter().any(|m| m.who == who) {
                return Err((409, format!("{name} is in it already")));
            }
            c.members.push(ConvMember { who, name, joined: at, history, root });
        }
        Change::Remove(who) => {
            if who == c.owner {
                return Err((400, "its owner can't leave it: delete it instead".into()));
            }
            if !owner && who != by {
                return Err((403, "only the conversation's owner takes people out of it".into()));
            }
            let before = c.members.len();
            c.members.retain(|m| m.who != who);
            if c.members.len() == before {
                return Err((404, "they aren't in it".into()));
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAM: &str = "account:sam";
    const RIL: &str = "tailnet:riley@example.com";

    fn conv() -> Conversation {
        create("owner", "Jake", "  calc\n tests ", 100).unwrap()
    }

    fn add(c: &mut Conversation, by: &str, who: &str, history: bool, at: u64) -> Result<(), Refusal> {
        apply(c, by, Change::Add { who: who.into(), name: who.into(), history, at, root: None })
    }

    #[test]
    fn a_new_one_has_a_fresh_id_its_owner_and_a_tidy_title() {
        let (a, b) = (conv(), conv());
        assert_ne!(a.id, b.id);
        assert_eq!(a.title, "calc tests");
        assert_eq!((a.owner.as_str(), a.owner_name.as_str(), a.created), ("owner", "Jake", 100));
        assert!(a.members.is_empty());
        assert_eq!(create("owner", "Jake", " \n ", 1).unwrap_err().0, 400);
        assert_eq!(title(&"x".repeat(500)).unwrap().chars().count(), MAX_TITLE);
    }

    #[test]
    fn the_record_round_trips_with_its_id_as_a_string() {
        let mut c = conv();
        add(&mut c, "owner", SAM, true, 200).unwrap();
        let v = serde_json::to_value(&c).unwrap();
        let id = v["id"].as_str().unwrap();
        assert!(id.starts_with("c-") && id.len() == 34, "{id}");
        assert_eq!(v["members"][0], serde_json::json!({ "who": SAM, "name": SAM, "joined": 200, "history": true }));
        assert_eq!(serde_json::from_value::<Conversation>(v).unwrap(), c);
        // An id that isn't one doesn't parse.
        assert!(serde_json::from_value::<ConvId>(serde_json::json!("c-12")).is_err());
        assert!(serde_json::from_value::<ConvId>(serde_json::json!(format!("x-{}", "0".repeat(32)))).is_err());
        assert!(serde_json::from_value::<ConvId>(serde_json::json!(format!("c-{}", "A".repeat(32)))).is_err());
    }

    #[test]
    fn members_read_from_when_they_joined_unless_added_with_history() {
        let mut c = conv();
        add(&mut c, "owner", SAM, false, 500).unwrap();
        add(&mut c, "owner", RIL, true, 900).unwrap();
        assert_eq!(c.floor("owner"), Some(0));
        assert_eq!(c.floor(SAM), Some(500));
        assert_eq!(c.floor(RIL), Some(0));
        assert_eq!(c.floor("account:stranger"), None);
        apply(&mut c, SAM, Change::Remove(SAM.into())).unwrap();
        assert_eq!(c.floor(SAM), None, "someone who left reads nothing");
        // Back in, from the new time.
        add(&mut c, "owner", SAM, false, 1200).unwrap();
        assert_eq!(c.floor(SAM), Some(1200));
    }

    #[test]
    fn only_the_owner_renames_deletes_adds_and_takes_out() {
        let mut c = conv();
        add(&mut c, "owner", SAM, false, 1).unwrap();
        let before = c.clone();
        assert_eq!(apply(&mut c, SAM, Change::Rename("mine now".into())).unwrap_err().0, 403);
        assert_eq!(add(&mut c, SAM, RIL, false, 2).unwrap_err().0, 403);
        add(&mut c, "owner", RIL, false, 2).unwrap();
        assert_eq!(apply(&mut c, SAM, Change::Remove(RIL.into())).unwrap_err().0, 403);
        assert_eq!(may_delete(&c, SAM).unwrap_err().0, 403);
        assert_eq!(c.members.len(), 2);
        assert_eq!(c.title, before.title);

        apply(&mut c, "owner", Change::Rename(" calc, again ".into())).unwrap();
        assert_eq!(c.title, "calc, again");
        apply(&mut c, "owner", Change::Remove(RIL.into())).unwrap();
        assert_eq!(c.members.iter().map(|m| m.who.as_str()).collect::<Vec<_>>(), [SAM]);
        may_delete(&c, "owner").unwrap();
    }

    #[test]
    fn a_member_may_leave_but_the_owner_may_not() {
        let mut c = conv();
        add(&mut c, "owner", SAM, false, 1).unwrap();
        apply(&mut c, SAM, Change::Remove(SAM.into())).unwrap();
        assert!(c.members.is_empty());
        assert_eq!(apply(&mut c, SAM, Change::Remove(SAM.into())).unwrap_err().0, 404);
        assert_eq!(apply(&mut c, "owner", Change::Remove("owner".into())).unwrap_err().0, 400);
    }

    #[test]
    fn nobody_is_added_twice_and_the_owner_is_no_member() {
        let mut c = conv();
        add(&mut c, "owner", SAM, false, 1).unwrap();
        assert_eq!(add(&mut c, "owner", SAM, true, 2).unwrap_err().0, 409);
        assert_eq!(add(&mut c, "owner", "owner", false, 2).unwrap_err().0, 409);
        assert_eq!(c.members.len(), 1);
        assert!(!c.members[0].history, "a refused change leaves it as it was");
    }
}
