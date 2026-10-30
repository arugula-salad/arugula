//! Chat's people (see migration 0041): the GitHub pictures control serves,
//! and presence's last-seen times. Who may see either is decided in
//! `crate::chat::people`; this is storage.

use super::{
    Db,
    chat::{in_json, json_list},
    sql::{Optional as _, Sql, params},
};
use anyhow::Result;
use std::collections::HashMap;

/// A login's picture as last fetched.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Avatar {
    pub ctype: String,
    /// `None`: GitHub had none (or refused) at `fetched`.
    pub body: Option<Vec<u8>>,
    pub fetched: u64,
}

/// What presence keeps of one account.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Seen {
    pub seen: u64,
    pub active: u64,
    pub mode: String,
    /// Live presence (#341): `off` or `watching`.
    pub live: String,
}

impl Db {
    pub fn chat_avatar(&self, login: &str) -> Result<Option<Avatar>> {
        self.c()
            .query_row("SELECT ctype, body, fetched FROM chat_avatars WHERE login = ?1", params![login], |r| {
                Ok(Avatar { ctype: r.get(0)?, body: r.get(1)?, fetched: r.get(2)? })
            })
            .optional()
    }

    pub fn set_chat_avatar(&self, login: &str, a: &Avatar) -> Result<()> {
        self.c().execute(
            "INSERT INTO chat_avatars (login, ctype, body, fetched) VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT (login) DO UPDATE SET ctype = excluded.ctype, body = excluded.body, fetched = excluded.fetched",
            params![login, a.ctype, a.body, a.fetched],
        )?;
        Ok(())
    }

    /// A heartbeat from one of `account`'s pages at `now`: `active` when the
    /// page had been used lately. A page that says it's going (`gone`) makes
    /// the account unseen at once, unless another page speaks first.
    pub fn chat_beat(&self, account: &str, now: u64, active: bool, gone: bool) -> Result<()> {
        let seen = if gone { 0 } else { now };
        let act = if active && !gone { now } else { 0 };
        self.c().execute(
            "INSERT INTO chat_presence (account, seen, active) VALUES (?1, ?2, ?3)
             ON CONFLICT (account) DO UPDATE SET seen = excluded.seen,
               active = CASE WHEN excluded.active > chat_presence.active THEN excluded.active ELSE chat_presence.active END",
            params![account, seen, act],
        )?;
        Ok(())
    }

    pub fn set_chat_presence_mode(&self, account: &str, mode: &str) -> Result<()> {
        self.c().execute(
            "INSERT INTO chat_presence (account, mode) VALUES (?1, ?2)
             ON CONFLICT (account) DO UPDATE SET mode = excluded.mode",
            params![account, mode],
        )?;
        Ok(())
    }

    /// `account`'s live presence choice (#341): `off` or `watching`.
    pub fn set_chat_live(&self, account: &str, live: &str) -> Result<()> {
        self.c().execute(
            "INSERT INTO chat_presence (account, live) VALUES (?1, ?2)
             ON CONFLICT (account) DO UPDATE SET live = excluded.live",
            params![account, live],
        )?;
        Ok(())
    }

    /// What presence kept of these accounts (those never heard from are
    /// left out).
    pub fn chat_seen(&self, accounts: &[String]) -> Result<HashMap<String, Seen>> {
        if accounts.is_empty() {
            return Ok(HashMap::new());
        }
        let c = self.c();
        let rows = c.query_map(
            &format!("SELECT account, seen, active, mode, live FROM chat_presence WHERE {}", in_json(&c, "account", 1)),
            params![json_list(accounts)?],
            |r| {
                Ok((r.get::<String>(0)?, Seen { seen: r.get(1)?, active: r.get(2)?, mode: r.get(3)?, live: r.get(4)? }))
            },
        )?;
        Ok(rows.into_iter().collect())
    }
}
