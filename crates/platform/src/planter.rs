//! Absorbing planter (M232, platform step 8, Arugula #30): its accounts,
//! repos and boxes, read from a backup of planter's `DATA_DIR`.
//!
//! **The backup** is a copy of planter's data directory, the JSON files
//! its `store.mjs` keeps (one Longhorn volume, `/data`). Two are read, and
//! nothing is ever written to it:
//! - `accounts.json`: `{ "<GitHub id>": { id, login, name, avatarUrl,
//!   createdAt, lastSignInAt, platformAccount } }`, keyed by the person's
//!   numeric GitHub id, which is also planter's own account id.
//!   `platformAccount` is M229's link, set only when the person linked
//!   through planter, else missing or null.
//! - `boxes.json`: `{ "<name>": { name, title, repo, branch, owner, state,
//!   createdAt, url, fountain, secrets, … } }`. `owner` is the planting
//!   account's GitHub id (a string); `repo` is `OWNER/NAME` as GitHub spelled
//!   it at planting. A missing `boxes.json` is no boxes, as planter reads it.
//!
//! The others (`sessions.json`, with sealed GitHub tokens; `shipTokens.json`;
//! `ships.json`) aren't read: the import needs none of them, so the backup
//! handed over need not hold them. A box's sealed `secrets` and its
//! `fountain` handles are never read or printed.
//!
//! **The import is two commands**, each where its rows live:
//! - `arugula-control import planter` (the hub): each account with a
//!   numeric GitHub id becomes an account link (M229, client `planter`) to
//!   the hub account with that GitHub id, audited `imported` by the
//!   operator. Never by login.
//! - `arugula-platform import planter` ([`import_repos`], this module): the
//!   repos planter's boxes were planted from become step 4 repo links, on
//!   the planter's personal project, or on a team's project for an
//!   organization the operator maps (`--org`). Each box is reported, and
//!   nothing of it is imported: boxes were Managoat conversations, and
//!   Managoat is gone (v0.60.2); their owners get step 5's hosted
//!   workspaces instead.
//!
//! Both are safe to run again: what's done is recorded (the hub's link
//! audit; `planter_imports` here), so a second run changes nothing, and a
//! link a person ended since is never made again. Neither deletes anything.
//! People not on Arugula yet are reported as waiting, and a later run picks
//! them up. `--dry-run` prints exactly what a run would change, and writes
//! nothing.

use std::{
    collections::{BTreeMap, HashMap},
    path::{Path, PathBuf},
};

use anyhow::{Context as _, bail};
use arugula_e2e::{now_ms, team::TeamRole};
use serde::Serialize;
use serde_json::Value;

use crate::{
    Platform,
    db::forge::{Installation, Linked, LinkedRepo},
    forge::{Caller, good_repo},
};

/// The client id planter's accounts are linked under, on the hub (M229).
pub const CLIENT: &str = "planter";

/// One of planter's accounts.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Account {
    /// The person's numeric GitHub id: planter's own id for them too.
    pub github_id: u64,
    /// The login planter last saw. Only ever shown, never matched on.
    pub login: String,
    /// The platform account planter says it's linked to (M229), if any.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub platform_account: Option<String>,
}

impl Account {
    /// The product's user id the hub links: planter's own, the GitHub id.
    pub fn user(&self) -> String {
        self.github_id.to_string()
    }
}

/// An entry of `accounts.json` the import can't use.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Skipped {
    /// Its key in `accounts.json`.
    pub key: String,
    pub login: String,
    pub why: String,
}

/// One of planter's boxes, as far as the import reads it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct PlanterBox {
    pub name: String,
    /// The planter's GitHub id, as `boxes.json` has it.
    pub owner: String,
    /// `OWNER/NAME`, when the box says.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub repo: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub branch: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub state: Option<String>,
    /// When it was planted (ms).
    pub created_at: u64,
}

/// planter's backup, read.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct Backup {
    #[serde(skip)]
    pub dir: PathBuf,
    /// By GitHub id.
    pub accounts: Vec<Account>,
    pub skipped: Vec<Skipped>,
    /// Oldest first.
    pub boxes: Vec<PlanterBox>,
}

fn text(v: &Value, k: &str) -> Option<String> {
    v.get(k).and_then(Value::as_str).map(str::trim).filter(|s| !s.is_empty()).map(str::to_owned)
}

