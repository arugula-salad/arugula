//! Threads on a pane or session, and conversations of their own (#855):
//! posting, reading, summaries, who gets told about a mention, and making
//! and changing conversations.

use super::{ConvReq, Daemon, Posted, SAVE_DEBOUNCE, ThreadError, ThreadPost};
use crate::{acl::Principal, pane::ToClient, store::now_ms};
use arugula_core::Role;
use arugula_proto::{
    Conversation, Driver, ServerMsg, SessionId, ThreadMsg, ThreadSummary, ThreadTarget,
    api::{Unreached, UnreachedWhy},
};
use tokio::time::Instant;

impl Daemon {
    /// Whether a thread's pane, session or conversation is still here.
    pub(super) fn thread_exists(&self, target: ThreadTarget) -> bool {
        match target {
            ThreadTarget::Pane(p) => self.panes.contains_key(&p) || self.blocks.contains_key(&p),
            ThreadTarget::Session(s) => self.mux.session(s).is_ok(),
            ThreadTarget::Conv(c) => self.threads.conv(c).is_some(),
        }
    }

    /// The session a thread is in, if a grant on it could open the thread:
    /// not a private pane's (its owner's alone).
    pub(super) fn thread_session(&self, target: ThreadTarget) -> Option<SessionId> {
        if !self.thread_exists(target) {
            return None;
        }
        match target {
            ThreadTarget::Pane(p) if self.meta.get(&p).is_some_and(|m| m.private) => None,
            ThreadTarget::Pane(p) => self.session_of(p),
            ThreadTarget::Session(s) => Some(s),
            // Its people are its members, not a session's.
            ThreadTarget::Conv(_) => None,
        }
    }

    /// Whether `who` may instruct conversation `conv`'s lead (#858): whoever
    /// may drive its block, as it runs here with the owner's rights (#959).
    /// With no lead yet, an `@lead` starts one: the owner's, or a team's
    /// member's.
    fn may_tell_lead(&self, who: &Principal, conv: arugula_proto::ConvId) -> bool {
        match self.threads.conv(conv).and_then(|c| c.lead.as_ref()) {
            Some(l) => self.may_drive_here(who, l.block).is_ok(),
            None => who.is_owner() || self.config.acl.team_role(who).is_some_and(|r| r >= Role::Editor),
        }
    }

    /// `who`'s role in a thread, and the time its messages start for them:
    /// a pane's thread is read by whoever may read the pane (a private
    /// pane's only by its owner), a session's by whoever has a role in it.
    /// A "from now" share sees messages from when it was made. A
    /// conversation's are its record's: its owner, and its members as
    /// editors from when they joined (no session's grant counts).
    pub(super) fn thread_role(&self, who: &Principal, target: ThreadTarget) -> Option<(Role, u64)> {
        if !self.thread_exists(target) {
            return None;
        }
        if who.is_owner() {
            return Some((Role::Owner, 0));
        }
        let session = match target {
            ThreadTarget::Pane(p) => self.session_of(p).filter(|_| self.readable(who, p))?,
            ThreadTarget::Session(s) => s,
            ThreadTarget::Conv(c) => {
                let conv = self.threads.conv(c)?;
                let floor = conv.floor(who.id())?;
                return Some((if who.id() == conv.owner { Role::Owner } else { Role::Editor }, floor));
            }
        };
        let role = self.config.acl.role(who, session)?;
        Some((role, self.config.acl.thread_floor(who, session, target).unwrap_or(0)))
    }

    /// Where an invite from a thread opens (#297): see [`Api::ThreadPlace`].
    pub(super) fn thread_place(
        &self,
        target: ThreadTarget,
        msg: Option<u64>,
    ) -> Result<(SessionId, Option<u64>), String> {
        if let ThreadTarget::Conv(_) = target {
            return Err("a conversation's people are its members: add them to it".into());
        }
        self.thread_session(target)
            .ok_or_else(|| "no such thread, or it's a private pane's".to_owned())
            .map(|s| (s, msg.and_then(|id| self.threads.get(target).iter().find(|m| m.id == id).map(|m| m.at))))
    }

    pub(super) fn thread_get(&self, target: ThreadTarget, who: &Principal) -> Result<Vec<ThreadMsg>, ThreadError> {
        let (_, floor) = self.thread_role(who, target).ok_or_else(|| ThreadError(404, "no such thread".into()))?;
        Ok(self.threads.get(target).iter().filter(|m| m.at >= floor).cloned().collect())
    }

