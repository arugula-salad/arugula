//! The Agents page's recipes (#403): every recipe on this machine, and
//! writing one from the page's form. The owner's.
//!
//! - **Where recipes are:** `~/.claude/agents/`, and `.claude/agents/` in
//!   each project this machine knows of: those the owner added on the page
//!   (`<state>/agent-projects.json`), those an offer works in, and the git
//!   repositories open panes are in.
//! - **Saving** writes a Claude Code subagent file, as Claude Code reads it:
//!   YAML frontmatter and the prompt as its body. Editing one keeps the
//!   frontmatter keys the form doesn't know (and inline MCP servers, which
//!   it shows but doesn't edit), in their order, so the file stays the
//!   person's. A new one is `NAME.md`, and never replaces a file.
//! - **Deleting** removes the file (and its offer).

use std::{
    collections::BTreeSet,
    path::{Path, PathBuf},
};

use serde::Deserialize;
use serde_json::{Value, json};

use super::{Recipe, all, good_name, offers, parse, set_offer};

/// Projects the owner added on the page.
const PROJECTS_FILE: &str = "agent-projects.json";

fn added(state_dir: &Path) -> Vec<String> {
    std::fs::read(state_dir.join(PROJECTS_FILE)).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default()
}

/// Add a project to the page's list (its git top), or drop it.
pub fn set_project(state_dir: &Path, home: &Path, dir: &str, keep: bool) -> Result<Vec<String>, String> {
    let d = super::super::fountain::wear::expand(dir, home);
    let d = d.canonicalize().map_err(|e| format!("{dir}: {e}"))?;
    let d = d.display().to_string();
    let mut list = added(state_dir);
    list.retain(|x| *x != d);
    if keep {
        list.push(d);
    }
    let b = serde_json::to_vec_pretty(&list).map_err(|e| e.to_string())?;
    crate::store::write_atomic(&state_dir.join(PROJECTS_FILE), &b).map_err(|e| e.to_string())?;
    Ok(list)
}

/// Every recipe this machine has: the user's, then each known project's
/// (only a project's own, not the user's again), with where each is offered.
pub fn every(state_dir: &Path, home: &Path, open: &[String]) -> Value {
    let offered = offers(state_dir);
    let row = |r: &Recipe| {
        let mut v = serde_json::to_value(r).unwrap_or_default();
        v["offered_from"] = json!(offered.iter().find(|o| o.agent == r.name).map(|o| &o.dir));
        v
    };
    let user_dir = home.join(".claude/agents");
    let user: Vec<Value> = all(Path::new("/nonexistent-project"), home)
        .iter()
        .filter(|r| r.path.starts_with(&user_dir))
        .map(row)
        .collect();
    let mut dirs: BTreeSet<String> = added(state_dir).into_iter().collect();
    dirs.extend(offered.iter().map(|o| o.dir.clone()));
    dirs.extend(open.iter().cloned());
    let projects: Vec<Value> = dirs
        .iter()
        .map(|d| {
            let own = Path::new(d).join(".claude/agents");
            let recipes: Vec<Value> =
                all(Path::new(d), home).iter().filter(|r| r.path.starts_with(&own)).map(row).collect();
            json!({ "dir": d, "added": added(state_dir).contains(d), "recipes": recipes })
        })
        .collect();
    json!({ "user": { "dir": user_dir, "recipes": user }, "projects": projects })
}

/// What the page's form sends.
#[derive(Debug, Default, Deserialize)]
pub struct Form {
    /// The file to rewrite; none for a new recipe.
    #[serde(default)]
    pub path: Option<String>,
    /// For a new one: its project, or none for `~/.claude/agents`.
    #[serde(default)]
    pub dir: Option<String>,
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub model: Option<String>,
    #[serde(default)]
    pub tools: Vec<String>,
    #[serde(default, rename = "disallowedTools")]
    pub disallowed_tools: Vec<String>,
    #[serde(default, rename = "permissionMode")]
    pub permission_mode: Option<String>,
    #[serde(default)]
    pub skills: Vec<String>,
    /// MCP servers by name (configured for the project or the user); inline
    /// ones in the file are kept as they are.
    #[serde(default, rename = "mcpServers")]
    pub mcp_servers: Vec<String>,
    #[serde(default)]
    pub prompt: String,
}