/// A GitHub id as planter writes one: a number, or a string of digits.
fn number(v: Option<&Value>) -> Option<u64> {
    match v? {
        Value::Number(n) => n.as_u64(),
        Value::String(s) => s.trim().parse().ok(),
        _ => None,
    }
    .filter(|n| *n > 0)
}

fn json_file(dir: &Path, name: &str) -> anyhow::Result<Option<serde_json::Map<String, Value>>> {
    let path = dir.join(name);
    let raw = match std::fs::read(&path) {
        Ok(b) => b,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(e).with_context(|| format!("reading {}", path.display())),
    };
    match serde_json::from_slice(&raw).with_context(|| format!("{} isn't JSON", path.display()))? {
        Value::Object(m) => Ok(Some(m)),
        _ => bail!("{} isn't a JSON object, as planter writes it", path.display()),
    }
}

impl Backup {
    /// planter's backup in `dir`: its `accounts.json`, and `boxes.json` if
    /// it's there. Only read.
    pub fn read(dir: &Path) -> anyhow::Result<Backup> {
        let Some(accounts) = json_file(dir, "accounts.json")? else {
            bail!("no accounts.json in {}: is it a copy of planter's DATA_DIR?", dir.display())
        };
        let mut b = Backup { dir: dir.to_owned(), ..Default::default() };
        for (key, v) in &accounts {
            let login = text(v, "login").unwrap_or_default();
            let skip = |why: &str| Skipped { key: key.clone(), login: login.clone(), why: why.into() };
            let Some(id) = number(v.get("id")) else {
                b.skipped.push(skip("no numeric GitHub id"));
                continue;
            };
            if key.trim().parse::<u64>().ok() != Some(id) {
                b.skipped.push(skip("its key isn't its GitHub id"));
                continue;
            }
            b.accounts.push(Account { github_id: id, login, platform_account: text(v, "platformAccount") });
        }
        b.accounts.sort_by_key(|a| a.github_id);
        for (key, v) in json_file(dir, "boxes.json")?.unwrap_or_default() {
            let Some(owner) = number(v.get("owner")) else { continue };
            b.boxes.push(PlanterBox {
                name: text(&v, "name").unwrap_or(key),
                owner: owner.to_string(),
                repo: text(&v, "repo"),
                branch: text(&v, "branch"),
                state: text(&v, "state"),
                created_at: number(v.get("createdAt")).unwrap_or(0),
            });
        }
        b.boxes.sort_by(|x, y| (x.created_at, &x.name).cmp(&(y.created_at, &y.name)));
        Ok(b)
    }

    /// The account with this GitHub id (as `boxes.json` writes an owner).
    pub fn account(&self, owner: &str) -> Option<&Account> {
        let id: u64 = owner.parse().ok()?;
        self.accounts.iter().find(|a| a.github_id == id)
    }

    /// Each repo planted, with who planted it, once per (person, repo), in
    /// the order of their first box: the first planter of a repo is the
    /// first asked to link it.
    pub fn planted(&self) -> Vec<(String, String)> {
        let mut seen = std::collections::HashSet::new();
        self.boxes
            .iter()
            .filter_map(|b| Some((b.owner.clone(), b.repo.clone()?)))
            .filter(|(o, r)| seen.insert((o.clone(), r.to_ascii_lowercase())))
            .collect()
    }
}

/// Where an organization's repos go (`--org ORG=PROJECT`): a team's
/// project, or each planter's own.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OrgTarget {
    Personal,
    Project(String),
}

/// `ORG=prj_…` or `ORG=personal`.
pub fn parse_org(s: &str) -> Result<(String, OrgTarget), String> {
    let (org, to) = s.split_once('=').ok_or("ORG=PROJECT (a project's id, or `personal`)")?;
    let (org, to) = (org.trim(), to.trim());
    if org.is_empty() || to.is_empty() {
        return Err("ORG=PROJECT (a project's id, or `personal`)".into());
    }
    let to = if to == "personal" { OrgTarget::Personal } else { OrgTarget::Project(to.to_owned()) };
    Ok((org.to_ascii_lowercase(), to))
}

