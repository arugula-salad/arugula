//! Who's in a thread and what each agent is doing (#645), for the strip at
//! its top. No new object: the roster comes from what control already has
//! (agents' posts, and the mentions told in the thread), and a hosted
//! session's state from its session. A personal agent's live state is asked
//! of its machine's daemon on each read, and control keeps none of it
//! (aru-051's line: what a terminal is doing stays on the machine, but for
//! what the owner's `agent_sessions` shows).

use super::*;
use crate::db::{
    chat::Message,
    chat_mention_status::MentionStatus,
    sessions::{Session, State as SessionState},
};
use arugula_control_wire::{
    chat::{ChatThreadAgent, ChatThreadAgentState, ChatThreadAgents},
    forge::{AgentSessionsAsk, MachineSession, MachineSessionState, session_slug},
    sessions::{CARD_VIA, SAID_VIA},
};
use std::collections::HashMap;

/// The most of a thread's newest replies the roster reads.
const ROSTER_REPLIES: u32 = 500;

/// One agent as the thread alone tells it, before anyone is asked.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(super) struct Seen {
    pub machine: String,
    pub machine_name: String,
    pub owner: String,
    /// As its machine called it; empty if a machine was told and no session
    /// has spoken.
    pub session: String,
    pub first: i64,
    pub read_seq: Option<i64>,
    pub waiting: u32,
}

/// Whether a message's status means the session has taken it in.
fn taken(state: &str) -> bool {
    matches!(state, "working" | "read" | "answered")
}

/// Whether it means a session has it, or will, and hasn't taken it in.
fn waits(state: &str) -> bool {
    matches!(state, "sending" | "delivered" | "fell_back" | "waiting" | "queued")
}

/// The personal agents in a thread, first in it first. A mention whose
/// session isn't known counts for the machine's one agent in the thread, or
/// else as an agent of no name: someone was told, and nothing has spoken yet.
pub(super) fn roster(messages: &[Message], statuses: &[MentionStatus]) -> Vec<Seen> {
    let mut out: Vec<Seen> = vec![];
    for m in messages {
        let Some(machine) = &m.agent else { continue };
        let session = m.agent_session.clone().unwrap_or_default();
        let key = session_slug(&session);
        let at = out.iter().position(|s| s.machine == *machine && session_slug(&s.session) == key);
        let s = match at {
            Some(i) => &mut out[i],
            None => {
                out.push(Seen {
                    machine: machine.clone(),
                    owner: m.author.clone(),
                    session,
                    first: m.seq,
                    ..Seen::default()
                });
                out.last_mut().unwrap()
            }
        };
        s.read_seq = s.read_seq.max(Some(m.seq));
    }
    for t in statuses {
        if t.state == "chain" {
            continue;
        }
        let key = t.session.as_deref().map(session_slug).unwrap_or_default();
        let mine = |s: &Seen| s.machine == t.machine;
        let at = if key.is_empty() {
            // No session named: the machine's only agent here, if it has one.
            let mut of = out.iter().enumerate().filter(|(_, s)| mine(s));
            match (of.next(), of.next()) {
                (Some((i, _)), None) => Some(i),
                _ => out.iter().position(|s| mine(s) && s.session.is_empty()),
            }
        } else {
            out.iter()
                .position(|s| mine(s) && session_slug(&s.session) == key)
                .or_else(|| out.iter().position(|s| mine(s) && s.session.is_empty()))
        };
        let s = match at {
            Some(i) => &mut out[i],
            None => {
                out.push(Seen {
                    machine: t.machine.clone(),
                    session: t.session.clone().unwrap_or_default(),
                    first: t.seq,
                    ..Seen::default()
                });
                out.last_mut().unwrap()
            }
        };
        if s.session.is_empty()
            && let Some(name) = &t.session
        {
            s.session = name.clone();
        }
        if s.machine_name.is_empty() {
            s.machine_name = t.machine_name.clone();
        }
        if taken(&t.state) {
            s.read_seq = s.read_seq.max(Some(t.seq));
        } else if waits(&t.state) {
            s.waiting += 1;
        }
    }
    out.sort_by_key(|s| s.first);
    out
}

