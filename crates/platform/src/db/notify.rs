//! How loud chat is for each person (#113) and their do not disturb
//! (#115): the levels and the schedule, as storage. What they mean for a
//! push is decided in `crate::notify`.

use std::collections::HashMap;

use anyhow::Result;
use arugula_control_wire::chat::{NotifyLevel, QuietHours};

use super::{
    Db,
    chat::{in_json, json_list},
    sql::{Optional as _, Row, Sql, params},
};

/// An account's settings, with the defaults for one that never chose.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Prefs {
    pub channels: NotifyLevel,
    pub dms: NotifyLevel,
    pub threads: NotifyLevel,
    pub snooze_until: Option<u64>,
    pub tz: String,
    pub quiet: Vec<QuietHours>,
    pub all_notices: bool,
}

impl Default for Prefs {
    fn default() -> Self {
        Prefs {
            channels: NotifyLevel::Mentions,
            dms: NotifyLevel::All,
            threads: NotifyLevel::All,
            snooze_until: None,
            tz: "UTC".into(),
            quiet: vec![],
            all_notices: false,
        }
    }
}

fn level(s: &str) -> NotifyLevel {
    match s {
        "all" => NotifyLevel::All,
        "nothing" => NotifyLevel::Nothing,
        _ => NotifyLevel::Mentions,
    }
}

pub fn level_str(l: NotifyLevel) -> &'static str {
    match l {
        NotifyLevel::All => "all",
        NotifyLevel::Mentions => "mentions",
        NotifyLevel::Nothing => "nothing",
    }
}

const COLUMNS: &str = "account, channels, dms, threads, snooze_until, tz, quiet, all_notices";

fn prefs_of(r: &Row) -> Result<(String, Prefs)> {
    let quiet: String = r.get(6)?;
    Ok((
        r.get(0)?,
        Prefs {
            channels: level(&r.get::<String>(1)?),
            dms: level(&r.get::<String>(2)?),
            threads: level(&r.get::<String>(3)?),
            snooze_until: r.get(4)?,
            tz: r.get(5)?,
            // A schedule nobody can read is no schedule: it never silences.
            quiet: serde_json::from_str(&quiet).unwrap_or_default(),
            all_notices: r.get(7)?,
        },
    ))
}

impl Db {
    /// An account's notification settings.
    pub fn notify_prefs(&self, account: &str) -> Result<Prefs> {
        Ok(self
            .c()
            .query_row(&format!("SELECT {COLUMNS} FROM notify_prefs WHERE account = ?1"), params![account], |r| {
                Ok(prefs_of(r)?.1)
            })
            .optional()?
            .unwrap_or_default())
    }

    /// [`notify_prefs`](Self::notify_prefs) for several accounts, in one
    /// statement; those that never chose are left out.
    pub fn notify_prefs_of(&self, accounts: &[String]) -> Result<HashMap<String, Prefs>> {
        if accounts.is_empty() {
            return Ok(Default::default());
        }
        let c = self.c();
        let rows = c.query_map(
            &format!("SELECT {COLUMNS} FROM notify_prefs WHERE {}", in_json(&c, "account", 1)),
            params![json_list(accounts)?],
            prefs_of,
        )?;
        Ok(rows.into_iter().collect())
    }

    pub fn set_notify_prefs(&self, account: &str, p: &Prefs) -> Result<()> {
        self.c().execute(
            "INSERT INTO notify_prefs (account, channels, dms, threads, snooze_until, tz, quiet, all_notices)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
             ON CONFLICT (account) DO UPDATE SET channels = excluded.channels, dms = excluded.dms,
                threads = excluded.threads, snooze_until = excluded.snooze_until, tz = excluded.tz,
                quiet = excluded.quiet, all_notices = excluded.all_notices",
            params![
                account,
                level_str(p.channels),
                level_str(p.dms),
                level_str(p.threads),
                p.snooze_until,
                p.tz.as_str(),
                serde_json::to_string(&p.quiet)?,
                p.all_notices
            ],
        )?;
        Ok(())
    }

    /// An account's own level for a room, if it chose one.
    pub fn chat_level(&self, room: &str, account: &str) -> Result<Option<NotifyLevel>> {
        Ok(self
            .c()
            .query_row("SELECT level FROM chat_reads WHERE room = ?1 AND account = ?2", params![room, account], |r| {
                r.get::<Option<String>>(0)
            })
            .optional()?
            .flatten()
            .map(|l| level(&l)))
    }

    /// The accounts among `accounts` that chose a level for `room`.
    pub fn chat_levels_in(&self, room: &str, accounts: &[String]) -> Result<HashMap<String, NotifyLevel>> {
        if accounts.is_empty() {
            return Ok(Default::default());
        }
        let c = self.c();
        let rows = c.query_map(
            &format!(
                "SELECT account, level FROM chat_reads WHERE room = ?1 AND level IS NOT NULL AND {}",
                in_json(&c, "account", 2)
            ),
            params![room, json_list(accounts)?],
            |r| Ok((r.get::<String>(0)?, level(&r.get::<String>(1)?))),
        )?;
        Ok(rows.into_iter().collect())
    }

    /// Set (or, with `None`, clear) an account's level for a room. `muted`
    /// follows `nothing`, for builds that still read it.
    pub fn set_chat_level(&self, room: &str, account: &str, level: Option<NotifyLevel>) -> Result<()> {
        self.c().execute(
            "INSERT INTO chat_reads (room, account, level, muted) VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT (room, account) DO UPDATE SET level = excluded.level, muted = excluded.muted",
            params![room, account, level.map(level_str), level == Some(NotifyLevel::Nothing)],
        )?;
        Ok(())
    }
}

impl Db {
    /// The levels an account chose, for a page of rooms, in one statement
    /// (the rest chose none).
    pub fn chat_levels_for(&self, account: &str, rooms: &[&str]) -> Result<HashMap<String, NotifyLevel>> {
        if rooms.is_empty() {
            return Ok(Default::default());
        }
        let c = self.c();
        let rows = c.query_map(
            &format!(
                "SELECT room, level FROM chat_reads WHERE account = ?1 AND level IS NOT NULL AND {}",
                in_json(&c, "room", 2)
            ),
            params![account, json_list(rooms)?],
            |r| Ok((r.get::<String>(0)?, level(&r.get::<String>(1)?))),
        )?;
        Ok(rows.into_iter().collect())
    }
}
