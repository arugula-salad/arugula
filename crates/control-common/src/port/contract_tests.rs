//! The port's JSON Schema and example payloads, checked the way
//! `arugula-control-wire` checks `contract/` (its `schema.rs` and
//! `examples.rs`; `contract/README.md` has the rules):
//!
//! - `port/schema.json` is every message type's schema, under `$defs` by
//!   name. `ARUGULA_WRITE_SCHEMA=1` rewrites it; otherwise a stale one fails.
//! - `port/examples/<Type>/<name>.json` is a payload as it's sent: decoding
//!   and encoding it gives back exactly its bytes. `<name>.lenient.json` is
//!   one a reader has to take although no up-to-date side sends it: it
//!   encodes to exactly `<name>.json`'s bytes.

use std::{collections::BTreeMap, path::PathBuf};

use schemars::{JsonSchema, SchemaGenerator, generate::SchemaSettings};
use serde::{Serialize, de::DeserializeOwned};
use serde_json::{Map, Value, json};

use super::*;

fn dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("port")
}

/// Each type's name and schema, in the order they're listed.
type Roots = Vec<(String, Value)>;

fn root<T: JsonSchema>(g: &mut SchemaGenerator, out: &mut Roots) {
    out.push((T::schema_name().into_owned(), g.subschema_for::<T>().to_value()));
}

macro_rules! listed {
    ($($t:ty),* $(,)?) => {
        (&[$(stringify!($t)),*] as &[&str], (|g: &mut SchemaGenerator, out: &mut Roots| {
            $(root::<$t>(g, out);)*
        }) as fn(&mut SchemaGenerator, &mut Roots))
    };
}

/// Every public message type with a serde derive.
#[allow(clippy::type_complexity)]
fn types() -> (&'static [&'static str], fn(&mut SchemaGenerator, &mut Roots)) {
    listed![
        ResolveArgs,
        IdsArgs,
        AccountArgs,
        WordArgs,
        ReachArgs,
        PushArgs,
        OwnersArgs,
        MetersArgs,
        ChangesArgs,
        DriveArgs,
        TeamArgs,
        DaemonArgs,
        HostedRemovedArgs,
        BlockersArgs,
        DeletedArgs,
        NoArgs,
        TokenArgs,
        GithubIdArgs,
        SendAuthArgs,
        WorkspacesArgs,
        DelegationArgs,
        OpArgs,
        SendArgs,
        WakeArgs,
        TurnArgs,
        WorkArgs,
        ReportArgs,
        SessionReportArgs,
        SessionPushArgs,
        SessionIssuesArgs,
        SecretsArgs,
        HostedArgs,
        ProjectArgs,
        DrivesArgs,
        TranscriptArgs,
        PreviewArgs,
        ProductUserArgs,
        UnlinkArgs,
        AccessArgs,
        UploadArgs,
        StandingsArgs,
        Standing,
        GithubUser,
        LinkedAccount,
        ProductLink,
        PasskeyAssertion,
        PushOptions,
        Account,
        Resolved,
        Team,
        Member,
        Daemon,
        Pushed,
        Meter,
        ChangePage,
        Change,
        ChangeKind,
        Deleted,
        TokenAnswer,
        UserToken,
        Outcome,
        TurnAnswer,
        Archived,
        PublicPreview,
        HostedMachine,
        BoxMember,
        BoxPost,
        BoxPosted,
        Principal,
        Vouch,
        NotVouched,
        Credential,
    ]
}

/// The schema file: its `$defs`, sorted by name, each listed type among them.
fn render() -> String {
    let mut g = SchemaSettings::draft2020_12().into_generator();
    let mut roots = Roots::new();
    (types().1)(&mut g, &mut roots);
    let mut defs: BTreeMap<String, Value> = g.take_definitions(true).into_iter().collect();
    for (name, schema) in roots {
        // A type schemars inlines (no `$ref`) is still one of the file's.
        if schema.get("$ref").is_none() {
            defs.entry(name).or_insert(schema);
        }
    }
    let doc = json!({
        "$schema": "https://json-schema.org/draft/2020-12/schema",
        "title": "arugula_control_common::port",
        "$defs": defs.into_iter().collect::<Map<String, Value>>(),
    });
    serde_json::to_string_pretty(&doc).expect("a schema serializes") + "\n"
}

#[test]
fn the_checked_in_schema_is_current() {
    let path = dir().join("schema.json");
    let want = render();
    if std::env::var_os("ARUGULA_WRITE_SCHEMA").is_some() {
        std::fs::write(&path, want).unwrap();
        return;
    }
    let have = std::fs::read_to_string(&path).unwrap_or_default();
    assert!(
        have == want,
        "port/schema.json is stale: run `ARUGULA_WRITE_SCHEMA=1 scripts/server-cargo test -p arugula-control-common port::`"
    );
}

/// Decode `text` as `T` and encode it again.
fn again<T: Serialize + DeserializeOwned>(text: &str) -> Result<String, serde_json::Error> {
    serde_json::to_string(&serde_json::from_str::<T>(text)?)
}

type Again = fn(&str) -> Result<String, serde_json::Error>;

macro_rules! examples {
    ($($t:ident,)*) => {
        BTreeMap::from([$((stringify!($t), again::<$t> as Again),)*])
    };
}

/// The types with examples, by directory.
fn examples() -> BTreeMap<&'static str, Again> {
    examples! {
        AccountArgs, Archived, BoxPost, BoxPosted, ChangePage, ChangesArgs, Deleted, IdsArgs, Meter, NoArgs, Outcome,
        PublicPreview, PushArgs, Pushed, Resolved, ResolveArgs, SendArgs, Team, TokenAnswer, TurnAnswer, TurnArgs,
        Vouch, WorkArgs,
    }
}

