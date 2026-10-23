//! Absorbing redone (M235, platform step 8, native Arugula #33): its
//! tenants, tokens and apps, read from a backup of redone's `DATA_DIR`.
//!
//! redone (arugula-salad/redone) is an MCP server through which a friend's
//! agent puts an app on home-cloud: tenants (a friend each), their agents'
//! `rdn_` tokens, an approver key per tenant, and plan → approve → apply
//! through planter's ship API. Each becomes the platform's own:
//!
//! - **A tenant becomes a project** with its people, found by numeric
//!   GitHub id only (the operator says whose a tenant is: redone never knew
//!   anyone's GitHub id, and matching its names or logins would be the
//!   login problem, aru-113). One person's tenant is their personal
//!   project; several people's is a team's project the operator names
//!   (`--team`), whose roster they must already be on (rosters are signed
//!   by the team's owners' devices: the import can't add anyone). Someone
//!   not on Arugula yet waits, and a later run picks them up.
//! - **An `rdn_` token becomes an agent grant token** (M230): redone kept
//!   each token's SHA-256, which is how the platform keeps its own, so the
//!   very token works on the platform's MCP endpoint, owned by the tenant's
//!   first person, with `deploys.plan` and `secrets.write` over its project,
//!   listed and revocable on their page like any other. Its agent also gets
//!   redone's tool names (the shim, [`crate::agent_mcp`]) until it's
//!   revoked, on a date Jake announces. Nothing here revokes anything.
//! - **The approver key is not carried over.** A deploy is approved on an
//!   owner's device, with a passkey ("A deploy needs an owner device's
//!   approval, and agents never approve", aru-101): the approval is a phone
//!   card. Neither is a tenant's ntfy topic (topics without auth, Arugula
//!   #55) or `auto-approve-updates` (every deploy waits for a device).
//! - **An app** is reported with its live spec; it becomes an environment
//!   of its tenant's project with planter's ship driver (M233).
//!
//! **The backup** is redone's `state.json` (`store.mjs`: `{ tenants,
//! tokens, apps, uploads, plans }`). It's only read. Approver keys' hashes
//! are never read; uploads and their tarballs aren't either.
//!
//! **Safe to run again** (`redone_imports`): what's carried over is
//! recorded, so a second run changes nothing, and a token revoked since is
//! never carried over again. It never deletes anything. `--dry-run` says
//! exactly what a run would do and writes nothing.

use std::{
    collections::{BTreeMap, HashMap},
    path::{Path, PathBuf},
};

use anyhow::{Context as _, bail};
use arugula_e2e::{now_ms, team::TeamRole};
use serde::Serialize;
use serde_json::{Value, json};

use crate::{
    Platform,
    db::{
        agent_tokens::{MAX_LIVE, NewToken},
        projects::{DEPLOYS_PLAN, SECRETS_WRITE},
        redone::Imported,
    },
};

/// What a redone agent token starts with.
pub const TOKEN_PREFIX: &str = "rdn_";
/// What a redone approver key starts with: never a token, here or there.
pub const APPROVER_PREFIX: &str = "rdn_approve_";

/// The powers a carried-over token gets over its tenant's project: what
/// redone's agents did (plan deploys, set secrets), and no more. Never an
/// approval: no grant is.
pub const POWERS: [&str; 2] = [DEPLOYS_PLAN, SECRETS_WRITE];

/// One of redone's tenants.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Tenant {
    pub id: String,
    pub name: String,
    /// It had an ntfy topic: not carried over.
    pub ntfy: bool,
    /// Updates to its apps skipped approval: not carried over.
    pub auto_approve_updates: bool,
}

/// One of redone's agent tokens: its hash, never the token.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Token {
    /// `tok_…`.
    pub id: String,
    pub tenant: String,
    /// What redone called its agent.
    pub agent: String,
    /// The token's SHA-256, hex, as redone kept it.
    #[serde(skip)]
    pub hash: String,
    pub at: u64,
}

/// One of redone's apps, with what's live on it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct App {
    pub name: String,
    pub tenant: String,
    /// The plan that's live, if one was applied.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub current: Option<String>,
    /// Its spec as that plan said: port, health, secrets, portEnv.
    #[serde(skip_serializing_if = "Value::is_null")]
    pub spec: Value,
    pub deploys: usize,
}

