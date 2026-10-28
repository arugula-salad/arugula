//! Which box a thread's session runs on (Arugula #214): the project's
//! workspaces as one pool.
//!
//! - **The candidates** ([`candidates`]): the workspace the project chose
//!   ("agents run on", `Db::agents_run_on`), else its only one, else every
//!   live one: none chosen means pick for me.
//! - **The repo first** ([`settle`]): the repos across the candidates'
//!   specs. The one the ask names (`owner/name`, or a name only one of
//!   them has) is it; else, when there's one, that one; none at all, the
//!   session works in no repo. A repo the project linked that no candidate
//!   has, named by `owner/name`, is said so ([`Settled::Missing`]);
//!   several and named none, the asker is asked ([`Settled::Ambiguous`]).
//! - **Then the box** ([`ranked`], [`Load::rank`]): of the candidates
//!   that have the repo, one with a free slot (fewer than
//!   `MAX_SESSIONS_PER_BOX` running, counting queued ones and done ones
//!   not yet parked, and fewer than `MAX_HELD_SESSIONS_PER_BOX` held,
//!   parked ones too; fewer on a box with fewer session users, as
//!   [`Caps`] says), spreading before it packs (Arugula #266): an idle
//!   one awake (ready, and online at the hub; ready alone when the hub
//!   can't say) first, then an idle one asleep that can wake (ready or
//!   paused, or waking already: its session's job wakes it, as `arugula
//!   cloud open` does, `sessions::job`), then one awake that's already
//!   building, then any other with a slot, then a full one. A box is
//!   **idle** when it has no live session (queued, or a turn under way,
//!   as its box last reported); one with only done or parked sessions is
//!   idle, since they wait without building. Within each: the fewest live
//!   sessions, then the fewest done ones, parked or not, then by name. So
//!   asks in a row land on a box each while there are idle ones, and only
//!   then double up.
//! - **Leftovers** (Arugula #266): a session its box took before the
//!   workspace's box changed (a rebuild: `machine` isn't the workspace's
//!   now) isn't counted: the new box doesn't hold it
//!   (`db::sessions::BoxLoad`).
//! - **Move to…** lists the others that have the session's repo the same
//!   way ([`crate::sessions::moves`]).

use std::collections::HashSet;

use anyhow::Result;

use crate::{
    Platform,
    db::{
        hosted::HostedWorkspace,
        sessions::{AgentsOn, BoxLoad, Caps},
    },
};

/// The workspaces a new session may go to: the one chosen or the only
/// one, else every live one. Empty: the project has none.
pub fn candidates(app: &Platform, project: &str) -> Result<Vec<HostedWorkspace>> {
    let ids = match app.db.agents_run_on(project)? {
        AgentsOn::Nowhere => return Ok(vec![]),
        AgentsOn::Chosen(id) | AgentsOn::Only(id) => vec![id],
        AgentsOn::Ask(ids) => ids,
    };
    live(app, project, &ids)
}

/// Every live workspace of the project, whatever its agents run on: where
/// Move to… may send a session.
pub fn candidates_all(app: &Platform, project: &str) -> Result<Vec<HostedWorkspace>> {
    let all = app.db.hosted_workspaces(project)?;
    Ok(all.into_iter().filter(|w| !matches!(w.state.as_str(), "deleting" | "deleted")).collect())
}

/// The project's live workspaces among `ids`, in that order.
pub fn live(app: &Platform, project: &str, ids: &[String]) -> Result<Vec<HostedWorkspace>> {
    let mut out = Vec::new();
    for id in ids {
        if let Some(w) = app.db.hosted_any(project, id)?.filter(|w| !matches!(w.state.as_str(), "deleting" | "deleted"))
        {
            out.push(w);
        }
    }
    Ok(out)
}

/// The box's repos as `owner/name` (or the clone URL, when it isn't
/// GitHub's), in its spec's order.
pub fn repos(w: &HostedWorkspace) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for r in &w.spec.repos {
        let name = crate::hosted::setup::github_repo(r).unwrap_or_else(|| r.clone());
        if !out.contains(&name) {
            out.push(name);
        }
    }
    out
}

