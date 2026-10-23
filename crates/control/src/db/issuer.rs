//! What the hub keeps as an issuer (M228, `issuer.rs`):
//! - `issuer_clients`: the products that may ask the hub who someone is:
//!   an id, a kind (`oidc`: a product that signs people in with
//!   `/oauth/authorize`, such as studio; `hud`: a hud box taking
//!   `/oauth/hud`'s assertions), a name, and the exact addresses a sign-in
//!   may go back to. Registered by an operator (`arugula-control issuer
//!   add-client`); nothing is seeded.
//! - `issuer_tokens`: access tokens handed out, by their hash, with the
//!   client each was issued to and the hub session each came from: a token
//!   ends when it runs out or when that session ends, whichever is first,
//!   as the platform page's sessions do.

use rusqlite::{Connection, OptionalExtension, params};

use super::Db;

pub(super) const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS issuer_clients (
    id TEXT PRIMARY KEY,
    kind TEXT NOT NULL,
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

/// Which flow a client takes. Each refuses the other kind, so a hud box's
/// address never receives a code and a product's never an assertion.
#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
pub enum ClientKind {
    /// A product signing people in: `/oauth/authorize`, `/oauth/token`.
    Oidc,
    /// A hud box: `/oauth/hud`.
    Hud,
}

impl ClientKind {
    pub fn as_str(self) -> &'static str {
        match self {
            ClientKind::Oidc => "oidc",
            ClientKind::Hud => "hud",
        }
    }

    fn parse(s: &str) -> Option<Self> {
        match s {
            "oidc" => Some(ClientKind::Oidc),
            "hud" => Some(ClientKind::Hud),
            _ => None,
        }
    }
}

/// A registered client.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Client {
    pub id: String,
    pub kind: ClientKind,
    pub name: String,
    /// Where a sign-in may go back to, compared whole with what a client
    /// sends, as registered (normalized).
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

/// A redirect address a client may register, normalized as a URL parser
/// writes it: an absolute https URL with no fragment and no user, or http
/// on this machine (loopback, for development).
pub fn redirect_normalized(uri: &str) -> Option<String> {
    let u = url::Url::parse(uri).ok()?;
    let loopback = matches!(u.host(), Some(url::Host::Domain("localhost")))
        || matches!(u.host(), Some(url::Host::Ipv4(a)) if a.is_loopback())
        || matches!(u.host(), Some(url::Host::Ipv6(a)) if a.is_loopback());
    let ok = u.fragment().is_none()
        && u.username().is_empty()
        && u.password().is_none()
        && u.host().is_some()
        && (u.scheme() == "https" || (u.scheme() == "http" && loopback));
    ok.then(|| u.as_str().to_owned())
}

fn row(r: &rusqlite::Row<'_>) -> rusqlite::Result<Client> {
    let kind: String = r.get(1)?;
    let uris: String = r.get(3)?;
    Ok(Client {
        id: r.get(0)?,
        // An unknown kind takes neither flow: as a hud box, which can only
        // be given an assertion at an address registered for it.
        kind: ClientKind::parse(&kind).unwrap_or(ClientKind::Hud),
        name: r.get(2)?,
        redirect_uris: serde_json::from_str(&uris).unwrap_or_default(),
        created: r.get(4)?,
    })
}

const COLUMNS: &str = "id, kind, name, redirect_uris, created";

/// Register a client, or change its kind, name and addresses: what was
/// kept, its addresses normalized. Changing a client's kind ends its
/// access tokens.
pub fn put_client(c: &Connection, client: &Client) -> anyhow::Result<Client> {
    anyhow::ensure!(
        client_id_ok(&client.id),
        "a client id is 1 to 64 of a-z 0-9 . _ -, starting with a letter or digit"
    );
    anyhow::ensure!(!client.name.trim().is_empty(), "a client needs a name");
    anyhow::ensure!(!client.redirect_uris.is_empty(), "a client needs at least one redirect address");
    let mut uris = Vec::new();
    for u in &client.redirect_uris {
        let n = redirect_normalized(u).ok_or_else(|| {
            anyhow::anyhow!("{u}: a redirect address is https (or http on loopback), with no fragment and no user")
        })?;
        if !uris.contains(&n) {
            uris.push(n);
        }
    }
    c.execute_batch(SCHEMA)?;
    c.execute(
        "DELETE FROM issuer_tokens WHERE client = ?1 AND EXISTS
             (SELECT 1 FROM issuer_clients WHERE id = ?1 AND kind != ?2)",
        params![client.id, client.kind.as_str()],
    )?;
    c.execute(
        "INSERT INTO issuer_clients (id, kind, name, redirect_uris, created) VALUES (?1, ?2, ?3, ?4, ?5)
         ON CONFLICT (id) DO UPDATE SET kind = ?2, name = ?3, redirect_uris = ?4",
        params![client.id, client.kind.as_str(), client.name.trim(), serde_json::to_string(&uris)?, client.created],
    )?;
    Ok(Client { redirect_uris: uris, name: client.name.trim().to_owned(), ..client.clone() })
}

pub fn clients(c: &Connection) -> anyhow::Result<Vec<Client>> {
    c.execute_batch(SCHEMA)?;
    let mut q = c.prepare(&format!("SELECT {COLUMNS} FROM issuer_clients ORDER BY id"))?;
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
            .query_row(&format!("SELECT {COLUMNS} FROM issuer_clients WHERE id = ?1"), params![id], row)
            .optional()?)
    }

    pub fn put_issuer_client(&self, client: &Client) -> anyhow::Result<Client> {
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
    /// session it came from and its (`oidc`) client all still there, and
    /// the account too.
    pub fn issuer_token(&self, token_hash: &str, now: u64) -> anyhow::Result<Option<(String, String)>> {
        Ok(self
            .c()
            .query_row(
                "SELECT t.account, t.client FROM issuer_tokens t
                 JOIN sessions s ON s.token_hash = t.hub_session AND s.account = t.account
                 JOIN accounts a ON a.id = t.account
                 JOIN issuer_clients c ON c.id = t.client AND c.kind = 'oidc'
                 WHERE t.token_hash = ?1 AND t.expires > ?2 AND s.expires > ?2",
                params![token_hash, now],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()?)
    }

    /// The name the person set for the account, if they set one: never a
    /// login standing in for it.
    pub fn issuer_set_name(&self, account: &str) -> anyhow::Result<Option<String>> {
        Ok(self
            .c()
            .query_row(
                "SELECT name FROM accounts WHERE id = ?1 AND name IS NOT NULL AND trim(name) != ''",
                params![account],
                |r| r.get(0),
            )
            .optional()?)
    }

    /// Whether another GitHub identity here (another numeric id) has
    /// `login` too, in any case: one of them renamed and the other took the
    /// name, so the login vouches for neither until they sign in again.
    pub fn github_login_shared(&self, login: &str, github_id: &str) -> anyhow::Result<bool> {
        Ok(self.c().query_row(
            "SELECT EXISTS (SELECT 1 FROM identities
                 WHERE provider = 'github' AND lower(login) = lower(?1) AND subject != ?2)",
            params![login, github_id],
            |r| r.get(0),
        )?)
    }
}
