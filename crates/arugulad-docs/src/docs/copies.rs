//! A machine's Docs copied up to the platform (#418): while this machine is
//! in control, each Doc in the library has a copy there
//! (`PUT /api/daemon/docs/{id}`, [`wire::MACHINE_DOC`]), which the platform
//! makes in the owner's personal project, private to them. It keys the copy
//! by the machine and the Doc's id, so the same Doc always lands on the same
//! copy, wherever it sits.
//!
//! - **When:** a change ([`changed`]: a new Doc, its outline, a fill, a
//!   picture) marks the Doc dirty, and it's put [`QUIET`] after its last
//!   change, or [`MOST`] after its first while it keeps changing: a fill of
//!   thirty sections is a few puts, not thirty. One put per Doc at a time,
//!   and [`FLYING`] in all; its pictures go after it, those whose `rev` the
//!   copy hasn't got.
//! - **Only on the account's own word, on a personal machine** (native
//!   #673): `DOC_COPIES`, "Your Docs on the platform", which control sends
//!   only to the account's personal machines and which a team never sets.
//!   A team's machine, a hosted box, and a personal machine without the
//!   word copy nothing; chat, a team's or heard, has no say here (it moves
//!   reader lists, `chat_readers`). An account that had its own chat on
//!   when the word came in was given it by control.
//! - **Off:** nothing is put, and a machine in control that never queued
//!   its library for its account marks nothing either, since that first
//!   queueing takes every Doc; after it, a Doc that changes while off
//!   stays dirty for when it's on again, as does one that changes while
//!   the machine is out of control, for the next control it joins.
//!   Copies already on the platform stay.
//! - **Not now:** out of control or out of reach, a Doc stays dirty and
//!   waits for the next refresh with control. Refused (403: the word off after
//!   all, or the owner may no longer write where the Doc now is) or told to
//!   slow down (429), nothing more is put until then: one refusal a refresh,
//!   however many Docs wait. Why is said once, at info. The dirty Docs are
//!   kept in `docs/.copies.json`, so a restart has them, and they wait for
//!   its first refresh.
//! - **Once:** after the first refresh of a machine in control with the
//!   word on, every Doc in the library is marked dirty; `docs/.copied-1` names
//!   the account it was done for, and joining another does it again.
//! - **Not on a hosted box:** its owner is whoever made the workspace, so a
//!   session's Docs would land in their personal project; nothing is copied.
//! - **Where it is:** the answer ([`wire::MachineDocPlaced`]) goes in the
//!   Doc's `meta.json` as `platform` ([`library::OnPlatform`]), with the
//!   pictures put. Nothing shows it yet.
//!
//! A Doc the platform can't take as it is (a 400 or 413: the platform's
//! checks are the block's, so it shouldn't happen) is left until it changes
//! again.
//!
//! **Moved to a team, its copy goes too** (Arugula #863): the team Doc is
//! its one home, so the pointer it leaves writes the same tombstone, and is
//! never copied.
//!
//! **Deleted here, its copy goes too** (native #673: the Doc is the truth
//! and the copy has none without it). The library writes a tombstone
//! (`docs/.gone/<id>`) before it removes the folder, and marks the Doc
//! like a change ([`removed`]); the put that finds the tombstone takes the
//! copy down instead (`DELETE`, [`wire::MACHINE_DOC`]) and drops the
//! tombstone, a copy that isn't there being done. Only a tombstone takes a
//! copy down: a Doc whose files can't be read, or are gone with no
//! tombstone (deleted by an older daemon, or by hand), is never taken for
//! deleted, and a tombstone whose Doc still reads (a removal that failed)
//! is dropped. Take-downs go whether or not copies are on: turning them off
//! stops new and changed copies, never a deleted Doc's copy coming down. A take-down the platform refuses (the owner may no longer
//! write there) is tried after the next refreshes, [`GONE_TRIES`] times,
//! without pausing the other Docs, then given up with a line in the log:
//! the copy stays, and its project's people can delete it on the
//! platform, as they can any copy. Being told to slow down is tried
//! again and never counted.

use std::{
    collections::{BTreeMap, BTreeSet},
    path::{Path, PathBuf},
    sync::{Arc, Mutex, OnceLock},
    time::{Duration, Instant},
};

use arugula_control_wire::project_docs::{self as wire, MachineDoc, MachineDocPlaced, MachineSection};
use serde_json::Value;
use tokio::sync::Notify;
use tracing::{info, warn};