/// Every repo the boxes have, each once, in their order.
pub fn all_repos(ws: &[HostedWorkspace]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for r in ws.iter().flat_map(repos) {
        if !out.contains(&r) {
            out.push(r);
        }
    }
    out
}

/// The boxes that have `repo` (every one, for none).
pub fn with_repo(ws: Vec<HostedWorkspace>, repo: Option<&str>) -> Vec<HostedWorkspace> {
    match repo {
        None => ws,
        Some(r) => ws.into_iter().filter(|w| repos(w).iter().any(|x| x == r)).collect(),
    }
}

/// The words of `text` as names may be written: lowercase, split at
/// anything but letters, digits, `/`, `.`, `-` and `_`, trailing dots off.
pub fn words(text: &str) -> Vec<String> {
    text.split(|c: char| !(c.is_alphanumeric() || matches!(c, '/' | '.' | '-' | '_')))
        .map(|w| w.trim_end_matches('.').to_lowercase())
        .filter(|w| !w.is_empty())
        .collect()
}

/// The one of `options` that `text` names, when it names exactly one: by
/// any of each option's names.
pub fn named<'a>(text: &str, options: &'a [(String, Vec<String>)]) -> Option<&'a str> {
    let ws = words(text);
    let mut hit = options.iter().filter(|(_, names)| names.iter().any(|n| ws.contains(&n.to_lowercase())));
    let first = hit.next()?;
    hit.next().is_none().then_some(first.0.as_str())
}

/// A repo goes by `owner/name`, and by `name` when no other listed repo
/// has it.
pub fn repo_options(all: &[String]) -> Vec<(String, Vec<String>)> {
    let short = |r: &str| r.rsplit('/').next().unwrap_or(r).to_owned();
    all.iter()
        .map(|r| {
            let mut names = vec![r.clone()];
            let s = short(r);
            if all.iter().filter(|o| short(o) == s).count() == 1 && s != *r {
                names.push(s);
            }
            (r.clone(), names)
        })
        .collect()
}

/// Which repo an ask is about ([`settle`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Settled {
    /// This one, or none: the boxes have no repos.
    Repo(Option<String>),
    /// The ask names a repo the project linked that none of the boxes has.
    Missing(String),
    /// Several, and the ask names none of them: ask its asker.
    Ambiguous,
}

/// Which of `all` (the boxes' repos) `text` is about, by the module's rule;
/// `linked`: the project's linked repos (`owner/name`), which the ask may
/// name though no box has them.
pub fn settle(text: &str, all: &[String], linked: &[String]) -> Settled {
    if let Some(r) = named(text, &repo_options(all)) {
        return Settled::Repo(Some(r.to_owned()));
    }
    // Only by `owner/name`: a linked repo's short name is often a plain
    // word ("docs", "api").
    let full: Vec<(String, Vec<String>)> = linked.iter().map(|r| (r.clone(), vec![r.clone()])).collect();
    if let Some(r) = named(text, &full).filter(|r| !all.iter().any(|a| a.eq_ignore_ascii_case(r))) {
        return Settled::Missing(r.to_owned());
    }
    match all {
        [] => Settled::Repo(None),
        [one] => Settled::Repo(Some(one.clone())),
        _ => Settled::Ambiguous,
    }
}

/// The project's linked repos, by `owner/name` (unlinked ones aside).
pub fn linked(app: &Platform, project: &str) -> Result<Vec<String>> {
    Ok(app.db.linked_repos(project)?.into_iter().filter(|l| l.unlinked_at.is_none()).map(|l| l.full_name).collect())
}

/// How a box looks to the pick.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Load {
    /// Ready, and online at the hub (or the hub can't say).
    pub awake: bool,
    /// Asleep but it can be woken for a session: ready (off the relay),
    /// paused, or waking already. Not one being made, or failed.
    pub wakeable: bool,
    /// The project's sessions it has queued or with a turn under way.
    pub live: u32,
    /// The project's done sessions it holds with their agent up, each
    /// waiting for its next prompt.
    pub done: u32,
    /// The project's parked sessions it holds: done, its agent closed, its
    /// checkout kept (Arugula #215).
    pub parked: u32,
    /// How many it may run and hold: fewer than the most on a box with
    /// fewer session users.
    pub caps: Caps,
}

