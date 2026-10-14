//! Conversations (#855): threads of their own, beside the pane and session
//! threads, kept on their home machine (`threads.rs` keeps the record and
//! the timeline). These are their rules, apart from the mux so they can be
//! tested alone: who may change one, and what a change leaves.
//!
//! - Only its owner renames or deletes it, and adds or removes members.
//! - A member may leave. The owner can't: deleting it is how it ends.
//! - A member reads from when they joined, or all of it when added with
//!   history (`Conversation::floor`). The owner reads all of it.
//! - The owner and members link panes (#856), named `L1`, `L2`… in order.
//!   A link is closed, never removed, and its name isn't used again; the
//!   owner or whoever linked one closes it (the daemon too, when the pane's
//!   machine refuses it).
//!
//! - Its lead (#858) is the daemon's to start and to keep track of
//!   ([`LeadChange`]): what it has seen, whom it owes an answer, and the
//!   links it waits on, so a restart loses none of it.
//!
//! Whom a member may be (someone who can reach this machine) is the mux's
//! call: it knows who that is.

use arugula_proto::{ConvId, ConvLead, ConvLink, ConvMember, Conversation};

pub use crate::mux::LeadChange;

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
    /// Link a pane (#856). Its `name` is given here, and `by` is who asks.
    Link(ConvLink),
    /// Close a link, by name.
    Unlink(String),
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
        links: Vec::new(),
        lead: None,
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
        Change::Link(mut l) => {
            if c.floor(by).is_none() {
                return Err((403, "only the people in a conversation link panes to it".into()));
            }
            if let Some(o) = c.links.iter().find(|o| o.closed.is_none() && o.machine == l.machine && o.pane == l.pane) {
                return Err((409, format!("that pane is linked already, as {}", o.name)));
            }
            l.name = format!("L{}", c.links.len() + 1);
            l.by = by.to_owned();
            l.closed = None;
            c.links.push(l);
        }
        Change::Unlink(name) => {
            let l = c
                .links
                .iter_mut()
                .find(|l| l.name.eq_ignore_ascii_case(&name))
                .ok_or((404, format!("no link {name}")))?;
            if !owner && l.by != by {
                return Err((403, "only the conversation's owner, or whoever linked it, closes a link".into()));
            }
            if l.closed.is_none() {
                l.closed = Some("unlinked".into());
            }
        }
    }
    Ok(())
}

/// Close link `name` for good, saying why (its pane went, or its machine
/// refused it): the daemon's, not anyone's request. False if it was closed
/// already, or there's no such link.
pub fn close_link(c: &mut Conversation, name: &str, why: &str) -> bool {
    match c.links.iter_mut().find(|l| l.name == name) {
        Some(l) if l.closed.is_none() => {
            l.closed = Some(why.to_owned());
            true
        }
        _ => false,
    }
}