use super::{assets, library};
use crate::{
    control::{Answered, Control},
    store::{private_dir, write_atomic},
};

/// How long a Doc is left after its last change before it's put...
pub const QUIET: Duration = Duration::from_secs(2);
/// ...and after its first, while it keeps changing.
pub const MOST: Duration = Duration::from_secs(10);
/// Puts at a time, of different Docs: the first copy of a big library
/// goes a few at a time, not all at once.
const FLYING: usize = 4;
/// Refusals a deleted Doc's take-down gets before it's given up.
const GONE_TRIES: u32 = 3;

/// In the library: the Docs not copied since they changed.
const DIRTY: &str = ".copies.json";
/// In the library: every Doc there was queued once, for this account.
const DONE: &str = ".copied-1";
/// The account's own word for copies, sent with the flags.
const WORD: &str = arugula_control_wire::DOC_COPIES;

static COPIES: OnceLock<Arc<Copies>> = OnceLock::new();

struct Copies {
    state_dir: PathBuf,
    control: Arc<Control>,
    queue: Mutex<Queue>,
    wake: Notify,
}

#[derive(Default)]
struct Queue {
    dirty: BTreeMap<String, Dirt>,
    /// Being put now.
    flying: BTreeSet<String>,
    /// Not taken: tried again after the next refresh.
    waiting: BTreeSet<String>,
    /// Told to slow down: nothing is put until the next refresh.
    paused: bool,
    /// What was last said of a put not taken, so it's said once.
    said: Option<String>,
}

/// A dirty Doc: when it first changed since it was last taken to be put,
/// when last, and a count of its changes.
struct Dirt {
    first: Instant,
    last: Instant,
    changes: u64,
}

/// Why a put didn't go through.
enum Fail {
    /// Not now: after the next refresh.
    Later(String),
    /// Refused or slow down (403, 429): nothing more until the next refresh.
    Pause(String),
    /// Not as it is: until it changes again.
    Never(String),
}

/// Start copying (the daemon's control is made): what was dirty when it
/// stopped waits for the first refresh.
pub fn start(control: Arc<Control>) {
    if crate::agent::hosted::gate().is_some() {
        return;
    }
    let state_dir = control.state_dir().to_owned();
    let now = Instant::now();
    let mut kept = read_dirty(&state_dir);
    // Deleted Docs whose copies aren't down yet.
    kept.extend(read_gone(&state_dir));
    kept.sort();
    kept.dedup();
    let queue = Queue {
        waiting: kept.iter().cloned().collect(),
        dirty: kept.into_iter().map(|id| (id, Dirt { first: now, last: now, changes: 0 })).collect(),
        ..Default::default()
    };
    let c = Arc::new(Copies { state_dir, control, queue: Mutex::new(queue), wake: Notify::new() });
    if COPIES.set(c.clone()).is_err() {
        return;
    }
    crate::task::spawn(c.clone().refreshes());
    crate::task::spawn(c.run());
}

/// The Doc `id` changed (or is new): copy it soon.
pub fn changed(id: &str) {
    if let Some(c) = COPIES.get() {
        c.mark([id.to_owned()]);
    }
}

/// The Doc `id` is gone from the library (its tombstone is written): its
/// copy goes soon.
pub fn removed(id: &str) {
    changed(id);
}

/// The tombstones' ids ([`library::GONE`]). One whose Doc still reads (a
/// removal that failed before the folder went) is dropped: the Doc lives,
/// and keeps its copy.
fn read_gone(state_dir: &Path) -> Vec<String> {
    let Ok(entries) = std::fs::read_dir(library::gone_dir(state_dir)) else { return vec![] };
    entries
        .flatten()
        .filter_map(|e| e.file_name().into_string().ok())
        .filter(|id| library::is_id(id))
        .filter(|id| {
            // A moved Doc's pointer reads, and its copy still comes down.
            let lives =
                library::dir(state_dir, id).is_ok_and(|d| library::read_meta(&d).is_some_and(|m| m.moved.is_none()));
            if lives {
                forget_gone(state_dir, id);
            }
            !lives
        })
        .collect()
}

fn is_gone(state_dir: &Path, id: &str) -> bool {
    library::gone_dir(state_dir).join(id).exists()
}

