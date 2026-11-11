//! #395: Claude Code settings that stop it running Arugula's hooks, so the
//! places a person looks (`arugula hooks status`, Getting started, the
//! `capabilities` tool) can say why a terminal Claude's questions never
//! reach a card, instead of saying to install hooks that are installed.
//!
//! Checked against Claude Code 2.1.322 in a container: a managed
//! `allowManagedHooksOnly: true` stops every hook in the user's settings and
//! in plugins (only the managed file's own run), from `managed-settings.json`
//! and from a file in `managed-settings.d/` alike. `disableAllHooks: true`
//! stops every hook, from the managed file or the user's own. A managed file
//! with other policy only (`permissions.deny`, say) changes nothing. Agent
//! blocks don't use hooks: their questions are cards either way.
//!
//! A Team or Enterprise policy sent from claude.ai isn't a file here; the
//! "no hook heard from this pane" check covers it.

use std::path::{Path, PathBuf};

use serde::Serialize;
use serde_json::Value;

/// What stops Claude Code running Arugula's hooks on this machine.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct HooksBlocked {
    /// The settings file that says so.
    pub file: PathBuf,
    /// `allowManagedHooksOnly` or `disableAllHooks`.
    pub key: &'static str,
    /// A managed (organization's) file, which the person may not be able to
    /// change; else their own settings.json.
    pub managed: bool,
}

impl HooksBlocked {
    /// One sentence for a person: what blocks them, and what still works.
    pub fn says(&self) -> String {
        let file = self.file.display();
        if self.managed {
            format!(
                "Claude Code won't run Arugula's hooks on this machine: {file} sets {}, a policy from whoever manages Claude Code here. A terminal Claude's questions and permission prompts stay in its pane, which Arugula still marks as waiting for you from its screen; agent blocks ask on cards without hooks.",
                self.key
            )
        } else {
            format!(
                "Claude Code runs no hooks, Arugula's included: {file} sets disableAllHooks. Take it out for a terminal Claude's questions and permission prompts to come to cards."
            )
        }
    }
}

/// The directories Claude Code reads managed settings from on this OS.
/// `ARUGULA_CLAUDE_MANAGED_DIR` replaces them (tests).
pub fn managed_dirs() -> Vec<PathBuf> {
    if let Some(d) = std::env::var_os("ARUGULA_CLAUDE_MANAGED_DIR").filter(|d| !d.is_empty()) {
        return vec![PathBuf::from(d)];
    }
    if cfg!(target_os = "macos") {
        vec![PathBuf::from("/Library/Application Support/ClaudeCode")]
    } else if cfg!(windows) {
        vec![PathBuf::from(r"C:\Program Files\ClaudeCode"), PathBuf::from(r"C:\ProgramData\ClaudeCode")]
    } else {
        vec![PathBuf::from("/etc/claude-code")]
    }
}

/// What blocks the hooks, if anything: the managed files in [`managed_dirs`]
/// first, then the user's `settings.json` (`user`).
pub fn hooks_blocked(user: Option<&Path>) -> Option<HooksBlocked> {
    hooks_blocked_in(&managed_dirs(), user)
}

/// [`hooks_blocked`], with the managed directories given.
pub fn hooks_blocked_in(dirs: &[PathBuf], user: Option<&Path>) -> Option<HooksBlocked> {
    for dir in dirs {
        let mut files = vec![dir.join("managed-settings.json")];
        if let Ok(rd) = std::fs::read_dir(dir.join("managed-settings.d")) {
            let mut more: Vec<PathBuf> =
                rd.flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|x| x == "json")).collect();
            more.sort();
            files.extend(more);
        }
        for file in files {
            let Some(v) = read(&file) else { continue };
            for key in ["allowManagedHooksOnly", "disableAllHooks"] {
                if v[key] == true {
                    return Some(HooksBlocked { file, key, managed: true });
                }
            }
        }
    }
    let file = user?;
    (read(file)?["disableAllHooks"] == true).then(|| HooksBlocked {
        file: file.to_owned(),
        key: "disableAllHooks",
        managed: false,
    })
}

fn read(file: &Path) -> Option<Value> {
    serde_json::from_str(&std::fs::read_to_string(file).ok()?).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dir(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("arugula-claude-policy-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn what_blocks_the_hooks_and_what_doesnt() {
        let m = dir("managed");
        let user = dir("user").join("settings.json");
        let dirs = [m.clone()];
        // Nothing there, or policy that has nothing to do with hooks.
        assert_eq!(hooks_blocked_in(&dirs, Some(&user)), None);
        std::fs::write(m.join("managed-settings.json"), r#"{"permissions":{"deny":["WebFetch"]}}"#).unwrap();
        std::fs::write(&user, r#"{"hooks":{}}"#).unwrap();
        assert_eq!(hooks_blocked_in(&dirs, Some(&user)), None);

        // The user's own switch.
        std::fs::write(&user, r#"{"disableAllHooks":true}"#).unwrap();
        let b = hooks_blocked_in(&dirs, Some(&user)).unwrap();
        assert_eq!((b.key, b.managed), ("disableAllHooks", false));
        assert!(b.says().contains("Take it out"), "{}", b.says());

        // A drop-in file's policy, over the user's.
        std::fs::create_dir_all(m.join("managed-settings.d")).unwrap();
        std::fs::write(m.join("managed-settings.d/10-hooks.json"), r#"{"allowManagedHooksOnly":true}"#).unwrap();
        let b = hooks_blocked_in(&dirs, Some(&user)).unwrap();
        assert_eq!(
            b,
            HooksBlocked {
                file: m.join("managed-settings.d/10-hooks.json"),
                key: "allowManagedHooksOnly",
                managed: true
            }
        );
        assert!(b.says().contains("sets allowManagedHooksOnly, a policy"), "{}", b.says());

        // The main managed file comes first; a broken one is skipped.
        std::fs::write(m.join("managed-settings.json"), r#"{"disableAllHooks":true}"#).unwrap();
        assert_eq!(hooks_blocked_in(&dirs, None).unwrap().key, "disableAllHooks");
        std::fs::write(m.join("managed-settings.json"), "{not json").unwrap();
        assert_eq!(hooks_blocked_in(&dirs, None).unwrap().key, "allowManagedHooksOnly");
        // False is no policy.
        std::fs::write(m.join("managed-settings.d/10-hooks.json"), r#"{"allowManagedHooksOnly":false}"#).unwrap();
        assert_eq!(hooks_blocked_in(&dirs, None), None);
    }
}
