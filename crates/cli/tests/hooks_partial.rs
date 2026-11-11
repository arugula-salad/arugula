//! #906: when Arugula's hooks are only partly in Claude Code's settings
//! (an older `install`, a hand edit), `arugula hooks status` says which are
//! missing and the one command that finishes the job, in text and JSON
//! (`partial`, which the daemon passes on to `arugula status` and Getting
//! started). Pinned for each case: all there, none, partly, and partly but
//! blocked by a policy, where the policy is said instead. Every run uses a
//! temp HOME and managed dir: no real settings are read or written.

use std::{path::Path, process::Command};

use serde_json::Value;

fn arugula(home: &Path, managed: &Path, args: &[&str]) -> (Value, String) {
    let out = Command::new(env!("CARGO_BIN_EXE_arugula"))
        .args(args)
        .env("HOME", home)
        .env("USERPROFILE", home)
        .env_remove("CLAUDE_CONFIG_DIR")
        .env_remove("CLAUDE_PROJECT_DIR")
        .env("ARUGULA_CLAUDE_MANAGED_DIR", managed)
        .env("ARUGULA_TELEMETRY", "off")
        .output()
        .unwrap();
    let text = String::from_utf8_lossy(&out.stdout).into_owned();
    assert!(out.status.success(), "{args:?}: {text}{}", String::from_utf8_lossy(&out.stderr));
    (serde_json::from_str(&text).unwrap_or(Value::Null), text)
}

const FINISH: &str = "`arugula hooks install` adds the rest and changes nothing else.";

#[test]
fn status_says_what_is_left_of_a_partial_install() {
    let root = std::env::temp_dir().join(format!("arugula-hooks-partial-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let (home, managed) = (root.join("home"), root.join("managed"));
    std::fs::create_dir_all(home.join(".claude")).unwrap();
    std::fs::create_dir_all(&managed).unwrap();
    let settings = home.join(".claude/settings.json");

    // None: an offer, as before, and no `partial`.
    std::fs::write(&settings, r#"{"model":"opus"}"#).unwrap();
    let (v, _) = arugula(&home, &managed, &["--json", "hooks", "status"]);
    assert_eq!((v["installed"].clone(), v["partial"].clone()), (Value::Bool(false), Value::Null), "{v}");
    let (_, text) = arugula(&home, &managed, &["hooks", "status"]);
    assert!(text.contains("`arugula hooks install` adds the missing ones"), "{text}");
    assert!(!text.contains("partly"), "{text}");

    // Full.
    arugula(&home, &managed, &["hooks", "install"]);
    let full: Value = serde_json::from_str(&std::fs::read_to_string(&settings).unwrap()).unwrap();
    let (v, _) = arugula(&home, &managed, &["--json", "hooks", "status"]);
    assert_eq!((v["installed"].clone(), v["partial"].clone()), (Value::Bool(true), Value::Null), "{v}");
    let (_, text) = arugula(&home, &managed, &["hooks", "status"]);
    assert!(!text.contains("partly"), "{text}");
    // Acting rules aren't there, and that's a choice: only the quiet line.
    assert!(text.contains("--allow-acting` adds the acting ones"), "{text}");

    // Partly: no SessionStart, one read rule short (what Jake's machine had).
    let mut s = full.clone();
    s["hooks"].as_object_mut().unwrap().remove("SessionStart");
    s["permissions"]["allow"].as_array_mut().unwrap().retain(|r| r != "Bash(arugula tail:*)");
    std::fs::write(&settings, s.to_string()).unwrap();
    let (v, _) = arugula(&home, &managed, &["--json", "hooks", "status"]);
    assert_eq!(v["partial"]["missing"], serde_json::json!(["SessionStart"]), "{v}");
    assert_eq!(v["partial"]["outdated"], serde_json::json!([]), "{v}");
    assert_eq!(v["partial"]["rules"], true, "{v}");
    let says = v["partial"]["says"].as_str().unwrap();
    assert_eq!(
        says,
        format!(
            "Arugula's hooks are only partly installed in ~/.claude/settings.json: SessionStart is missing and some read-only permission rules are missing. {FINISH}"
        )
    );
    let (_, text) = arugula(&home, &managed, &["hooks", "status"]);
    assert!(text.contains(says), "{text}");
    assert!(!text.contains("adds the missing ones"), "said once, not twice: {text}");

    // Partly and blocked by a policy: the policy is said, not "install".
    std::fs::write(managed.join("managed-settings.json"), r#"{"allowManagedHooksOnly":true}"#).unwrap();
    let (v, _) = arugula(&home, &managed, &["--json", "hooks", "status"]);
    assert_eq!(v["partial"], Value::Null, "{v}");
    assert_eq!(v["blocked"]["key"], "allowManagedHooksOnly", "{v}");
    let (_, text) = arugula(&home, &managed, &["hooks", "status"]);
    assert!(text.contains("blocked: Claude Code won't run Arugula's hooks"), "{text}");
    assert!(!text.contains("partly") && !text.contains("adds the missing ones"), "{text}");
    std::fs::remove_file(managed.join("managed-settings.json")).unwrap();

    // `install` finishes it, and keeps what was there.
    arugula(&home, &managed, &["hooks", "install"]);
    let (v, _) = arugula(&home, &managed, &["--json", "hooks", "status"]);
    assert_eq!((v["installed"].clone(), v["partial"].clone()), (Value::Bool(true), Value::Null), "{v}");
    let after: Value = serde_json::from_str(&std::fs::read_to_string(&settings).unwrap()).unwrap();
    assert_eq!(after["model"], "opus");

    let _ = std::fs::remove_dir_all(&root);
}
