//! What every example file is, and the keys they're made with.
//!
//! A file:
//!
//! ```text
//! {
//!   "about":   what the file checks, in words
//!   "checks":  the Rust functions it checks, by path
//!   "input":   what a case's input holds
//!   "output":  what a case's output holds
//!   "keys":    { label: { "seed", "sign", "noise", "device" } }, the fixed keys
//!   "objects": { label: a signed object (a certificate, a roster, ...) }
//!   "cases":   [ { "about", "input", "output" } ]
//! }
//! ```
//!
//! An input names objects by their label; the file's `objects` holds them.

use std::{collections::BTreeMap, path::PathBuf};

use anyhow::{Context as _, anyhow, bail, ensure};
use arugula_e2e::DeviceKeys;
use ed25519_dalek::SigningKey;
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use serde_json::{Value, json};

/// Where the files are.
pub fn dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../contract/examples/logic")
}

/// A device's keys from one byte: its Ed25519 seed is that byte 32 times,
/// and so is its Noise public key (no check here uses the Noise key but to
/// derive the device's id, so it needs no private half). Obviously not
/// anyone's real key.
pub fn keys(n: u8) -> DeviceKeys {
    DeviceKeys { noise_private: [0; 32], noise_public: [n; 32], sign: SigningKey::from_bytes(&[n; 32]) }
}

/// What a file says of a key.
pub fn key_info(n: u8) -> Value {
    let k = keys(n);
    json!({
        "seed": hex::encode([n; 32]),
        "sign": hex::encode(k.sign_public()),
        "noise": hex::encode(k.noise_public),
        "device": k.id(),
    })
}

/// A case's run: its input, with the file's objects, to its output.
pub type Run = fn(&Ctx, &Value) -> anyhow::Result<Value>;

/// The file's objects, by label.
pub struct Ctx {
    objects: BTreeMap<String, Value>,
}

impl Ctx {
    /// The object labelled `label`, as a `T`.
    pub fn get<T: DeserializeOwned>(&self, label: &str) -> anyhow::Result<T> {
        let v = self.objects.get(label).ok_or_else(|| anyhow!("no object {label:?}"))?;
        serde_json::from_value(v.clone()).with_context(|| format!("object {label:?}"))
    }

    /// The objects labelled `labels`, in order.
    pub fn all<T: DeserializeOwned>(&self, labels: &[String]) -> anyhow::Result<Vec<T>> {
        labels.iter().map(|l| self.get(l)).collect()
    }
}

/// A case's input, read strictly: an unknown field is an error.
pub fn input<T: DeserializeOwned>(v: &Value) -> anyhow::Result<T> {
    serde_json::from_value(v.clone()).context("the case's input")
}

/// A file: where it goes, what runs its cases, and what makes it.
pub struct Spec {
    pub module: &'static str,
    pub name: &'static str,
    pub run: Run,
    pub make: fn() -> Example,
}

impl Spec {
    /// Make it, checking it's the file this says.
    pub fn example(&self) -> Example {
        let ex = (self.make)();
        assert_eq!((ex.module, ex.name), (self.module, self.name), "a spec that makes another file");
        ex
    }

    pub fn path(&self) -> PathBuf {
        dir().join(self.module).join(format!("{}.json", self.name))
    }
}