/// What a personal agent is doing, from what its machine answered: `None` if
/// it gave no word of this session. Only a machine not on the relay is offline.
fn state_of(answer: Option<&MachineSession>, offline: bool) -> ChatThreadAgentState {
    if offline {
        return ChatThreadAgentState::Offline;
    }
    match answer.and_then(|a| a.state) {
        Some(MachineSessionState::Working) => ChatThreadAgentState::Working,
        Some(MachineSessionState::Idle) => ChatThreadAgentState::Idle,
        Some(MachineSessionState::NeedsInput) => ChatThreadAgentState::NeedsYou,
        None => ChatThreadAgentState::Unknown,
    }
}

/// A hosted session's state, as the strip says it.
fn hosted_state(s: &Session) -> ChatThreadAgentState {
    match s.state {
        SessionState::Unresponsive => ChatThreadAgentState::Offline,
        _ => match s.seen_as() {
            SessionState::Queued | SessionState::Working => ChatThreadAgentState::Working,
            SessionState::NeedsInput => ChatThreadAgentState::NeedsYou,
            _ => ChatThreadAgentState::Idle,
        },
    }
}

/// The hosted sessions a thread's messages name, oldest first.
fn hosted_ids(messages: &[Message]) -> Vec<(String, i64)> {
    let mut out: Vec<(String, i64)> = vec![];
    for m in messages {
        let Some(via) = &m.via else { continue };
        let Some(id) = via.strip_prefix(CARD_VIA).or_else(|| via.strip_prefix(SAID_VIA)) else { continue };
        if !out.iter().any(|(s, _)| s == id) {
            out.push((id.to_owned(), m.seq));
        }
    }
    out
}

/// A thread's messages, and its personal agents as they tell it.
pub(super) fn in_thread(
    app: &Platform,
    room: &Room,
    root: i64,
    first: Message,
) -> anyhow::Result<(Vec<Message>, Vec<Seen>)> {
    let (replies, _) = app.db.chat_thread_messages(&room.id, root, Page::Newest, ROSTER_REPLIES)?;
    let mut messages = vec![first];
    messages.extend(replies);
    let statuses = app.db.chat_mention_statuses_of_thread(&room.id, root)?;
    let seen = roster(&messages, &statuses);
    Ok((messages, seen))
}

/// `GET /api/chat/rooms/{id}/threads/{seq}/agents`.
pub async fn agents(
    State(app): State<Arc<Platform>>,
    s: Person,
    Path((id, root)): Path<(String, i64)>,
) -> J<ChatThreadAgents> {
    gate(&app, &s.account).await?;
    let (room, from, _) = room_for(&app, &s.account, &id).await?;
    let first = threads::parent_in(&app, &room, from, root)?;
    Ok(Json(of_thread(&app, &room, root, first).await?))
}

