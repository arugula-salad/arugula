//! The list of deploy environments whose secrets the platform keeps
//! (`crates/control/secrets.toml`, M209), beside the table manifest: code
//! that names an environment's secret
//! (`arugula_platform::keys::environment_secret`) names one listed there.

/// Every deploy environment's secret that code names
/// (`arugula_platform::keys::environment_secret`) is in
/// `crates/control/secrets.toml`, named with literals.
#[test]
fn environment_secrets_named_in_code_are_listed() {
    #[derive(serde::Deserialize)]
    #[serde(deny_unknown_fields)]
    struct List {
        environment: Vec<Environment>,
    }
    #[derive(serde::Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Environment {
        name: String,
        #[allow(dead_code)]
        what: String,
        secrets: Vec<String>,
    }
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let text = std::fs::read_to_string(root.join("crates/control/secrets.toml")).unwrap();
    let list: List = toml::from_str(&text).unwrap_or_else(|e| panic!("crates/control/secrets.toml: {e}"));
    for e in &list.environment {
        assert!(
            e.secrets.iter().all(|s| arugula_platform::cloud::secrets::valid_name(s)),
            "{}: a reserved or bad name",
            e.name
        );
    }
    let named = named_in(&root.join("crates"));
    for (file, call) in &named {
        let found = parse_call(call).unwrap_or_else(|| panic!("{file}: environment_secret takes two literals: {call}"));
        assert!(
            list.environment.iter().any(|e| e.name == found.0 && e.secrets.contains(&found.1)),
            "{file}: {}'s secret {} isn't in crates/control/secrets.toml",
            found.0,
            found.1
        );
    }
    assert!(parse_call(r#"("arugula-release", "FLY_API_TOKEN")"#).is_some());
    assert!(parse_call("(env, name)").is_none());
}

/// Each `environment_secret(...)` call in the Rust under `dir`, but its
/// definition and these tests: the file and what follows the name.
fn named_in(dir: &std::path::Path) -> Vec<(String, String)> {
    let mut out = Vec::new();
    for e in std::fs::read_dir(dir).unwrap().flatten() {
        let p = e.path();
        if p.is_dir() {
            if !p.ends_with("target") && !p.ends_with("node_modules") {
                out.extend(named_in(&p));
            }
            continue;
        }
        if p.extension().is_none_or(|x| x != "rs") || p.ends_with("secrets_list.rs") {
            continue;
        }
        let text = std::fs::read_to_string(&p).unwrap();
        for (i, _) in text.match_indices("environment_secret(") {
            if text[..i].ends_with("fn ") {
                continue;
            }
            let rest = &text[i + "environment_secret".len()..];
            let end = rest.find(')').map_or(rest.len(), |n| n + 1);
            out.push((p.display().to_string(), rest[..end].to_owned()));
        }
    }
    out
}

/// `("env", "NAME")`: the two literals, else `None`.
fn parse_call(call: &str) -> Option<(String, String)> {
    let inner = call.trim().strip_prefix('(')?.strip_suffix(')')?;
    let (a, b) = inner.split_once(',')?;
    let lit = |s: &str| {
        let s = s.trim();
        let s = s.strip_prefix('"')?.strip_suffix('"')?;
        (!s.contains('"')).then(|| s.to_owned())
    };
    Some((lit(a)?, lit(b)?))
}