/// How the repo import is asked.
#[derive(Debug, Clone, Default)]
pub struct Options {
    /// The hub's client planter's accounts are linked under.
    pub client: String,
    /// Organizations (lower case) and where their repos go.
    pub orgs: BTreeMap<String, OrgTarget>,
    /// Write nothing; say what would change.
    pub dry_run: bool,
}

/// What became of one planted repo.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "outcome", rename_all = "snake_case")]
pub enum Outcome {
    /// Linked now (or, in a dry run, would be): `personal` when it's the
    /// planter's personal project, `made` when that project is made now.
    Linked { project: String, repo_id: u64, personal: bool, made: bool },
    /// It was linked to that project already (by hand): recorded, and
    /// left as it is.
    Already { project: String, repo_id: u64 },
    /// An earlier run imported it (for this planter, or the repo for
    /// another): never touched again, even if it was unlinked since.
    ImportedBefore { project: String, at: u64 },
    /// The planter isn't linked on the hub: not on Arugula yet, or the
    /// hub's import hasn't run. A later run picks it up.
    WaitingSignIn,
    /// The platform's GitHub App isn't installed for the repo. A later run
    /// picks it up.
    WaitingApp { install_url: String },
    /// An organization owns it, and the operator didn't say where its repos
    /// go (`--org`). A later run picks it up.
    WaitingOrg { org: String },
    /// Linking takes admin or maintain on GitHub.
    Role { login: String, role: String },
    /// It's linked to another project.
    Elsewhere { project: String },
    /// It was linked here and unlinked since: never linked again by the
    /// import.
    Unlinked { project: String },
    /// The person isn't an owner or editor of the team project its
    /// organization maps to.
    NotMember { project: String },
    /// Not a repo the import can link, or GitHub refused: the reason.
    Refused { why: String },
}

impl Outcome {
    /// Whether it writes something (or, dry, would).
    pub fn changes(&self) -> bool {
        matches!(self, Outcome::Linked { .. } | Outcome::Already { .. })
    }

    fn waits(&self) -> bool {
        matches!(self, Outcome::WaitingSignIn | Outcome::WaitingApp { .. } | Outcome::WaitingOrg { .. })
    }
}

/// One planted repo's line.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RepoLine {
    /// The planter's GitHub id.
    pub owner: String,
    pub login: String,
    /// As planter wrote it.
    pub repo: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub account: Option<String>,
    #[serde(flatten)]
    pub outcome: Outcome,
}

/// One box, reported.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct BoxLine {
    #[serde(flatten)]
    pub planted: PlanterBox,
    pub login: String,
    /// The owner's platform account, if they're linked on the hub.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub account: Option<String>,
}

/// What the repo import did (or, dry, would do).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Report {
    pub dry_run: bool,
    pub repos: Vec<RepoLine>,
    pub boxes: Vec<BoxLine>,
}

impl Report {
    /// How many repo links it makes (or would).
    pub fn changed(&self) -> usize {
        self.repos.iter().filter(|r| r.outcome.changes()).count()
    }

    pub fn waiting(&self) -> usize {
        self.repos.iter().filter(|r| r.outcome.waits()).count()
    }

