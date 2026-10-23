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
//! - `account_link_events` (M229): the audit of links made, renamed and
//!   ended, kept a year.
//! - `issuer_grants` (M229): which products a person let sign them in (the
//!   consent screen), per account and client; ending it is unlinking.
//! - `account_link_tombstones` (M229): when an account last unlinked from a
//!   product: assertions issued until then never link.
//! - `link_alerts` (M229): the ops counter of bulk unlinks and lookups by a
//!   client.
//! - `issuer_clients.project` (M237): the platform project a `hud` client
//!   (a box) is bound to. A bound box reads its project's members as its
//!   roster and posts reviews to object threads in that project, with its
//!   client secret; an unbound box keeps its own roster.

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
CREATE TABLE IF NOT EXISTS issuer_grants (
    account TEXT NOT NULL,
    client TEXT NOT NULL,
    granted INTEGER NOT NULL,
    PRIMARY KEY (account, client)
);
CREATE TABLE IF NOT EXISTS account_link_tombstones (
    client TEXT NOT NULL,
    account TEXT NOT NULL,
    unlinked INTEGER NOT NULL,
    PRIMARY KEY (client, account)
);
CREATE TABLE IF NOT EXISTS link_alerts (
    client TEXT NOT NULL,
    what TEXT NOT NULL,
    count INTEGER NOT NULL,
    last INTEGER NOT NULL,
    PRIMARY KEY (client, what)
);
";

/// The tables, and the columns added since they were first made (M237's
/// `issuer_clients.project`): what the hub runs at start, and the CLI on a
/// hub's file.
pub fn ensure(c: &Connection) -> rusqlite::Result<()> {
    c.execute_batch(SCHEMA)?;
    let mut q = c.prepare("PRAGMA table_info(issuer_clients)")?;
    let names: Vec<String> = q.query_map([], |r| r.get(1))?.collect::<Result<_, _>>()?;
    if !names.iter().any(|n| n == "project") {
        c.execute_batch("ALTER TABLE issuer_clients ADD COLUMN project TEXT")?;
    }
    Ok(())
}

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
/// How long an unlink's tombstone is kept: far longer than any assertion
/// issued before it lives.
pub const TOMBSTONE_TTL_MS: u64 = 24 * 3_600_000;

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
    /// The platform project a `hud` client is bound to (M237): its
    /// members are the box's roster. Never set on an `oidc` client.
    pub project: Option<String>,
}

/// A platform project id as a binding names it: `prj_` and 1 to 64 letters
/// and digits.
pub fn project_id_ok(id: &str) -> bool {
    id.strip_prefix("prj_").is_some_and(|r| (1..=64).contains(&r.len()) && r.bytes().all(|b| b.is_ascii_alphanumeric()))
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
        project: r.get(5)?,
    })
}

const COLUMNS: &str = "id, kind, name, redirect_uris, created, project";

/// Register a client, or change its kind, name, addresses and project:
/// what was kept, its addresses normalized. Changing a client's kind ends
/// its access tokens and its secret.
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
    if let Some(p) = &client.project {
        anyhow::ensure!(client.kind == ClientKind::Hud, "only a hud client (a box) is bound to a project");
        anyhow::ensure!(project_id_ok(p), "{p}: a project id is prj_ and letters and digits");
    }
    ensure(c)?;
    c.execute(
        "DELETE FROM issuer_tokens WHERE client = ?1 AND EXISTS
             (SELECT 1 FROM issuer_clients WHERE id = ?1 AND kind != ?2)",
        params![client.id, client.kind.as_str()],
    )?;
    // A secret was given for one kind's routes (M237): a client that
    // changes kind gets a new one.
    c.execute(
        "DELETE FROM issuer_client_secrets WHERE client = ?1 AND EXISTS
             (SELECT 1 FROM issuer_clients WHERE id = ?1 AND kind != ?2)",
        params![client.id, client.kind.as_str()],
    )?;
    c.execute(
        "INSERT INTO issuer_clients (id, kind, name, redirect_uris, created, project) VALUES (?1, ?2, ?3, ?4, ?5, ?6)
         ON CONFLICT (id) DO UPDATE SET kind = ?2, name = ?3, redirect_uris = ?4, project = ?6",
        params![
            client.id,
            client.kind.as_str(),
            client.name.trim(),
            serde_json::to_string(&uris)?,
            client.created,
            client.project
        ],
    )?;
    Ok(Client { redirect_uris: uris, name: client.name.trim().to_owned(), ..client.clone() })
}

