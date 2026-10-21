//! Projects, each team's roster projection and grants, as the platform keeps
//! them (platform step 2, #1150): the tables are in [`db::projects`], and
//! what they need of the hub (that an account or a team is there, a team's
//! name and members, whose a machine is) comes through `HubPort`.
//!
//! **Following the hub.** [`follow`] applies the hub's change feed
//! (`HubPort::changes`) to them, every [`FEED_TICK`] while the platform
//! runs ([`start`]), from where it last got to (`hub_feed`, so a restart
//! misses nothing the feed still has):
//! - `team.roster`: the team's `team_members` are rewritten from
//!   `HubPort::teams`, and it gets its default project if it has none.
//! - `daemon.joined`: the machine gets `cloud.drive` over its account, unless
//!   it has had one there before ([`crate::grants`]).
//! - `daemon.left`: its grants from before it left go, so a machine that
//!   joins again starts with a new one.
//! - A gap (changes pruned before the platform read them), a kind it doesn't
//!   know, and its first read ever: [`reconcile`], which asks the hub about
//!   every team and machine the platform holds something of. The first read
//!   ever (no `hub_feed` row yet) only logs the grants it would drop and
//!   keeps them ([`Reconcile::Report`]): grants are dropped this way only
//!   from the second start on.
//!
//! An account's deletion reaches these tables through
//! `PlatformPort::account_deleted`, not the feed (its change names nobody).
//!
//! The feed is never the only way in: a machine that just joined is never
//! refused for want of a grant the feed hasn't brought yet
//! ([`crate::grants::can_drive_cloud`]), and a project is made the first
//! time it's asked for ([`personal_project`], [`team_project`]).
//!
//! [`db::projects`]: crate::db::projects

use std::{sync::Arc, time::Duration};

use arugula_e2e::now_ms;

use crate::{
    Platform,
    hub::{Change, ChangeKind, HubResult},
};

/// How often [`start`] reads the hub's change feed: chat's tick.
pub const FEED_TICK: Duration = crate::cache::FEED_TICK;
/// Changes read from the feed at a time.
const PAGE: u32 = 500;

/// The account's personal project, which its Cloud workspaces and cloud
/// secrets belong to (M129): made if it has none yet, the same one every
/// time after. `None` when the hub has no such account, which is how Cloud
/// tells an account that's gone (M137).
pub async fn personal_project(app: &Platform, account: &str) -> HubResult<Option<String>> {
    if app.hub.accounts(&[account.to_owned()]).await?.is_empty() {
        return Ok(None);
    }
    Ok(Some(app.db.personal_project(account, now_ms())?))
}

/// The team's default project (M129), which its chat channels belong to:
/// made if it has none yet (named after the team), the same one every time
/// after. `None` when the hub has no such team (M138).
pub async fn team_project(app: &Platform, team: &str) -> HubResult<Option<String>> {
    let Some(t) = app.hub.teams(&[team.to_owned()]).await?.pop() else { return Ok(None) };
    Ok(Some(app.db.team_project(team, &t.name, now_ms())?))
}

/// Apply the hub's change feed from where this last got to, to its end.
/// The first read ever starts at the newest change and reconciles without
/// dropping a grant ([`Reconcile::Report`]). Whether it applied anything.
pub async fn follow(app: &Platform) -> HubResult<bool> {
    // One reader at a time: the tick, or a check catching up.
    let _one = app.feed.lock().await;
    let Some(mut at) = app.db.feed_after()? else {
        let last = app.hub.changes(0, 0).await?.last;
        reconcile(app, Reconcile::Report).await?;
        app.db.set_feed_after(last)?;
        return Ok(true);
    };
    let mut applied = false;
    loop {
        let page = app.hub.changes(at, PAGE).await?;
        if page.gap {
            reconcile(app, Reconcile::Drop).await?;
            applied = true;
        }
        for c in &page.changes {
            apply(app, c).await?;
            at = at.max(c.seq);
            applied = true;
        }
        if page.changes.len() < PAGE as usize {
            // A gap with nothing after it: on from the newest.
            at = at.max(if page.gap { page.last } else { at });
            break;
        }
    }
    app.db.set_feed_after(at)?;
    Ok(applied)
}

/// One change, applied to projects, team members and grants.
async fn apply(app: &Platform, c: &Change) -> HubResult<()> {
    use ChangeKind::*;
    match c.kind {
        TeamRoster => {
            if let Some(t) = app.hub.teams(std::slice::from_ref(&c.subject)).await?.pop() {
                app.db.project_roster(&t, now_ms())?;
            }
        }
        DaemonJoined => {
            if let Some(d) = app.hub.daemons(std::slice::from_ref(&c.subject)).await?.pop() {
                app.db.grant_on_join(&d.id, &d.account, now_ms())?;
            }
        }
        DaemonLeft => app.db.machine_left(&c.subject, c.at)?,
        Unknown => reconcile(app, Reconcile::Drop).await?,
        // Nothing here follows a name, a revoked device (the hub refuses
        // its calls), a lock, a move (a machine's grant is over its owner's
        // account, wherever it is), a share or a flag. An account's deletion
        // comes through `PlatformPort::account_deleted`.
        AccountName | AccountDeleted | DeviceRevoked | TeamLocked | DaemonMoved | DaemonAccess | LabsFlag => {}
    }
    Ok(())
}