/// One file, as the generator makes it.
pub struct Example {
    pub module: &'static str,
    pub name: &'static str,
    pub run: Run,
    about: String,
    checks: Vec<String>,
    input: String,
    output: String,
    keys: BTreeMap<String, Value>,
    objects: BTreeMap<String, Value>,
    cases: Vec<Value>,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct File {
    about: String,
    checks: Vec<String>,
    input: String,
    output: String,
    #[serde(default)]
    keys: BTreeMap<String, Value>,
    #[serde(default)]
    objects: BTreeMap<String, Value>,
    cases: Vec<Case>,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Case {
    about: String,
    input: Value,
    output: Value,
}

impl Example {
    pub fn new(module: &'static str, name: &'static str, run: Run, about: &str, checks: &[&str]) -> Self {
        Self {
            module,
            name,
            run,
            about: about.into(),
            checks: checks.iter().map(|c| (*c).into()).collect(),
            input: String::new(),
            output: String::new(),
            keys: BTreeMap::new(),
            objects: BTreeMap::new(),
            cases: vec![],
        }
    }

    /// What a case's input and output hold, in words.
    pub fn shapes(mut self, input: &str, output: &str) -> Self {
        self.input = input.into();
        self.output = output.into();
        self
    }

    /// Note key `n` under `label` (its seed, public keys and device id).
    pub fn key(&mut self, label: &str, n: u8) -> DeviceKeys {
        let info = key_info(n);
        if let Some(old) = self.keys.insert(label.into(), info.clone()) {
            assert_eq!(old, info, "{}/{}: key {label} twice", self.module, self.name);
        }
        keys(n)
    }

    /// Note any other key material by label (a hosted key's seed and public half).
    #[allow(dead_code)]
    pub fn key_value(&mut self, label: &str, v: Value) {
        self.keys.insert(label.into(), v);
    }

    /// Keep `v` under `label`, for inputs to name; returns the label.
    pub fn object(&mut self, label: &str, v: &impl Serialize) -> String {
        let v = serde_json::to_value(v).unwrap();
        if let Some(old) = self.objects.insert(label.into(), v.clone()) {
            assert_eq!(old, v, "{}/{}: object {label} twice, unlike", self.module, self.name);
        }
        label.into()
    }

    /// A case whose output is what the code gives, and which must be `want`.
    pub fn case(&mut self, about: &str, input: Value, want: Value) {
        let got = self.output_of(&input);
        assert_eq!(got, want, "{}/{}: {about}\n{input:#}", self.module, self.name);
        self.push(about, input, got);
    }

    /// A case whose output is what the code gives (a text, a digest), with no
    /// answer written down beside it.
    pub fn computed(&mut self, about: &str, input: Value) -> Value {
        let got = self.output_of(&input);
        self.push(about, input, got.clone());
        got
    }

    fn output_of(&self, input: &Value) -> Value {
        let ctx = Ctx { objects: self.objects.clone() };
        (self.run)(&ctx, input).unwrap_or_else(|e| panic!("{}/{}: {e:#}\n{input:#}", self.module, self.name))
    }

    fn push(&mut self, about: &str, input: Value, output: Value) {
        assert!(!about.is_empty());
        assert!(
            !self.cases.iter().any(|c| c["about"] == about),
            "{}/{}: two cases are {about:?}",
            self.module,
            self.name
        );
        self.cases.push(json!({ "about": about, "input": input, "output": output }));
    }

    /// The file's text: pretty JSON, keys sorted, a newline at the end.
    pub fn text(&self) -> String {
        assert!(!self.input.is_empty() && !self.output.is_empty(), "{}/{}: no shapes", self.module, self.name);
        let f = json!({
            "about": self.about,
            "checks": self.checks,
            "input": self.input,
            "output": self.output,
            "keys": self.keys,
            "objects": self.objects,
            "cases": self.cases,
        });
        let mut out = String::new();
        pretty(&f, 0, &mut out);
        out + "\n"
    }
}

/// JSON, two spaces an indent, keys sorted, and an array of plain values,
/// or an object of up to four, on one line when it fits in 100 characters.
fn pretty(v: &Value, depth: usize, out: &mut String) {
    let pad = |n: usize| "  ".repeat(n);
    match v {
        Value::Array(a) if a.is_empty() => out.push_str("[]"),
        Value::Object(o) if o.is_empty() => out.push_str("{}"),
        Value::Array(a) => {
            let items: Vec<String> = a.iter().map(|x| serde_json::to_string(x).unwrap()).collect();
            let flat = format!("[{}]", items.join(", "));
            if a.iter().all(|x| !x.is_array() && !x.is_object()) && flat.len() + depth * 2 <= 100 {
                out.push_str(&flat);
                return;
            }
            out.push_str("[\n");
            for (i, x) in a.iter().enumerate() {
                out.push_str(&pad(depth + 1));
                pretty(x, depth + 1, out);
                out.push_str(if i + 1 < a.len() { ",\n" } else { "\n" });
            }
            out.push_str(&pad(depth));
            out.push(']');
        }
        Value::Object(o) => {
            let items: Vec<String> = o
                .iter()
                .map(|(k, x)| format!("{}: {}", serde_json::to_string(k).unwrap(), serde_json::to_string(x).unwrap()))
                .collect();
            let flat = format!("{{{}}}", items.join(", "));
            if o.values().all(|x| !x.is_array() && !x.is_object()) && o.len() <= 4 && flat.len() + depth * 2 <= 100 {
                out.push_str(&flat);
                return;
            }
            out.push_str("{\n");
            for (i, (k, x)) in o.iter().enumerate() {
                out.push_str(&pad(depth + 1));
                out.push_str(&serde_json::to_string(k).unwrap());
                out.push_str(": ");
                pretty(x, depth + 1, out);
                out.push_str(if i + 1 < o.len() { ",\n" } else { "\n" });
            }
            out.push_str(&pad(depth));
            out.push('}');
        }
        _ => out.push_str(&serde_json::to_string(v).unwrap()),
    }
}

/// Run every case of a file's text with `run`: how many there were, or why
/// one didn't give its output.
pub fn run_file(text: &str, run: Run) -> anyhow::Result<usize> {
    let f: File = serde_json::from_str(text)?;
    ensure!(!f.about.trim().is_empty(), "the file says nothing of what it checks");
    ensure!(!f.checks.is_empty() && !f.input.is_empty() && !f.output.is_empty(), "the file has no checks or shapes");
    ensure!(!f.cases.is_empty(), "no cases");
    let ctx = Ctx { objects: f.objects };
    for (i, c) in f.cases.iter().enumerate() {
        if c.about.trim().is_empty() {
            bail!("case {i} says nothing of what it checks");
        }
        let got = run(&ctx, &c.input).with_context(|| format!("case {i} ({})", c.about))?;
        ensure!(got == c.output, "case {i} ({}): the code gives {got}, the file says {}", c.about, c.output);
    }
    Ok(f.cases.len())
}

/// A certificate for `k`, made at `created` and signed by `by`.
pub fn cert(
    k: &DeviceKeys,
    account: &str,
    kind: arugula_e2e::Kind,
    name: &str,
    created: u64,
    by: &DeviceKeys,
) -> arugula_e2e::Cert {
    let mut c = arugula_e2e::Cert::new(k, account, kind, name);
    c.created = created;
    c.sign_with(by);
    c
}

/// `by`'s revocation of `device` at `at`.
pub fn revoke(account: &str, device: &str, at: u64, by: &DeviceKeys) -> arugula_e2e::Revocation {
    let mut r = arugula_e2e::Revocation::new(account, device, by);
    r.at = at;
    r.sig = hex::encode(by.signature(r.body().as_bytes()));
    r
}

/// Labels, as an input lists them.
pub fn labels(l: &[&str]) -> Value {
    json!(l)
}

/// An account's certificates and revocations, by label, as an input lists them.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Account {
    pub certs: Vec<String>,
    #[serde(default)]
    pub revocations: Vec<String>,
}

pub fn account_certs(ctx: &Ctx, a: &BTreeMap<String, Account>) -> anyhow::Result<arugula_e2e::team::AccountCerts> {
    a.iter()
        .map(|(k, v)| {
            Ok((
                k.clone(),
                (ctx.all::<arugula_e2e::Cert>(&v.certs)?, ctx.all::<arugula_e2e::Revocation>(&v.revocations)?),
            ))
        })
        .collect()
}

pub const CERTS_SHAPE: &str = "certs: every member account's certificates and revocations, as control hands them \
                               out: { account: { certs: [labels], revocations: [labels] } }";

pub const OWNERS_SHAPE: &str = "owners: who may sign for the project, as the checking side pinned them: { team: { \
                                roster: a label, certs: { account: { certs, revocations } } } } for a team's \
                                project, or { account: { account, root, certs: [labels], revocations: [labels] } } \
                                for one account's";

/// A project's owners, as an input names them.
#[derive(Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum OwnersIn {
    Team {
        roster: String,
        certs: BTreeMap<String, Account>,
    },
    Account {
        account: String,
        root: String,
        certs: Vec<String>,
        #[serde(default)]
        revocations: Vec<String>,
    },
}