pub fn clients(c: &Connection) -> anyhow::Result<Vec<Client>> {
    ensure(c)?;
    let mut q = c.prepare(&format!("SELECT {COLUMNS} FROM issuer_clients ORDER BY id"))?;
    Ok(q.query_map([], row)?.collect::<Result<_, _>>()?)
}

/// Give a registered client a secret (its hash), replacing any it had: the
/// old one stops working at once. An `oidc` client links accounts with it
/// (M229); a `hud` client (a box) reads its project's members and posts
/// review threads (M237). Each kind's routes refuse the other's.
pub fn set_client_secret(c: &Connection, client: &str, secret_hash: &str, now: u64) -> anyhow::Result<()> {
    ensure(c)?;
    let there: bool =
        c.query_row("SELECT COUNT(*) > 0 FROM issuer_clients WHERE id = ?1", params![client], |r| r.get(0))?;
    anyhow::ensure!(there, "no client {client}: register it first (issuer add-client)");
    c.execute(
        "INSERT INTO issuer_client_secrets (client, secret_hash, created) VALUES (?1, ?2, ?3)
         ON CONFLICT (client) DO UPDATE SET secret_hash = ?2, created = ?3",
        params![client, secret_hash, now],
    )?;
    Ok(())
}

/// Take a client's link secret away. Whether it had one.
pub fn drop_client_secret(c: &Connection, client: &str) -> anyhow::Result<bool> {
    ensure(c)?;
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

/// What the platform page lists for one product (M229): the person let it
/// sign them in (`granted`, the consent), and maybe linked one of its users.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Product {
    pub client: String,
    /// The client's registered name.
    pub name: String,
    pub granted: Option<u64>,
    pub link: Option<Link>,
}

/// Why a link wasn't made.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LinkRefused {
    /// The product's user is another account's: unlink it first.
    UserLinkedElsewhere,
    /// The account is another of the product's users'.
    AccountLinkedElsewhere,
    /// The assertion was issued no later than this account's last unlink
    /// from this product: an unlink sticks until the person signs in again.
    Unlinked,
}

/// Who made or ended a link, for the audit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LinkBy {
    /// The product, with its secret.
    Product,
    /// The person, on the platform page.
    Account,
    /// An operator, taking the client out.
    Operator,
}

impl LinkBy {
    fn as_str(self) -> &'static str {
        match self {
            LinkBy::Product => "product",
            LinkBy::Account => "account",
            LinkBy::Operator => "operator",
        }
    }
}

/// One entry of the audit of links.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LinkEvent {
    pub client: String,
    /// The product's user (empty for a consent with no link).
    pub user: String,
    /// `linked`, `renamed`, `unlinked`, or `imported` (M232: by an operator, from a backup).
    pub event: String,
    /// `product`, `account` or `operator`.
    pub by: String,
    pub at: u64,
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

fn link_event(
    c: &Connection,
    (account, client, user): (&str, &str, &str),
    event: &str,
    by: LinkBy,
    now: u64,
) -> rusqlite::Result<usize> {
    c.execute(
        "INSERT INTO account_link_events (account, client, product_user, event, by, at) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![account, client, user, event, by.as_str(), now],
    )
}

fn link_of_user(c: &Connection, client: &str, user: &str) -> rusqlite::Result<Option<Link>> {
    c.query_row(
        &format!("SELECT {LINK_COLUMNS} FROM account_links WHERE client = ?1 AND product_user = ?2"),
        params![client, user],
        link_row,
    )
    .optional()
}

