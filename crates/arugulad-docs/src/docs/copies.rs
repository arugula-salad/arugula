//! A machine's Docs copied up to the platform (#418): while this machine is
//! in control, each Doc in the library has a copy there
//! (`PUT /api/daemon/docs/{id}`, [`wire::MACHINE_DOC`]), which the platform
//! makes in the owner's personal project, private to them until they move
//! it into a team's. It keys the copy by the machine and the Doc's id, so
//! the same Doc always lands on the same copy, wherever it was moved.
//!
//! - **When:** a change ([`changed`]: a new Doc, its outline, a fill, a
//!   picture) marks the Doc dirty, and it's put [`QUIET`] after its last
//!   change, or [`MOST`] after its first while it keeps changing: a fill of
//!   thirty sections is a few puts, not thirty. One put per Doc at a time,
//!   and [`FLYING`] in all; its pictures go after it, those whose `rev` the
//!   copy hasn't got.
//! - **Not now:** out of control or out of reach, refused (chat off for the
//!   account, or the owner may no longer write where the Doc now is) or told
//!   to slow down (429: nothing more until then), a Doc stays dirty and
//!   waits for the next refresh with control. Why is said once, at info.
//!   The dirty Docs are kept in `docs/.copies.json`, so a restart has them,
//!   and they wait for its first refresh.
//! - **Once:** after the first refresh of a machine in control (one that
//!   joins, or was in before it copied), every Doc in the library is marked
//!   dirty; `docs/.copied-1` says it's done.
//! - **Where it is:** the answer ([`wire::MachineDocPlaced`]) goes in the
//!   Doc's `meta.json` as `platform` ([`library::OnPlatform`]), with the
//!   pictures put. Nothing shows it yet.
//!
//! A Doc the platform can't take as it is (a 400 or 413: the platform's
//! checks are the block's, so it shouldn't happen) is left until it changes
//! again. Deleting a Doc here leaves its copy: it may be a team's by then.

use std::{
    collections::{BTreeMap, BTreeSet},
    path::{Path, PathBuf},
    sync::{Arc, Mutex, OnceLock},
    time::{Duration, Instant},
};

use arugula_control_wire::project_docs::{self as wire, MachineDoc, MachineDocPlaced, MachineSection};
use tokio::sync::Notify;
use tracing::{info, warn};

use super::{assets, library};
use crate::{
    control::{Answered, Control},
    store::{now_ms, private_dir, write_atomic},
};

/// How long a Doc is left after its last change before it's put...
pub const QUIET: Duration = Duration::from_secs(2);
/// ...and after its first, while it keeps changing.
pub const MOST: Duration = Duration::from_secs(10);
/// Puts at a time, of different Docs: the first copy of a big library
/// goes a few at a time, not all at once.
const FLYING: usize = 4;

/// In the library: the Docs not copied since they changed.
const DIRTY: &str = ".copies.json";
/// In the library: every Doc there was queued once.
const DONE: &str = ".copied-1";

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
    /// Slow down (429): nothing more until the next refresh.
    Slow(String),
    /// Not as it is: until it changes again.
    Never(String),
}

/// Start copying (the daemon's control is made): what was dirty when it
/// stopped waits for the first refresh.
pub fn start(control: Arc<Control>) {
    let state_dir = control.state_dir().to_owned();
    let now = Instant::now();
    let kept = read_dirty(&state_dir);
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

fn read_dirty(state_dir: &Path) -> Vec<String> {
    let file = library::root(state_dir).join(DIRTY);
    let ids: Vec<String> = std::fs::read(file).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default();
    ids.into_iter().filter(|id| library::is_id(id)).collect()
}

impl Copies {
    fn mark(&self, ids: impl IntoIterator<Item = String>) {
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
            if self.control.enrolled().is_some() {
                self.copy_all();
            }
            let mut q = self.queue.lock().unwrap();
            q.waiting.clear();
            q.paused = false;
            drop(q);
            self.wake.notify_one();
        }
    }

    /// Every Doc in the library, once.
    fn copy_all(&self) {
        let done = library::root(&self.state_dir).join(DONE);
        if done.exists() {
            return;
        }
        let ids: Vec<String> = library::list(&self.state_dir).into_iter().map(|m| m.id).collect();
        let n = ids.len();
        // Kept dirty on disk before the marker says so.
        self.mark(ids);
        if let Err(e) =
            private_dir(done.parent().unwrap()).and_then(|()| write_atomic(&done, now_ms().to_string().as_bytes()))
        {
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
        let now = Instant::now();
        let mut q = self.queue.lock().unwrap();
        if q.paused {
            return (Vec::new(), None);
        }
        let Queue { dirty, flying, waiting, .. } = &mut *q;
        let (mut due, mut next) = (Vec::new(), None::<Duration>);
        for (id, d) in dirty.iter_mut().filter(|(id, _)| !flying.contains(*id) && !waiting.contains(*id)) {
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
            Err(Fail::Slow(why)) => {
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

    /// The Doc as it is now, put, then its pictures the copy hasn't got.
    async fn copy(&self, id: &str) -> Result<(), Fail> {
        let dir = library::dir(&self.state_dir, id).map_err(Fail::Never)?;
        let at = dir.clone();
        let read = crate::task::spawn_blocking(move || whole(&at)).await.map_err(|e| Fail::Later(e.to_string()))?;
        // Gone: deleted since it changed.
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

/// The Doc in `dir` as the platform takes it, and its pictures: `None` once
/// it's gone.
type Whole = (library::Meta, MachineDoc, Vec<assets::Asset>);

fn whole(dir: &Path) -> Result<Option<Whole>, Fail> {
    let Some(meta) = library::read_meta(dir) else { return Ok(None) };
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
    Ok(Some((meta, MachineDoc { title, sections }, pictures)))
}

/// What control's answer means for the Doc.
fn fail(e: anyhow::Error) -> Fail {
    match e.downcast_ref::<Answered>() {
        Some(Answered(429, why)) => Fail::Slow(why.clone()),
        Some(Answered(400 | 413, why)) => Fail::Never(why.clone()),
        Some(Answered(_, why)) => Fail::Later(why.clone()),
        None => Fail::Later(e.to_string()),
    }
}
