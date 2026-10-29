//! What the platform keeps of the hub's answers, following the hub's change
//! feed (`HubPort::changes`, M138). Chat, grants, projects, billing and
//! Cloud ask who is in which team, which machine is whose, who may reach
//! it, which Labs flags are set and what an account used; this keeps those
//! answers, so a request costs a few hub calls at most, and none when
//! nothing changed. Out of process each hub call is a round trip over the
//! private network (between Fly regions in production), so every read of
//! the hub's data goes through here (`hub_calls_tests` holds it to that).
//!
//! **Positive answers come from here; a refusal may ask again.** Where a
//! cached "no" would refuse someone (a machine that isn't the account's, a
//! team that isn't there, a pane they can't see), the caller confirms it
//! with the `*_now` lookups, which ask the hub and keep its answer. A grant
//! is only ever written on the hub's word now, never on a kept answer, so a
//! machine that left isn't given a new one from what was kept before it
//! left.
//!
//! **Read your writes.** Only what's there is kept (a team, a machine, an
//! account, a non-empty list, a "yes"): an absence is asked again every
//! time, so a team made or a machine joined is never hidden by an answer
//! from before. And the hub passes every call on with its newest change
//! ([`SEQ_HEADER`]): when the cache hasn't read that far, the call reads
//! the feed first ([`fresh`]), so a second team or a second machine shows
//! at once too. A roster the hub says changed (`roster_changed`) is
//! dropped there and then.
//!
//! **How long an answer is kept.** Two kinds:
//! - **What the feed covers** (teams and their rosters, which teams an
//!   account is in, machines, accounts and their names, whom a word names,
//!   Labs flags, the machines an account might reach and whether it may):
//!   for as long as the feed is read. Every hub write that changes one of
//!   these writes a change in the same transaction (the outbox,
//!   `arugula-control`'s `db/changes.rs`), so an answer is right until a
//!   change names it. While the last feed read that worked began less than
//!   [`GRACE`] ago, these are kept however old they are.
//! - **What it doesn't** (billing's meters: the feed says nothing of
//!   usage): at most [`CACHE_TICK`]. So is everything else while the feed
//!   isn't being read.
//!
//! Each map holds at most [`CAP`] answers; past that the least recently
//! used quarter goes.
//!
//! **How fast a change reaches it** (the delay chat's docs give): at most
//! [`DELAY`], a feed tick plus a cache tick, and within [`GRACE`] for
//! anything the feed covers while it's read.
//! - [`follow`] reads the feed every [`FEED_TICK`] and drops whatever each
//!   change names (a team's roster or lock, a machine joining, moving,
//!   leaving or being shared, a flag, a name).
//! - A covered answer is only trusted past [`CACHE_TICK`] while a feed read
//!   that began less than [`GRACE`] ago worked: a change made after that
//!   read began is either in the next read or past the grace. If the feed
//!   can't be read for longer than [`GRACE`], everything is dropped, and
//!   until it can, nothing is kept past [`CACHE_TICK`].
//! - `account.deleted`, a kind this build doesn't know, a gap (changes
//!   pruned before they were read) and a feed that went backwards (the
//!   hub's outbox replaced) drop everything.
//!
//! A lookup that was already asking the hub when something was dropped
//! doesn't keep what it was told (the cache's epoch moved, checked under
//! the same lock as the write), so a drop is never undone by an answer from
//! before it.
//!
//! [`follow`]: Cache::follow

use std::{
    collections::{HashMap, HashSet},
    hash::Hash,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    time::{Duration, Instant},
};

use crate::{
    Platform,
    hub::{Account, Change, ChangeKind, Daemon, Flags, HubPort, HubResult, Meter, Team},
};

/// How often [`start`] reads the hub's change feed.
pub const FEED_TICK: Duration = Duration::from_secs(1);
/// The longest an answer the feed doesn't cover is kept, and any answer
/// while the feed isn't being read.
pub const CACHE_TICK: Duration = Duration::from_secs(10);
/// How long after the last feed read that worked began the feed still
/// counts as read: two feed ticks.
pub const GRACE: Duration = Duration::from_secs(2 * FEED_TICK.as_secs());
/// The most a change in the hub takes to reach the platform's answers: a
/// feed tick plus a cache tick.
pub const DELAY: Duration = Duration::from_secs(FEED_TICK.as_secs() + CACHE_TICK.as_secs());
/// The most answers each map keeps.
pub const CAP: usize = 20_000;
/// Changes read from the feed at a time.
const PAGE: u32 = 500;
/// The newest change the hub had written when it passed a call on (its
/// gateway out of process, its gate in process; never a client's). A call
/// that comes after a write of its own, or anyone's, reads the feed first
/// when the cache is behind it ([`fresh`]): the platform reads its writes
/// without waiting for a feed tick.
pub use arugula_platform_api::SEQ_HEADER;

/// Whether the feed covers a map's answers.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Kept {
    /// For as long as the feed is read.
    Feed,
    /// At most the cache's tick.
    Tick,
}

struct Entry<V> {
    value: V,
    /// When it was asked for.
    at: Instant,
    /// When it was last read.
    used: Instant,
}

/// Answers by key.
struct Map<K, V> {
    kept: Kept,
    /// Which answers are kept at all: only what's there (a team, a
    /// machine, a "yes"), never an absence, which a write in the hub may
    /// have just ended (read your writes).
    keep: fn(&V) -> bool,
    cap: usize,
    m: Mutex<HashMap<K, Entry<V>>>,
}