fn link_of_account(c: &Connection, client: &str, account: &str) -> rusqlite::Result<Option<Link>> {
    c.query_row(
        &format!("SELECT {LINK_COLUMNS} FROM account_links WHERE client = ?1 AND account = ?2"),
        params![client, account],
        link_row,
    )
    .optional()
}

/// End what `account` has with `client` (M229): its link, if any, and its
/// consent, which the next sign-in through that product asks for again; a
/// tombstone refuses any assertion issued until now; audited as `by`'s.
/// Whether there was a link or a consent to end.
fn end(c: &Connection, client: &str, account: &str, by: LinkBy, now: u64) -> rusqlite::Result<bool> {
    let link = link_of_account(c, client, account)?;
    let links = c.execute("DELETE FROM account_links WHERE client = ?1 AND account = ?2", params![client, account])?;
    let grants = c.execute("DELETE FROM issuer_grants WHERE client = ?1 AND account = ?2", params![client, account])?;
    if links + grants == 0 {
        return Ok(false);
    }
    c.execute(
        "INSERT INTO account_link_tombstones (client, account, unlinked) VALUES (?1, ?2, ?3)
         ON CONFLICT (client, account) DO UPDATE SET unlinked = MAX(unlinked, ?3)",
        params![client, account, now],
    )?;
    let user = link.map(|l| l.user).unwrap_or_default();
    link_event(c, (account, client, &user), "unlinked", by, now)?;
    Ok(true)
}

/// Take a client out, and the access tokens, link secret, consents and
/// account links it holds: each link ended is audited as the operator's
/// (the audit keeps them). Whether it was there.
pub fn remove_client(c: &mut Connection, id: &str) -> anyhow::Result<bool> {
    ensure(c)?;
    let tx = c.transaction()?;
    let now = arugula_e2e::now_ms();
    let accounts: Vec<String> = {
        let mut q = tx.prepare(
            "SELECT account FROM account_links WHERE client = ?1 UNION SELECT account FROM issuer_grants WHERE client = ?1",
        )?;
        q.query_map(params![id], |r| r.get(0))?.collect::<Result<_, _>>()?
    };
    for a in &accounts {
        end(&tx, id, a, LinkBy::Operator, now)?;
    }
    tx.execute("DELETE FROM account_link_tombstones WHERE client = ?1", params![id])?;
    tx.execute("DELETE FROM issuer_tokens WHERE client = ?1", params![id])?;
    tx.execute("DELETE FROM issuer_client_secrets WHERE client = ?1", params![id])?;
    let gone = tx.execute("DELETE FROM issuer_clients WHERE id = ?1", params![id])? > 0;
    tx.commit()?;
    Ok(gone)
}

/// What importing a product's user (M232, planter's import) found, or
/// did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Imported {
    /// Linked now (or, dry, would be), audited `imported` by the operator.
    Linked,
    /// Linked to this account already: left as it is.
    Already,
    /// The account had a link with this product, and it ended at this time
    /// (ms): the import never links it again, so an unlink sticks.
    UnlinkedSince(u64),
    /// The product's user is linked to this other account.
    UserLinkedElsewhere(String),
    /// The account is linked to this other user of the product.
    AccountLinkedElsewhere(String),
}

/// The account whose GitHub identity has this numeric id, and the login it
/// last signed in with: by the number only, never a login.
pub fn account_by_github(c: &Connection, github_id: u64) -> anyhow::Result<Option<(String, String)>> {
    Ok(c.query_row(
        "SELECT account, COALESCE(login, '') FROM identities WHERE provider = 'github' AND subject = ?1",
        params![github_id.to_string()],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )
    .optional()?)
}

