//! What the platform keeps of the hub's answers, following the hub's change
//! feed (`HubPort::changes`, M138). Chat asks the hub who is in which team,
//! which machine is whose, who may reach it and which Labs flags are set on
//! every request; this keeps those answers, so a page of rooms costs a few
//! hub calls at most, and none when nothing changed.
//!
//! **How fast a change reaches it** (the delay chat's docs give): at most
//! [`DELAY`], a feed tick plus a cache tick.
//! - [`follow`] reads the feed every [`FEED_TICK`] and drops whatever each
//!   change names (a team's roster or lock, a machine joining, moving,
//!   leaving or being shared, a flag, a name). So a change the hub writes to
//!   the feed usually reaches chat within one feed tick.
//! - Nothing is kept longer than [`CACHE_TICK`]. That bounds what the feed
//!   can't say (a share that runs out at its time) and a feed the platform
//!   couldn't read for a while.
//! - `account.deleted`, a kind this build doesn't know, and a gap (changes
//!   pruned before they were read) drop everything.
//!
//! A lookup that was already asking the hub when something was dropped
//! doesn't keep what it was told (the cache's epoch moved), so a drop is
//! never undone by an answer from before it.

use std::{
    collections::{HashMap, HashSet},
    hash::Hash,
    sync::{
        Arc, Mutex,
        atomic::{AtomicU64, Ordering},
    },
    time::{Duration, Instant},
};

use crate::{
    Platform,
    hub::{Account, Change, ChangeKind, Daemon, Flags, HubPort, HubResult, Team},
};

/// How often [`follow`] reads the hub's change feed.
pub const FEED_TICK: Duration = Duration::from_secs(1);
/// The longest any answer is kept.
pub const CACHE_TICK: Duration = Duration::from_secs(10);
/// The most a change in the hub takes to reach chat: a feed tick plus a
/// cache tick.
pub const DELAY: Duration = Duration::from_secs(FEED_TICK.as_secs() + CACHE_TICK.as_secs());
/// Changes read from the feed at a time.
const PAGE: u32 = 500;

/// Answers by key, each with when it was asked for.
struct Map<K, V>(Mutex<HashMap<K, (V, Instant)>>);

impl<K, V> Default for Map<K, V> {
    fn default() -> Self {
        Map(Mutex::new(HashMap::new()))
    }
}

impl<K: Eq + Hash + Clone, V: Clone> Map<K, V> {
    /// What's kept for each key, fresh enough, and the keys with nothing.
    fn get<'a>(&self, keys: impl IntoIterator<Item = &'a K>, ttl: Duration) -> (Vec<(K, V)>, Vec<K>)
    where
        K: 'a,
    {
        let m = self.0.lock().unwrap();
        let (mut hit, mut miss) = (Vec::new(), Vec::new());
        for k in keys {
            match m.get(k) {
                Some((v, at)) if at.elapsed() < ttl => hit.push((k.clone(), v.clone())),
                _ => miss.push(k.clone()),
            }
        }
        (hit, miss)
    }

    fn put(&self, entries: impl IntoIterator<Item = (K, V)>, at: Instant) {
        let mut m = self.0.lock().unwrap();
        if m.len() > 50_000 {
            m.clear();
        }
        m.extend(entries.into_iter().map(|(k, v)| (k, (v, at))));
    }

    fn forget(&self, k: &K) {
        self.0.lock().unwrap().remove(k);
    }

    fn clear(&self) {
        self.0.lock().unwrap().clear();
    }
}

/// The hub's answers chat asks for, by what they answer.
pub struct Cache {
    ttl: Duration,
    /// Moves on every drop: an answer asked for before it isn't kept.
    epoch: AtomicU64,
    /// The last change read from the feed; `None` before the first read.
    after: tokio::sync::Mutex<Option<u64>>,
    teams: Map<String, Option<Team>>,
    teams_of: Map<String, Vec<String>>,
    daemons: Map<String, Option<Daemon>>,
    accounts: Map<String, Option<Account>>,
    flags: Map<String, Flags>,
    machines_for: Map<String, Vec<String>>,
    /// (account, machine): whether control routes the account there.
    reach: Map<(String, String), bool>,
}

impl Default for Cache {
    fn default() -> Self {
        Cache::new(CACHE_TICK)
    }
}

impl Cache {
    /// A cache that keeps each answer at most `ttl`.
    pub fn new(ttl: Duration) -> Self {
        Cache {
            ttl,
            epoch: AtomicU64::new(0),
            after: tokio::sync::Mutex::new(None),
            teams: Map::default(),
            teams_of: Map::default(),
            daemons: Map::default(),
            accounts: Map::default(),
            flags: Map::default(),
            machines_for: Map::default(),
            reach: Map::default(),
        }
    }