/// The agents in the thread of `root`, for whoever may read `room`.
pub(super) async fn of_thread(
    app: &Arc<Platform>,
    room: &Room,
    root: i64,
    first: Message,
) -> Result<ChatThreadAgents, ApiError> {
    let (messages, seen) = in_thread(app, room, root, first)?;

    // Each machine once, for the sessions the thread knows on it.
    let mut wanted: HashMap<&str, Vec<String>> = HashMap::new();
    for a in &seen {
        wanted.entry(&a.machine).or_default().push(session_slug(&a.session));
    }
    let mut asks = tokio::task::JoinSet::new();
    for (machine, names) in &wanted {
        let ask = AgentSessionsAsk {
            room: room.id.clone(),
            from_session: None,
            only: names.iter().filter(|n| !n.is_empty()).cloned().collect(),
        };
        let (app, machine) = (app.clone(), machine.to_string());
        asks.spawn(async move { (machine.clone(), ask_sessions(&app, &machine, ask).await) });
    }
    let live: HashMap<String, SessionsAnswer> = asks.join_all().await.into_iter().collect();

    let machines: Vec<String> = wanted.keys().map(|m| m.to_string()).collect();
    let daemons = crate::grants::agent_names(app, &machines).await?;
    let hosted: Vec<(Session, i64)> = hosted_ids(&messages)
        .into_iter()
        .filter_map(|(id, seq)| app.db.session(&id).ok().flatten().map(|s| (s, seq)))
        .collect();
    let mut owners: Vec<String> = seen.iter().map(|a| a.owner.clone()).collect();
    owners.extend(daemons.values().map(|d| d.account.clone()));
    owners.extend(hosted.iter().map(|(s, _)| s.asker.clone()));
    owners.retain(|o| !o.is_empty());
    owners.sort();
    owners.dedup();
    let names = app.cache.accounts(&*app.hub, &owners).await?;
    let name_of = |a: &str| names.get(a).map(|p| p.name.clone()).unwrap_or_default();

    let mut out: Vec<(i64, ChatThreadAgent)> = vec![];
    for (h, seq) in &hosted {
        let short = h.id.strip_prefix("ses_").unwrap_or(&h.id);
        out.push((
            *seq,
            ChatThreadAgent {
                name: format!("ses_{}", short.chars().take(4).collect::<String>()),
                id: h.id.clone(),
                machine_name: h.workspace.clone(),
                owner: h.asker.clone(),
                owner_name: name_of(&h.asker),
                hosted: true,
                state: hosted_state(h),
                working_on: None,
                tool: None,
                read_seq: None,
                waiting: 0,
                lead: false,
            },
        ));
    }
    for a in seen {
        let answer = live.get(&a.machine);
        let key = session_slug(&a.session);
        let one = match answer {
            Some(SessionsAnswer::Said { sessions, .. }) if !key.is_empty() => sessions.iter().find(|m| m.name == key),
            _ => None,
        };
        let d = daemons.get(&a.machine);
        let owner = if a.owner.is_empty() { d.map(|d| d.account.clone()).unwrap_or_default() } else { a.owner };
        let machine_name = match a.machine_name.is_empty() {
            true => d.map(|d| d.name.clone()).unwrap_or_default(),
            false => a.machine_name,
        };
        out.push((
            a.first,
            ChatThreadAgent {
                name: a.session,
                id: a.machine.clone(),
                machine_name,
                owner_name: name_of(&owner),
                owner,
                hosted: false,
                state: state_of(one, answer == Some(&SessionsAnswer::Offline)),
                working_on: one.and_then(|m| m.working_on.clone()),
                tool: one.and_then(|m| m.tool.clone()),
                read_seq: a.read_seq,
                waiting: a.waiting,
                lead: false,
            },
        ));
    }
    out.sort_by_key(|(seq, _)| *seq);
    let lead = out.iter().position(|(_, a)| a.hosted).or(if out.is_empty() { None } else { Some(0) });
    if let Some(i) = lead {
        out[i].1.lead = true;
    }
    Ok(ChatThreadAgents { agents: out.into_iter().map(|(_, a)| a).collect() })
}