/// The tombstone goes: the copy is down, or given up on.
fn forget_gone(state_dir: &Path, id: &str) {
    let _ = std::fs::remove_file(library::gone_dir(state_dir).join(id));
}

/// One more refusal of the take-down of `id`, kept in its tombstone: how
/// many so far.
fn refused_gone(state_dir: &Path, id: &str) -> u32 {
    let file = library::gone_dir(state_dir).join(id);
    let n: u32 = std::fs::read_to_string(&file).ok().and_then(|t| t.trim().parse().ok()).unwrap_or(0) + 1;
    let _ = write_atomic(&file, n.to_string().as_bytes());
    n
}

fn read_dirty(state_dir: &Path) -> Vec<String> {
    let file = library::root(state_dir).join(DIRTY);
    let ids: Vec<String> = std::fs::read(file).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default();
    ids.into_iter().filter(|id| library::is_id(id)).collect()
}

impl Copies {
    fn mark(&self, ids: impl IntoIterator<Item = String>) {
        // In control, not allowed, and never queued for this account: the
        // first queueing takes every Doc, so nothing waits in the meantime.
        // Out of control nothing is known: a change waits for the next
        // control, which may be the same account again.
        // A deleted Doc's take-down is queued whatever: it may have a copy
        // from before.
        let mut ids: Vec<String> = ids.into_iter().collect();
        if self.control.enrolled().is_some() && !self.allowed() && !self.queued_once() {
            ids.retain(|id| is_gone(&self.state_dir, id));
            if ids.is_empty() {
                return;
            }
        }
        let now = Instant::now();
        let mut q = self.queue.lock().unwrap();
        let mut new = false;
        for id in ids {
            let d = q.dirty.entry(id).or_insert_with(|| {
                new = true;
                Dirt { first: now, last: now, changes: 0 }
            });
            d.last = now;
            d.changes += 1;
        }
        if new {
            self.save(&q);
        }
        drop(q);
        self.wake.notify_one();
    }

    /// Keep the dirty set on disk: only when a Doc joins or leaves it.
    fn save(&self, q: &Queue) {
        let root = library::root(&self.state_dir);
        let ids: Vec<&String> = q.dirty.keys().collect();
        let wrote = private_dir(&root)
            .and_then(|()| write_atomic(&root.join(DIRTY), &serde_json::to_vec(&ids).unwrap_or_default()));
        if let Err(e) = wrote {
            warn!(error = %e, "can't keep which Docs are still to copy up");
        }
    }

    /// After each refresh: queue every Doc once, and try again what waited.
    async fn refreshes(self: Arc<Self>) {
        let mut refreshed = self.control.on_refresh();
        while refreshed.changed().await.is_ok() {
            if let Some(account) = self.control.account()
                && self.allowed()
            {
                self.copy_all(&account);
            }
            let mut q = self.queue.lock().unwrap();
            q.waiting.clear();
            q.paused = false;
            drop(q);
            self.wake.notify_one();
        }
    }

    /// Copies are allowed: a personal machine, with its account's own
    /// word (never a team's, nor chat).
    fn allowed(&self) -> bool {
        let personal =
            self.control.enrolled().is_some_and(|e| matches!(e.saved.kind, crate::control::MachineKind::Personal));
        personal && arugula_proto::flags::control_account_says(&self.state_dir, WORD) == Some(true)
    }

    /// Whether the library was queued once for the account this machine
    /// is joined to now.
    fn queued_once(&self) -> bool {
        let Some(account) = self.control.account() else { return false };
        std::fs::read_to_string(library::root(&self.state_dir).join(DONE)).is_ok_and(|a| a.trim() == account)
    }

    /// Every Doc in the library, once for each account the machine joins.
    fn copy_all(&self, account: &str) {
        let done = library::root(&self.state_dir).join(DONE);
        if std::fs::read_to_string(&done).is_ok_and(|a| a.trim() == account) {
            return;
        }
        let ids: Vec<String> = library::list(&self.state_dir).into_iter().map(|m| m.id).collect();
        let n = ids.len();
        // Kept dirty on disk before the marker says so.
        self.mark(ids);
        if let Err(e) = private_dir(done.parent().unwrap()).and_then(|()| write_atomic(&done, account.as_bytes())) {
            warn!(error = %e, "can't note that this machine's Docs were queued to copy up");
        }
        info!(docs = n, "copying this machine's Docs up to the platform");
    }