    /// As the operator reads it.
    pub fn render(&self, dir: &Path) -> String {
        use std::fmt::Write as _;
        let mut out = String::new();
        let dry = self.dry_run;
        let _ =
            writeln!(out, "planter's backup at {} (read only){}", dir.display(), if dry { ", DRY RUN" } else { "" });
        let _ = writeln!(out, "\nrepos planted ({}):", self.repos.len());
        for r in &self.repos {
            let what = match &r.outcome {
                Outcome::Linked { project, repo_id, personal, made } => {
                    let whose = match (personal, made) {
                        (true, true) if dry => "",
                        (true, true) => "their new personal project ",
                        (true, false) => "their personal project ",
                        (false, _) => "team project ",
                    };
                    format!("{} to {whose}{project} (repo {repo_id})", if dry { "would link" } else { "linked" })
                }
                Outcome::Already { project, .. } => {
                    format!("already linked to {project}: {}", if dry { "would record it" } else { "recorded" })
                }
                Outcome::ImportedBefore { project, at } => format!("imported before (at {at}) to {project}: unchanged"),
                Outcome::WaitingSignIn => {
                    "waiting: not linked on the hub (not on Arugula yet, or `arugula-control import planter` \
                     hasn't run)"
                        .into()
                }
                Outcome::WaitingApp { install_url } => {
                    format!("waiting: the platform's GitHub App isn't installed for it ({install_url})")
                }
                Outcome::WaitingOrg { org } => {
                    format!("waiting: {org} owns it; say where its repos go with --org {org}=<project or personal>")
                }
                Outcome::Role { login, role } => {
                    format!("not linked: {login} has {role} on it, linking takes admin or maintain")
                }
                Outcome::Elsewhere { project } => format!("not linked: it's linked to {project}"),
                Outcome::Unlinked { project } => format!("not linked: it was unlinked from {project}, and stays so"),
                Outcome::NotMember { project } => {
                    format!("not linked: they aren't an owner or editor of {project}")
                }
                Outcome::Refused { why } => format!("not linked: {why}"),
            };
            let _ = writeln!(out, "  {:<40} {:<16} {what}", r.repo, r.login);
        }
        let _ = writeln!(
            out,
            "\nboxes ({}): nothing imported; Managoat is gone, so each owner gets a hosted workspace",
            self.boxes.len()
        );
        for b in &self.boxes {
            let p = &b.planted;
            let _ = writeln!(
                out,
                "  {:<24} {:<16} {}@{} {} planted {}{}",
                p.name,
                b.login,
                p.repo.as_deref().unwrap_or("?"),
                p.branch.as_deref().unwrap_or("?"),
                p.state.as_deref().unwrap_or("?"),
                p.created_at,
                match &b.account {
                    Some(a) => format!(", owner is {a} on Arugula"),
                    None => ", owner not linked on Arugula yet".into(),
                }
            );
        }
        let before = self.repos.iter().filter(|r| matches!(r.outcome, Outcome::ImportedBefore { .. })).count();
        let _ = writeln!(
            out,
            "\n{} repo links {}, {} imported before, {} waiting, {} not linked{}",
            self.changed(),
            if dry { "would be made or recorded" } else { "made or recorded" },
            before,
            self.waiting(),
            self.repos.len() - self.changed() - self.waiting() - before,
            if dry { "; nothing written" } else { "" }
        );
        out
    }
}

/// The repos planter's boxes were planted from, as step 4 repo links (the
/// module's docs). Writes only `linked_repos` (through the link M163
/// makes), `github_installations` (as linking does), a personal project
/// when one has to be made, and `planter_imports`; nothing when dry.
pub async fn import_repos(app: &Platform, backup: &Backup, o: &Options) -> anyhow::Result<Report> {
    let Some(gh) = &app.github else { bail!("no GitHub App here (GITHUB_APP_ID): repos can't be linked without it") };
    let now = now_ms();
    let mut accounts: HashMap<String, Option<String>> = HashMap::new();
    // What this run links, so a dry run's later lines see its earlier ones.
    let mut planned: HashMap<u64, String> = HashMap::new();
    let mut repos = Vec::new();
    for (owner, repo) in backup.planted() {
        let login = backup.account(&owner).map(|a| a.login.clone()).unwrap_or_default();
        let account = account_of(app, &o.client, &mut accounts, &owner).await?;
        let mut line = RepoLine {
            owner: owner.clone(),
            login,
            repo: repo.clone(),
            account: account.clone(),
            outcome: Outcome::WaitingSignIn,
        };
        if let Some((project, at)) = app.db.planter_imported(&owner, &repo)? {
            line.outcome = Outcome::ImportedBefore { project, at };
        } else if let Some(account) = &account {
            line.outcome = one_repo(app, gh, o, &mut planned, account, &owner, &repo, now).await?;
        }
        repos.push(line);
    }
    let mut boxes = Vec::new();
    for b in &backup.boxes {
        let login = backup.account(&b.owner).map(|a| a.login.clone()).unwrap_or_default();
        let account = account_of(app, &o.client, &mut accounts, &b.owner).await?;
        boxes.push(BoxLine { planted: b.clone(), login, account });
    }
    Ok(Report { dry_run: o.dry_run, repos, boxes })
}