fn any<V>(_: &V) -> bool {
    true
}

fn there<T>(v: &Option<T>) -> bool {
    v.is_some()
}

// A `Map`'s `keep` is `fn(&V)`, and `V` is the `Vec`.
#[allow(clippy::ptr_arg)]
fn some<T>(v: &Vec<T>) -> bool {
    !v.is_empty()
}

fn yes(v: &bool) -> bool {
    *v
}

impl<K: Eq + Hash + Clone, V: Clone> Map<K, V> {
    fn new(kept: Kept, keep: fn(&V) -> bool) -> Self {
        Map { kept, keep, cap: CAP, m: Mutex::new(HashMap::new()) }
    }

    /// What's kept for each key, fresh enough, and the keys with nothing:
    /// fresh is younger than `ttl`, or, for what the feed covers, any age
    /// while `read` (the feed is being read).
    fn get<'a>(
        &self,
        keys: impl IntoIterator<Item = &'a K>,
        now: Instant,
        ttl: Duration,
        read: bool,
    ) -> (Vec<(K, V)>, Vec<K>)
    where
        K: 'a,
    {
        let long = read && self.kept == Kept::Feed;
        let mut m = self.m.lock().unwrap();
        let (mut hit, mut miss) = (Vec::new(), Vec::new());
        for k in keys {
            match m.get_mut(k) {
                Some(e) if long || now.saturating_duration_since(e.at) < ttl => {
                    e.used = now;
                    hit.push((k.clone(), e.value.clone()));
                }
                _ => miss.push(k.clone()),
            }
        }
        (hit, miss)
    }

    /// Keep these, asked for `at`, if `still` holds under the map's lock
    /// (nothing was dropped since they were asked for).
    fn put(&self, entries: impl IntoIterator<Item = (K, V)>, at: Instant, now: Instant, still: impl FnOnce() -> bool) {
        let mut m = self.m.lock().unwrap();
        if !still() {
            return;
        }
        let keep = self.keep;
        m.extend(entries.into_iter().filter(|(_, v)| keep(v)).map(|(k, value)| (k, Entry { value, at, used: now })));
        if m.len() > self.cap {
            // The least recently used quarter goes.
            let keep = self.cap - self.cap / 4;
            let mut used: Vec<Instant> = m.values().map(|e| e.used).collect();
            let cut = m.len() - keep;
            let (_, oldest, _) = used.select_nth_unstable(cut - 1);
            let oldest = *oldest;
            m.retain(|_, e| e.used > oldest);
        }
    }

    fn forget(&self, k: &K) {
        self.m.lock().unwrap().remove(k);
    }

    fn clear(&self) {
        self.m.lock().unwrap().clear();
    }

    #[cfg(test)]
    fn len(&self) -> usize {
        self.m.lock().unwrap().len()
    }
}

/// The hub's answers the platform asks for, by what they answer.
pub struct Cache {
    ttl: Duration,
    /// How far tests and benches moved this cache's clock ([`Cache::advance`]), ms.
    skew: AtomicU64,
    /// Moves on every drop: an answer asked for before it isn't kept.
    epoch: AtomicU64,
    /// The last change read from the feed; `None` before the first read.
    after: tokio::sync::Mutex<Option<u64>>,
    /// The same, plus one, for a look without the lock: 0 before the first
    /// read.
    read_to: AtomicU64,
    /// When the last feed read that worked began; `None` before the first.
    read_from: Mutex<Option<Instant>>,
    /// Everything was dropped for a feed that couldn't be read, and it
    /// hasn't been read since.
    dropped_unread: AtomicBool,
    teams: Map<String, Option<Team>>,
    teams_of: Map<String, Vec<String>>,
    daemons: Map<String, Option<Daemon>>,
    accounts: Map<String, Option<Account>>,
    /// Whom a word names (chat's `from:`), by the word.
    called: Map<String, Vec<Account>>,
    flags: Map<String, Flags>,
    machines_for: Map<String, Vec<String>>,
    /// (account, machine): whether control routes the account there.
    reach: Map<(String, String), bool>,
    /// (account, the month's start): what it used, for billing's page. The
    /// feed says nothing of usage, so this is only ever a cache tick old.
    meters: Map<(String, u64), Meter>,
}

impl Default for Cache {
    fn default() -> Self {
        Cache::new(CACHE_TICK)
    }
}

impl Cache {
    /// A cache that keeps what the feed doesn't cover at most `ttl`.
    pub fn new(ttl: Duration) -> Self {
        Cache {
            ttl,
            skew: AtomicU64::new(0),
            epoch: AtomicU64::new(0),
            after: tokio::sync::Mutex::new(None),
            read_to: AtomicU64::new(0),
            read_from: Mutex::new(None),
            dropped_unread: AtomicBool::new(false),
            teams: Map::new(Kept::Feed, there),
            teams_of: Map::new(Kept::Feed, some),
            daemons: Map::new(Kept::Feed, there),
            accounts: Map::new(Kept::Feed, there),
            called: Map::new(Kept::Feed, some),
            flags: Map::new(Kept::Feed, any),
            machines_for: Map::new(Kept::Feed, some),
            reach: Map::new(Kept::Feed, yes),
            meters: Map::new(Kept::Tick, any),
        }
    }

    /// The cache's clock.
    fn now(&self) -> Instant {
        Instant::now() + Duration::from_millis(self.skew.load(Ordering::SeqCst))
    }

