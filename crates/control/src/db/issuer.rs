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
//! - `issuer_client_secrets` (M229): a client's secret for the link API, by
//!   its hash. A client without one can't link accounts.
//! - `account_links` (M229): which platform account (the hub's account) a
//!   product's own user is, matched by numeric GitHub id. One account per
//!   product user, and one product user per account in each product.
//! - `account_link_events` (M229): the audit of links made and ended, kept
//!   a year.

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
CREATE TABLE IF NOT EXISTS issuer_client_secrets (
    client TEXT PRIMARY KEY,
    secret_hash TEXT NOT NULL,
    created INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS account_links (
    client TEXT NOT NULL,
    product_user TEXT NOT NULL,
    account TEXT NOT NULL,
    github_id TEXT NOT NULL,
    name TEXT,
    linked INTEGER NOT NULL,
    PRIMARY KEY (client, product_user),
    UNIQUE (client, account)
);
CREATE INDEX IF NOT EXISTS account_links_account ON account_links (account);
CREATE TABLE IF NOT EXISTS account_link_events (
    id INTEGER PRIMARY KEY,
    account TEXT NOT NULL,
    client TEXT NOT NULL,
    product_user TEXT NOT NULL,
    event TEXT NOT NULL,
    by TEXT NOT NULL,
    at INTEGER NOT NULL
);
CREATE INDEX IF NOT EXISTS account_link_events_account ON account_link_events (account);
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

/// How long the audit of links keeps an event.
pub const LINK_EVENT_TTL_MS: u64 = 365 * 24 * 3_600_000;

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

/// Take a client out, and the access tokens, link secret and account
/// links it holds (the audit keeps its events). Whether it was there.
pub fn remove_client(c: &Connection, id: &str) -> anyhow::Result<bool> {
    c.execute_batch(SCHEMA)?;
    c.execute("DELETE FROM issuer_tokens WHERE client = ?1", params![id])?;
    c.execute("DELETE FROM issuer_client_secrets WHERE client = ?1", params![id])?;
    c.execute("DELETE FROM account_links WHERE client = ?1", params![id])?;
    Ok(c.execute("DELETE FROM issuer_clients WHERE id = ?1", params![id])? > 0)
}

/// Give a registered client a link secret (its hash), replacing any it
/// had: the old one stops working at once.
pub fn set_client_secret(c: &Connection, client: &str, secret_hash: &str, now: u64) -> anyhow::Result<()> {
    c.execute_batch(SCHEMA)?;
    let kind: Option<String> =
        c.query_row("SELECT kind FROM issuer_clients WHERE id = ?1", params![client], |r| r.get(0)).optional()?;
    let Some(kind) = kind else { anyhow::bail!("no client {client}: register it first (issuer add-client)") };
    anyhow::ensure!(
        kind == ClientKind::Oidc.as_str(),
        "client {client} is a {kind} client: only oidc clients (products with users of their own) link accounts"
    );
    c.execute(
        "INSERT INTO issuer_client_secrets (client, secret_hash, created) VALUES (?1, ?2, ?3)
         ON CONFLICT (client) DO UPDATE SET secret_hash = ?2, created = ?3",
        params![client, secret_hash, now],
    )?;
    Ok(())
}

/// Take a client's link secret away. Whether it had one.
pub fn drop_client_secret(c: &Connection, client: &str) -> anyhow::Result<bool> {
    c.execute_batch(SCHEMA)?;
    Ok(c.execute("DELETE FROM issuer_client_secrets WHERE client = ?1", params![client])? > 0)
}

/// A product's user linked to a platform account (M229).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Link {
    pub client: String,
    /// The product's own id for its user.
    pub user: String,
    /// The hub's account: the platform account.
    pub account: String,
    /// The numeric GitHub id both sides had when it was linked.
    pub github_id: u64,
    /// How the product names its user, for the platform page.
    pub name: Option<String>,
    pub linked: u64,
}

/// Why a link wasn't made.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LinkRefused {
    /// The product's user is another account's: unlink it first.
    UserLinkedElsewhere,
    /// The account is another of the product's users'.
    AccountLinkedElsewhere,
}

/// One entry of the audit of links.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LinkEvent {
    pub client: String,
    pub user: String,
    /// `linked` or `unlinked`.
    pub event: String,
    /// `product` or `account`.
    pub by: String,
    pub at: u64,
}

/// Who ended (or made) a link, for the audit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LinkBy {
    /// The product, with its secret.
    Product,
    /// The person, on the platform page.
    Account,
}

impl LinkBy {
    fn as_str(self) -> &'static str {
        match self {
            LinkBy::Product => "product",
            LinkBy::Account => "account",
        }
    }
}

fn link_row(r: &rusqlite::Row<'_>) -> rusqlite::Result<Link> {
    let gh: String = r.get(3)?;
    Ok(Link {
        client: r.get(0)?,
        user: r.get(1)?,
        account: r.get(2)?,
        github_id: gh.parse().unwrap_or_default(),
        name: r.get(4)?,
        linked: r.get(5)?,
    })
}

const LINK_COLUMNS: &str = "client, product_user, account, github_id, name, linked";

