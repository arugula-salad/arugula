//! What the hub keeps as an issuer (M228, `issuer.rs`):
//! - `issuer_clients`: the products that may ask the hub who someone is
//!   (studio, hud boxes, the platform page): an id, a name, and the exact
//!   addresses a sign-in may go back to. Registered by an operator
//!   (`arugula-control issuer client add`); nothing is seeded.
//! - `issuer_tokens`: access tokens handed out, by their hash, with the
//!   hub session each came from: a token ends when it runs out or when that
//!   session ends, whichever is first, as the platform page's sessions do.

use rusqlite::{Connection, OptionalExtension, params};

use super::Db;

pub(super) const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS issuer_clients (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL,
    redirect_uris TEXT NOT NULL,
    created INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS issuer_tokens (
    token_hash TEXT PRIMARY KEY,
    account TEXT NOT NULL,
    client TEXT NOT NULL,
    hub_session TEXT NOT NULL,
    created INTEGER NOT NULL,
    expires INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS issuer_tokens_hub_session ON issuer_tokens (hub_session);
";

/// A registered client.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Client {
    pub id: String,
    pub name: String,
    /// Where a sign-in may go back to, compared whole.
    pub redirect_uris: Vec<String>,
    pub created: u64,
}

/// A client id: lowercase letters, digits, `.`, `_` and `-`, 1 to 64, as a
/// hud box's `HUD_FORGE_SIGNIN_AUDIENCE` can be.
pub fn client_id_ok(id: &str) -> bool {
    (1..=64).contains(&id.len())
        && id.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b".-_".contains(&b))
        && id.bytes().next().is_some_and(|b| b.is_ascii_alphanumeric())
}

/// A redirect address a client may register: an absolute https URL with
/// no fragment, or http on this machine (loopback, for development).
pub fn redirect_ok(uri: &str) -> bool {
    let Ok(u) = url::Url::parse(uri) else { return false };
    let loopback = matches!(u.host(), Some(url::Host::Domain("localhost")))
        || matches!(u.host(), Some(url::Host::Ipv4(a)) if a.is_loopback())
        || matches!(u.host(), Some(url::Host::Ipv6(a)) if a.is_loopback());
    u.fragment().is_none()
        && u.username().is_empty()
        && u.password().is_none()
        && u.host().is_some()
        && (u.scheme() == "https" || (u.scheme() == "http" && loopback))
}

fn row(r: &rusqlite::Row<'_>) -> rusqlite::Result<Client> {
    let uris: String = r.get(2)?;
    Ok(Client {
        id: r.get(0)?,
        name: r.get(1)?,
        redirect_uris: serde_json::from_str(&uris).unwrap_or_default(),
        created: r.get(3)?,
    })
}

/// Register a client, or change its name and addresses.
pub fn put_client(c: &Connection, client: &Client) -> anyhow::Result<()> {
    anyhow::ensure!(
        client_id_ok(&client.id),
        "a client id is 1 to 64 of a-z 0-9 . _ -, starting with a letter or digit"
    );
    anyhow::ensure!(!client.name.trim().is_empty(), "a client needs a name");
    anyhow::ensure!(!client.redirect_uris.is_empty(), "a client needs at least one redirect address");
    for u in &client.redirect_uris {
        anyhow::ensure!(redirect_ok(u), "{u}: a redirect address is https (or http on loopback), with no fragment");
    }
    c.execute_batch(SCHEMA)?;
    c.execute(
        "INSERT INTO issuer_clients (id, name, redirect_uris, created) VALUES (?1, ?2, ?3, ?4)
         ON CONFLICT (id) DO UPDATE SET name = ?2, redirect_uris = ?3",
        params![client.id, client.name.trim(), serde_json::to_string(&client.redirect_uris)?, client.created],
    )?;
    Ok(())
}

pub fn clients(c: &Connection) -> anyhow::Result<Vec<Client>> {
    c.execute_batch(SCHEMA)?;
    let mut q = c.prepare("SELECT id, name, redirect_uris, created FROM issuer_clients ORDER BY id")?;
    Ok(q.query_map([], row)?.collect::<Result<_, _>>()?)
}

/// Take a client out, and the access tokens it holds. Whether it was
/// there.
pub fn remove_client(c: &Connection, id: &str) -> anyhow::Result<bool> {
    c.execute_batch(SCHEMA)?;
    c.execute("DELETE FROM issuer_tokens WHERE client = ?1", params![id])?;
    Ok(c.execute("DELETE FROM issuer_clients WHERE id = ?1", params![id])? > 0)
}

impl Db {
    pub fn issuer_client(&self, id: &str) -> anyhow::Result<Option<Client>> {
        Ok(self
            .c()
            .query_row("SELECT id, name, redirect_uris, created FROM issuer_clients WHERE id = ?1", params![id], row)
            .optional()?)
    }

    pub fn put_issuer_client(&self, client: &Client) -> anyhow::Result<()> {
        put_client(&self.c(), client)
    }

    /// An access token for `account`'s `client`, made from the hub session
    /// `hub_session` (its token's hash).
    pub fn add_issuer_token(
        &self,
        token_hash: &str,
        account: &str,
        client: &str,
        hub_session: &str,
        now: u64,
        expires: u64,
    ) -> anyhow::Result<()> {
        let c = self.c();
        c.execute("DELETE FROM issuer_tokens WHERE expires <= ?1", params![now])?;
        c.execute(
            "INSERT INTO issuer_tokens (token_hash, account, client, hub_session, created, expires)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![token_hash, account, client, hub_session, now, expires],
        )?;
        Ok(())
    }

    /// The account and client a live access token is for: it, the hub
    /// session it came from and its client all still there, and the
    /// account too.
    pub fn issuer_token(&self, token_hash: &str, now: u64) -> anyhow::Result<Option<(String, String)>> {
        Ok(self
            .c()
            .query_row(
                "SELECT t.account, t.client FROM issuer_tokens t
                 JOIN sessions s ON s.token_hash = t.hub_session AND s.account = t.account
                 JOIN accounts a ON a.id = t.account
                 JOIN issuer_clients c ON c.id = t.client
                 WHERE t.token_hash = ?1 AND t.expires > ?2 AND s.expires > ?2",
                params![token_hash, now],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()?)
    }
}