    /// Move this cache's clock on by `by`, as if that long had passed: for
    /// tests and benches, which can't wait for it. Nothing else reads it.
    pub fn advance(&self, by: Duration) {
        self.skew.fetch_add(by.as_millis() as u64, Ordering::SeqCst);
    }

    /// Whether the feed counts as read now: a read that worked began less
    /// than [`GRACE`] ago.
    pub fn feed_read(&self) -> bool {
        let now = self.now();
        self.read_from.lock().unwrap().is_some_and(|t| now.saturating_duration_since(t) < GRACE)
    }

    /// Whether the hub has written a change (`seq`, the newest it has, as
    /// its gateway says in [`SEQ_HEADER`]) this cache hasn't read yet.
    pub fn behind(&self, seq: u64) -> bool {
        seq >= self.read_to.load(Ordering::SeqCst)
    }

    /// A team's roster changed, as the hub just told the platform
    /// (`PlatformPort::roster_changed`): drop what's kept of it now rather
    /// than at the next feed tick. Whether it may change who sees what.
    pub fn team_changed(&self, team: &str) -> bool {
        self.apply(&Change { seq: 0, at: 0, kind: ChangeKind::TeamRoster, subject: team.to_owned() })
    }

    /// Asks the hub for what `get` didn't have (everything, when `now`), and
    /// keeps the answer unless something was dropped meanwhile.
    async fn fill<K: Eq + Hash + Clone, V: Clone, F>(
        &self,
        map: &Map<K, V>,
        keys: &[K],
        now: bool,
        ask: impl FnOnce(Vec<K>) -> F,
    ) -> HubResult<Vec<(K, V)>>
    where
        F: Future<Output = HubResult<Vec<(K, V)>>>,
    {
        let (mut hit, miss) =
            if now { (vec![], keys.to_vec()) } else { map.get(keys, self.now(), self.ttl, self.feed_read()) };
        if miss.is_empty() {
            return Ok(hit);
        }
        let (epoch, at) = (self.epoch.load(Ordering::SeqCst), self.now());
        let got = ask(miss).await?;
        map.put(got.iter().cloned(), at, self.now(), || self.epoch.load(Ordering::SeqCst) == epoch);
        hit.extend(got);
        Ok(hit)
    }

    /// Teams by id, as [`HubPort::teams`]: those that aren't there are left
    /// out.
    pub async fn teams(&self, hub: &dyn HubPort, ids: &[String]) -> HubResult<HashMap<String, Team>> {
        self.teams_asked(hub, ids, false).await
    }

    /// [`Cache::teams`] as the hub says now, kept for next time: to confirm
    /// a refusal.
    pub async fn teams_now(&self, hub: &dyn HubPort, ids: &[String]) -> HubResult<HashMap<String, Team>> {
        self.teams_asked(hub, ids, true).await
    }

    async fn teams_asked(&self, hub: &dyn HubPort, ids: &[String], now: bool) -> HubResult<HashMap<String, Team>> {
        let got = self
            .fill(&self.teams, ids, now, |miss| async move {
                let mut found: HashMap<String, Team> =
                    hub.teams(&miss).await?.into_iter().map(|t| (t.id.clone(), t)).collect();
                Ok(miss.into_iter().map(|id| (id.clone(), found.remove(&id))).collect())
            })
            .await?;
        Ok(got.into_iter().filter_map(|(id, t)| Some((id, t?))).collect())
    }

    /// One team.
    pub async fn team(&self, hub: &dyn HubPort, id: &str) -> HubResult<Option<Team>> {
        Ok(self.teams(hub, &[id.to_owned()]).await?.remove(id))
    }

    /// One team as the hub says now ([`Cache::teams_now`]).
    pub async fn team_now(&self, hub: &dyn HubPort, id: &str) -> HubResult<Option<Team>> {
        Ok(self.teams_now(hub, &[id.to_owned()]).await?.remove(id))
    }

    /// The teams an account is in, as [`HubPort::teams_of`].
    pub async fn teams_of(&self, hub: &dyn HubPort, account: &str) -> HubResult<Vec<String>> {
        let got = self
            .fill(&self.teams_of, &[account.to_owned()], false, |miss| async move {
                let a = miss.into_iter().next().unwrap_or_default();
                let teams = hub.teams_of(&a).await?;
                Ok(vec![(a, teams)])
            })
            .await?;
        Ok(got.into_iter().next().map(|(_, t)| t).unwrap_or_default())
    }

    /// Machines by id, as [`HubPort::daemons`].
    pub async fn daemons(&self, hub: &dyn HubPort, ids: &[String]) -> HubResult<HashMap<String, Daemon>> {
        self.daemons_asked(hub, ids, false).await
    }

    /// [`Cache::daemons`] as the hub says now, kept for next time: to
    /// confirm a refusal, or before writing a grant.
    pub async fn daemons_now(&self, hub: &dyn HubPort, ids: &[String]) -> HubResult<HashMap<String, Daemon>> {
        self.daemons_asked(hub, ids, true).await
    }

    async fn daemons_asked(&self, hub: &dyn HubPort, ids: &[String], now: bool) -> HubResult<HashMap<String, Daemon>> {
        let got = self
            .fill(&self.daemons, ids, now, |miss| async move {
                let mut found: HashMap<String, Daemon> =
                    hub.daemons(&miss).await?.into_iter().map(|d| (d.id.clone(), d)).collect();
                Ok(miss.into_iter().map(|id| (id.clone(), found.remove(&id))).collect())
            })
            .await?;
        Ok(got.into_iter().filter_map(|(id, d)| Some((id, d?))).collect())
    }