/// The platform account the planter `owner` (a GitHub id) is linked to on
/// the hub, asked once per run. By the number: a link whose GitHub id isn't
/// the owner's isn't this person's.
async fn account_of(
    app: &Platform,
    client: &str,
    seen: &mut HashMap<String, Option<String>>,
    owner: &str,
) -> anyhow::Result<Option<String>> {
    if let Some(a) = seen.get(owner) {
        return Ok(a.clone());
    }
    let a = app.hub.linked_account(client, owner).await?;
    let a = a.filter(|l| l.github_id.to_string() == owner).map(|l| l.account);
    seen.insert(owner.to_owned(), a.clone());
    Ok(a)
}

/// One planted repo, for `account` (the planter, linked on the hub).
#[allow(clippy::too_many_arguments)]
async fn one_repo(
    app: &Platform,
    gh: &crate::forge::GithubApp,
    o: &Options,
    planned: &mut HashMap<u64, String>,
    account: &str,
    owner: &str,
    repo: &str,
    now: u64,
) -> anyhow::Result<Outcome> {
    let refused = |why: String| Ok(Outcome::Refused { why });
    if !good_repo(repo) {
        return refused(format!("{repo:?} isn't OWNER/NAME"));
    }
    let Some(user) = app.hub.github_user(account).await? else {
        return refused("their Arugula account has no GitHub identity".into());
    };
    if user.id.to_string() != owner {
        return refused(format!("their Arugula account's GitHub id is {}, not {owner}", user.id));
    }
    let install = match gh.installation_now(&app.http, repo).await {
        Ok(Some(i)) => i,
        Ok(None) => return Ok(Outcome::WaitingApp { install_url: gh.install_url(&format!("personal:{repo}")) }),
        Err(e) => return refused(format!("couldn't ask GitHub about the App's installation: {e:#}")),
    };
    let role = match gh.repo_role(&Caller::of(app, account), &install, &user, repo).await {
        Ok(r) => r,
        Err(why) => return refused(why),
    };
    if !role.may_link() {
        return Ok(Outcome::Role { login: user.login, role: role.role });
    }
    // Where it goes: an organization's repo where the operator said, else
    // the planter's own project.
    let org = (install.account_type == "Organization").then(|| install.account.to_ascii_lowercase());
    let target = match &org {
        Some(org) => match o.orgs.get(org) {
            Some(t) => t.clone(),
            None => return Ok(Outcome::WaitingOrg { org: install.account.clone() }),
        },
        None => OrgTarget::Personal,
    };
    let (project, personal, made) = match target {
        OrgTarget::Personal => match app.db.personal_project_of(account)? {
            Some(p) => (p, true, false),
            None => (format!("(the personal project of {account}, made now)"), true, true),
        },
        OrgTarget::Project(p) => {
            match crate::forge::links::membership(app, account, &p, true).await {
                Ok((_, role)) if role != TeamRole::Viewer => {}
                _ => return Ok(Outcome::NotMember { project: p }),
            }
            if app.db.project_holder(&p)?.is_some_and(|(_, team)| team.is_none()) {
                return refused(format!("{p} isn't a team's project"));
            }
            (p, false, false)
        }
    };
    let was = app.db.linked_repo(role.id)?.map(|r| (r.project_id, r.unlinked_at.is_some()));
    let was = was.or_else(|| planned.get(&role.id).map(|p| (p.clone(), false)));
    match was {
        Some((p, false)) if p == project => {
            if !o.dry_run {
                app.db.record_planter_import(owner, repo, role.id, &p, account, "already", now)?;
            }
            return Ok(Outcome::Already { project: p, repo_id: role.id });
        }
        Some((p, false)) => return Ok(Outcome::Elsewhere { project: p }),
        Some((p, true)) => return Ok(Outcome::Unlinked { project: p }),
        None => {
            // Linked once by the import, for anyone, and gone since: never
            // again.
            if let Some((project, at)) = app.db.planter_imported_repo(role.id)? {
                return Ok(Outcome::ImportedBefore { project, at });
            }
        }
    }
    if o.dry_run {
        planned.insert(role.id, project.clone());
        return Ok(Outcome::Linked { project, repo_id: role.id, personal, made });
    }
    let project = if made {
        match crate::projects::personal_project(app, account).await? {
            Some(p) => p,
            None => return refused("their Arugula account is gone".into()),
        }
    } else {
        project
    };
    let l = LinkedRepo {
        repo_id: role.id,
        full_name: role.full_name.clone(),
        private: role.private,
        project_id: project.clone(),
        installation_id: install.id,
        linked_by: Some(account.to_owned()),
        linked_at: 0,
        unlinked_at: None,
        purge_after: None,
        mirror_content: false,
    };
    let i = Installation {
        id: install.id,
        account_id: install.account_id,
        login: install.account.clone(),
        account_type: install.account_type.clone(),
        permissions: "{}".into(),
    };
    match app.db.link_repo(&l, &i, account, now)? {
        Linked::Now { .. } => {
            app.db.record_planter_import(owner, repo, role.id, &project, account, "linked", now)?;
            planned.insert(role.id, project.clone());
            tracing::info!(%account, %project, repo = role.id, "imported a repo planter linked");
            Ok(Outcome::Linked { project, repo_id: role.id, personal, made })
        }
        Linked::Elsewhere => Ok(Outcome::Elsewhere { project: "another project".into() }),
    }
}

