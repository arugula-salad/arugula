//! planter's backup as `arugula-platform import planter` and the hub's
//! `import planter` read it (M232): its accounts, its boxes and what was
//! skipped. Here (#314) so the hub reads it without the platform.

use std::path::{Path, PathBuf};

use anyhow::{Context as _, bail};
use serde::Serialize;
use serde_json::Value;

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