/// What [`reconcile`] does with a grant of a machine that isn't its
/// account's any more.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reconcile {
    /// Log it (account, machine, grant) at warn and keep it: the follower's
    /// very first start, before it has read the feed even once, so a
    /// mistake in the first re-check can't revoke anyone.
    Report,
    /// Drop it: a gap, or a kind this build doesn't know, from the second
    /// start on.
    Drop,
}

/// What the feed may have missed: every team the platform holds something
/// of gets its projection again (one that's gone is left to its account's
/// deletion), and grants of a machine that isn't its account's any more go
/// (or, with [`Reconcile::Report`], are logged and kept). A machine that
/// left and joined again while the feed was missed keeps a revoked grant;
/// its owner can give it back.
pub async fn reconcile(app: &Platform, grants: Reconcile) -> HubResult<()> {
    let teams = app.db.known_teams()?;
    for chunk in teams.chunks(PAGE as usize) {
        for t in app.hub.teams(chunk).await? {
            app.db.project_roster(&t, now_ms())?;
        }
    }
    let granted = app.db.granted_machines()?;
    for chunk in granted.chunks(PAGE as usize) {
        let ids: Vec<String> = chunk.iter().map(|(d, _)| d.clone()).collect();
        let joined = app.hub.daemons(&ids).await?;
        for (daemon, account) in chunk {
            if joined.iter().any(|d| &d.id == daemon && &d.account == account) {
                continue;
            }
            match grants {
                Reconcile::Drop => app.db.drop_machine_grants(daemon, account)?,
                Reconcile::Report => {
                    for g in app.db.grants(account)?.iter().filter(|g| &g.subject_id == daemon) {
                        tracing::warn!(
                            %account, %daemon, grant = %g.id,
                            "the first re-check of grants would drop this grant (its machine isn't the account's); kept"
                        );
                    }
                }
            }
        }
    }
    Ok(())
}

/// Team channels from before M129 get their team's default project.
async fn attach_team_channels(app: &Platform) -> HubResult<()> {
    for team in app.db.unattached_team_channels()? {
        if let Some(project) = team_project(app, &team).await? {
            app.db.attach_team_channels(&team, &project)?;
        }
    }
    Ok(())
}