    async fn run(self: Arc<Self>) {
        loop {
            let (due, next) = self.due();
            for (id, changes) in due {
                crate::task::spawn(self.clone().put(id, changes));
            }
            match next {
                Some(wait) => {
                    let _ = tokio::time::timeout(wait, self.wake.notified()).await;
                }
                None => self.wake.notified().await,
            }
        }
    }

    /// The Docs to put now, each with its count of changes, and how long
    /// until the next is due.
    fn due(&self) -> (Vec<(String, u64)>, Option<Duration>) {
        if self.control.enrolled().is_none() {
            return (Vec::new(), None);
        }
        // Not allowed to copy: a deleted Doc's copy still comes down.
        let allowed = self.allowed();
        let now = Instant::now();
        let mut q = self.queue.lock().unwrap();
        if q.paused {
            return (Vec::new(), None);
        }
        let Queue { dirty, flying, waiting, .. } = &mut *q;
        let (mut due, mut next) = (Vec::new(), None::<Duration>);
        let state_dir = &self.state_dir;
        for (id, d) in dirty
            .iter_mut()
            .filter(|(id, _)| !flying.contains(*id) && !waiting.contains(*id))
            .filter(|(id, _)| allowed || is_gone(state_dir, id))
        {
            let at = (d.last + QUIET).min(d.first + MOST);
            if at <= now {
                // The rest wait for a put to end, which wakes this.
                if flying.len() + due.len() >= FLYING {
                    continue;
                }
                d.first = now;
                due.push((id.clone(), d.changes));
            } else {
                next = Some(next.map_or(at - now, |n| n.min(at - now)));
            }
        }
        flying.extend(due.iter().map(|(id, _)| id.clone()));
        (due, next)
    }

    /// Put the Doc `id`, and say how it went: it leaves the dirty set if it
    /// didn't change meanwhile (`changes`).
    async fn put(self: Arc<Self>, id: String, changes: u64) {
        let got = self.copy(&id).await;
        let mut q = self.queue.lock().unwrap();
        q.flying.remove(&id);
        let why = match got {
            Ok(()) => None,
            Err(Fail::Later(why)) => {
                q.waiting.insert(id.clone());
                Some((why, false))
            }
            Err(Fail::Pause(why)) => {
                q.waiting.insert(id.clone());
                q.paused = true;
                Some((why, false))
            }
            Err(Fail::Never(why)) => Some((why, true)),
        };
        if matches!(why, None | Some((_, true))) && q.dirty.get(&id).is_some_and(|d| d.changes == changes) {
            q.dirty.remove(&id);
            self.save(&q);
        }
        match why {
            None => q.said = None,
            Some((why, never)) if q.said.as_deref() != Some(why.as_str()) => {
                if never {
                    warn!(
                        doc = id,
                        why, "the platform won't take this Doc's copy as it is; it's tried again when it changes"
                    );
                } else {
                    info!(
                        doc = id,
                        why, "this machine's Docs aren't copied up for now; tried again after the next refresh"
                    );
                }
                q.said = Some(why);
            }
            Some(_) => {}
        }
        drop(q);
        self.wake.notify_one();
    }

    /// The Doc as it is now, put, then its pictures the copy hasn't got;
    /// or, for a deleted Doc, its copy taken down.
    async fn copy(&self, id: &str) -> Result<(), Fail> {
        let dir = library::dir(&self.state_dir, id).map_err(Fail::Never)?;
        if is_gone(&self.state_dir, id) {
            return self.take_down(id).await;
        }
        let at = dir.clone();
        let read = crate::task::spawn_blocking(move || whole(&at)).await.map_err(|e| Fail::Later(e.to_string()))?;
        // Gone with no tombstone (an older daemon's delete, or by hand): the
        // copy is left, for its project's people.
        let Some((meta, doc, pictures)) = read? else { return Ok(()) };
        let placed: MachineDocPlaced = self.control.put_signed(&wire::machine_doc(id), &doc).await.map_err(fail)?;
        let mut sent = meta.platform.filter(|p| p.id == placed.id).map(|p| p.assets).unwrap_or_default();
        let mut got = Ok(());
        for a in pictures {
            if sent.get(&a.name) == Some(&a.rev) {
                continue;
            }
            let Ok(bytes) = tokio::fs::read(dir.join(assets::FOLDER).join(&a.name)).await else { continue };
            let path = wire::machine_doc_asset(id, &a.name);
            match self.control.put_signed_bytes::<wire::Asset>(&path, bytes).await {
                Ok(_) => {
                    sent.insert(a.name, a.rev);
                }
                Err(e) => {
                    got = Err(fail(e));
                    break;
                }
            }
        }
        let on = library::OnPlatform {
            project: placed.project,
            id: placed.id,
            link: placed.link,
            rev: placed.rev,
            assets: sent,
        };
        let _ = crate::task::spawn_blocking(move || library::set_platform(&dir, on)).await;
        got
    }
}

