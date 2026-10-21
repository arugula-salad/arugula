//! An editor block's colors follow the page's palette (#1048).
//!
//! The page makes VS Code's colors from its palette (`web/src/shell/editor-theme.ts`)
//! and sends them with the block's `theme` call. They go into the block's
//! workspace file as settings (Arugula's dark or light theme, with the
//! palette's colors over it), and VS Code applies a change to that file at
//! once, so a block changes with the palette while it's open, through any
//! origin it's framed from.
//!
//! Only while the theme is Arugula's: someone who picked another one, in
//! their settings or in the block's workspace, keeps it.

use serde::Deserialize;
use serde_json::{Map, Value, json};

/// Arugula's themes (the extension's `contributes.themes` labels).
pub const DARK: &str = "arugula";
pub const LIGHT: &str = "arugula light";

const MAX_COLORS: usize = 300;
const MAX_RULES: usize = 100;

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct Theme {
    light: bool,
    colors: Map<String, Value>,
    token_colors: Vec<Rule>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct Rule {
    scope: Vec<String>,
    settings: RuleSettings,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct RuleSettings {
    foreground: String,
    #[serde(default)]
    font_style: Option<String>,
}

fn color(s: &str) -> bool {
    let hex = s.strip_prefix('#').unwrap_or("");
    matches!(hex.len(), 6 | 8) && hex.bytes().all(|b| b.is_ascii_hexdigit())
}

/// What a `theme` call (or a block's config) says, checked: colors only,
/// as `#rrggbb[aa]`, under VS Code's names for them.
pub fn parse(v: &Value) -> Result<Theme, String> {
    let t: Theme = serde_json::from_value(v.clone()).map_err(|e| format!("theme: {e}"))?;
    if t.colors.len() > MAX_COLORS || t.token_colors.len() > MAX_RULES {
        return Err("theme: too many colors".into());
    }
    for (k, v) in &t.colors {
        let name = !k.is_empty() && k.len() <= 80 && k.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'.');
        if !name || !v.as_str().is_some_and(color) {
            return Err(format!("theme: {k}"));
        }
    }
    for r in &t.token_colors {
        let scope = |s: &String| {
            !s.is_empty() && s.len() <= 100 && s.bytes().all(|b| b.is_ascii_alphanumeric() || b".-_ ".contains(&b))
        };
        if r.scope.is_empty() || r.scope.len() > 20 || !r.scope.iter().all(scope) || !color(&r.settings.foreground) {
            return Err("theme: a token color".into());
        }
        if let Some(f) = &r.settings.font_style
            && !f.split(' ').all(|w| matches!(w, "" | "italic" | "bold" | "underline" | "strikethrough"))
        {
            return Err("theme: a font style".into());
        }
    }
    Ok(t)
}

impl Theme {
    /// Whether these workspace fields are still exactly what we generated.
    pub fn applied(&self, workspace: &Value) -> bool {
        let generated = self.settings();
        OWNED.iter().all(|key| workspace["settings"][key] == generated[key])
    }

    /// As workspace settings.
    fn settings(&self) -> Value {
        let rules: Vec<Value> = self
            .token_colors
            .iter()
            .map(|r| {
                let mut s = json!({ "foreground": r.settings.foreground });
                if let Some(f) = &r.settings.font_style {
                    s["fontStyle"] = json!(f);
                }
                json!({ "scope": r.scope, "settings": s })
            })
            .collect();
        json!({
            "workbench.colorTheme": if self.light { LIGHT } else { DARK },
            "workbench.colorCustomizations": self.colors,
            "editor.tokenColorCustomizations": { "textMateRules": rules },
        })
    }
}

/// Whether a `workbench.colorTheme` is Arugula's (none set: the one we'd set).
fn ours(v: &Value) -> bool {
    v.is_null() || v == DARK || v == LIGHT
}

/// Explicit theme or color customizations belong to the user. JSONC or
/// unreadable text is conservatively treated as customized, never overwritten.
pub fn user_leaves_it(settings: Option<&str>) -> bool {
    match settings {
        None => true,
        Some(text) => serde_json::from_str::<Value>(text).is_ok_and(|v| {
            ours(&v["workbench.colorTheme"])
                && !custom(&v["workbench.colorCustomizations"])
                && !custom(&v["editor.tokenColorCustomizations"])
        }),
    }
}

fn custom(value: &Value) -> bool {
    !value.is_null() && !value.as_object().is_some_and(Map::is_empty)
}

const OWNED: [&str; 3] = ["workbench.colorTheme", "workbench.colorCustomizations", "editor.tokenColorCustomizations"];

/// Preserve the whole workspace, and update only settings still identical to
/// our previous generated values. Existing JSONC/invalid JSON is left on disk
/// untouched rather than guessing which settings can safely be discarded.
pub fn workspace(
    old: Option<&str>,
    folder: &str,
    block: Value,
    theme: Option<&Theme>,
    previous: Option<&Theme>,
    user_leaves_it: bool,
) -> Result<Value, String> {
    let mut workspace = match old {
        Some(text) => serde_json::from_str::<Value>(text)
            .map_err(|_| "editor workspace uses JSONC or invalid JSON; leaving it unchanged".to_owned())?,
        None => json!({ "folders": [{ "path": folder }], "settings": {} }),
    };
    let root = workspace.as_object_mut().ok_or("editor workspace must be an object; leaving it unchanged")?;
    let settings = root
        .entry("settings")
        .or_insert_with(|| json!({}))
        .as_object_mut()
        .ok_or("editor workspace settings must be an object; leaving it unchanged")?;
    settings.insert("arugula.block".into(), block);
    if old.is_none() {
        settings.insert("arugula.compact".into(), json!(true));
    }
    let prior = previous.map(Theme::settings);
    let generated = |key: &str, value: &Value| prior.as_ref().is_some_and(|p| &p[key] == value);
    let customized = !ours(settings.get("workbench.colorTheme").unwrap_or(&Value::Null))
        || OWNED[1..].iter().any(|key| settings.get(*key).is_some_and(|v| custom(v) && !generated(key, v)));
    if !user_leaves_it {
        // A global choice must not remain shadowed by our old workspace values.
        // Remove only unchanged generated fields; any user edits remain theirs.
        for key in OWNED {
            if settings.get(key).is_some_and(|v| generated(key, v)) {
                settings.remove(key);
            }
        }
    } else if !customized
        && let Some(t) = theme
        && let Value::Object(next) = t.settings()
    {
        settings.extend(next);
    }
    Ok(workspace)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn latte() -> Value {
        json!({
            "light": true,
            "colors": { "editor.background": "#eff1f5", "list.hoverBackground": "#ccd0da80" },
            "tokenColors": [{ "scope": ["comment"], "settings": { "foreground": "#6c6f85", "fontStyle": "italic" } }],
        })
    }

    #[test]
    fn only_colors_get_in() {
        assert!(parse(&latte()).is_ok());
        let mut v = latte();
        v["colors"]["editor.background"] = json!("red; }");
        assert!(parse(&v).is_err());
        let mut v = latte();
        v["colors"]["a\"b"] = json!("#000000");
        assert!(parse(&v).is_err());
        let mut v = latte();
        v["tokenColors"][0]["scope"][0] = json!("comment\n");
        assert!(parse(&v).is_err());
        let mut v = latte();
        v["tokenColors"][0]["settings"]["fontStyle"] = json!("wild");
        assert!(parse(&v).is_err());
        let mut v = latte();
        v["files.autoSave"] = json!("afterDelay");
        assert!(parse(&v).is_err(), "nothing but colors");
    }

    #[test]
    fn the_workspace_takes_the_palette() {
        let t = parse(&latte()).unwrap();
        let w = workspace(None, "/r/p", json!(7), Some(&t), None, true).unwrap();
        assert_eq!(w["folders"][0]["path"], "/r/p");
        let s = &w["settings"];
        assert_eq!(s["arugula.block"], 7);
        assert_eq!(s["arugula.compact"], true);
        assert_eq!(s["workbench.colorTheme"], LIGHT);
        assert_eq!(s["workbench.colorCustomizations"]["editor.background"], "#eff1f5");
        assert_eq!(s["editor.tokenColorCustomizations"]["textMateRules"][0]["settings"]["fontStyle"], "italic");
        // Back to dark, keeping what else is there.
        let mut old = w.clone();
        old["settings"]["editor.fontSize"] = json!(15);
        let mut dark = latte();
        dark["light"] = json!(false);
        let w =
            workspace(Some(&old.to_string()), "/r/p", json!(7), Some(&parse(&dark).unwrap()), Some(&t), true).unwrap();
        assert_eq!(w["settings"]["workbench.colorTheme"], DARK);
        assert_eq!(w["settings"]["editor.fontSize"], 15);
        let existing = workspace(Some(r#"{"settings":{}}"#), "/r/p", json!(7), None, None, true).unwrap();
        assert!(existing["settings"].get("arugula.compact").is_none());
        // No palette yet: just the block.
        let w = workspace(None, "/r/p", json!(7), None, None, true).unwrap();
        assert!(w["settings"].get("workbench.colorTheme").is_none());
    }

    #[test]
    fn a_theme_someone_picked_stays() {
        let t = parse(&latte()).unwrap();
        // In their settings.
        assert!(user_leaves_it(None));
        assert!(user_leaves_it(Some(r#"{"workbench.colorTheme": "arugula"}"#)));
        assert!(user_leaves_it(Some("{}")));
        assert!(!user_leaves_it(Some(r#"{"workbench.colorTheme": "Default Light Modern"}"#)));
        assert!(!user_leaves_it(Some("{ // mine\n}")));
        let w = workspace(None, "/r/p", json!(7), Some(&t), None, false).unwrap();
        assert!(w["settings"].get("workbench.colorTheme").is_none());
        // In the block (VS Code's picker writes where it's set).
        let old = json!({ "settings": { "workbench.colorTheme": "Monokai" } }).to_string();
        let w = workspace(Some(&old), "/r/p", json!(7), Some(&t), None, true).unwrap();
        assert_eq!(w["settings"]["workbench.colorTheme"], "Monokai");
        assert!(w["settings"].get("workbench.colorCustomizations").is_none());
    }

    #[test]
    fn jsonc_and_invalid_workspace_settings_are_never_replaced() {
        let t = parse(&latte()).unwrap();
        for old in ["{ // user comment\n}", "{", r#"{"settings":null}"#, "[]"] {
            assert!(workspace(Some(old), "/r/p", json!(7), Some(&t), None, true).is_err());
        }
    }

    #[test]
    fn root_fields_and_color_overrides_are_preserved() {
        let t = parse(&latte()).unwrap();
        let mut old = workspace(None, "/r/p", json!(7), Some(&t), None, true).unwrap();
        old["folders"] = json!([{ "path": "/first" }, { "path": "/second" }]);
        old["tasks"] = json!({ "version": "2.0.0", "tasks": [] });
        old["settings"]["workbench.colorCustomizations"]["editor.background"] = json!("#123456");
        let mut dark = latte();
        dark["light"] = json!(false);
        let next = parse(&dark).unwrap();
        let w = workspace(Some(&old.to_string()), "/r/p", json!(7), Some(&next), Some(&t), true).unwrap();
        assert_eq!(w, old, "custom colors on Arugula also opt out");
        assert!(!next.applied(&w));
        old["settings"].as_object_mut().unwrap().remove("workbench.colorCustomizations");
        old["settings"]["editor.tokenColorCustomizations"] = json!({"comments":"#123456"});
        let w = workspace(Some(&old.to_string()), "/r/p", json!(7), Some(&next), Some(&t), true).unwrap();
        assert_eq!(w, old, "custom token colors also opt out");
    }

    #[test]
    fn global_choices_are_not_shadowed_by_generated_workspace_values() {
        let t = parse(&latte()).unwrap();
        let old = workspace(None, "/r/p", json!(7), Some(&t), None, true).unwrap();
        assert!(t.applied(&old));
        for user in [
            json!({"workbench.colorTheme":"Monokai"}),
            json!({"workbench.colorCustomizations":{"editor.background":"#123456"}}),
            json!({"editor.tokenColorCustomizations":{"comments":"#123456"}}),
        ] {
            assert!(!user_leaves_it(Some(&user.to_string())));
            let w = workspace(Some(&old.to_string()), "/r/p", json!(7), Some(&t), Some(&t), false).unwrap();
            for key in OWNED {
                assert!(w["settings"].get(key).is_none());
            }
        }
        let mut custom = old;
        custom["settings"]["workbench.colorTheme"] = json!("Monokai");
        custom["settings"]["workbench.colorCustomizations"] = json!({"editor.background":"#123456"});
        let w = workspace(Some(&custom.to_string()), "/r/p", json!(7), Some(&t), Some(&t), false).unwrap();
        assert_eq!(w["settings"]["workbench.colorTheme"], "Monokai");
        assert_eq!(w["settings"]["workbench.colorCustomizations"], custom["settings"]["workbench.colorCustomizations"]);
    }
}