/// The other agents in a thread, as people read them (#649): for telling
/// `machine`'s `session` of a post there. What's in the thread alone, with no
/// machine asked, since a post mustn't wait on one.
pub(super) fn others_in(seen: &[Seen], agents: &[ChatRoomAgent], machine: &str, session: Option<&str>) -> Vec<String> {
    let me = session.map(session_slug);
    seen.iter()
        .filter(|s| !s.session.is_empty())
        .filter(|s| s.machine != machine || me.as_ref().is_some_and(|m| *m != session_slug(&s.session)))
        .map(|s| match agents.iter().find(|a| a.machine == s.machine) {
            Some(a) => format!("{} ({}'s, {})", s.session, a.owner_name, a.machine_name),
            None => s.session.clone(),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn agent(seq: i64, machine: &str, session: &str) -> Message {
        Message {
            seq,
            agent: Some(machine.to_owned()),
            agent_session: Some(session.to_owned()),
            author: format!("owner-of-{machine}"),
            ..Message::default()
        }
    }

    fn told(seq: i64, machine: &str, session: Option<&str>, state: &str) -> MentionStatus {
        MentionStatus {
            room: "ch_x".into(),
            seq,
            machine: machine.into(),
            machine_name: machine.to_uppercase(),
            root: 1,
            state: state.into(),
            session: session.map(str::to_owned),
            asked: None,
            names: vec![],
            reply: None,
            at: 0,
        }
    }

    /// Each session is one agent, first in the thread first, however its name
    /// is written; a person's message isn't one.
    #[test]
    fn the_roster_is_each_session_that_spoke_or_was_told() {
        let person = Message { seq: 1, author: "ada".into(), ..Message::default() };
        let messages = [
            person,
            agent(2, "geek", "UX Work"),
            agent(3, "mini", "wake"),
            agent(4, "geek", "ux-work"),
            agent(5, "geek", "Other"),
        ];
        let got = roster(&messages, &[]);
        let who: Vec<_> = got.iter().map(|s| (s.machine.as_str(), s.session.as_str(), s.read_seq)).collect();
        assert_eq!(who, [("geek", "UX Work", Some(4)), ("mini", "wake", Some(3)), ("geek", "Other", Some(5))]);
    }

    /// Taken in moves how far it has read; told and not taken in counts as
    /// waiting; a chain that was never told counts for nothing.
    #[test]
    fn a_status_says_how_far_it_read_and_what_waits() {
        let statuses = [
            told(2, "geek", Some("ux"), "answered"),
            told(5, "geek", Some("ux"), "working"),
            told(6, "geek", Some("ux"), "delivered"),
            told(7, "geek", Some("ux"), "queued"),
            told(8, "geek", Some("ux"), "chain"),
        ];
        let got = roster(&[agent(3, "geek", "ux")], &statuses);
        assert_eq!(got.len(), 1);
        assert_eq!((got[0].read_seq, got[0].waiting), (Some(5), 2));
        assert_eq!(got[0].machine_name, "GEEK");
    }

    /// A machine told with no session yet is an agent of no name, and takes
    /// the session's name once a status has one. With one agent on the machine,
    /// a status of no session is that agent's; with two, it can't say whose.
    #[test]
    fn a_status_of_no_session_finds_its_agent_or_stands_alone() {
        let alone = roster(&[], &[told(2, "geek", None, "waiting")]);
        assert_eq!((alone[0].session.as_str(), alone[0].waiting, alone[0].first), ("", 1, 2));
        let named = roster(&[], &[told(2, "geek", None, "waiting"), told(3, "geek", Some("ux"), "working")]);
        assert_eq!(named.len(), 1);
        assert_eq!((named[0].session.as_str(), named[0].read_seq), ("ux", Some(3)));
        let one = roster(&[agent(2, "geek", "ux")], &[told(3, "geek", None, "delivered")]);
        assert_eq!((one.len(), one[0].waiting), (1, 1));
        let two = roster(&[agent(2, "geek", "a"), agent(3, "geek", "b")], &[told(4, "geek", None, "delivered")]);
        assert_eq!(two.len(), 3, "{two:?}");
    }

    /// The others are everyone but the session being told; with no session
    /// named, everyone on other machines.
    #[test]
    fn the_others_in_a_thread_leave_out_who_is_being_told() {
        let seen = roster(&[agent(2, "geek", "UX Work"), agent(3, "geek", "other"), agent(4, "mini", "wake")], &[]);
        let room = |machine: &str, owner: &str, name: &str| ChatRoomAgent {
            owner: owner.to_lowercase(),
            owner_name: owner.into(),
            machine: machine.into(),
            machine_name: name.into(),
            powers: vec![],
            mentionable: true,
            sessions: vec![],
        };
        let rooms = [room("geek", "Jake", "geek"), room("mini", "Aaron", "Mac mini")];
        assert_eq!(
            others_in(&seen, &rooms, "geek", Some("ux-work")),
            ["other (Jake's, geek)", "wake (Aaron's, Mac mini)"]
        );
        assert_eq!(others_in(&seen, &rooms, "mini", Some("wake")), ["UX Work (Jake's, geek)", "other (Jake's, geek)"]);
        assert_eq!(others_in(&seen, &rooms, "geek", None), ["wake (Aaron's, Mac mini)"]);
    }

    #[test]
    fn what_a_machine_answers_is_the_state() {
        let with = |state| MachineSession {
            name: "ux".into(),
            tab: None,
            active_ms_ago: None,
            working_on: None,
            state,
            tool: None,
        };
        use ChatThreadAgentState as S;
        assert_eq!(state_of(None, true), S::Offline);
        assert_eq!(state_of(None, false), S::Unknown);
        assert_eq!(state_of(Some(&with(None)), false), S::Unknown, "an older daemon");
        assert_eq!(state_of(Some(&with(Some(MachineSessionState::Working))), false), S::Working);
        assert_eq!(state_of(Some(&with(Some(MachineSessionState::NeedsInput))), false), S::NeedsYou);
    }

    #[test]
    fn a_threads_hosted_sessions_come_from_its_messages() {
        let via = |seq, v: &str| Message { seq, via: Some(v.to_owned()), ..Message::default() };
        let got = hosted_ids(&[via(2, "session:ses_a"), via(3, "said:ses_a"), via(4, "said:ses_b"), via(5, "hud box")]);
        assert_eq!(got, [("ses_a".to_owned(), 2), ("ses_b".to_owned(), 4)]);
    }
}