    /// The threads `who` may read that have messages, with what they
    /// haven't read. `me`: who they are in chat, `who` unless a team's other
    /// owner (#935): reads and mentions are theirs.
    pub(super) fn threads_for(&self, who: &Principal, me: &Principal) -> Vec<ThreadSummary> {
        let mut out: Vec<ThreadSummary> = self
            .threads
            .targets()
            .filter_map(|t| {
                let (_, floor) = self.thread_role(who, t)?;
                let msgs: Vec<&ThreadMsg> = self.threads.get(t).iter().filter(|m| m.at >= floor).collect();
                let last = msgs.last()?;
                let read = self.threads.read_upto(me.id(), t);
                let new: Vec<&&ThreadMsg> = msgs.iter().filter(|m| m.id > read && m.who != me.id()).collect();
                Some(ThreadSummary {
                    target: t,
                    last: last.id,
                    at: last.at,
                    unread: new.len() as u32,
                    mention: new.iter().any(|m| m.mentions.iter().any(|x| x == me.id())),
                })
            })
            .collect();
        out.sort_by_key(|t| t.target);
        out
    }

    /// Everyone a message could @mention: the owner, the team's other
    /// owners (#386), everyone shared with, team members (connected or
    /// not), and whoever is connected.
    fn mentionable(&self) -> Vec<Principal> {
        let mut out = vec![Principal::Owner];
        out.extend(self.config.people.co_owners());
        for g in self.config.acl.list() {
            out.push(Principal::User { id: g.principal.clone(), name: g.name.clone(), pic: None });
        }
        out.extend(self.config.people.team_people());
        for c in self.clients.values() {
            out.push(c.principal.clone());
        }
        let mut seen = std::collections::HashSet::new();
        out.retain(|p| seen.insert(p.id().to_owned()));
        out
    }