/// A file's one line, or why it isn't one.
fn line(name: &str, text: &str) -> Result<String, String> {
    let body = text.strip_suffix('\n').ok_or(format!("{name}: no newline at its end"))?;
    if body.contains('\n') {
        return Err(format!("{name}: more than one line: an example is compact JSON, as it's sent"));
    }
    Ok(body.to_owned())
}

#[test]
fn every_example_round_trips_byte_for_byte() {
    let types = examples();
    let mut wrong = vec![];
    let mut seen = BTreeMap::new();
    let mut dirs: Vec<_> = std::fs::read_dir(dir().join("examples")).unwrap().flatten().map(|e| e.path()).collect();
    dirs.sort();
    for d in dirs {
        let ty = d.file_name().unwrap().to_string_lossy().into_owned();
        let Some(again) = types.get(ty.as_str()) else {
            wrong.push(format!("{ty}/: no such type in contract_tests.rs's examples()"));
            continue;
        };
        let mut files: BTreeMap<String, String> = std::fs::read_dir(&d)
            .unwrap()
            .flatten()
            .map(|e| (e.file_name().to_string_lossy().into_owned(), std::fs::read_to_string(e.path()).unwrap()))
            .collect();
        let lenient: Vec<String> = files.keys().filter(|n| n.ends_with(".lenient.json")).cloned().collect();
        for name in lenient {
            let text = files.remove(&name).unwrap();
            let path = format!("{ty}/{name}");
            let sent = name.replace(".lenient.json", ".json");
            let Some(want) = files.get(&sent) else {
                wrong.push(format!("{path}: no {sent} beside it, the bytes it encodes to"));
                continue;
            };
            match (line(&path, &text), line(&sent, want)) {
                (Ok(body), Ok(want)) if body == want => {
                    wrong.push(format!("{path}: the same as {sent}: a lenient example differs from what's sent"));
                }
                (Ok(body), Ok(want)) => match again(&body) {
                    Ok(got) if got == want => {}
                    Ok(got) => wrong.push(format!("{path}: encodes as\n  {got}\nnot {sent}'s\n  {want}")),
                    Err(e) => wrong.push(format!("{path}: doesn't decode: {e}")),
                },
                (Err(e), _) | (_, Err(e)) => wrong.push(e),
            }
        }
        for (name, text) in &files {
            let path = format!("{ty}/{name}");
            if !name.ends_with(".json") {
                wrong.push(format!("{path}: not a .json"));
                continue;
            }
            match line(&path, text) {
                Ok(body) => match again(&body) {
                    Ok(got) if got == body => {}
                    Ok(got) => wrong.push(format!("{path}: encodes as\n  {got}\nnot\n  {body}")),
                    Err(e) => wrong.push(format!("{path}: doesn't decode: {e}")),
                },
                Err(e) => wrong.push(e),
            }
        }
        seen.insert(ty, files.len());
    }
    for ty in types.keys() {
        if seen.get(*ty).copied().unwrap_or(0) == 0 {
            wrong.push(format!("{ty}: in examples() but no examples"));
        }
    }
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
}

/// Each example's type is in the schema, by that name: what an example is
/// checked against in another language.
#[test]
fn every_example_type_is_in_the_schema() {
    let defs = serde_json::from_str::<Value>(&render()).unwrap()["$defs"].clone();
    let missing: Vec<_> = examples().into_keys().filter(|t| defs.get(t).is_none()).collect();
    assert!(missing.is_empty(), "no $defs for {missing:?} in port/schema.json");
}

/// Every public type with a serde derive outside the tests: its schema
/// derive beside it, and its name in [`types`].
#[test]
fn every_message_type_is_in_the_schema() {
    let src = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src/port");
    let listed: Vec<&str> = types().0.to_vec();
    let mut missing = vec![];
    for e in std::fs::read_dir(&src).unwrap().flatten() {
        let rel = e.file_name().to_string_lossy().into_owned();
        if !rel.ends_with(".rs") || rel == "contract_tests.rs" {
            continue;
        }
        let text = std::fs::read_to_string(e.path()).unwrap();
        let text = text.split("#[cfg(test)]\nmod tests").next().unwrap();
        let lines: Vec<&str> = text.lines().collect();
        let mut i = 0;
        while i < lines.len() {
            let l = lines[i].trim_start();
            if !l.starts_with("#[derive(") {
                i += 1;
                continue;
            }
            let mut derive = l.to_owned();
            while !lines[i].contains(")]") {
                i += 1;
                derive.push_str(lines[i]);
            }
            i += 1;
            if !derive.contains("Serialize") && !derive.contains("Deserialize") {
                continue;
            }
            let mut schema = false;
            while i < lines.len() {
                let l = lines[i].trim_start();
                if l.starts_with("#[") || l.starts_with("//") {
                    schema |= l.contains("derive(schemars::JsonSchema)");
                    i += 1;
                } else {
                    break;
                }
            }
            let Some(decl) = lines.get(i) else { break };
            let words: Vec<&str> = decl.split_whitespace().collect();
            let (vis, name) = match words.as_slice() {
                ["struct" | "enum", name, ..] => ("", *name),
                [vis, _, name, ..] => (*vis, *name),
                _ => continue,
            };
            let name = name.split(['<', '(', '{', ';']).next().unwrap();
            if !schema {
                missing.push(format!("{rel}: {name} has no schema derive"));
            }
            if vis == "pub" && !listed.contains(&name) {
                missing.push(format!("{rel}: {name} isn't in contract_tests.rs's types()"));
            }
        }
    }
    assert!(missing.is_empty(), "{}", missing.join("\n"));
}