/// redone's backup, read.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct Backup {
    #[serde(skip)]
    pub dir: PathBuf,
    pub tenants: Vec<Tenant>,
    pub tokens: Vec<Token>,
    pub apps: Vec<App>,
}

fn text(v: &Value, k: &str) -> Option<String> {
    v.get(k).and_then(Value::as_str).map(str::trim).filter(|s| !s.is_empty()).map(str::to_owned)
}

/// A token's hash as redone keeps it: 64 lower-case hex characters.
fn good_hash(h: &str) -> bool {
    h.len() == 64 && h.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
}

impl Backup {
    /// redone's backup in `dir` (its `state.json`). Only read.
    pub fn read(dir: &Path) -> anyhow::Result<Backup> {
        let path = dir.join("state.json");
        let raw = std::fs::read(&path)
            .with_context(|| format!("reading {}: is it a copy of redone's DATA_DIR?", path.display()))?;
        let v: Value = serde_json::from_slice(&raw).with_context(|| format!("{} isn't JSON", path.display()))?;
        if !v.is_object() {
            bail!("{} isn't a JSON object, as redone writes it", path.display());
        }
        let mut b = Backup { dir: dir.to_owned(), ..Default::default() };
        for (key, t) in v["tenants"].as_object().into_iter().flatten() {
            let id = text(t, "id").unwrap_or_else(|| key.clone());
            b.tenants.push(Tenant {
                name: text(t, "name").unwrap_or_else(|| id.clone()),
                id,
                ntfy: text(t, "ntfyTopic").is_some(),
                auto_approve_updates: t["autoApproveUpdates"].as_bool().unwrap_or(false),
            });
        }
        b.tenants.sort_by(|x, y| x.id.cmp(&y.id));
        for (hash, t) in v["tokens"].as_object().into_iter().flatten() {
            let (Some(id), Some(tenant)) = (text(t, "id"), text(t, "tenant")) else { continue };
            b.tokens.push(Token {
                id,
                tenant,
                agent: text(t, "agent").unwrap_or_default(),
                hash: hash.to_ascii_lowercase(),
                at: t["at"].as_u64().unwrap_or(0),
            });
        }
        b.tokens.sort_by(|x, y| (x.at, &x.id).cmp(&(y.at, &y.id)));
        let plans = &v["plans"];
        for (key, a) in v["apps"].as_object().into_iter().flatten() {
            let Some(tenant) = text(a, "tenant") else { continue };
            let current = text(a, "current");
            let spec = current.as_ref().map(|c| plans[c]["spec"].clone()).unwrap_or(Value::Null);
            b.apps.push(App {
                name: text(a, "name").unwrap_or_else(|| key.clone()),
                tenant,
                current,
                spec,
                deploys: a["history"].as_array().map_or(0, Vec::len),
            });
        }
        b.apps.sort_by(|x, y| x.name.cmp(&y.name));
        Ok(b)
    }
}

/// `TENANT=ID[,ID…]`: the tenant's people by numeric GitHub id, the first
/// the one its tokens become the agents of.
pub fn parse_people(s: &str) -> Result<(String, Vec<u64>), String> {
    let bad = || "TENANT=GITHUB_ID[,GITHUB_ID…] (numeric GitHub ids, never logins)".to_owned();
    let (t, ids) = s.split_once('=').ok_or_else(bad)?;
    let ids: Vec<u64> = ids.split(',').map(|i| i.trim().parse::<u64>()).collect::<Result<_, _>>().map_err(|_| bad())?;
    if t.trim().is_empty() || ids.is_empty() || ids.contains(&0) {
        return Err(bad());
    }
    Ok((t.trim().to_owned(), ids))
}

/// `TENANT=TEAM`: a team's id, whose project a tenant of several people
/// becomes.
pub fn parse_team(s: &str) -> Result<(String, String), String> {
    match s.split_once('=') {
        Some((t, team)) if !t.trim().is_empty() && !team.trim().is_empty() => {
            Ok((t.trim().to_owned(), team.trim().to_owned()))
        }
        _ => Err("TENANT=TEAM (the team's id)".into()),
    }
}

/// How the import is asked.
#[derive(Debug, Clone, Default)]
pub struct Options {
    /// Each tenant's people, by GitHub id: the first is its lead.
    pub people: BTreeMap<String, Vec<u64>>,
    /// Tenants that become a team's project.
    pub teams: BTreeMap<String, String>,
    /// The driver each app's environment gets: planter's ship (M233)
    /// unless said (tests and testnet: `fake`).
    pub driver: String,
    pub dry_run: bool,
}