    pub(super) fn thread_post(&mut self, post: ThreadPost) -> Result<Posted, ThreadError> {
        let ThreadPost { target, who, acting, as_agent, text, quote, event } = post;
        if event.is_some() && !matches!(target, ThreadTarget::Conv(_)) {
            return Err(ThreadError(400, "only a conversation's links post events".into()));
        }
        let (role, _) = self.thread_role(&who, target).ok_or_else(|| ThreadError(404, "no such thread".into()))?;
        if role < Role::Editor {
            return Err(ThreadError(403, "you're watching this session; you can't post in its threads".into()));
        }
        let text = text.trim().to_owned();
        if text.is_empty() && quote.is_none() {
            return Err(ThreadError(400, "nothing to post".into()));
        }
        if let Some(q) = &quote
            && !self.readable(&who, q.pane)
        {
            return Err(ThreadError(403, format!("you can't read %{}", q.pane)));
        }
        // Rights are `who`'s; the name on the post is the speaker's (#935).
        let speaker = acting.as_ref().unwrap_or(&who);
        let by = as_agent
            .clone()
            .unwrap_or_else(|| Driver { who: speaker.id().to_owned(), name: self.chat_name_of(speaker) });
        // An event's line mentions nobody: what it quotes is a pane's.
        let tokens = if event.is_some() { Vec::new() } else { crate::labs::threads::mentions(&text) };
        // A team's other owner reads every thread, as the owner does.
        let co_owner = |p: &Principal| p.id().strip_prefix("account:").is_some_and(|a| self.config.people.owns_here(a));
        let mentions: Vec<String> = self
            .mentionable()
            .into_iter()
            .filter(|p| p.id() != by.who)
            .filter(|p| self.thread_role(p, target).is_some() || (co_owner(p) && self.thread_exists(target)))
            .filter(|p| {
                let name = self.chat_name_of(p);
                tokens.iter().any(|t| crate::labs::threads::names(t, p.id(), &name))
            })
            .map(|p| p.id().to_owned())
            .collect();
        // An @agent goes to the pane's agent as a follow-up: an instruction,
        // so only from someone who may drive it (and never from an agent).
        // In a conversation, @lead goes to its lead (#858), which runs here
        // with the owner's rights: so only from someone who may drive the
        // lead's block (#959). Anyone else's is a comment.
        let to_agent = as_agent.is_none()
            && match target {
                ThreadTarget::Pane(p) => {
                    crate::labs::threads::calls_agent(&tokens) && self.may_drive_here(&who, p).is_ok()
                }
                ThreadTarget::Conv(c) => crate::labs::threads::calls_lead(&tokens) && self.may_tell_lead(&who, c),
                ThreadTarget::Session(_) => false,
            };
        let reaches_agent = |t: &String| match target {
            ThreadTarget::Conv(_) => crate::labs::threads::calls_lead(std::slice::from_ref(t)),
            _ => crate::labs::threads::calls_agent(std::slice::from_ref(t)),
        };
        // Which tokens went anywhere; the rest are the poster's to hear about.
        let me = self.chat_name_of(speaker);
        let (mut landed, mut unreached) = (Vec::new(), Vec::new());
        for t in &tokens {
            let person = self.mentionable().into_iter().any(|p| {
                mentions.iter().any(|m| m == p.id()) && crate::labs::threads::names(t, p.id(), &self.chat_name_of(&p))
            });
            if person || (to_agent && reaches_agent(t)) {
                landed.push(t.clone());
            } else if crate::labs::threads::names(t, speaker.id(), &me) {
                // Yourself: nothing to say.
            } else if matches!(target, ThreadTarget::Conv(_)) && reaches_agent(t) {
                unreached.push(Unreached { token: t.clone(), why: UnreachedWhy::MayNotDrive });
            } else if crate::labs::threads::calls_agent(std::slice::from_ref(t)) {
                let why = if matches!(target, ThreadTarget::Pane(_)) {
                    UnreachedWhy::MayNotDrive
                } else {
                    UnreachedWhy::AgentNeedsPane
                };
                unreached.push(Unreached { token: t.clone(), why });
            } else {
                unreached.push(Unreached { token: t.clone(), why: UnreachedWhy::Nobody });
            }
        }
        let msg = ThreadMsg {
            id: 0,
            at: now_ms(),
            who: by.who.clone(),
            name: by.name.clone(),
            pic: if as_agent.is_some() { None } else { self.pic_of(speaker) },
            text,
            quote,
            mentions,
            landed,
            to_agent,
            agent: as_agent.is_some(),
            event,
        };
        let msg = self.threads.post(target, msg).map_err(|e| ThreadError(500, format!("can't save it: {e}")))?;
        // What you post, you've read.
        self.threads.mark_read(&by.who, target, msg.id);
        self.save_due.get_or_insert_with(|| Instant::now() + SAVE_DEBOUNCE);
        for c in self.clients.values() {
            if self.thread_role(&c.principal, target).is_some() {
                let _ = c.ctrl.send(ToClient::Msg(ServerMsg::Thread { target, msg: msg.clone() }));
            }
        }
        self.soon();
        self.notify_mentions(target, &msg);
        self.notify_card(target, &msg);
        Ok((msg, to_agent, unreached))
    }

    /// Tell everyone a message mentions, on their phones too.
    fn notify_mentions(&self, target: ThreadTarget, msg: &ThreadMsg) {
        if msg.mentions.is_empty() {
            return;
        }
        // A notification opens a pane: the thread's, or the session's first.
        // A conversation's opens itself (`thread`), with no pane.
        let pane = match target {
            ThreadTarget::Pane(p) => Some(p),
            ThreadTarget::Conv(_) => Some(0),
            ThreadTarget::Session(s) => self
                .mux
                .session(s)
                .ok()
                .and_then(|s| s.tabs.first().copied())
                .and_then(|t| self.mux.tab(t).ok())
                .and_then(|t| t.root.panes().into_iter().next()),
        };
        let Some(pane) = pane else { return };
        let title = format!("{} mentioned you", msg.name);
        let body: String = msg.text.chars().take(200).collect();
        // Its own notification, not the pane's.
        let extra = serde_json::json!({ "thread": target.key(), "tag": format!("thread-{}", target.key()) });
        for id in &msg.mentions {
            self.config
                .notify
                .notify(pane, &title, &body, Some(extra.clone()), Some(extra.clone()), &|p| p.id() == id.as_str());
        }
    }
}