    /// Accounts by id, as [`HubPort::accounts`].
    pub async fn accounts(&self, hub: &dyn HubPort, ids: &[String]) -> HubResult<HashMap<String, Account>> {
        self.accounts_asked(hub, ids, false).await
    }

    /// [`Cache::accounts`] as the hub says now, kept for next time: to
    /// confirm a refusal.
    pub async fn accounts_now(&self, hub: &dyn HubPort, ids: &[String]) -> HubResult<HashMap<String, Account>> {
        self.accounts_asked(hub, ids, true).await
    }

    /// Whether the hub has this account: kept if so, asked again if not.
    pub async fn account_there(&self, hub: &dyn HubPort, id: &str) -> HubResult<bool> {
        let ids = [id.to_owned()];
        Ok(self.accounts(hub, &ids).await?.contains_key(id) || self.accounts_now(hub, &ids).await?.contains_key(id))
    }

    /// Whom a word names, as [`HubPort::accounts_called`].
    pub async fn accounts_called(&self, hub: &dyn HubPort, word: &str) -> HubResult<Vec<Account>> {
        let key = word.to_owned();
        let got = self
            .fill(&self.called, &[key], false, |miss| async move {
                let k = miss.into_iter().next().unwrap_or_default();
                let found = hub.accounts_called(word).await?;
                Ok(vec![(k, found)])
            })
            .await?;
        Ok(got.into_iter().next().map(|(_, a)| a).unwrap_or_default())
    }

    async fn accounts_asked(
        &self,
        hub: &dyn HubPort,
        ids: &[String],
        now: bool,
    ) -> HubResult<HashMap<String, Account>> {
        let got = self
            .fill(&self.accounts, ids, now, |miss| async move {
                let mut found: HashMap<String, Account> =
                    hub.accounts(&miss).await?.into_iter().map(|a| (a.id.clone(), a)).collect();
                Ok(miss.into_iter().map(|id| (id.clone(), found.remove(&id))).collect())
            })
            .await?;
        Ok(got.into_iter().filter_map(|(id, a)| Some((id, a?))).collect())
    }

    /// The Labs flags each owner set, as [`HubPort::labs_flags`], with an
    /// owner that set none as no flags.
    pub async fn flags(&self, hub: &dyn HubPort, owners: &[String]) -> HubResult<HashMap<String, Flags>> {
        let got = self
            .fill(&self.flags, owners, false, |miss| async move {
                let mut found = hub.labs_flags(&miss).await?;
                Ok(miss.into_iter().map(|o| (o.clone(), found.remove(&o).unwrap_or_default())).collect())
            })
            .await?;
        Ok(got.into_iter().collect())
    }

    /// The machines an account might reach, as [`HubPort::machines_for`].
    pub async fn machines_for(&self, hub: &dyn HubPort, account: &str) -> HubResult<Vec<String>> {
        let got = self
            .fill(&self.machines_for, &[account.to_owned()], false, |miss| async move {
                let a = miss.into_iter().next().unwrap_or_default();
                let machines = hub.machines_for(&a).await?;
                Ok(vec![(a, machines)])
            })
            .await?;
        Ok(got.into_iter().next().map(|(_, m)| m).unwrap_or_default())
    }

    /// Of these machines, those control routes `account` to, as
    /// [`HubPort::may_reach`]: one hub call for all those not kept.
    pub async fn may_reach(&self, hub: &dyn HubPort, account: &str, machines: &[String]) -> HubResult<HashSet<String>> {
        self.may_reach_asked(hub, account, machines, false).await
    }

    /// [`Cache::may_reach`] as the hub says now, kept for next time: to
    /// confirm a refusal.
    pub async fn may_reach_now(
        &self,
        hub: &dyn HubPort,
        account: &str,
        machines: &[String],
    ) -> HubResult<HashSet<String>> {
        self.may_reach_asked(hub, account, machines, true).await
    }

    async fn may_reach_asked(
        &self,
        hub: &dyn HubPort,
        account: &str,
        machines: &[String],
        now: bool,
    ) -> HubResult<HashSet<String>> {
        let keys: Vec<(String, String)> = machines.iter().map(|m| (account.to_owned(), m.clone())).collect();
        let got = self
            .fill(&self.reach, &keys, now, |miss| async move {
                let asked: Vec<String> = miss.iter().map(|(_, m)| m.clone()).collect();
                let yes: HashSet<String> = hub.may_reach(account, &asked).await?.into_iter().collect();
                Ok(miss.into_iter().map(|k| (k.clone(), yes.contains(&k.1))).collect())
            })
            .await?;
        Ok(got.into_iter().filter(|(_, ok)| *ok).map(|((_, m), _)| m).collect())
    }

    /// What these accounts used this month, as [`HubPort::meters`] with
    /// `since` the month's start: relay bytes and sandbox minutes, each at
    /// most a cache tick old (the feed says nothing of usage). Billing's
    /// page reads it; what billing reports to Stripe asks the hub.
    pub async fn meters(
        &self,
        hub: &dyn HubPort,
        accounts: &[String],
        since: u64,
        until: u64,
    ) -> HubResult<HashMap<String, Meter>> {
        let keys: Vec<(String, u64)> = accounts.iter().map(|a| (a.clone(), since)).collect();
        let got = self
            .fill(&self.meters, &keys, false, |miss| async move {
                let asked: Vec<String> = miss.iter().map(|(a, _)| a.clone()).collect();
                let mut found: HashMap<String, Meter> =
                    hub.meters(&asked, since, until).await?.into_iter().map(|m| (m.account.clone(), m)).collect();
                Ok(miss
                    .into_iter()
                    .map(|k| {
                        let m = found.remove(&k.0).unwrap_or_else(|| Meter {
                            account: k.0.clone(),
                            relay_bytes: 0,
                            sandbox_minutes: 0,
                        });
                        (k, m)
                    })
                    .collect())
            })
            .await?;
        Ok(got.into_iter().map(|((a, _), m)| (a, m)).collect())
    }