impl Load {
    pub fn of(state: &str, awake: bool, l: BoxLoad, caps: Caps) -> Self {
        let wakeable = !awake && matches!(state, "ready" | "paused" | "waking");
        Load { awake, wakeable, live: l.live, done: l.done, parked: l.parked, caps }
    }

    /// The sessions it holds or has queued, parked ones too: each has a
    /// session user there (`MAX_HELD_SESSIONS_PER_BOX`).
    pub fn busy(self) -> u32 {
        self.live + self.done + self.parked
    }

    /// A slot for one more, awake or not: room to run it (live and done
    /// ones; a parked one isn't running) and to hold it, by its [`Caps`].
    pub fn room(self) -> bool {
        ((self.live + self.done) as usize) < self.caps.run && (self.busy() as usize) < self.caps.hold
    }

    /// Building: a session queued there or with a turn under way. Done and
    /// parked ones only wait.
    pub fn building(self) -> bool {
        self.live > 0
    }

    /// Where it goes in the pick, best first (the module's rule): idle and
    /// awake, idle and asleep but wakeable (woken for it), awake and
    /// building, any other with room, full; within each, the fewest live
    /// sessions, then the fewest done ones, parked or not. A live session
    /// works the box's CPU and memory now; a done one only waits, and a
    /// parked one only keeps its checkout.
    pub fn rank(self) -> (u8, u32, u32) {
        let tier = match (self.room(), self.awake, self.wakeable, self.building()) {
            (false, ..) => 4,
            (true, true, _, false) => 0,
            (true, false, true, false) => 1,
            (true, true, _, true) => 2,
            (true, false, ..) => 3,
        };
        (tier, self.live, self.done + self.parked)
    }
}

/// `ws`, best first, each with its load (the module's rule).
pub async fn ranked(app: &Platform, project: &str, ws: Vec<HostedWorkspace>) -> Result<Vec<(HostedWorkspace, Load)>> {
    let held = app.db.workspace_load(project, arugula_e2e::now_ms())?;
    let mut caps = std::collections::HashMap::new();
    for w in &ws {
        caps.insert(w.id.clone(), app.db.session_caps(&w.id)?);
    }
    let machines: Vec<String> = ws.iter().filter_map(|w| w.machine.clone()).collect();
    // The hub not saying is "can't say", not "offline".
    let online: Option<HashSet<String>> = if machines.is_empty() {
        Some(HashSet::new())
    } else {
        app.hub.machines_online(&machines).await.ok().map(|m| m.into_iter().collect())
    };
    let mut out: Vec<(HostedWorkspace, Load)> = ws
        .into_iter()
        .map(|w| {
            let up = w.state == "ready"
                && w.machine.as_ref().is_some_and(|m| online.as_ref().is_none_or(|on| on.contains(m)));
            let caps = caps.get(&w.id).copied().unwrap_or(Caps::of(None));
            let load = Load::of(&w.state, up, held.get(&w.id).copied().unwrap_or_default(), caps);
            (w, load)
        })
        .collect();
    out.sort_by(|(a, la), (b, lb)| {
        (la.rank(), a.name.to_lowercase(), &a.id).cmp(&(lb.rank(), b.name.to_lowercase(), &b.id))
    });
    Ok(out)
}