fn link_event(c: &Connection, l: &Link, event: &str, by: LinkBy, now: u64) -> rusqlite::Result<usize> {
    c.execute(
        "INSERT INTO account_link_events (account, client, product_user, event, by, at) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![l.account, l.client, l.user, event, by.as_str(), now],
    )
}

impl Db {
    /// Whether `secret` is `client`'s link secret, and the client is still
    /// registered, as an `oidc` client.
    pub fn issuer_client_secret_ok(&self, client: &str, secret: &str) -> anyhow::Result<bool> {
        Ok(self.c().query_row(
            "SELECT COUNT(*) > 0 FROM issuer_client_secrets s JOIN issuer_clients c ON c.id = s.client
             WHERE s.client = ?1 AND s.secret_hash = ?2 AND c.kind = ?3",
            params![client, crate::auth::hash(secret), ClientKind::Oidc.as_str()],
            |r| r.get(0),
        )?)
    }

    /// Link `link.user` of `link.client` to `link.account`, in one
    /// transaction: the same link again is no change (`Ok(false)`, though a
    /// new name is kept); a user or account already linked otherwise is
    /// refused. A new link is audited.
    pub fn link_account(&self, link: &Link) -> anyhow::Result<Result<bool, LinkRefused>> {
        let mut c = self.c();
        let tx = c.transaction()?;
        let by_user = tx
            .query_row(
                &format!("SELECT {LINK_COLUMNS} FROM account_links WHERE client = ?1 AND product_user = ?2"),
                params![link.client, link.user],
                link_row,
            )
            .optional()?;
        if let Some(had) = by_user {
            if had.account != link.account {
                return Ok(Err(LinkRefused::UserLinkedElsewhere));
            }
            if link.name.is_some() && link.name != had.name {
                tx.execute(
                    "UPDATE account_links SET name = ?3 WHERE client = ?1 AND product_user = ?2",
                    params![link.client, link.user, link.name],
                )?;
                tx.commit()?;
            }
            return Ok(Ok(false));
        }
        let by_account: bool = tx.query_row(
            "SELECT COUNT(*) > 0 FROM account_links WHERE client = ?1 AND account = ?2",
            params![link.client, link.account],
            |r| r.get(0),
        )?;
        if by_account {
            return Ok(Err(LinkRefused::AccountLinkedElsewhere));
        }
        tx.execute(
            "INSERT INTO account_links (client, product_user, account, github_id, name, linked)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![link.client, link.user, link.account, link.github_id.to_string(), link.name, link.linked],
        )?;
        link_event(&tx, link, "linked", LinkBy::Product, link.linked)?;
        tx.commit()?;
        Ok(Ok(true))
    }

    /// The link of a product's user, when there is one.
    pub fn account_link(&self, client: &str, user: &str) -> anyhow::Result<Option<Link>> {
        Ok(self
            .c()
            .query_row(
                &format!("SELECT {LINK_COLUMNS} FROM account_links WHERE client = ?1 AND product_user = ?2"),
                params![client, user],
                link_row,
            )
            .optional()?)
    }

    /// An account's links, with each product's name, by product.
    pub fn account_links(&self, account: &str) -> anyhow::Result<Vec<(Link, String)>> {
        let c = self.c();
        let mut q = c.prepare(
            "SELECT l.client, l.product_user, l.account, l.github_id, l.name, l.linked, COALESCE(c.name, l.client)
             FROM account_links l LEFT JOIN issuer_clients c ON c.id = l.client
             WHERE l.account = ?1 ORDER BY l.client, l.product_user",
        )?;
        let rows = q.query_map(params![account], |r| Ok((link_row(r)?, r.get(6)?)))?;
        Ok(rows.collect::<Result<_, _>>()?)
    }

    /// End a product user's link, audited as `by`'s. With `account`, only
    /// if it's that account's (the platform page ends only its own).
    /// Whether there was one.
    pub fn unlink_account(
        &self,
        client: &str,
        user: &str,
        account: Option<&str>,
        by: LinkBy,
        now: u64,
    ) -> anyhow::Result<bool> {
        let mut c = self.c();
        let tx = c.transaction()?;
        let had = tx
            .query_row(
                &format!("SELECT {LINK_COLUMNS} FROM account_links WHERE client = ?1 AND product_user = ?2"),
                params![client, user],
                link_row,
            )
            .optional()?
            .filter(|l| account.is_none_or(|a| a == l.account));
        let Some(had) = had else { return Ok(false) };
        tx.execute("DELETE FROM account_links WHERE client = ?1 AND product_user = ?2", params![client, user])?;
        link_event(&tx, &had, "unlinked", by, now)?;
        tx.commit()?;
        Ok(true)
    }

    /// The audit of an account's links, newest first.
    pub fn account_link_events(&self, account: &str) -> anyhow::Result<Vec<LinkEvent>> {
        let c = self.c();
        let mut q = c.prepare(
            "SELECT client, product_user, event, by, at FROM account_link_events WHERE account = ?1 ORDER BY id DESC",
        )?;
        let rows = q.query_map(params![account], |r| {
            Ok(LinkEvent { client: r.get(0)?, user: r.get(1)?, event: r.get(2)?, by: r.get(3)?, at: r.get(4)? })
        })?;
        Ok(rows.collect::<Result<_, _>>()?)
    }

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