    fn dropped(&self) {
        self.epoch.fetch_add(1, Ordering::SeqCst);
    }

    /// Forget everything.
    pub fn drop_all(&self) {
        self.dropped();
        self.teams.clear();
        self.teams_of.clear();
        self.daemons.clear();
        self.accounts.clear();
        self.called.clear();
        self.flags.clear();
        self.machines_for.clear();
        self.reach.clear();
        self.meters.clear();
    }

    /// Forget what a change names. Whether it may change who sees what in
    /// chat (open pages re-read their rooms).
    pub fn apply(&self, c: &Change) -> bool {
        use ChangeKind::*;
        let s = &c.subject;
        match c.kind {
            AccountName => {
                self.dropped();
                self.accounts.forget(s);
                self.called.clear();
                false
            }
            // A device's revocation changes nothing kept here, but a
            // machine's may change whom it's routed to: forget it.
            DeviceRevoked => {
                self.dropped();
                self.daemons.forget(s);
                self.machines_for.clear();
                self.reach.clear();
                true
            }
            TeamRoster => {
                self.dropped();
                self.teams.forget(s);
                self.teams_of.clear();
                self.machines_for.clear();
                self.reach.clear();
                true
            }
            TeamLocked => {
                self.dropped();
                self.teams.forget(s);
                self.reach.clear();
                true
            }
            DaemonJoined | DaemonMoved | DaemonLeft => {
                self.dropped();
                self.daemons.forget(s);
                self.machines_for.clear();
                self.reach.clear();
                true
            }
            DaemonAccess => {
                self.dropped();
                self.machines_for.clear();
                self.reach.clear();
                true
            }
            LabsFlag => {
                self.dropped();
                self.flags.forget(s);
                true
            }
            AccountDeleted | Unknown => {
                self.drop_all();
                true
            }
        }
    }

    /// Read the feed to its end and forget what it names. The first read
    /// starts at the newest change and forgets everything kept before it.
    /// Whether anything read may change who sees what.
    ///
    /// A read that works keeps what the feed covers trusted for [`GRACE`]
    /// from when it began. One that fails once the feed hasn't been read
    /// for [`GRACE`] drops everything.
    pub async fn follow(&self, hub: &dyn HubPort) -> HubResult<bool> {
        self.follow_past(hub, None).await
    }

    /// [`Cache::follow`], unless a read that finished while this one waited
    /// already got past `seq`.
    async fn follow_past(&self, hub: &dyn HubPort, seq: Option<u64>) -> HubResult<bool> {
        let mut after = self.after.lock().await;
        if let (Some(seq), Some(at)) = (seq, *after)
            && at >= seq
        {
            return Ok(false);
        }
        let began = self.now();
        let read = self.read(hub, &mut after).await;
        if let Some(at) = *after {
            self.read_to.store(at + 1, Ordering::SeqCst);
        }
        match read {
            Ok(moved) => {
                let mut from = self.read_from.lock().unwrap();
                *from = Some(from.map_or(began, |t| t.max(began)));
                self.dropped_unread.store(false, Ordering::SeqCst);
                Ok(moved)
            }
            Err(e) => {
                if !self.feed_read() && !self.dropped_unread.swap(true, Ordering::SeqCst) {
                    tracing::warn!("the hub's change feed hasn't been read for {GRACE:?}: dropping what's kept");
                    self.drop_all();
                }
                Err(e)
            }
        }
    }

    async fn read(&self, hub: &dyn HubPort, after: &mut Option<u64>) -> HubResult<bool> {
        let Some(mut at) = *after else {
            let page = hub.changes(0, 0).await?;
            *after = Some(page.last);
            self.drop_all();
            return Ok(true);
        };
        let mut moved = false;
        loop {
            let page = hub.changes(at, PAGE).await?;
            if page.last < at {
                // The feed went backwards (the hub's outbox was replaced):
                // nothing read before means anything now.
                self.drop_all();
                at = page.last;
                moved = true;
                break;
            }
            if page.gap {
                self.drop_all();
                moved = true;
            }
            for c in &page.changes {
                moved |= self.apply(c);
                at = at.max(c.seq);
            }
            if page.changes.len() < PAGE as usize {
                // A gap with nothing after it: on from the newest.
                at = at.max(if page.gap { page.last } else { at });
                break;
            }
        }
        *after = Some(at);
        Ok(moved)
    }
}

/// Follow the hub's feed every [`FEED_TICK`] while the platform runs; open
/// chat pages re-read their rooms when a change may move them. Reads start
/// a tick apart (not a tick after the last one ended), so a read that works
/// lands well within [`GRACE`] of the one before.
pub fn start(app: &Arc<Platform>) {
    let app = Arc::downgrade(app);
    tokio::spawn(async move {
        let mut tick = tokio::time::interval(FEED_TICK);
        tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        loop {
            tick.tick().await;
            let Some(p) = app.upgrade() else { return };
            if let Err(e) = catch_up(&p).await {
                tracing::warn!(error = %e, "the platform couldn't read the hub's change feed");
            }
            drop(p);
        }
    });
}