impl Daemon {
    /// A link's card (#857) goes to everyone in its conversation, on their
    /// phones too, from here: they may not reach the link's machine.
    fn notify_card(&self, target: ThreadTarget, msg: &ThreadMsg) {
        let (ThreadTarget::Conv(id), Some(e)) = (target, &msg.event) else { return };
        if e.kind != arugula_proto::ConvEventKind::NeedsInput {
            return;
        }
        let Some(c) = self.threads.conv(id) else { return };
        let title = format!("{} needs you in {}", e.link, c.title);
        let body: String = e.ask.as_ref().map(|a| a.input.as_str()).unwrap_or(&msg.text).chars().take(200).collect();
        let extra = serde_json::json!({ "thread": target.key(), "tag": format!("thread-{}-{}", target.key(), e.link) });
        let people: Vec<String> =
            std::iter::once(c.owner.clone()).chain(c.members.iter().map(|m| m.who.clone())).collect();
        self.config
            .notify
            .notify(0, &title, &body, Some(extra.clone()), Some(extra), &|p| people.iter().any(|w| w == p.id()));
    }
}

impl Daemon {
    /// The conversations kept here that `who` is in, oldest first.
    pub(super) fn convs_for(&self, who: &Principal) -> Vec<Conversation> {
        let mut out: Vec<Conversation> = self
            .threads
            .convs()
            .filter(|c| self.thread_role(who, ThreadTarget::Conv(c.id)).is_some())
            .cloned()
            .collect();
        out.sort_by_key(|c| (c.created, c.id));
        out
    }

    /// Make, change or delete a conversation, as `who` (#855). Its rules are
    /// `labs::convs`'; whom it may add is decided here: someone who can
    /// reach this machine (a team's member, or someone something here is
    /// shared with), named by id or as an `@` names them.
    pub(super) fn conv(&mut self, req: ConvReq, who: &Principal) -> Result<Conversation, ThreadError> {
        use crate::labs::convs::{self, Change};
        let err = |(code, why): convs::Refusal| ThreadError(code, why);
        let by = who.id().to_owned();
        let get =
            |d: &Self, id| d.threads.conv(id).cloned().ok_or_else(|| ThreadError(404, "no such conversation".into()));
        let (mut c, change) = match req {
            ConvReq::Get { id } => {
                // Nobody outside it learns it's here.
                return get(self, id)
                    .ok()
                    .filter(|c| c.floor(&by).is_some())
                    .ok_or_else(|| ThreadError(404, "no such conversation".into()));
            }
            ConvReq::Create { title } => {
                if !who.is_owner() {
                    return Err(ThreadError(403, "only this machine's owner makes conversations on it".into()));
                }
                (convs::create(&by, &self.chat_name_of(who), &title, now_ms()).map_err(err)?, None)
            }
            ConvReq::Delete { id } => {
                let c = get(self, id)?;
                convs::may_delete(&c, &by).map_err(err)?;
                self.threads.remove_conv(id).map_err(|e| ThreadError(500, format!("can't delete it: {e}")))?;
                self.config.acl.set_reach(self.threads.reach());
                self.save_due.get_or_insert_with(|| Instant::now() + SAVE_DEBOUNCE);
                self.soon();
                return Ok(c);
            }
            ConvReq::Rename { id, title } => (get(self, id)?, Some(Change::Rename(title))),
            ConvReq::AddMember { id, member, history, reach } => {
                let c = get(self, id)?;
                if by != c.owner {
                    // Before looking anyone up: who exists isn't theirs to learn.
                    return Err(ThreadError(403, "only the conversation's owner adds people to it".into()));
                }
                // Someone who reaches this machine already, or (#856) an
                // account that being a member lets in, with its root.
                let (who, name, root) = match self.member_named(&member) {
                    Ok(p) => (p.id().to_owned(), self.chat_name_of(&p), None),
                    Err(e) => match reach {
                        Some((name, root)) if member.starts_with("account:") && !root.is_empty() => {
                            let name = if name.trim().is_empty() { member.clone() } else { name };
                            (member.clone(), name, Some(root))
                        }
                        _ => return Err(err(e)),
                    },
                };
                (c, Some(Change::Add { who, name, history, at: now_ms(), root }))
            }
            ConvReq::RemoveMember { id, member } => (get(self, id)?, Some(Change::Remove(member))),
            ConvReq::Link { id, mut link } => {
                link.by_name = self.chat_name_of(who);
                link.at = now_ms();
                (get(self, id)?, Some(Change::Link(link)))
            }
            ConvReq::Unlink { id, name } => (get(self, id)?, Some(Change::Unlink(name))),
            ConvReq::Lead { id, change } => {
                let mut c = get(self, id)?;
                let reply = match &change {
                    convs::LeadChange::Replied { who, msg } => Some((who.clone(), *msg)),
                    _ => None,
                };
                convs::lead(&mut c, change).map_err(err)?;
                self.threads.save_conv(c.clone()).map_err(|e| ThreadError(500, format!("can't save it: {e}")))?;
                self.soon();
                if let Some((who, msg)) = reply {
                    self.notify_reply(&c, &who, msg);
                }
                return Ok(c);
            }
            ConvReq::LinkClosed { id, name, why } => {
                let mut c = get(self, id)?;
                if !convs::close_link(&mut c, &name, &why) {
                    return Ok(c);
                }
                (c, None)
            }
        };
        let (mut added, mut gone) = (None, None);
        if let Some(change) = change {
            match &change {
                Change::Add { who, .. } => added = Some(who.clone()),
                Change::Remove(who) => gone = Some(who.clone()),
                Change::Rename(_) | Change::Link(_) | Change::Unlink(_) => {}
            }
            convs::apply(&mut c, &by, change).map_err(err)?;
        }
        if let Some(who) = gone {
            self.threads.forget_reads(&who, ThreadTarget::Conv(c.id));
        }
        self.threads.save_conv(c.clone()).map_err(|e| ThreadError(500, format!("can't save it: {e}")))?;
        self.config.acl.set_reach(self.threads.reach());
        self.save_due.get_or_insert_with(|| Instant::now() + SAVE_DEBOUNCE);
        self.soon();
        if let Some(member) = added {
            self.notify_added(&c, &member);
        }
        Ok(c)
    }