/// Import `link` (M232): an operator links a product's user to the
/// account with the same numeric GitHub id, without the person's assertion,
/// audited `imported` by `operator`. Nothing that's there is changed: the
/// same link again is [`Imported::Already`], a link either side has
/// elsewhere is reported, and an account that was ever unlinked from this
/// product (its tombstone, or an `unlinked` in the audit, kept a year) is
/// never linked again by an import. `dry`: write nothing. Run it inside the
/// caller's transaction.
pub fn import_link(c: &Connection, link: &Link, dry: bool) -> anyhow::Result<Imported> {
    c.execute_batch(SCHEMA)?;
    if let Some(had) = link_of_user(c, &link.client, &link.user)? {
        return Ok(if had.account == link.account {
            Imported::Already
        } else {
            Imported::UserLinkedElsewhere(had.account)
        });
    }
    if let Some(had) = link_of_account(c, &link.client, &link.account)? {
        return Ok(Imported::AccountLinkedElsewhere(had.user));
    }
    let ended: Option<u64> = c.query_row(
        "SELECT MAX(at) FROM (
             SELECT at FROM account_link_events WHERE account = ?1 AND client = ?2 AND event = 'unlinked'
             UNION ALL SELECT unlinked FROM account_link_tombstones WHERE account = ?1 AND client = ?2)",
        params![link.account, link.client],
        |r| r.get(0),
    )?;
    if let Some(at) = ended {
        return Ok(Imported::UnlinkedSince(at));
    }
    if !dry {
        c.execute(
            "INSERT INTO account_links (client, product_user, account, github_id, name, linked)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![link.client, link.user, link.account, link.github_id.to_string(), link.name, link.linked],
        )?;
        link_event(c, (&link.account, &link.client, &link.user), "imported", LinkBy::Operator, link.linked)?;
    }
    Ok(Imported::Linked)
}

/// The ops counter of bulk link calls: (client, what, count, last ms).
pub fn link_alerts(c: &Connection) -> anyhow::Result<Vec<(String, String, u64, u64)>> {
    ensure(c)?;
    let mut q = c.prepare("SELECT client, what, count, last FROM link_alerts ORDER BY last DESC")?;
    let rows = q.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))?;
    Ok(rows.collect::<Result<_, _>>()?)
}

impl Db {
    /// The client whose secret `secret` is, when it's still registered as
    /// a `kind` client.
    pub fn issuer_client_by_secret(
        &self,
        client: &str,
        secret: &str,
        kind: ClientKind,
    ) -> anyhow::Result<Option<Client>> {
        let cols = COLUMNS.split(", ").map(|c| format!("c.{c}")).collect::<Vec<_>>().join(", ");
        Ok(self
            .c()
            .query_row(
                &format!(
                    "SELECT {cols} FROM issuer_client_secrets s JOIN issuer_clients c ON c.id = s.client
                     WHERE s.client = ?1 AND s.secret_hash = ?2 AND c.kind = ?3"
                ),
                params![client, crate::auth::hash(secret), kind.as_str()],
                row,
            )
            .optional()?)
    }

    /// Whether `account` let `client` sign it in (the consent, M229).
    pub fn issuer_grant(&self, account: &str, client: &str) -> anyhow::Result<bool> {
        Ok(self.c().query_row(
            "SELECT COUNT(*) > 0 FROM issuer_grants WHERE account = ?1 AND client = ?2",
            params![account, client],
            |r| r.get(0),
        )?)
    }

    /// `account` lets `client` sign it in, from now until it unlinks.
    pub fn put_issuer_grant(&self, account: &str, client: &str, now: u64) -> anyhow::Result<()> {
        self.c().execute(
            "INSERT INTO issuer_grants (account, client, granted) VALUES (?1, ?2, ?3)
             ON CONFLICT (account, client) DO NOTHING",
            params![account, client, now],
        )?;
        Ok(())
    }