/// The longest a call waits for [`fresh`]'s read of the feed (a hub call
/// takes milliseconds) before it's answered from what's kept, as when the
/// read fails: a slow hub costs a call this at most, not the internal
/// client's 30 s. The follower reads the feed again within a tick.
pub const FRESH_WAIT: Duration = Duration::from_millis(250);

/// Every platform call: if the hub had written a change the cache hasn't
/// read ([`SEQ_HEADER`]), read the feed before answering. One hub call, and
/// only after something changed; a call with no header (tests) or a cache
/// that's caught up costs nothing. At most [`FRESH_WAIT`]: a read cut short
/// leaves the cache where it was, having only dropped what it read (the
/// next read reads those changes again).
pub async fn fresh(
    axum::extract::State(app): axum::extract::State<Arc<Platform>>,
    req: axum::extract::Request,
    next: axum::middleware::Next,
) -> axum::response::Response {
    let seq = req.headers().get(SEQ_HEADER).and_then(|v| v.to_str().ok()?.parse::<u64>().ok());
    if let Some(seq) = seq
        && app.cache.behind(seq)
    {
        match tokio::time::timeout(FRESH_WAIT, catch_up_past(&app, Some(seq))).await {
            Ok(Ok(())) => {}
            Ok(Err(e)) => tracing::warn!(error = %e, "the platform couldn't read the hub's change feed before a call"),
            Err(_) => tracing::warn!(
                "the hub's change feed took over {FRESH_WAIT:?} to read before a call: answering from what's kept"
            ),
        }
    }
    next.run(req).await
}

/// Read the feed now, as a tick would.
pub async fn catch_up(app: &Platform) -> HubResult<()> {
    catch_up_past(app, None).await
}