    /// Whom `name` names, of the people who can reach this machine: a
    /// principal id, or a name as an `@` gives it (with or without the `@`).
    fn member_named(&self, name: &str) -> Result<Principal, crate::labs::convs::Refusal> {
        let token = name.trim().trim_start_matches('@').to_lowercase();
        if token.is_empty() {
            return Err((400, "whom? give their name or id".into()));
        }
        let people: Vec<Principal> = self
            .mentionable()
            .into_iter()
            .filter(|p| !p.is_owner() && !p.id().starts_with("link:") && !p.id().starts_with("mcp:"))
            .filter(|p| self.config.acl.knows(p))
            .collect();
        if let Some(p) = people.iter().find(|p| p.id().eq_ignore_ascii_case(name.trim())) {
            return Ok(p.clone());
        }
        let named: Vec<&Principal> =
            people.iter().filter(|p| crate::labs::threads::names(&token, p.id(), &self.chat_name_of(p))).collect();
        match named.as_slice() {
            [p] => Ok((*p).clone()),
            [] => Err((
                404,
                format!(
                    "nobody called {name} can reach this machine: share a session with them first, or have them in your team"
                ),
            )),
            _ => Err((
                409,
                format!("{name} names {}: give their id", named.iter().map(|p| p.id()).collect::<Vec<_>>().join(", ")),
            )),
        }
    }

    /// Tell whoever asked the lead (#858) that it answered, on their phones
    /// too: its posts mention no one, so nothing else would.
    fn notify_reply(&self, c: &Conversation, who: &[String], msg: u64) {
        let target = ThreadTarget::Conv(c.id);
        let Some(m) = self.threads.get(target).iter().find(|m| m.id == msg) else { return };
        let title = format!("The lead answered you in {}", c.title);
        let body: String = m.text.chars().take(200).collect();
        let extra = serde_json::json!({ "thread": target.key(), "tag": format!("thread-{}", target.key()) });
        self.config.notify.notify(0, &title, &body, Some(extra.clone()), Some(extra), &|p| {
            who.iter().any(|w| w == p.id()) && c.floor(p.id()).is_some()
        });
    }

    /// Tell someone just added to a conversation, on their phone too: what
    /// being a member lets them do is the invite's (#855).
    fn notify_added(&self, c: &Conversation, member: &str) {
        let target = ThreadTarget::Conv(c.id);
        let title = format!("{} added you to {}", c.owner_name, c.title);
        let extra = serde_json::json!({ "thread": target.key(), "tag": format!("thread-{}", target.key()) });
        self.config.notify.notify(0, &title, arugula_proto::CONV_MEMBERS_SEE, Some(extra.clone()), Some(extra), &|p| {
            p.id() == member
        });
    }
}