    /// Link `link.user` of `link.client` to `link.account`, in one
    /// transaction, for an assertion issued at `iat` (seconds): the same link
    /// again is no change (`Ok(false)`, though a new name is kept and
    /// audited); a user or account already linked otherwise is refused, and
    /// so is an assertion issued no later than the second of the account's
    /// last unlink from this product (`iat` has a second's grain, so one
    /// issued in that second counts as before it). A new link is audited.
    pub fn link_account(&self, link: &Link, iat: u64) -> anyhow::Result<Result<bool, LinkRefused>> {
        let mut c = self.c();
        let tx = c.transaction()?;
        let unlinked: Option<u64> = tx
            .query_row(
                "SELECT unlinked FROM account_link_tombstones WHERE client = ?1 AND account = ?2",
                params![link.client, link.account],
                |r| r.get(0),
            )
            .optional()?;
        if unlinked.is_some_and(|u| iat <= u / 1000) {
            return Ok(Err(LinkRefused::Unlinked));
        }
        let who = (link.account.as_str(), link.client.as_str(), link.user.as_str());
        if let Some(had) = link_of_user(&tx, &link.client, &link.user)? {
            if had.account != link.account {
                return Ok(Err(LinkRefused::UserLinkedElsewhere));
            }
            if link.name.is_some() && link.name != had.name {
                tx.execute(
                    "UPDATE account_links SET name = ?3 WHERE client = ?1 AND product_user = ?2",
                    params![link.client, link.user, link.name],
                )?;
                link_event(&tx, who, "renamed", LinkBy::Product, link.linked)?;
                tx.commit()?;
            }
            return Ok(Ok(false));
        }
        if link_of_account(&tx, &link.client, &link.account)?.is_some() {
            return Ok(Err(LinkRefused::AccountLinkedElsewhere));
        }
        tx.execute(
            "INSERT INTO account_links (client, product_user, account, github_id, name, linked)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![link.client, link.user, link.account, link.github_id.to_string(), link.name, link.linked],
        )?;
        link_event(&tx, who, "linked", LinkBy::Product, link.linked)?;
        tx.commit()?;
        Ok(Ok(true))
    }

    /// The link of a product's user, when there is one.
    pub fn account_link(&self, client: &str, user: &str) -> anyhow::Result<Option<Link>> {
        Ok(link_of_user(&self.c(), client, user)?)
    }

    /// The products an account let sign it in or linked, by product: each
    /// with its consent and its link.
    pub fn account_products(&self, account: &str) -> anyhow::Result<Vec<Product>> {
        let c = self.c();
        let mut q = c.prepare(
            "SELECT k.client, COALESCE(c.name, k.client), g.granted FROM
                 (SELECT client FROM issuer_grants WHERE account = ?1
                  UNION SELECT client FROM account_links WHERE account = ?1) k
             LEFT JOIN issuer_clients c ON c.id = k.client
             LEFT JOIN issuer_grants g ON g.client = k.client AND g.account = ?1
             ORDER BY k.client",
        )?;
        let rows: Vec<(String, String, Option<u64>)> =
            q.query_map(params![account], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?.collect::<Result<_, _>>()?;
        rows.into_iter()
            .map(|(client, name, granted)| {
                let link = link_of_account(&c, &client, account)?;
                Ok(Product { client, name, granted, link })
            })
            .collect()
    }

    /// The product ends its user's link (and that account's consent),
    /// audited as the product's. Whether there was one.
    pub fn unlink_user(&self, client: &str, user: &str, now: u64) -> anyhow::Result<bool> {
        let mut c = self.c();
        let tx = c.transaction()?;
        let Some(l) = link_of_user(&tx, client, user)? else { return Ok(false) };
        let gone = end(&tx, client, &l.account, LinkBy::Product, now)?;
        tx.commit()?;
        Ok(gone)
    }

    /// The person ends what their account has with a product, on the
    /// platform page: its link and its consent, audited as theirs. Whether
    /// there was either.
    pub fn unlink_product(&self, account: &str, client: &str, now: u64) -> anyhow::Result<bool> {
        let mut c = self.c();
        let tx = c.transaction()?;
        let gone = end(&tx, client, account, LinkBy::Account, now)?;
        tx.commit()?;
        Ok(gone)
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

    /// One more bulk use of the link API by `client` (`what`: `unlinks` or
    /// `lookups`) went over its alert line: the ops counter, which
    /// `arugula-control issuer link-alerts` reads.
    pub fn bump_link_alert(&self, client: &str, what: &str, now: u64) -> anyhow::Result<()> {
        self.c().execute(
            "INSERT INTO link_alerts (client, what, count, last) VALUES (?1, ?2, 1, ?3)
             ON CONFLICT (client, what) DO UPDATE SET count = count + 1, last = ?3",
            params![client, what, now],
        )?;
        Ok(())
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