/// The driver redone's apps become environments of: planter's ship
/// (M233), which ships a source to loam as redone's apply did.
pub const SHIP_DRIVER: &str = "planter-ship";

/// A person of a tenant.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Person {
    pub github_id: u64,
    /// Their Arugula account, if one signs in with that GitHub id.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub account: Option<String>,
    /// Their role on the tenant's team, for a team's project.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub role: Option<String>,
}

/// What became of a tenant.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "outcome", rename_all = "snake_case")]
pub enum TenantOutcome {
    /// It's this project now (or, dry, would be): `made` when that project
    /// is made now.
    Project { project: String, personal: bool, made: bool },
    /// An earlier run made it that project, its tokens `account`'s:
    /// unchanged.
    ImportedBefore { project: String, account: Option<String>, at: u64 },
    /// The operator didn't say whose it is (`--people`).
    WaitingPeople,
    /// Its first person isn't on Arugula yet: a later run picks it up.
    WaitingSignIn { github_id: u64 },
    /// Its first person isn't an owner or editor on its team.
    NotOnTeam { team: String },
    /// Not one the import can carry over, and why.
    Refused { why: String },
}

/// What became of a token.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "outcome", rename_all = "snake_case")]
pub enum TokenOutcome {
    /// An agent grant token now (or, dry, would be), owned by `account`.
    Carried {
        account: String,
        agent_token: Option<String>,
    },
    /// An earlier run carried it over: unchanged, revoked or not.
    ImportedBefore {
        agent_token: Option<String>,
        at: u64,
    },
    /// It waits for its tenant.
    WaitingTenant,
    /// Its owner holds as many live agent tokens as they may.
    TooMany {
        account: String,
    },
    Refused {
        why: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TokenLine {
    #[serde(flatten)]
    pub token: Token,
    #[serde(flatten)]
    pub outcome: TokenOutcome,
}

/// What became of an app.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "outcome", rename_all = "snake_case")]
pub enum AppOutcome {
    /// It's this environment of the tenant's project now (or, dry, would
    /// be), and its name the project's.
    Environment {
        environment: Option<String>,
        driver: String,
    },
    /// The project has an environment by its name already: recorded as
    /// the app's, and left as it is.
    Already {
        environment: String,
    },
    /// An earlier run carried it over: unchanged.
    ImportedBefore {
        environment: Option<String>,
        at: u64,
    },
    /// It waits for its tenant.
    WaitingTenant,
    /// This platform has no such driver yet.
    WaitingDriver {
        driver: String,
    },
    Refused {
        why: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct AppLine {
    #[serde(flatten)]
    pub app: App,
    /// Its tenant's project, when the tenant has one.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub project: Option<String>,
    #[serde(flatten)]
    pub outcome: AppOutcome,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TenantLine {
    #[serde(flatten)]
    pub tenant: Tenant,
    pub people: Vec<Person>,
    #[serde(flatten)]
    pub outcome: TenantOutcome,
    /// What of it isn't carried over, and why.
    pub dropped: Vec<String>,
    pub tokens: Vec<TokenLine>,
    pub apps: Vec<AppLine>,
}

/// What the import did (or, dry, would do).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Report {
    pub dry_run: bool,
    pub tenants: Vec<TenantLine>,
    /// Tokens and apps of no tenant in the backup.
    pub orphans: Vec<String>,
}

impl Report {
    pub fn tokens_carried(&self) -> usize {
        self.tenants
            .iter()
            .flat_map(|t| &t.tokens)
            .filter(|t| matches!(t.outcome, TokenOutcome::Carried { .. }))
            .count()
    }

    /// As the operator reads it.
    pub fn render(&self, dir: &Path) -> String {
        use std::fmt::Write as _;
        let dry = self.dry_run;
        let mut out = String::new();
        let _ = writeln!(out, "redone's backup at {} (read only){}", dir.display(), if dry { ", DRY RUN" } else { "" });
        for t in &self.tenants {
            let what = match &t.outcome {
                TenantOutcome::Project { project, personal, made } => format!(
                    "{} {}{project}",
                    if dry { "would become" } else { "became" },
                    match (personal, made) {
                        (true, true) => "a new personal project ",
                        (true, false) => "the personal project ",
                        (false, _) => "the team's project ",
                    }
                ),
                TenantOutcome::ImportedBefore { project, at, .. } => format!("imported before (at {at}) as {project}"),
                TenantOutcome::WaitingPeople => {
                    format!("waiting: say whose it is with --people {}=<GitHub id>[,…] (numeric ids)", t.tenant.id)
                }
                TenantOutcome::WaitingSignIn { github_id } => {
                    format!("waiting: nobody on Arugula signs in with GitHub id {github_id} yet")
                }
                TenantOutcome::NotOnTeam { team } => {
                    format!("not carried over: its first person isn't an owner or editor of team {team}")
                }
                TenantOutcome::Refused { why } => format!("not carried over: {why}"),
            };
            let _ = writeln!(out, "\ntenant {} ({}): {what}", t.tenant.id, t.tenant.name);
            for p in &t.people {
                let _ = writeln!(
                    out,
                    "  person {}: {}{}",
                    p.github_id,
                    p.account.as_deref().unwrap_or("not on Arugula yet (waits)"),
                    p.role.as_ref().map(|r| format!(", {r} on the team")).unwrap_or_default()
                );
            }
            for d in &t.dropped {
                let _ = writeln!(out, "  not carried over: {d}");
            }
            for k in &t.tokens {
                let what = match &k.outcome {
                    TokenOutcome::Carried { account, agent_token } => format!(
                        "{} {account}'s agent token{}",
                        if dry { "would become" } else { "became" },
                        agent_token.as_ref().map(|a| format!(" {a}")).unwrap_or_default()
                    ),
                    TokenOutcome::ImportedBefore { agent_token, at } => format!(
                        "carried over before (at {at}){}: unchanged",
                        agent_token.as_ref().map(|a| format!(" as {a}")).unwrap_or_default()
                    ),
                    TokenOutcome::WaitingTenant => "waits for its tenant".into(),
                    TokenOutcome::TooMany { account } => {
                        format!("waiting: {account} holds {MAX_LIVE} live agent tokens already")
                    }
                    TokenOutcome::Refused { why } => format!("not carried over: {why}"),
                };
                let _ = writeln!(out, "  token {} ({}): {what}", k.token.id, k.token.agent);
            }
            for a in &t.apps {
                let what = match &a.outcome {
                    AppOutcome::Environment { environment, driver } => format!(
                        "{} its environment ({driver}){}",
                        if dry { "would become" } else { "became" },
                        environment.as_ref().map(|e| format!(" {e}")).unwrap_or_default()
                    ),
                    AppOutcome::Already { environment } => format!("the project's environment {environment} already"),
                    AppOutcome::ImportedBefore { at, .. } => format!("carried over before (at {at}): unchanged"),
                    AppOutcome::WaitingTenant => "waits for its tenant".into(),
                    AppOutcome::WaitingDriver { driver } => {
                        format!("waiting: this platform has no {driver} driver yet")
                    }
                    AppOutcome::Refused { why } => format!("not carried over: {why}"),
                };
                let _ = writeln!(out, "  app {} ({} deploys): {what}", a.app.name, a.app.deploys);
            }
        }
        for o in &self.orphans {
            let _ = writeln!(out, "\nnot carried over: {o}");
        }
        let _ = writeln!(
            out,
            "\n{} tenant(s), {} token(s) {}{}",
            self.tenants.len(),
            self.tokens_carried(),
            if dry { "would be carried over" } else { "carried over" },
            if dry { "; nothing written" } else { "" }
        );
        out
    }
}

/// The account that signs in with GitHub id `id`, asked once per run.
async fn account_of(
    app: &Platform,
    seen: &mut HashMap<u64, Option<String>>,
    id: u64,
) -> anyhow::Result<Option<String>> {
    if let Some(a) = seen.get(&id) {
        return Ok(a.clone());
    }
    let a = app.hub.github_account(id).await?;
    // By the number, both ways: the account's own GitHub id must be it.
    let a = match a {
        Some(a) => app.hub.github_user(&a).await?.filter(|u| u.id == id).map(|_| a),
        None => None,
    };
    seen.insert(id, a.clone());
    Ok(a)
}

/// The tenants, their tokens and apps, as the module's docs say. Writes
/// only `redone_imports`, `agent_tokens` and their `grants`, and a
/// personal or team project when one has to be made; nothing when dry.
pub async fn import(app: &Platform, backup: &Backup, o: &Options) -> anyhow::Result<Report> {
    let now = now_ms();
    let mut seen = HashMap::new();
    let mut tenants = vec![];
    for t in &backup.tenants {
        let mut line = TenantLine {
            tenant: t.clone(),
            people: vec![],
            outcome: TenantOutcome::WaitingPeople,
            dropped: vec!["its approver key: an owner approves each deploy on their device, with a passkey".into()],
            tokens: vec![],
            apps: vec![],
        };
        if t.ntfy {
            line.dropped.push("its ntfy topic: the project's owners get a phone card for each deploy".into());
        }
        if t.auto_approve_updates {
            line.dropped
                .push("auto-approve-updates: every deploy waits for an owner's device, updates too (aru-101)".into());
        }
        let ids = o.people.get(&t.id).cloned().unwrap_or_default();
        for id in &ids {
            line.people.push(Person { github_id: *id, account: account_of(app, &mut seen, *id).await?, role: None });
        }
        line.outcome = tenant(app, o, t, &mut line.people, now).await?;
        let (project, lead) = match &line.outcome {
            TenantOutcome::Project { project, made, .. } => {
                (Some(project.clone()).filter(|_| !*made || !o.dry_run), line.people[0].account.clone())
            }
            TenantOutcome::ImportedBefore { project, account, .. } => (Some(project.clone()), account.clone()),
            _ => (None, None),
        };
        let ready = matches!(line.outcome, TenantOutcome::Project { .. } | TenantOutcome::ImportedBefore { .. });
        for k in backup.tokens.iter().filter(|k| k.tenant == t.id) {
            let lead = if ready { lead.as_deref() } else { None };
            let outcome = token(app, o, t, k, lead, project.as_deref(), now).await?;
            line.tokens.push(TokenLine { token: k.clone(), outcome });
        }
        for a in backup.apps.iter().filter(|a| a.tenant == t.id) {
            let outcome =
                if ready { app_env(app, o, t, a, project.as_deref(), now).await? } else { AppOutcome::WaitingTenant };
            line.apps.push(AppLine { app: a.clone(), project: project.clone(), outcome });
        }
        tenants.push(line);
    }
    let known: std::collections::HashSet<&str> = backup.tenants.iter().map(|t| t.id.as_str()).collect();
    let mut orphans = vec![];
    for k in backup.tokens.iter().filter(|k| !known.contains(k.tenant.as_str())) {
        orphans.push(format!("token {} of tenant {}, which isn't in the backup", k.id, k.tenant));
    }
    for a in backup.apps.iter().filter(|a| !known.contains(a.tenant.as_str())) {
        orphans.push(format!("app {} of tenant {}, which isn't in the backup", a.name, a.tenant));
    }
    for t in o.people.keys().chain(o.teams.keys()) {
        if !known.contains(t.as_str()) {
            orphans.push(format!("--people/--team name tenant {t}, which isn't in the backup"));
        }
    }
    Ok(Report { dry_run: o.dry_run, tenants, orphans })
}

/// One tenant's project.
async fn tenant(
    app: &Platform,
    o: &Options,
    t: &Tenant,
    people: &mut [Person],
    now: u64,
) -> anyhow::Result<TenantOutcome> {
    if let Some(i) = app.db.redone_imported("tenant", &t.id)? {
        return Ok(TenantOutcome::ImportedBefore { project: i.project, account: i.account, at: i.at });
    }
    let Some(lead) = people.first() else { return Ok(TenantOutcome::WaitingPeople) };
    let Some(account) = lead.account.clone() else {
        return Ok(TenantOutcome::WaitingSignIn { github_id: lead.github_id });
    };
    let record = |project: &str| Imported {
        kind: "tenant".into(),
        redone_id: t.id.clone(),
        tenant: t.id.clone(),
        project: project.to_owned(),
        account: Some(account.clone()),
        platform_id: None,
        at: now,
    };
    match o.teams.get(&t.id) {
        None if people.len() > 1 => Ok(TenantOutcome::Refused {
            why: format!(
                "it has {} people: name the team whose project it becomes (--team {}=<team>)",
                people.len(),
                t.id
            ),
        }),
        None => {
            if o.dry_run {
                return Ok(match app.db.redone_personal_project(&account)? {
                    Some(p) => TenantOutcome::Project { project: p, personal: true, made: false },
                    None => TenantOutcome::Project {
                        project: format!("(the personal project of {account}, made now)"),
                        personal: true,
                        made: true,
                    },
                });
            }
            let had = app.db.redone_personal_project(&account)?;
            let Some(p) = crate::projects::personal_project(app, &account).await? else {
                return Ok(TenantOutcome::Refused { why: format!("{account}'s account is gone") });
            };
            app.db.record_redone_import(&record(&p))?;
            Ok(TenantOutcome::Project { project: p, personal: true, made: had.is_none() })
        }
        Some(team) => {
            let Some(roster) = app.cache.team_now(&*app.hub, team).await? else {
                return Ok(TenantOutcome::Refused { why: format!("there's no team {team}") });
            };
            for p in people.iter_mut() {
                p.role = p.account.as_deref().and_then(|a| roster.role(a)).map(|r| r.as_str().to_owned());
            }
            if !matches!(roster.role(&account), Some(TeamRole::Owner | TeamRole::Editor)) {
                return Ok(TenantOutcome::NotOnTeam { team: team.clone() });
            }
            let had = app.db.team_default_project(team)?;
            if o.dry_run {
                return Ok(TenantOutcome::Project {
                    project: had.clone().unwrap_or_else(|| format!("(team {team}'s project, made now)")),
                    personal: false,
                    made: had.is_none(),
                });
            }
            let Some(p) = crate::projects::team_project(app, team).await? else {
                return Ok(TenantOutcome::Refused { why: format!("there's no team {team}") });
            };
            app.db.record_redone_import(&record(&p))?;
            Ok(TenantOutcome::Project { project: p, personal: false, made: had.is_none() })
        }
    }
}

/// An app's environment's settings for `driver`, from its live plan's
/// spec: planter's ship takes the app's name, port, health path, port
/// variable and secrets' names (M233); a driver that takes none gets none.
fn app_settings(driver: &str, a: &App) -> arugula_e2e::drivers::Settings {
    let mut s = arugula_e2e::drivers::Settings::new();
    if driver != SHIP_DRIVER {
        return s;
    }
    s.insert("app".into(), a.name.clone());
    if let Some(port) = a.spec["port"].as_u64() {
        s.insert("port".into(), port.to_string());
    }
    if let Some(h) = a.spec["health"].as_str() {
        s.insert("health".into(), h.to_owned());
    }
    if let Some(e) = a.spec["portEnv"].as_str() {
        s.insert("port_env".into(), e.to_owned());
    }
    let secrets: Vec<&str> = a.spec["secrets"].as_array().into_iter().flatten().filter_map(Value::as_str).collect();
    if !secrets.is_empty() {
        s.insert("secrets".into(), secrets.join(","));
    }
    s
}

/// One app: an environment of its tenant's `project` (`None` while dry
/// and the project isn't made), named as the app, with the driver the
/// import is given.
async fn app_env(
    app: &Platform,
    o: &Options,
    t: &Tenant,
    a: &App,
    project: Option<&str>,
    now: u64,
) -> anyhow::Result<AppOutcome> {
    if let Some(i) = app.db.redone_imported("app", &a.name)? {
        return Ok(AppOutcome::ImportedBefore { environment: i.platform_id, at: i.at });
    }
    if !crate::infisical::valid_app(&a.name) || !crate::db::deploys::valid_environment_name(&a.name) {
        return Ok(AppOutcome::Refused { why: format!("{:?} isn't an app's name here", a.name) });
    }
    let Some(d) = crate::drivers::driver(&o.driver) else {
        return Ok(AppOutcome::WaitingDriver { driver: o.driver.clone() });
    };
    let record = |project: &str, env: &str| Imported {
        kind: "app".into(),
        redone_id: a.name.clone(),
        tenant: t.id.clone(),
        project: project.to_owned(),
        account: None,
        platform_id: Some(env.to_owned()),
        at: now,
    };
    if let Some(p) = project
        && let Some(e) = app.db.environments(p)?.into_iter().find(|e| e.name == a.name)
    {
        if !o.dry_run {
            app.db.record_redone_import(&record(p, &e.id))?;
        }
        return Ok(AppOutcome::Already { environment: e.id });
    }
    if let Err(why) = project.map_or(Ok(()), |p| d.allowed_in(p)) {
        return Ok(AppOutcome::Refused { why });
    }
    let settings = match d.settings(&app_settings(&o.driver, a)) {
        Ok(s) => s,
        Err(why) => return Ok(AppOutcome::Refused { why }),
    };
    let (Some(p), false) = (project, o.dry_run) else {
        return Ok(AppOutcome::Environment { environment: None, driver: o.driver.clone() });
    };
    let env = match app.db.create_environment_with(p, &a.name, &o.driver, &[], &settings, now) {
        Ok(e) => e,
        Err(e) => return Ok(AppOutcome::Refused { why: format!("{e:#}") }),
    };
    app.db.record_redone_import(&record(p, &env.id))?;
    tracing::info!(project = %p, app = %a.name, environment = %env.id, "carried a redone app over");
    Ok(AppOutcome::Environment { environment: Some(env.id), driver: o.driver.clone() })
}

/// A token's name on the platform: "redone: <its agent>", at most 64
/// characters, on one line.
fn token_name(agent: &str) -> String {
    let clean: String = agent
        .chars()
        .filter(|c| !c.is_control() && !crate::agent_tokens::is_format_char(*c))
        .collect::<String>()
        .trim()
        .to_owned();
    let n = if clean.is_empty() { "redone".to_owned() } else { format!("redone: {clean}") };
    n.chars().take(64).collect()
}

/// One token, for its tenant's `lead` over `project` (both `None` while
/// the tenant waits).
async fn token(
    app: &Platform,
    o: &Options,
    t: &Tenant,
    k: &Token,
    lead: Option<&str>,
    project: Option<&str>,
    now: u64,
) -> anyhow::Result<TokenOutcome> {
    if let Some(i) = app.db.redone_imported("token", &k.id)? {
        return Ok(TokenOutcome::ImportedBefore { agent_token: i.platform_id, at: i.at });
    }
    if !good_hash(&k.hash) {
        return Ok(TokenOutcome::Refused { why: "its hash isn't a SHA-256".into() });
    }
    let Some(account) = lead else { return Ok(TokenOutcome::WaitingTenant) };
    if app.db.agent_token_by_hash(&k.hash)?.is_some() {
        return Ok(TokenOutcome::Refused { why: "a token with its hash is here already".into() });
    }
    if app.db.live_agent_tokens(account)? >= MAX_LIVE {
        return Ok(TokenOutcome::TooMany { account: account.to_owned() });
    }
    let Some(project) = project else {
        // Dry, and the project is made only by a real run.
        return Ok(TokenOutcome::Carried { account: account.to_owned(), agent_token: None });
    };
    // Only over what its owner reads, on the hub's word.
    let scope = crate::grants::AgentScope::Project(project.to_owned());
    if crate::grants::owner_reads(app, account, &scope, true).await?.is_none() {
        return Ok(TokenOutcome::Refused { why: format!("{account} doesn't read {project}") });
    }
    if o.dry_run {
        return Ok(TokenOutcome::Carried { account: account.to_owned(), agent_token: None });
    }
    let (id, _) = crate::db::agent_tokens::new_token();
    let new = NewToken {
        id,
        account: account.to_owned(),
        name: token_name(&k.agent),
        token_hash: k.hash.clone(),
        minted_from: format!("carried over from redone (its token {}, tenant {})", k.id, t.id),
    };
    let grants: Vec<(&str, &str)> = POWERS.iter().map(|p| (*p, project)).collect();
    let Some(made) = app.db.import_redone_token(&k.id, &t.id, &new, &grants, now)? else {
        return Ok(TokenOutcome::Refused { why: "a token with its hash is here already".into() });
    };
    tracing::info!(%account, token = %made.id, redone = %k.id, %project, "carried a redone token over");
    notify(app, account, &made.name, &made.id);
    Ok(TokenOutcome::Carried { account: account.to_owned(), agent_token: Some(made.id) })
}

/// Tell the token's owner that their redone token works on Arugula now.
fn notify(app: &Platform, account: &str, name: &str, id: &str) {
    let msg = json!({
        "title": "Your redone token works on Arugula",
        "body": format!(
            "\u{201c}{name}\u{201d} was carried over from redone: its agent plans deploys and sets secrets in your \
             project through Arugula's MCP endpoint, and you approve each deploy on your phone. Revoke it in \
             Settings, Agents with no machine."
        ),
        "tag": format!("agent-token-{id}"),
        "control": true,
    });
    let hub = app.hub.clone();
    let (account, id) = (account.to_owned(), id.to_owned());
    tokio::spawn(async move {
        if let Err(e) = hub.push(&account, &msg).await {
            tracing::warn!(%account, token = %id, error = %e, "the carried-over token's notice didn't go");
        }
    });
}

/// `arugula-platform import redone …`.
#[derive(clap::Parser, Debug)]
struct Args {
    #[command(subcommand)]
    what: What,
}

#[derive(clap::Subcommand, Debug)]
enum What {
    /// redone's tenants as projects, their rdn_ tokens as agent grant tokens,
    /// and their apps reported (M235). Safe to run again; never deletes;
    /// reads the backup only.
    Redone(RedoneArgs),
}

#[derive(clap::Args, Debug)]
struct RedoneArgs {
    /// A copy of redone's DATA_DIR: its state.json.
    #[arg(long)]
    backup: PathBuf,
    /// A tenant's people: `TENANT=GITHUB_ID[,GITHUB_ID…]`, numeric GitHub
    /// ids, the first the owner of its tokens. Repeatable. A tenant
    /// without one waits.
    #[arg(long = "people", value_parser = parse_people)]
    people: Vec<(String, Vec<u64>)>,
    /// A tenant that becomes a team's project: `TENANT=TEAM`. Repeatable.
    /// Without one, a tenant is its one person's personal project.
    #[arg(long = "team", value_parser = parse_team)]
    teams: Vec<(String, String)>,
    /// The driver each app's environment gets: planter's ship (M233). Only
    /// a test platform has `fake`.
    #[arg(long, default_value = SHIP_DRIVER)]
    driver: String,
    /// Say what would change, and write nothing.
    #[arg(long)]
    dry_run: bool,
    /// The report as JSON.
    #[arg(long)]
    json: bool,
    /// The platform's SQLite database, when ARUGULA_PLATFORM_DATABASE_URL
    /// doesn't name Postgres.
    #[arg(long, default_value = "platform.db", env = "ARUGULA_PLATFORM_DB")]
    db: PathBuf,
    #[command(flatten)]
    database: crate::db::Args,
    /// The hub's internal API, as the platform has it.
    #[arg(long, env = "ARUGULA_PLATFORM_HUB")]
    hub: String,
    /// The key the hub and the platform share.
    #[arg(long, env = "ARUGULA_PLATFORM_KEY", hide_env_values = true)]
    key: String,
}

/// What the import needs from the running platform's environment, when
/// this shell hasn't got it, taken from the server's own (as the windows'
/// tools do).
const SERVER_ENV: [&str; 4] =
    ["ARUGULA_PLATFORM_HUB", "ARUGULA_PLATFORM_KEY", "ARUGULA_PLATFORM_DATABASE_URL", "ARUGULA_PLATFORM_DB"];

/// `<prog> redone …` (`prog` is `arugula-platform import`). Called before
/// any thread starts: it may set this process's environment.
pub fn run(prog: &str, args: impl Iterator<Item = String>) -> anyhow::Result<()> {
    for var in SERVER_ENV {
        if std::env::var_os(var).is_none()
            && let Some((_, v)) = crate::window::running_env(var)
        {
            // SAFETY: no other thread has started yet.
            unsafe { std::env::set_var(var, v) };
        }
    }
    let a = <Args as clap::Parser>::parse_from(std::iter::once(prog.to_owned()).chain(args));
    let What::Redone(a) = a.what;
    let backup = Backup::read(&a.backup)?;
    let rt = tokio::runtime::Builder::new_multi_thread().enable_all().build()?;
    rt.block_on(async move {
        let key = arugula_control_common::principal::PrincipalKey::from_hex(&a.key).context("ARUGULA_PLATFORM_KEY")?;
        let hub = std::sync::Arc::new(crate::remote::HttpHub::new(&a.hub, key)?);
        let db = a.database.open(&a.db).context("opening the platform's database")?;
        let http = arugula_control_common::roots::client(std::time::Duration::from_secs(15))?;
        let p = Platform::new(crate::Config::default(), db, hub, http, Default::default());
        let o = Options {
            people: a.people.into_iter().collect(),
            teams: a.teams.into_iter().collect(),
            driver: a.driver,
            dry_run: a.dry_run,
        };
        let r = import(&p, &backup, &o).await?;
        if a.json {
            println!("{}", serde_json::to_string_pretty(&r)?);
        } else {
            print!("{}", r.render(&backup.dir));
        }
        // The owners' notices go before the process does.
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
        anyhow::Ok(())
    })
}

#[cfg(test)]
#[path = "redone_tests.rs"]
mod tests;