    /// Asks the hub for what `get` didn't have, and keeps the answer unless
    /// something was dropped meanwhile.
    async fn fill<K: Eq + Hash + Clone, V: Clone, F>(
        &self,
        map: &Map<K, V>,
        keys: &[K],
        ask: impl FnOnce(Vec<K>) -> F,
    ) -> HubResult<Vec<(K, V)>>
    where
        F: Future<Output = HubResult<Vec<(K, V)>>>,
    {
        let (mut hit, miss) = map.get(keys, self.ttl);
        if miss.is_empty() {
            return Ok(hit);
        }
        let (epoch, at) = (self.epoch.load(Ordering::SeqCst), Instant::now());
        let got = ask(miss).await?;
        if self.epoch.load(Ordering::SeqCst) == epoch {
            map.put(got.iter().cloned(), at);
        }
        hit.extend(got);
        Ok(hit)
    }

    /// Teams by id, as [`HubPort::teams`]: those that aren't there are left
    /// out.
    pub async fn teams(&self, hub: &dyn HubPort, ids: &[String]) -> HubResult<HashMap<String, Team>> {
        let got = self
            .fill(&self.teams, ids, |miss| async move {
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

    /// The teams an account is in, as [`HubPort::teams_of`].
    pub async fn teams_of(&self, hub: &dyn HubPort, account: &str) -> HubResult<Vec<String>> {
        let got = self
            .fill(&self.teams_of, &[account.to_owned()], |miss| async move {
                let a = miss.into_iter().next().unwrap_or_default();
                let teams = hub.teams_of(&a).await?;
                Ok(vec![(a, teams)])
            })
            .await?;
        Ok(got.into_iter().next().map(|(_, t)| t).unwrap_or_default())
    }

    /// Machines by id, as [`HubPort::daemons`].
    pub async fn daemons(&self, hub: &dyn HubPort, ids: &[String]) -> HubResult<HashMap<String, Daemon>> {
        let got = self
            .fill(&self.daemons, ids, |miss| async move {
                let mut found: HashMap<String, Daemon> =
                    hub.daemons(&miss).await?.into_iter().map(|d| (d.id.clone(), d)).collect();
                Ok(miss.into_iter().map(|id| (id.clone(), found.remove(&id))).collect())
            })
            .await?;
        Ok(got.into_iter().filter_map(|(id, d)| Some((id, d?))).collect())
    }

    /// Accounts by id, as [`HubPort::accounts`].
    pub async fn accounts(&self, hub: &dyn HubPort, ids: &[String]) -> HubResult<HashMap<String, Account>> {
        let got = self
            .fill(&self.accounts, ids, |miss| async move {
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
            .fill(&self.flags, owners, |miss| async move {
                let mut found = hub.labs_flags(&miss).await?;
                Ok(miss.into_iter().map(|o| (o.clone(), found.remove(&o).unwrap_or_default())).collect())
            })
            .await?;
        Ok(got.into_iter().collect())
    }

    /// The machines an account might reach, as [`HubPort::machines_for`].
    pub async fn machines_for(&self, hub: &dyn HubPort, account: &str) -> HubResult<Vec<String>> {
        let got = self
            .fill(&self.machines_for, &[account.to_owned()], |miss| async move {
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
        let keys: Vec<(String, String)> = machines.iter().map(|m| (account.to_owned(), m.clone())).collect();
        let got = self
            .fill(&self.reach, &keys, |miss| async move {
                let asked: Vec<String> = miss.iter().map(|(_, m)| m.clone()).collect();
                let yes: HashSet<String> = hub.may_reach(account, &asked).await?.into_iter().collect();
                Ok(miss.into_iter().map(|k| (k.clone(), yes.contains(&k.1))).collect())
            })
            .await?;
        Ok(got.into_iter().filter(|(_, ok)| *ok).map(|((_, m), _)| m).collect())
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
        self.flags.clear();
        self.machines_for.clear();
        self.reach.clear();
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
                false
            }
            // Chat keeps nothing of a device's.
            DeviceRevoked => false,
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
    pub async fn follow(&self, hub: &dyn HubPort) -> HubResult<bool> {
        let mut after = self.after.lock().await;
        let Some(mut at) = *after else {
            let page = hub.changes(0, 0).await?;
            *after = Some(page.last);
            self.drop_all();
            return Ok(true);
        };
        let mut moved = false;
        loop {
            let page = hub.changes(at, PAGE).await?;
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
/// chat pages re-read their rooms when a change may move them.
pub fn start(app: &Arc<Platform>) {
    let app = Arc::downgrade(app);
    tokio::spawn(async move {
        loop {
            let Some(p) = app.upgrade() else { return };
            if let Err(e) = catch_up(&p).await {
                tracing::warn!(error = %e, "the platform couldn't read the hub's change feed");
            }
            drop(p);
            tokio::time::sleep(FEED_TICK).await;
        }
    });
}

/// Read the feed now, as a tick would.
pub async fn catch_up(app: &Platform) -> HubResult<()> {
    if app.cache.follow(&*app.hub).await? {
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

    #[test]
    fn the_delay_is_a_feed_tick_plus_a_cache_tick() {
        assert_eq!(DELAY, FEED_TICK + CACHE_TICK);
    }
}