/// Follow the hub's feed every [`FEED_TICK`] while the platform runs, after
/// attaching any team channels from before M129.
pub fn start(app: &Arc<Platform>) {
    let app = Arc::downgrade(app);
    tokio::spawn(async move {
        if let Some(p) = app.upgrade()
            && let Err(e) = attach_team_channels(&p).await
        {
            tracing::warn!(error = %e, "the platform couldn't give team channels their projects");
        }
        loop {
            let Some(p) = app.upgrade() else { return };
            if let Err(e) = follow(&p).await {
                tracing::warn!(error = %e, "the platform couldn't apply the hub's change feed to projects and grants");
            }
            drop(p);
            tokio::time::sleep(FEED_TICK).await;
        }
    });
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use arugula_e2e::team::TeamRole;

    use super::*;
    use crate::chat_tests::TestHub;

    /// The platform over a hub of the test's own.
    fn app(hub: &Arc<TestHub>) -> Arc<Platform> {
        let cfg = crate::Config { public_url: "https://control.example".into(), ..Default::default() };
        Arc::new(Platform::new(cfg, crate::db::Db::memory(), hub.clone(), reqwest::Client::new(), Default::default()))
    }

    fn members(app: &Platform, team: &str) -> Vec<String> {
        let c = app.db.c();
        let mut q = c
            .prepare("SELECT account_id || ' ' || role || ' ' || roster_version FROM team_members WHERE team_id = ?1 ORDER BY 1")
            .unwrap();
        q.query_map([team], |r| r.get(0)).unwrap().map(Result::unwrap).collect()
    }

    /// A roster the feed brings rewrites the team's projection and gives
    /// the team its default project; a project is made the first time it's
    /// asked for, and only for what the hub has.
    #[tokio::test]
    async fn the_feed_keeps_team_members_and_default_projects() {
        let hub = Arc::new(TestHub::default());
        let app = app(&hub);
        assert!(follow(&app).await.unwrap(), "the first read starts at the newest");
        hub.team("t1", &[("alice", TeamRole::Owner), ("bob", TeamRole::Viewer)]);
        assert!(members(&app, "t1").is_empty(), "until the feed is read");
        follow(&app).await.unwrap();
        assert_eq!(members(&app, "t1"), ["alice owner 1", "bob viewer 1"]);
        let default = app.db.team_default_project("t1").unwrap().expect("made with the team");
        assert_eq!(team_project(&app, "t1").await.unwrap(), Some(default.clone()));

        hub.team("t1", &[("bob", TeamRole::Owner)]);
        follow(&app).await.unwrap();
        assert_eq!(members(&app, "t1"), ["bob owner 2"]);
        assert_eq!(app.db.team_default_project("t1").unwrap(), Some(default), "the same project");
        assert!(!follow(&app).await.unwrap(), "nothing new");

        assert_eq!(team_project(&app, "t-none").await.unwrap(), None, "no such team");
        let personal = personal_project(&app, "alice").await.unwrap().expect("an account the hub has");
        assert_eq!(personal_project(&app, "alice").await.unwrap(), Some(personal));
        assert_eq!(personal_project(&app, "nobody").await.unwrap(), None, "no such account");
    }

    /// A machine gets its grant when the feed says it joined, and loses it
    /// when the feed says it left; a reconcile drops grants of machines
    /// that aren't their account's any more.
    #[tokio::test]
    async fn the_feed_gives_and_takes_grants() {
        let hub = Arc::new(TestHub::default());
        let app = app(&hub);
        follow(&app).await.unwrap();
        hub.daemon("geek", "alice", None);
        follow(&app).await.unwrap();
        assert_eq!(app.db.cloud_drive("geek", "alice").unwrap(), Some(true));
        hub.leave("geek");
        follow(&app).await.unwrap();
        assert_eq!(app.db.cloud_drive("geek", "alice").unwrap(), None);

        // A grant the feed missed the end of.
        app.db.grant_on_join("gone", "alice", 1).unwrap();
        hub.daemon("box", "bob", None);
        app.db.grant_on_join("box", "bob", 1).unwrap();
        reconcile(&app, Reconcile::Drop).await.unwrap();
        assert_eq!(app.db.cloud_drive("gone", "alice").unwrap(), None, "not a joined machine");
        assert_eq!(app.db.cloud_drive("box", "bob").unwrap(), Some(true));
    }

    /// The follower's very first start (no `hub_feed` row) re-checks every
    /// team and machine but drops no grant, even one whose machine isn't
    /// joined: it's logged and kept. From the second start on, a gap's
    /// re-check drops it.
    #[tokio::test]
    async fn the_first_start_keeps_every_grant() {
        let hub = Arc::new(TestHub::default());
        hub.team("t1", &[("alice", TeamRole::Owner)]);
        hub.daemon("box", "alice", None);
        let app = app(&hub);
        app.db.grant_on_join("box", "alice", 1).unwrap();
        app.db.grant_on_join("gone", "alice", 1).unwrap();
        app.db.project_roster(&app.hub.teams(&["t1".into()]).await.unwrap()[0], 1).unwrap();
        hub.team("t1", &[("alice", TeamRole::Owner), ("bob", TeamRole::Editor)]);
        assert_eq!(app.db.feed_after().unwrap(), None, "never started");

        follow(&app).await.unwrap();
        assert!(app.db.feed_after().unwrap().is_some());
        assert_eq!(app.db.cloud_drive("gone", "alice").unwrap(), Some(true), "logged and kept");
        assert_eq!(app.db.cloud_drive("box", "alice").unwrap(), Some(true));
        assert_eq!(members(&app, "t1"), ["alice owner 2", "bob editor 2"], "the teams were re-checked");

        // From the second start on, a re-check drops it.
        reconcile(&app, Reconcile::Drop).await.unwrap();
        assert_eq!(app.db.cloud_drive("gone", "alice").unwrap(), None);
        assert_eq!(app.db.cloud_drive("box", "alice").unwrap(), Some(true));
    }

    /// Team channels from a database before M129 get their team's default
    /// project once the platform is running.
    #[tokio::test]
    async fn team_channels_from_before_get_their_project() {
        let hub = Arc::new(TestHub::default());
        hub.team("t1", &[("alice", TeamRole::Owner)]);
        let app = app(&hub);
        let room = |id: &str, scope: &str, kind: &str| crate::db::chat::Room {
            id: id.into(),
            scope: scope.into(),
            kind: kind.into(),
            name: id.into(),
            machine: None,
            target: None,
            created_by: "alice".into(),
            created: 0,
            archived: None,
            last_seq: 0,
        };
        app.db.add_chat_room(&room("c1", "team:t1", "channel"), &[], None).unwrap();
        app.db.add_chat_room(&room("c2", "team:t-gone", "channel"), &[], None).unwrap();
        app.db.add_chat_room(&room("d1", "team:t1", "dm"), &["alice", "bob"], None).unwrap();
        assert_eq!(app.db.unattached_team_channels().unwrap(), ["t-gone", "t1"]);
        attach_team_channels(&app).await.unwrap();
        let project = app.db.team_default_project("t1").unwrap().unwrap();
        let of = |id: &str| -> Option<String> {
            app.db.c().query_row("SELECT project_id FROM chat_rooms WHERE id = ?1", [id], |r| r.get(0)).unwrap()
        };
        assert_eq!(
            (of("c1"), of("c2"), of("d1")),
            (Some(project), None, None),
            "a team that's gone, and a DM, get none"
        );
    }
}
