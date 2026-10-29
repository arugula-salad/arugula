//! The contract's example payloads (#219): `contract/examples/wire/
//! <module>.<Type>/*.json`, each decoded as its type and encoded again.
//! "The bytes don't change", as a test (`contract/README.md` has the rule):
//!
//! - `<name>.json` is a payload as it's sent: encoding what it decodes to
//!   gives back exactly its bytes.
//! - `<name>.lenient.json` is one a reader has to take although no
//!   up-to-date side sends it (a field it doesn't know, one an older side
//!   left out). It decodes, and encodes to exactly `<name>.json`'s bytes.
//!
//! Each file is one line of JSON and a newline. Each directory names a type
//! listed in [`types`], which is in its module's schema
//! (`contract/schema/<module>.json`).

use std::{collections::BTreeMap, path::PathBuf};

use serde::{Serialize, de::DeserializeOwned};
use serde_json::Value;

use crate::*;

fn dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../contract")
}

/// Decode `text` as `T` and encode it again.
fn again<T: Serialize + DeserializeOwned>(text: &str) -> Result<String, serde_json::Error> {
    serde_json::to_string(&serde_json::from_str::<T>(text)?)
}

type Again = fn(&str) -> Result<String, serde_json::Error>;

macro_rules! types {
    ($($name:literal => $t:ty,)*) => {
        BTreeMap::from([$(($name, again::<$t> as Again),)*])
    };
}

/// The types with examples, by directory.
fn types() -> BTreeMap<&'static str, Again> {
    types! {
        "activity.Activity" => activity::Activity,
        "forge.Poke" => forge::Poke,
        "internal.AccountArgs" => internal::AccountArgs,
        "internal.Archived" => internal::Archived,
        "internal.BoxPost" => internal::BoxPost,
        "internal.BoxPosted" => internal::BoxPosted,
        "internal.ChangePage" => internal::ChangePage,
        "internal.ChangesArgs" => internal::ChangesArgs,
        "internal.Deleted" => internal::Deleted,
        "internal.DeviceTrust" => internal::DeviceTrust,
        "internal.IdsArgs" => internal::IdsArgs,
        "internal.Meter" => internal::Meter,
        "internal.NoArgs" => internal::NoArgs,
        "internal.Outcome" => internal::Outcome,
        "internal.PublicPreview" => internal::PublicPreview,
        "internal.PushArgs" => internal::PushArgs,
        "internal.Pushed" => internal::Pushed,
        "internal.Resolved" => internal::Resolved,
        "internal.ResolveArgs" => internal::ResolveArgs,
        "internal.SendArgs" => internal::SendArgs,
        "internal.Team" => internal::Team,
        "internal.TokenAnswer" => internal::TokenAnswer,
        "internal.TurnAnswer" => internal::TurnAnswer,
        "internal.TurnArgs" => internal::TurnArgs,
        "internal.Vouch" => internal::Vouch,
        "internal.WorkArgs" => internal::WorkArgs,
        "join.JoinPoll" => join::JoinPoll,
        "join.JoinRequest" => join::JoinRequest,
        "join.JoinStarted" => join::JoinStarted,
        "lib.ControlInfo" => ControlInfo,
        "team.AccessList" => team::AccessList,
        "team.TrustAnswer" => team::TrustAnswer,
        "telemetry.Report" => telemetry::Report,
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
    let types = types();
    let wire = dir().join("examples/wire");
    let mut wrong = vec![];
    let mut seen = BTreeMap::new();
    let mut dirs: Vec<_> = std::fs::read_dir(&wire).unwrap().flatten().map(|e| e.path()).collect();
    dirs.sort();
    for d in dirs {
        let ty = d.file_name().unwrap().to_string_lossy().into_owned();
        let Some(again) = types.get(ty.as_str()) else {
            wrong.push(format!("{ty}/: no such type in examples.rs's types()"));
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
            wrong.push(format!("{ty}: in types() but no examples"));
        }
    }
    assert!(wrong.is_empty(), "{}", wrong.join("\n"));
}

/// Each directory's type is in its module's schema, by that name: what an
/// example is checked against in another language.
#[test]
fn every_example_type_is_in_the_schema() {
    let mut missing = vec![];
    for ty in types().keys() {
        let (module, name) = ty.split_once('.').unwrap();
        let schema = dir().join(format!("schema/{module}.json"));
        let defs = std::fs::read_to_string(&schema)
            .ok()
            .and_then(|t| serde_json::from_str::<Value>(&t).ok())
            .and_then(|v| v.get("$defs").cloned());
        if defs.as_ref().and_then(|d| d.get(name)).is_none() {
            missing.push(format!("{ty}: no $defs/{name} in contract/schema/{module}.json"));
        }
    }
    assert!(missing.is_empty(), "{}", missing.join("\n"));
}