impl Copies {
    /// A deleted Doc's copy, taken down; none there is done. A refusal is
    /// tried again after the next refresh, [`GONE_TRIES`] times, then the
    /// copy is left for its project's people; being told to slow down
    /// (429) is tried again too, and never counted.
    async fn take_down(&self, id: &str) -> Result<(), Fail> {
        let got = self.control.delete_signed::<Value>(&wire::machine_doc(id)).await;
        let why = match got {
            Ok(_) => {
                forget_gone(&self.state_dir, id);
                return Ok(());
            }
            Err(e) => match e.downcast_ref::<Answered>() {
                Some(Answered(404, _)) => {
                    forget_gone(&self.state_dir, id);
                    return Ok(());
                }
                // Slow down isn't a refusal: after the next refresh, uncounted.
                Some(Answered(429, why)) => return Err(Fail::Later(why.clone())),
                Some(Answered(400 | 403 | 413, why)) => why.clone(),
                _ => return Err(Fail::Later(e.to_string())),
            },
        };
        if refused_gone(&self.state_dir, id) >= GONE_TRIES {
            forget_gone(&self.state_dir, id);
            warn!(
                doc = id,
                why,
                "the platform won't take a deleted Doc's copy down; the copy stays, for its project's people to delete"
            );
            return Ok(());
        }
        Err(Fail::Later(why))
    }
}

/// The Doc in `dir` as the platform takes it, and its pictures: `None` once
/// it's gone for sure.
type Whole = (library::Meta, MachineDoc, Vec<assets::Asset>);

fn whole(dir: &Path) -> Result<Option<Whole>, Fail> {
    let Some(meta) = library::read_meta_or_gone(dir).map_err(Fail::Later)? else { return Ok(None) };
    // A moved Doc's pointer has nothing to copy (Arugula #863).
    if meta.moved.is_some() {
        return Ok(None);
    }
    let (title, sections) = if dir.join(super::OUTLINE).exists() {
        let loaded = super::load(dir).map_err(Fail::Never)?;
        let sections = loaded
            .entries
            .into_iter()
            .map(|e| MachineSection { id: e.id, heading: e.heading, intent: e.intent, body: e.body })
            .collect();
        (loaded.title, sections)
    } else {
        (meta.title.clone(), Vec::new())
    };
    let pictures = assets::load(&dir.join(assets::FOLDER));
    let layout = (!meta.layout.is_doc()).then(|| meta.layout.as_str().to_owned());
    Ok(Some((meta, MachineDoc { title, sections, layout }, pictures)))
}

/// What control's answer means for the Doc.
fn fail(e: anyhow::Error) -> Fail {
    match e.downcast_ref::<Answered>() {
        Some(Answered(403 | 429, why)) => Fail::Pause(why.clone()),
        Some(Answered(400 | 413, why)) => Fail::Never(why.clone()),
        Some(Answered(_, why)) => Fail::Later(why.clone()),
        None => Fail::Later(e.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use arugula_proto::api::DocLayout;

    use super::*;

    #[test]
    fn the_copy_names_a_layout_only_when_it_isnt_doc() {
        let state = std::env::temp_dir().join(format!(
            "arugula-copies-layout-{}-{}",
            std::process::id(),
            crate::store::now_ms()
        ));
        let meta = library::make(&state, "Plan", "me").unwrap();
        let dir = library::dir(&state, &meta.id).unwrap();
        let body = |dir: &Path| serde_json::to_value(whole(dir).ok().flatten().unwrap().1).unwrap();
        // A control from before layouts sees the body it always did.
        assert!(body(&dir).get("layout").is_none());
        library::set_layout(&dir, DocLayout::Tabs);
        assert_eq!(body(&dir)["layout"], "tabs");
        library::set_layout(&dir, DocLayout::Doc);
        assert!(body(&dir).get("layout").is_none());
        std::fs::remove_dir_all(state).unwrap();
    }
}
