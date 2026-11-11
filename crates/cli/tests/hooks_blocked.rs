//! #395: when Claude Code's settings stop it running hooks, `arugula hooks
//! status` and `hooks install` say so, and why, instead of only listing the
//! hooks as present. Pinned for each case: no policy, policy that isn't
//! about hooks, a managed `allowManagedHooksOnly`, and the user's own
//! `disableAllHooks`.

use std::{path::Path, process::Command};

use serde_json::Value;

fn arugula(home: &Path, managed: &Path, args: &[&str]) -> (Value, String) {
    let out = Command::new(env!("CARGO_BIN_EXE_arugula"))
        .args(args)
        .env("HOME", home)
        .env("USERPROFILE", home)
        .env_remove("CLAUDE_CONFIG_DIR")
        .env("ARUGULA_CLAUDE_MANAGED_DIR", managed)
        .env("ARUGULA_TELEMETRY", "off")
        .output()
        .unwrap();
    let text = String::from_utf8_lossy(&out.stdout).into_owned();
    assert!(out.status.success(), "{args:?}: {text}{}", String::from_utf8_lossy(&out.stderr));
    (serde_json::from_str(&text).unwrap_or(Value::Null), text)
}

#[test]
fn status_and_install_say_what_blocks_the_hooks() {
    let root = std::env::temp_dir().join(format!("arugula-hooks-blocked-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let (home, managed) = (root.join("home"), root.join("managed"));
    std::fs::create_dir_all(&managed).unwrap();
    let settings = home.join(".claude/settings.json");

    // Installed, with nothing blocking them.
    let (v, _) = arugula(&home, &managed, &["--json", "hooks", "install"]);
    assert_eq!(v["blocked"], Value::Null, "{v}");
    let (v, _) = arugula(&home, &managed, &["--json", "hooks", "status"]);
    assert_eq!((v["installed"].clone(), v["blocked"].clone()), (Value::Bool(true), Value::Null), "{v}");

    // Managed policy about something else: still nothing.
    std::fs::write(managed.join("managed-settings.json"), r#"{"permissions":{"deny":["WebFetch"]}}"#).unwrap();
    let (v, _) = arugula(&home, &managed, &["--json", "hooks", "status"]);
    assert_eq!(v["blocked"], Value::Null, "{v}");

    // An organization's allowManagedHooksOnly: installed, and blocked.
    std::fs::write(managed.join("managed-settings.json"), r#"{"allowManagedHooksOnly":true}"#).unwrap();
    let (v, _) = arugula(&home, &managed, &["--json", "hooks", "status"]);
    assert_eq!(v["installed"], true, "{v}");
    assert_eq!(v["blocked"]["key"], "allowManagedHooksOnly", "{v}");
    assert_eq!(v["blocked"]["managed"], true, "{v}");
    let (_, text) = arugula(&home, &managed, &["hooks", "status"]);
    assert!(text.contains("blocked: Claude Code won't run Arugula's hooks on this machine"), "{text}");
    assert!(text.contains("sets allowManagedHooksOnly"), "{text}");
    let (_, text) = arugula(&home, &managed, &["hooks", "install"]);
    assert!(text.contains("already there") && text.contains("Claude Code won't run Arugula's hooks"), "{text}");

    // The user's own disableAllHooks.
    std::fs::remove_file(managed.join("managed-settings.json")).unwrap();
    let mut s: Value = serde_json::from_str(&std::fs::read_to_string(&settings).unwrap()).unwrap();
    s["disableAllHooks"] = Value::Bool(true);
    std::fs::write(&settings, s.to_string()).unwrap();
    let (v, _) = arugula(&home, &managed, &["--json", "hooks", "status"]);
    assert_eq!(
        (v["blocked"]["key"].clone(), v["blocked"]["managed"].clone()),
        ("disableAllHooks".into(), Value::Bool(false)),
        "{v}"
    );
    assert!(v["blocked"]["says"].as_str().unwrap().contains("Take it out"), "{v}");
}