/// Whether `p` is a recipe file's place: `…/.claude/agents/X.md`.
fn in_agents_dir(p: &Path) -> bool {
    p.extension().is_some_and(|x| x == "md")
        && p.parent().and_then(Path::file_name).is_some_and(|n| n == "agents")
        && p.parent().and_then(Path::parent).and_then(Path::file_name).is_some_and(|n| n == ".claude")
}

fn clean(list: &[String]) -> Vec<String> {
    list.iter().map(|s| s.trim().to_owned()).filter(|s| !s.is_empty()).collect()
}

/// The file's text for `f`, over the frontmatter it had (`old`).
fn render(f: &Form, old: Option<&str>) -> Result<String, String> {
    use serde_norway::{Mapping, Value as Y};
    let mut m: Mapping = match old {
        Some(text) => {
            let text = text.strip_prefix('\u{feff}').unwrap_or(text).replace("\r\n", "\n");
            let rest = text.strip_prefix("---\n").ok_or("the file has no frontmatter")?;
            let end = rest.find("\n---").ok_or("the file's frontmatter never ends")?;
            serde_norway::from_str(&rest[..end]).map_err(|e| format!("its frontmatter: {e}"))?
        }
        None => Mapping::new(),
    };
    let s = |v: &str| Y::String(v.to_owned());
    // Servers: the form's names, and any written inline in the file, kept.
    let inline: Vec<Y> = match m.get(s("mcpServers")) {
        Some(Y::Sequence(items)) => items.iter().filter(|i| matches!(i, Y::Mapping(_))).cloned().collect(),
        Some(Y::Mapping(map)) => map
            .iter()
            .filter(|(_, v)| !v.is_null())
            .map(|(k, v)| {
                let mut one = Mapping::new();
                one.insert(k.clone(), v.clone());
                Y::Mapping(one)
            })
            .collect(),
        _ => vec![],
    };
    let mut put = |k: &str, v: Option<Y>| match v {
        Some(v) => {
            m.insert(s(k), v);
        }
        None => {
            m.remove(s(k));
        }
    };
    put("name", Some(s(f.name.trim())));
    put("description", Some(s(f.description.trim())));
    let joined = |l: &[String]| {
        let l = clean(l);
        (!l.is_empty()).then(|| s(&l.join(", ")))
    };
    put("tools", joined(&f.tools));
    put("disallowedTools", joined(&f.disallowed_tools));
    put("model", f.model.as_deref().map(str::trim).filter(|x| !x.is_empty() && *x != "inherit").map(s));
    put(
        "permissionMode",
        f.permission_mode.as_deref().map(str::trim).filter(|x| !x.is_empty() && *x != "default").map(s),
    );
    let skills = clean(&f.skills);
    put("skills", (!skills.is_empty()).then(|| Y::Sequence(skills.iter().map(|x| s(x)).collect())));
    let mut servers: Vec<Y> = clean(&f.mcp_servers).iter().map(|x| s(x)).collect();
    servers.extend(inline);
    put("mcpServers", (!servers.is_empty()).then_some(Y::Sequence(servers)));
    let front = serde_norway::to_string(&m).map_err(|e| e.to_string())?;
    Ok(format!("---\n{}---\n\n{}\n", front, f.prompt.trim()))
}

/// Write the form's recipe; its path.
pub fn save(home: &Path, f: &Form) -> Result<PathBuf, String> {
    if !good_name(f.name.trim()) {
        return Err(format!("{:?} isn't a name (letters, digits, - _ .)", f.name));
    }
    if f.description.trim().is_empty() {
        return Err("a description: it's how agents and the catalog know what it's for".into());
    }
    let (path, old) = match f.path.as_deref().filter(|p| !p.is_empty()) {
        Some(p) => {
            let p = PathBuf::from(p);
            if !in_agents_dir(&p) || !p.is_file() {
                return Err(format!("{} isn't a recipe file (.claude/agents/NAME.md)", p.display()));
            }
            let text = std::fs::read_to_string(&p).map_err(|e| e.to_string())?;
            (p, Some(text))
        }
        None => {
            let dir = match f.dir.as_deref().filter(|d| !d.is_empty()) {
                Some(d) => super::super::fountain::wear::expand(d, home).join(".claude/agents"),
                None => home.join(".claude/agents"),
            };
            let p = dir.join(format!("{}.md", f.name.trim()));
            if p.exists() {
                return Err(format!("{} is there already: edit it", p.display()));
            }
            std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
            (p, None)
        }
    };
    let text = render(f, old.as_deref())?;
    // It reads back as the recipe it says.
    parse(&text, &path).map_err(|e| format!("it wouldn't read back: {e}"))?;
    crate::store::write_atomic(&path, text.as_bytes()).map_err(|e| format!("{}: {e}", path.display()))?;
    Ok(path)
}