/// `change` to `c`'s lead. Refused (404) when it has none, except a start.
pub fn lead(c: &mut Conversation, change: LeadChange) -> Result<(), Refusal> {
    if let LeadChange::Start { block, at, cursor } = change {
        let was = c.lead.take();
        let (asked, awaiting, woke) = was.map(|l| (l.asked, l.awaiting, l.woke)).unwrap_or_default();
        c.lead = Some(ConvLead { block, at, cursor, asked, awaiting, woke, posted: 0 });
        return Ok(());
    }
    let l = c.lead.as_mut().ok_or((404, "this conversation has no lead".to_owned()))?;
    let add = |v: &mut Vec<String>, x: String| {
        if !v.contains(&x) {
            v.push(x);
        }
    };
    match change {
        LeadChange::Start { .. } => unreachable!("handled above"),
        LeadChange::Turn { cursor } => l.cursor = l.cursor.max(cursor),
        LeadChange::Asked(who) => add(&mut l.asked, who),
        LeadChange::Posted => l.posted += 1,
        LeadChange::Replied { who, .. } => l.asked.retain(|w| !who.contains(w)),
        LeadChange::TurnDone => l.posted = 0,
        LeadChange::Await(link) => {
            l.woke.retain(|w| *w != link);
            add(&mut l.awaiting, link);
        }
        LeadChange::Settled(link) => {
            l.awaiting.retain(|w| *w != link);
            l.woke.retain(|w| *w != link);
        }
        LeadChange::LinkEnded(link) => {
            if l.awaiting.contains(&link) {
                l.awaiting.retain(|w| *w != link);
                add(&mut l.woke, link);
            }
        }
        LeadChange::Woken(links) => l.woke.retain(|w| !links.contains(w)),
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

    fn link(machine: Option<&str>, pane: arugula_proto::PaneId) -> Change {
        Change::Link(ConvLink {
            name: "ignored".into(),
            machine: machine.map(str::to_owned),
            machine_name: String::new(),
            pane,
            epoch: 1,
            title: "claude".into(),
            by: "ignored".into(),
            by_name: String::new(),
            at: 1,
            closed: Some("ignored".into()),
        })
    }

    #[test]
    fn people_in_it_link_panes_named_in_order_and_a_closed_name_is_never_used_again() {
        let mut c = conv();
        add(&mut c, "owner", SAM, false, 1).unwrap();
        apply(&mut c, "owner", link(None, 3)).unwrap();
        apply(&mut c, SAM, link(Some("dev-b"), 3)).unwrap();
        assert_eq!(
            c.links.iter().map(|l| (l.name.as_str(), l.by.as_str(), l.closed.is_none())).collect::<Vec<_>>(),
            [("L1", "owner", true), ("L2", SAM, true)]
        );
        // The same pane twice is one link; a stranger links nothing.
        assert_eq!(apply(&mut c, SAM, link(None, 3)).unwrap_err().0, 409);
        assert_eq!(apply(&mut c, "account:stranger", link(None, 4)).unwrap_err().0, 403);
        // A member closes only what they linked; the owner closes any.
        assert_eq!(apply(&mut c, SAM, Change::Unlink("L1".into())).unwrap_err().0, 403);
        apply(&mut c, SAM, Change::Unlink("l2".into())).unwrap();
        assert!(close_link(&mut c, "L1", "revoked"));
        assert!(!close_link(&mut c, "L1", "again"), "closed once");
        assert_eq!(c.links[0].closed.as_deref(), Some("revoked"));
        // Linked again, it's a new link with a new name.
        apply(&mut c, "owner", link(None, 3)).unwrap();
        assert_eq!(c.links.last().unwrap().name, "L3");
        assert_eq!(c.links.len(), 3);
    }

    #[test]
    fn a_lead_keeps_what_it_has_seen_whom_it_owes_and_what_it_waits_on() {
        let mut c = conv();
        assert_eq!(lead(&mut c, LeadChange::Posted).unwrap_err().0, 404, "no lead yet");
        lead(&mut c, LeadChange::Start { block: 7, at: 1, cursor: 0 }).unwrap();
        lead(&mut c, LeadChange::Asked(SAM.into())).unwrap();
        lead(&mut c, LeadChange::Asked(SAM.into())).unwrap();
        lead(&mut c, LeadChange::Turn { cursor: 5 }).unwrap();
        lead(&mut c, LeadChange::Turn { cursor: 3 }).unwrap();
        lead(&mut c, LeadChange::Await("L1".into())).unwrap();
        lead(&mut c, LeadChange::Await("L2".into())).unwrap();
        lead(&mut c, LeadChange::Posted).unwrap();
        let l = c.lead.clone().unwrap();
        assert_eq!((l.cursor, l.asked.as_slice(), l.awaiting.len(), l.posted), (5, [SAM.to_owned()].as_slice(), 2, 1));
        // L1 ends while it's busy: it's to be woken. L3 it never prompted.
        lead(&mut c, LeadChange::LinkEnded("L1".into())).unwrap();
        lead(&mut c, LeadChange::LinkEnded("L3".into())).unwrap();
        // It waited on L2 itself.
        lead(&mut c, LeadChange::Settled("L2".into())).unwrap();
        let l = c.lead.clone().unwrap();
        assert_eq!((l.awaiting.len(), l.woke.as_slice()), (0, ["L1".to_owned()].as_slice()));
        // Its turn ends; the next one has posted nothing yet.
        lead(&mut c, LeadChange::TurnDone).unwrap();
        lead(&mut c, LeadChange::Turn { cursor: 9 }).unwrap();
        lead(&mut c, LeadChange::Woken(vec!["L1".into()])).unwrap();
        lead(&mut c, LeadChange::Replied { who: vec![SAM.into()], msg: 10 }).unwrap();
        let l = c.lead.clone().unwrap();
        assert_eq!((l.cursor, l.posted, l.woke.len(), l.asked.len()), (9, 0, 0, 0));
        // A lost lead's replacement owes what it owed and waits on what it
        // waited on; it has seen what it was shown.
        lead(&mut c, LeadChange::Asked(RIL.into())).unwrap();
        lead(&mut c, LeadChange::Await("L1".into())).unwrap();
        lead(&mut c, LeadChange::Start { block: 12, at: 2, cursor: 11 }).unwrap();
        let l = c.lead.unwrap();
        assert_eq!((l.block, l.cursor, l.asked, l.awaiting), (12, 11, vec![RIL.to_owned()], vec!["L1".to_owned()]));
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