/// What [`arugula_e2e::hosted::Owners`] borrows.
#[allow(clippy::large_enum_variant)]
pub enum OwnedOwners {
    Team(arugula_e2e::team::Roster, arugula_e2e::team::AccountCerts),
    Account(arugula_e2e::Trust, Vec<arugula_e2e::Cert>, Vec<arugula_e2e::Revocation>),
}

impl OwnersIn {
    pub fn load(&self, ctx: &Ctx) -> anyhow::Result<OwnedOwners> {
        Ok(match self {
            OwnersIn::Team { roster, certs } => OwnedOwners::Team(ctx.get(roster)?, account_certs(ctx, certs)?),
            OwnersIn::Account { account, root, certs, revocations } => OwnedOwners::Account(
                arugula_e2e::Trust { account: account.clone(), root: root.clone() },
                ctx.all(certs)?,
                ctx.all(revocations)?,
            ),
        })
    }
}

impl OwnedOwners {
    pub fn owners(&self) -> arugula_e2e::hosted::Owners<'_> {
        match self {
            OwnedOwners::Team(roster, certs) => arugula_e2e::hosted::Owners::Team { roster, certs },
            OwnedOwners::Account(trust, certs, revocations) => {
                arugula_e2e::hosted::Owners::Account { trust, certs, revocations }
            }
        }
    }
}

/// Someone with a browser as their account's root (made at 1000), kept
/// under their account's name as a key and as an object.
pub struct Person {
    pub account: &'static str,
    pub keys: DeviceKeys,
    pub cert: arugula_e2e::Cert,
}

pub fn person(ex: &mut Example, account: &'static str, n: u8) -> Person {
    let keys = ex.key(account, n);
    let cert = cert(&keys, account, arugula_e2e::Kind::Browser, &format!("{account}'s laptop"), 1_000, &keys);
    ex.object(account, &cert);
    Person { account, keys, cert }
}

pub fn member(p: &Person, role: arugula_e2e::team::TeamRole) -> arugula_e2e::team::Member {
    arugula_e2e::team::Member {
        account: p.account.into(),
        root: p.cert.device.clone(),
        role,
        name: format!("{}-login", p.account),
    }
}