async fn catch_up_past(app: &Platform, seq: Option<u64>) -> HubResult<()> {
    if app.cache.follow_past(&*app.hub, seq).await? {
        app.chat.changed();
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chat_tests::TestHub;
    use arugula_e2e::team::TeamRole;

    #[tokio::test]
    async fn what_a_change_names_is_asked_again() {
        let hub = TestHub::default();
        hub.team("t1", &[("a", TeamRole::Owner), ("b", TeamRole::Editor)]);
        let cache = Cache::default();
        cache.follow(&hub).await.unwrap();
        let ask = |id: &'static str| {
            let (cache, hub) = (&cache, &hub);
            async move { cache.team(hub, id).await.unwrap().map(|t| t.members.len()) }
        };
        assert_eq!(ask("t1").await, Some(2));
        let calls = hub.calls();
        assert_eq!(ask("t1").await, Some(2));
        assert_eq!(hub.calls(), calls, "kept");

        hub.team("t1", &[("a", TeamRole::Owner)]);
        assert_eq!(ask("t1").await, Some(2), "until the feed says so");
        assert!(cache.follow(&hub).await.unwrap(), "a roster moves who sees what");
        assert_eq!(ask("t1").await, Some(1));

        // A gap, or a kind it doesn't know: everything goes.
        let calls = hub.calls();
        hub.change(ChangeKind::Unknown, "");
        cache.follow(&hub).await.unwrap();
        assert_eq!(ask("t1").await, Some(1));
        assert_eq!(hub.calls(), calls + 1, "asked again");
    }

    #[tokio::test]
    async fn nothing_is_kept_past_its_tick() {
        let hub = TestHub::default();
        hub.team("t1", &[("a", TeamRole::Owner)]);
        let cache = Cache::new(Duration::from_millis(50));
        cache.team(&hub, "t1").await.unwrap();
        let calls = hub.calls();
        cache.team(&hub, "t1").await.unwrap();
        assert_eq!(hub.calls(), calls);
        tokio::time::sleep(Duration::from_millis(80)).await;
        cache.team(&hub, "t1").await.unwrap();
        assert_eq!(hub.calls(), calls + 1);
    }

    /// `*_now` asks the hub whatever is kept, and keeps what it said.
    #[tokio::test]
    async fn asking_now_asks_and_keeps() {
        let hub = TestHub::default();
        let cache = Cache::default();
        cache.follow(&hub).await.unwrap();
        let ids = ["m1".to_owned()];
        assert!(cache.daemons(&hub, &ids).await.unwrap().is_empty(), "not joined yet");
        hub.daemon("m1", "a", None);
        assert_eq!(cache.daemons(&hub, &ids).await.unwrap()["m1"].account, "a", "an absence isn't kept");
        let calls = hub.calls();
        assert_eq!(cache.daemons_now(&hub, &ids).await.unwrap()["m1"].account, "a");
        assert_eq!(hub.calls(), calls + 1);
        assert_eq!(cache.daemons(&hub, &ids).await.unwrap()["m1"].account, "a", "kept");
        assert_eq!(hub.calls(), calls + 1);
    }

    /// Billing's meters are kept a cache tick (the feed says nothing of
    /// usage), and a machine's revocation drops what's kept of it.
    #[tokio::test]
    async fn meters_are_kept_and_a_revoked_machine_is_dropped() {
        let hub = TestHub::default();
        let cache = Cache::default();
        cache.follow(&hub).await.unwrap();
        let who = ["a".to_owned(), "b".to_owned()];
        let m = cache.meters(&hub, &who, 1, 2).await.unwrap();
        assert_eq!((m.len(), m["a"].relay_bytes), (2, 1024));
        let calls = hub.calls();
        cache.meters(&hub, &who, 1, 3).await.unwrap();
        assert_eq!(hub.calls(), calls, "kept within the month");
        cache.meters(&hub, &who, 9, 10).await.unwrap();
        assert_eq!(hub.calls(), calls + 1, "a new month asks");

        hub.daemon("m1", "a", None);
        cache.follow(&hub).await.unwrap();
        cache.daemons(&hub, &["m1".to_owned()]).await.unwrap();
        let calls = hub.calls();
        hub.change(ChangeKind::DeviceRevoked, "m1");
        assert!(cache.follow(&hub).await.unwrap(), "a machine's revocation may move who sees what");
        cache.daemons(&hub, &["m1".to_owned()]).await.unwrap();
        assert_eq!(hub.calls(), calls + 1, "asked again");
    }

    /// The feed read every tick for `secs` seconds of the cache's clock.
    async fn read_for(cache: &Cache, hub: &TestHub, secs: u64) {
        for _ in 0..secs {
            cache.advance(FEED_TICK);
            cache.follow(hub).await.unwrap();
        }
    }

    /// What the feed covers is kept however old while the feed is read;
    /// what it doesn't (meters) still runs out at the cache's tick.
    #[tokio::test]
    async fn what_the_feed_covers_is_kept_while_it_is_read() {
        let hub = TestHub::default();
        hub.team("t1", &[("a", TeamRole::Owner)]);
        hub.daemon("m1", "a", Some("t1"));
        let cache = Cache::default();
        cache.follow(&hub).await.unwrap();
        let ask = async || {
            cache.team(&hub, "t1").await.unwrap();
            cache.teams_of(&hub, "a").await.unwrap();
            cache.daemons(&hub, &["m1".to_owned()]).await.unwrap();
            cache.accounts(&hub, &["a".to_owned()]).await.unwrap();
            cache.accounts_called(&hub, "a").await.unwrap();
            cache.flags(&hub, &["team:t1".to_owned()]).await.unwrap();
            cache.machines_for(&hub, "a").await.unwrap();
            cache.may_reach(&hub, "a", &["m1".to_owned()]).await.unwrap();
        };
        ask().await;
        cache.meters(&hub, &["a".to_owned()], 1, 2).await.unwrap();
        let calls = hub.calls();
        read_for(&cache, &hub, 30).await;
        ask().await;
        assert_eq!(hub.calls(), calls, "30 s on, the feed read throughout: kept");
        cache.meters(&hub, &["a".to_owned()], 1, 2).await.unwrap();
        assert_eq!(hub.calls(), calls + 1, "meters ran out at the cache's tick");
    }

    /// A feed that can't be read: within the grace, what's kept is kept;
    /// past it, everything goes, and until it's read again nothing is kept
    /// past the cache's tick. A follower that stopped without failing
    /// leaves the same: nothing trusted past the cache's tick.
    #[tokio::test]
    async fn a_feed_that_cant_be_read_drops_everything() {
        let hub = TestHub::default();
        hub.team("t1", &[("a", TeamRole::Owner)]);
        let cache = Cache::default();
        cache.follow(&hub).await.unwrap();
        cache.team(&hub, "t1").await.unwrap();
        read_for(&cache, &hub, 20).await;
        let calls = hub.calls();
        hub.feed_down(true);
        cache.advance(FEED_TICK);
        assert!(cache.follow(&hub).await.is_err());
        cache.team(&hub, "t1").await.unwrap();
        assert_eq!(hub.calls(), calls, "within the grace: kept");
        cache.advance(FEED_TICK);
        assert!(cache.follow(&hub).await.is_err());
        assert!(!cache.feed_read());
        cache.team(&hub, "t1").await.unwrap();
        assert_eq!(hub.calls(), calls + 1, "past the grace: dropped");
        cache.advance(CACHE_TICK - FEED_TICK);
        cache.team(&hub, "t1").await.unwrap();
        assert_eq!(hub.calls(), calls + 1, "kept a cache tick while it's down");
        cache.advance(FEED_TICK);
        cache.team(&hub, "t1").await.unwrap();
        assert_eq!(hub.calls(), calls + 2, "and no longer");

        // Read again: kept for as long as it's read.
        hub.feed_down(false);
        cache.follow(&hub).await.unwrap();
        read_for(&cache, &hub, 30).await;
        cache.team(&hub, "t1").await.unwrap();
        assert_eq!(hub.calls(), calls + 2);
        // A follower that stopped: trusted only a cache tick.
        cache.advance(CACHE_TICK);
        cache.team(&hub, "t1").await.unwrap();
        assert_eq!(hub.calls(), calls + 3);
    }

    /// A gap, or a feed that went backwards (the hub's outbox replaced),
    /// drops everything.
    #[tokio::test]
    async fn a_gap_or_a_feed_gone_backwards_drops_everything() {
        let hub = TestHub::default();
        hub.team("t1", &[("a", TeamRole::Owner)]);
        hub.team("t1", &[("a", TeamRole::Owner)]);
        let cache = Cache::default();
        cache.follow(&hub).await.unwrap();
        cache.team(&hub, "t1").await.unwrap();
        let calls = hub.calls();
        hub.gap();
        assert!(cache.follow(&hub).await.unwrap(), "a gap moves who sees what");
        cache.team(&hub, "t1").await.unwrap();
        assert_eq!(hub.calls(), calls + 1, "asked again");

        let other = TestHub::default();
        other.team("t1", &[("a", TeamRole::Owner)]);
        assert!(cache.follow(&other).await.unwrap(), "back from 2 to 1");
        let calls = other.calls();
        cache.team(&other, "t1").await.unwrap();
        assert_eq!(other.calls(), calls + 1, "asked again");
        other.team("t1", &[("b", TeamRole::Owner)]);
        cache.follow(&other).await.unwrap();
        let t = cache.team(&other, "t1").await.unwrap().unwrap();
        assert_eq!(t.members[0].account, "b", "and followed from where it is now");
    }

    /// A share that ends reaches the hub as `daemon.access` (the machine
    /// sends its new list): the feed carries it, whatever its age.
    #[tokio::test]
    async fn a_share_that_ends_is_dropped_through_the_feed() {
        let hub = TestHub::default();
        hub.daemon("m1", "a", None);
        hub.share("m1", "c", true);
        let cache = Cache::default();
        cache.follow(&hub).await.unwrap();
        let m1 = ["m1".to_owned()];
        assert_eq!(cache.may_reach(&hub, "c", &m1).await.unwrap().len(), 1);
        assert_eq!(cache.machines_for(&hub, "c").await.unwrap(), m1);
        read_for(&cache, &hub, 30).await;
        hub.share("m1", "c", false);
        assert!(cache.follow(&hub).await.unwrap());
        assert!(cache.may_reach(&hub, "c", &m1).await.unwrap().is_empty());
        assert!(cache.machines_for(&hub, "c").await.unwrap().is_empty());
    }

    /// Only what's there is kept: an empty list, a missing id or a "no" is
    /// asked again every time, so a hub write that just ended it (a team
    /// made, a machine joined, a share given) is read at once, feed or no
    /// feed.
    #[tokio::test]
    async fn an_absence_is_never_kept() {
        let hub = TestHub::default();
        let cache = Cache::default();
        cache.follow(&hub).await.unwrap();
        let (m1, a) = (["m1".to_owned()], ["a".to_owned()]);
        let ask = async || {
            (
                cache.teams_of(&hub, "a").await.unwrap().len(),
                cache.team(&hub, "t1").await.unwrap().is_some(),
                cache.machines_for(&hub, "a").await.unwrap().len(),
                cache.may_reach(&hub, "c", &m1).await.unwrap().len(),
                cache.daemons(&hub, &m1).await.unwrap().len(),
                cache.accounts(&hub, &a).await.unwrap().len(),
                cache.accounts_called(&hub, "a").await.unwrap().len(),
            )
        };
        assert_eq!(ask().await, (0, false, 0, 0, 0, 0, 0));
        let calls = hub.calls();
        assert_eq!(ask().await, (0, false, 0, 0, 0, 0, 0));
        assert_eq!(hub.calls(), calls + 7, "every absence asked again");
        // Written in the hub, and not yet read from the feed.
        hub.team("t1", &[("a", TeamRole::Owner)]);
        hub.daemon("m1", "a", Some("t1"));
        hub.share("m1", "c", true);
        assert_eq!(ask().await, (1, true, 1, 1, 1, 1, 1), "read at once");
        let calls = hub.calls();
        assert_eq!(ask().await, (1, true, 1, 1, 1, 1, 1));
        assert_eq!(hub.calls(), calls, "and kept");
    }

    /// The hub telling the platform a roster changed drops the team at
    /// once; and a cache behind the hub's newest change says so.
    #[tokio::test]
    async fn a_roster_the_hub_says_changed_is_dropped_at_once() {
        let hub = TestHub::default();
        hub.team("t1", &[("a", TeamRole::Owner), ("b", TeamRole::Editor)]);
        let cache = Cache::default();
        assert!(cache.behind(0), "nothing read yet");
        cache.follow(&hub).await.unwrap();
        assert!(!cache.behind(1));
        assert_eq!(cache.teams_of(&hub, "b").await.unwrap(), ["t1"]);
        hub.team("t1", &[("a", TeamRole::Owner)]);
        assert!(cache.behind(2), "the hub wrote 2");
        assert_eq!(cache.teams_of(&hub, "b").await.unwrap(), ["t1"], "kept, until");
        assert!(cache.team_changed("t1"));
        assert!(cache.teams_of(&hub, "b").await.unwrap().is_empty());
        assert_eq!(cache.team(&hub, "t1").await.unwrap().unwrap().members.len(), 1);
    }

    /// Each map keeps at most its cap: past it, the least recently used
    /// quarter goes.
    #[test]
    fn a_full_map_forgets_the_least_recently_used() {
        let mut map: Map<u32, u32> = Map::new(Kept::Feed, any);
        map.cap = 8;
        let t0 = Instant::now();
        let at = |s: u64| t0 + Duration::from_secs(s);
        for k in 0..8 {
            map.put([(k, k)], at(0), at(k as u64), || true);
        }
        // 0 and 1 read since: 2, 3 and 4 are the least recently used.
        map.get(&[0, 1], at(10), CACHE_TICK, true);
        map.put([(8, 8)], at(11), at(11), || true);
        assert_eq!(map.len(), 6);
        let (hit, miss) = map.get(&[0, 1, 2, 3, 4, 5, 6, 7, 8], at(12), CACHE_TICK, true);
        assert_eq!(hit.len(), 6);
        assert_eq!(miss, [2, 3, 4]);
    }

    #[test]
    fn the_delay_is_a_feed_tick_plus_a_cache_tick() {
        assert_eq!(DELAY, FEED_TICK + CACHE_TICK);
        assert!(GRACE < CACHE_TICK, "what the feed covers reaches the platform sooner than the rest");
    }
}