/// `arugula-platform import planter …`: the repo import, as the running
/// platform is configured (its database, its hub, its GitHub App).
#[derive(clap::Parser, Debug)]
struct Args {
    #[command(subcommand)]
    what: What,
}

#[derive(clap::Subcommand, Debug)]
enum What {
    /// The repos planter's boxes were planted from, as repo links on
    /// projects, and each box reported (M232). Run `arugula-control import
    /// planter` on the hub first: this links repos only for planters linked
    /// there. Safe to run again; never deletes; reads the backup only.
    Planter(PlanterArgs),
}

#[derive(clap::Args, Debug)]
struct PlanterArgs {
    /// A copy of planter's DATA_DIR: its accounts.json and boxes.json.
    #[arg(long)]
    backup: PathBuf,
    /// The hub's client planter's accounts are linked under.
    #[arg(long, default_value = CLIENT)]
    client: String,
    /// Where an organization's repos go: `ORG=prj_…` (a team's project the
    /// planter is an owner or editor of) or `ORG=personal`. Repeatable. An
    /// organization's repos wait without one.
    #[arg(long = "org", value_parser = parse_org)]
    orgs: Vec<(String, OrgTarget)>,
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
    #[command(flatten)]
    github_app: crate::forge::Args,
    #[arg(long, default_value = "https://api.github.com", hide = true)]
    github_api: String,
}

/// What the import needs from the running platform's environment, when
/// this shell hasn't got it (`fly ssh console` may not carry the app's
/// secrets): taken from the server's own, as the windows' tools do.
const SERVER_ENV: [&str; 9] = [
    "ARUGULA_PLATFORM_HUB",
    "ARUGULA_PLATFORM_KEY",
    "ARUGULA_PLATFORM_DATABASE_URL",
    "ARUGULA_PLATFORM_DB",
    "GITHUB_APP_ID",
    "GITHUB_APP_SLUG",
    "GITHUB_APP_PRIVATE_KEY",
    "GITHUB_APP_PRIVATE_KEY_FILE",
    "GITHUB_APP_WEBHOOK_SECRET",
];

/// `<prog> planter …` (`prog` is `arugula-platform import`). Called before
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
    let What::Planter(a) = a.what;
    let backup = Backup::read(&a.backup)?;
    let rt = tokio::runtime::Builder::new_multi_thread().enable_all().build()?;
    rt.block_on(async move {
        let key = arugula_control_common::principal::PrincipalKey::from_hex(&a.key).context("ARUGULA_PLATFORM_KEY")?;
        let hub = std::sync::Arc::new(crate::remote::HttpHub::new(&a.hub, key)?);
        let db = a.database.open(&a.db).context("opening the platform's database")?;
        let http = arugula_control_common::roots::client(std::time::Duration::from_secs(15))?;
        let mut p = Platform::new(crate::Config::default(), db, hub, http, Default::default());
        p.github = a.github_app.app(&a.github_api)?;
        let o = Options { client: a.client, orgs: a.orgs.into_iter().collect(), dry_run: a.dry_run };
        let r = import_repos(&p, &backup, &o).await?;
        if a.json {
            println!("{}", serde_json::to_string_pretty(&r)?);
        } else {
            print!("{}", r.render(&backup.dir));
        }
        anyhow::Ok(())
    })
}

#[cfg(test)]
#[path = "planter_tests.rs"]
mod tests;