/// The best of `ws` for a session in `repo` (module docs); `None` when
/// none of them has it.
pub async fn pick(
    app: &Platform,
    project: &str,
    ws: Vec<HostedWorkspace>,
    repo: Option<&str>,
) -> Result<Option<HostedWorkspace>> {
    Ok(ranked(app, project, with_repo(ws, repo)).await?.into_iter().next().map(|(w, _)| w))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn opts(names: &[&str]) -> Vec<(String, Vec<String>)> {
        names.iter().map(|n| ((*n).to_owned(), vec![(*n).to_owned()])).collect()
    }

    /// A reply picks the one option it names, and nothing when it names
    /// none or several.
    #[test]
    fn a_reply_names_one_option() {
        let boxes = opts(&["dev", "staging"]);
        assert_eq!(named("dev please.", &boxes), Some("dev"));
        assert_eq!(named("Staging", &boxes), Some("staging"));
        assert_eq!(named("either dev or staging", &boxes), None);
        assert_eq!(named("devbox", &boxes), None);
        let repos = repo_options(&["o/app".into(), "o/web".into(), "p/web".into()]);
        assert_eq!(named("app", &repos), Some("o/app"));
        assert_eq!(named("web", &repos), None, "two repos are called web");
        assert_eq!(named("use p/web.", &repos), Some("p/web"));
    }

    /// Idle and awake first, then idle and asleep (woken for it), then
    /// awake and building, then any other with room, then full; within
    /// each, fewest live, then fewest done. A box with three session users
    /// (one from before Arugula #215, or that says nothing) is full at
    /// three held.
    #[test]
    fn idle_boxes_first_then_live_sessions_weigh_more_than_done() {
        let l = |awake: bool, live, done, parked| Load {
            awake,
            wakeable: !awake,
            live,
            done,
            parked,
            caps: Caps::of(Some(6)),
        };
        let mut boxes = [
            ("full-awake", l(true, 0, 3, 0)),
            ("asleep-empty", l(false, 0, 0, 0)),
            ("one-live", l(true, 1, 0, 0)),
            ("two-done", l(true, 0, 2, 0)),
            ("one-each", l(true, 1, 1, 0)),
            // Asleep with one queued: building, after the awake ones that
            // are.
            ("asleep-one-live", l(false, 1, 0, 0)),
            // Failed: it won't wake, so not before a building one awake.
            ("failed-empty", Load { wakeable: false, ..l(false, 0, 0, 0) }),
            // Three parked: none running, so room and idle, but last of
            // the idle awake.
            ("three-parked", l(true, 0, 0, 3)),
            // Six held, though only two run: full.
            ("six-held", l(true, 0, 2, 4)),
            // Three users, three parked: full; and one that doesn't say
            // is the same.
            ("three-users", Load { caps: Caps::of(Some(3)), ..l(true, 0, 0, 3) }),
            ("unsaid", Load { caps: Caps::of(None), ..l(true, 0, 0, 3) }),
        ];
        boxes.sort_by_key(|(name, load)| (load.rank(), *name));
        let order: Vec<&str> = boxes.iter().map(|(n, _)| *n).collect();
        assert_eq!(
            order,
            [
                "two-done",
                "three-parked",
                "asleep-empty",
                "one-live",
                "one-each",
                "failed-empty",
                "asleep-one-live",
                "full-awake",
                "three-users",
                "unsaid",
                "six-held"
            ]
        );
    }

    /// Only a ready, paused or waking box that isn't awake can be woken.
    #[test]
    fn a_box_wakes_from_ready_paused_or_waking() {
        let wakes = |state, awake| Load::of(state, awake, BoxLoad::default(), Caps::of(None)).wakeable;
        assert!(wakes("ready", false) && wakes("paused", false) && wakes("waking", false));
        assert!(!wakes("ready", true), "awake already");
        assert!(!wakes("creating", false) && !wakes("failed", false));
    }

    /// The repo an ask is about: the one it names, the only one, none at
    /// all; a linked one no box has is missing; several and none named is
    /// a question.
    #[test]
    fn the_repo_is_settled_before_the_box() {
        let all: Vec<String> = vec!["o/app".into(), "o/web".into()];
        let linked: Vec<String> = vec!["o/app".into(), "o/web".into(), "o/docs".into()];
        assert_eq!(settle("@arugula fix the web canary", &all, &linked), Settled::Repo(Some("o/web".into())));
        assert_eq!(settle("@arugula fix o/app", &all, &linked), Settled::Repo(Some("o/app".into())));
        assert_eq!(settle("@arugula fix the canary", &all, &linked), Settled::Ambiguous);
        assert_eq!(settle("@arugula fix app and web", &all, &linked), Settled::Ambiguous, "two named");
        assert_eq!(settle("@arugula update o/docs", &all, &linked), Settled::Missing("o/docs".into()));
        assert_eq!(settle("@arugula update the docs", &all, &linked), Settled::Ambiguous, "not by its short name");
        assert_eq!(settle("@arugula fix it", &all[..1], &linked), Settled::Repo(Some("o/app".into())));
        assert_eq!(settle("@arugula fix it", &[], &[]), Settled::Repo(None));
    }
}