/// Remove a recipe file, and stop offering it if it's offered from there.
pub fn delete(state_dir: &Path, home: &Path, path: &str) -> Result<(), String> {
    let p = PathBuf::from(path);
    if !in_agents_dir(&p) {
        return Err(format!("{path} isn't a recipe file (.claude/agents/NAME.md)"));
    }
    let text = std::fs::read_to_string(&p).map_err(|e| format!("{path}: {e}"))?;
    let r = parse(&text, &p)?;
    if let Some(o) = offers(state_dir).into_iter().find(|o| o.agent == r.name)
        && super::find(Path::new(&o.dir), home, &r.name).is_ok_and(|x| x.path == p)
    {
        set_offer(state_dir, home, &r.name, None)?;
    }
    std::fs::remove_file(&p).map_err(|e| format!("{path}: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp() -> PathBuf {
        let d = std::env::temp_dir().join(format!("arugula-manage-{}-{}", std::process::id(), crate::store::now_ms()));
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn a_new_recipe_reads_back_and_an_edit_keeps_what_the_form_doesnt_know() {
        let home = tmp();
        let f = Form {
            name: "fixer".into(),
            description: "Fixes tests".into(),
            model: Some("haiku".into()),
            tools: vec!["Read".into(), " Edit ".into(), String::new()],
            skills: vec!["testing".into()],
            prompt: "You fix tests.".into(),
            ..Default::default()
        };
        let p = save(&home, &f).unwrap();
        assert_eq!(p, home.join(".claude/agents/fixer.md"));
        let r = parse(&std::fs::read_to_string(&p).unwrap(), &p).unwrap();
        assert_eq!(
            (r.tools.clone(), r.model.clone(), r.prompt.clone()),
            (vec!["Read".to_owned(), "Edit".to_owned()], Some("haiku".to_owned()), "You fix tests.".to_owned())
        );
        assert!(save(&home, &f).unwrap_err().contains("there already"), "never replaced");

        // Someone's own keys and an inline server survive an edit.
        std::fs::write(&p, "---\nname: fixer\ndescription: x\ncolor: blue\nmcpServers:\n  - docs\n  - tracker:\n      type: http\n      url: https://t/mcp\n---\nold\n").unwrap();
        let edit = Form {
            path: Some(p.display().to_string()),
            name: "fixer".into(),
            description: "Fixes failing tests".into(),
            mcp_servers: vec!["github".into()],
            prompt: "New prompt.".into(),
            ..Default::default()
        };
        save(&home, &edit).unwrap();
        let text = std::fs::read_to_string(&p).unwrap();
        assert!(text.contains("color: blue"), "{text}");
        let r = parse(&text, &p).unwrap();
        assert_eq!(r.description, "Fixes failing tests");
        assert_eq!(r.model, None);
        assert_eq!(r.prompt, "New prompt.");
        let names: Vec<String> = r
            .mcp_servers
            .iter()
            .map(|s| match s {
                super::super::RecipeServer::Named(n) | super::super::RecipeServer::Inline { name: n, .. } => n.clone(),
            })
            .collect();
        assert_eq!(
            names,
            ["github", "tracker"],
            "the form's names, then the inline one kept; docs was dropped by the form"
        );

        // Only recipe files.
        assert!(save(&home, &Form { path: Some(home.join("x.md").display().to_string()), ..edit }).is_err());
        assert!(delete(&home, &home, &home.join("notes.md").display().to_string()).is_err());
        let state = home.join("state");
        std::fs::create_dir_all(&state).unwrap();
        delete(&state, &home, &p.display().to_string()).unwrap();
        assert!(!p.exists());
        let _ = std::fs::remove_dir_all(&home);
    }
}
