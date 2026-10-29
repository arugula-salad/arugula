//! A driver's plan, as the platform makes it and a box checks it (M220,
//! platform step 7, native Arugula #16): [`plan_text`] for every driver,
//! with what the chant→Fly driver adds ([`CHANT_FLY`]).
//!
//! The plan is what an owner reads before they approve a deploy, and its
//! SHA-256 is the deploy's `plan`, which the signed operation holds. So
//! everything the plan says binds the approval: the driver, the
//! environment's settings (its Fly app, where its chant op is), and for a
//! roll back, the deploy it goes back to and what that deploy ran. A box
//! recomputes it from the operation, the driver, the settings and the roll
//! back it's handed ([`planned`]), and refuses anything else.
//!
//! A driver with no settings and no roll back makes exactly
//! [`crate::deploy::plan_text`], so the plans M219's drivers made don't
//! change.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::deploy::{Operation, planter};

#[path = "drivers_arugula_release.rs"]
pub mod arugula_release;

/// The chant→Fly driver's name, as environments name it.
pub const CHANT_FLY: &str = "chant-fly";

/// A driver's settings on an environment: what it needs that isn't
/// secret, by name. Sorted, so the plan reads the same everywhere.
pub type Settings = BTreeMap<String, String>;

/// What a roll back goes back to: an earlier deploy of the same
/// environment that succeeded, and what its log kept of what it ran (for
/// chant→Fly: [`IMAGE`], and chant's release record, [`CHANT_COMPONENT`],
/// [`CHANT_ENV`] and [`CHANT_DIGEST`]).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct RollBack {
    /// `dep_` and 32 hex characters.
    pub deploy: String,
    pub to: Settings,
}

/// chant→Fly's settings:
/// - `fly_app` (required): the Fly app;
/// - `chant_env` (required): the environment chant's release ledger
///   records the release under, which the record found must name;
/// - `chant_dir`, `chant_op`: the chant member (from the repository's
///   root, default `.`) and its op (default `release`);
/// - `fly_config` (optional): the `fly.toml` a roll back deploys with;
/// - `chant_gates` (optional): the only gates the driver approves, comma
///   separated; any other stops the deploy;
/// - `npm_scripts` (`true`, optional): `npm ci` runs the packages' install
///   scripts (otherwise `--ignore-scripts`);
/// - `chant_github` (`true`, optional): chant's op gets `GITHUB_TOKEN` too
///   (an op that pushes, such as a release that tags). Without it, chant
///   reads and writes its ledger through a stand-in for origin on the box,
///   and the driver pushes it;
/// - `chant_digest` (`plan` or `image`, default `plan`): what the release
///   record's digest is. `image`: it must be the digest of the image Fly
///   runs after. `plan`: Fly must run another image than before.
pub const FLY_APP: &str = "fly_app";
pub const CHANT_DIR: &str = "chant_dir";
pub const CHANT_OP: &str = "chant_op";
pub const FLY_CONFIG: &str = "fly_config";
pub const CHANT_GATES: &str = "chant_gates";
pub const NPM_SCRIPTS: &str = "npm_scripts";
pub const CHANT_GITHUB: &str = "chant_github";
pub const CHANT_DIGEST_IS: &str = "chant_digest";
const CHANT_FLY_SETTINGS: &[&str] =
    &[FLY_APP, CHANT_ENV, CHANT_DIR, CHANT_OP, FLY_CONFIG, CHANT_GATES, NPM_SCRIPTS, CHANT_GITHUB, CHANT_DIGEST_IS];

/// The longest a chant→Fly token may live, from the box's check (the
/// token's own `ValidityWindow`): 30 days.
pub const FLY_TOKEN_MAX_HOURS: u64 = 720;

/// What a chant→Fly deploy's log keeps of what it ran, and a roll back
/// goes back to. (`chant_env` is a setting's name too.)
pub const IMAGE: &str = "image";
pub const CHANT_COMPONENT: &str = "chant_component";
pub const CHANT_ENV: &str = "chant_env";
pub const CHANT_DIGEST: &str = "chant_digest";

/// The secret chant→Fly needs: a Fly deploy token for the one app, that
/// expires (`fly tokens create deploy -a <app> -x 720h`).
pub const FLY_API_TOKEN: &str = "FLY_API_TOKEN";

/// Whether a chant→Fly (or arugula-release, M221) deploy's source is one:
/// a commit, by its full 40-hex id (never a branch, a tag or a short id
/// the box resolves).
pub fn source_ok(driver: &str, kind: &str, reference: &str) -> bool {
    // planter's (M233): Ship an artifact by its digest, the rest a commit.
    if planter::is_planter(driver) {
        return planter::source_ok(driver, kind, reference);
    }
    (driver != CHANT_FLY && driver != arugula_release::DRIVER)
        || (kind == "commit"
            && reference.len() == 40
            && reference.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f')))
}

/// Whether `image` is one of Fly's registry's for `app`, by digest:
/// `registry.fly.io/<app>[:<tag>]@sha256:<64 hex>`.
pub fn fly_image_ok(app: &str, image: &str) -> bool {
    let Some(rest) = image.strip_prefix("registry.fly.io/").and_then(|r| r.strip_prefix(app)) else { return false };
    let Some((tag, digest)) = rest.split_once("@sha256:") else { return false };
    let tag_ok = tag.is_empty()
        || tag.strip_prefix(':').is_some_and(|t| {
            (1..=128).contains(&t.len())
                && t.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-'))
        });
    tag_ok && digest.len() == 64 && digest.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
}

/// Whether `link` is a line of chant's ledger in `place`'s repository on
/// GitHub, at a commit: `https://github.com/<place>/blob/<40 hex>/<path
/// to a .jsonl>#L<n>`.
pub fn ledger_link_ok(place: &str, link: &str) -> bool {
    let Some(rest) = link.strip_prefix("https://github.com/").and_then(|r| r.strip_prefix(place)) else {
        return false;
    };
    let Some(rest) = rest.strip_prefix("/blob/") else { return false };
    let Some((commit, rest)) = rest.split_once('/') else { return false };
    let Some((path, line)) = rest.split_once("#L") else { return false };
    commit.len() == 40
        && commit.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
        && path.ends_with(".jsonl")
        && path.len() <= 200
        && path.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-' | b'/'))
        && path.split('/').all(|p| !p.is_empty() && p != "." && p != "..")
        && (1..=7).contains(&line.len())
        && line.bytes().all(|b| b.is_ascii_digit())
        && !line.starts_with('0')
}

/// A Fly app's name: a-z, 0-9 and `-`, up to 63.
fn fly_app_ok(s: &str) -> bool {
    (1..=63).contains(&s.len()) && s.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
}

/// A path in the repository: relative, no `..`, letters, digits and
/// `._-/` only.
fn repo_path_ok(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 200
        && !s.starts_with('/')
        && s.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-' | b'/'))
        && s.split('/').all(|p| p != "..")
}

/// A chant op's (or gate's, or ledger environment's) name: a letter or
/// digit first.
fn op_ok(s: &str) -> bool {
    (1..=64).contains(&s.len())
        && s.as_bytes()[0].is_ascii_alphanumeric()
        && s.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}

/// One word: printable, no space, at most 300 bytes.
pub fn word(s: &str) -> bool {
    !s.is_empty() && s.len() <= 300 && s.bytes().all(|b| b.is_ascii_graphic())
}

/// `driver`'s settings as an environment keeps them, defaults filled in:
/// `Err` why not. A driver without settings takes none.
pub fn settings(driver: &str, given: &Settings) -> Result<Settings, String> {
    if driver == arugula_release::DRIVER {
        return arugula_release::settings(given);
    }
    if planter::is_planter(driver) {
        return planter::settings(driver, given);
    }
    if driver != CHANT_FLY {
        return match given.keys().next() {
            None => Ok(Settings::new()),
            Some(k) => Err(format!("the {driver} driver takes no settings ({k})")),
        };
    }
    if let Some(k) = given.keys().find(|k| !CHANT_FLY_SETTINGS.contains(&k.as_str())) {
        return Err(format!("chant-fly has no setting {k}: it takes {}", CHANT_FLY_SETTINGS.join(", ")));
    }
    // A letter or digit first (or the repository's root, `.`): no value
    // reads as a flag.
    let first_ok =
        |k: &str, v: &str| v.as_bytes().first().is_some_and(u8::is_ascii_alphanumeric) || (k == CHANT_DIR && v == ".");
    if let Some((k, _)) = given.iter().find(|(k, v)| !first_ok(k, v)) {
        return Err(format!("{k} starts with a letter or a digit"));
    }
    let mut s = given.clone();
    match s.get(FLY_APP) {
        Some(a) if fly_app_ok(a) => {}
        Some(_) => return Err("fly_app is a Fly app's name: a-z, 0-9 and -".into()),
        None => return Err("chant-fly needs fly_app: the Fly app it deploys".into()),
    }
    match s.get(CHANT_ENV) {
        Some(e) if op_ok(e) => {}
        Some(_) => return Err("chant_env is a chant ledger environment's name".into()),
        None => {
            return Err("chant-fly needs chant_env: the environment chant's ledger records its releases under".into());
        }
    }
    s.entry(CHANT_DIR.into()).or_insert_with(|| ".".into());
    s.entry(CHANT_OP.into()).or_insert_with(|| "release".into());
    s.entry(CHANT_DIGEST_IS.into()).or_insert_with(|| "plan".into());
    if !repo_path_ok(&s[CHANT_DIR]) || s.get(FLY_CONFIG).is_some_and(|c| !repo_path_ok(c)) {
        return Err("chant_dir and fly_config are paths in the repository: relative, no ..".into());
    }
    if !op_ok(&s[CHANT_OP]) {
        return Err("chant_op is a chant op's name".into());
    }
    if s.get(CHANT_GATES).is_some_and(|g| !g.split(',').all(op_ok)) {
        return Err("chant_gates is gates' names, comma separated".into());
    }
    for flag in [NPM_SCRIPTS, CHANT_GITHUB] {
        if s.get(flag).is_some_and(|v| v != "true") {
            return Err(format!("{flag} is true, or left out"));
        }
    }
    if !["plan", "image"].contains(&s[CHANT_DIGEST_IS].as_str()) {
        return Err("chant_digest is plan or image".into());
    }
    Ok(s)
}

/// Whether `r` is a roll back `driver` can do with `settings`: the
/// deploy's id, and (for chant→Fly) an image of the app's, by digest, and
/// chant's release record in the settings' ledger environment, each one
/// word starting with a letter or a digit.
pub fn roll_back_ok(driver: &str, settings: &Settings, r: &RollBack) -> bool {
    let id = r.deploy.strip_prefix("dep_").is_some_and(|h| h.len() == 32 && h.bytes().all(|b| b.is_ascii_hexdigit()));
    let keys: Vec<&str> = r.to.keys().map(String::as_str).collect();
    let get = |m: &Settings, k: &str| m.get(k).map_or("", String::as_str).to_owned();
    id && r.to.values().all(|v| word(v) && v.as_bytes()[0].is_ascii_alphanumeric())
        && match driver {
            CHANT_FLY => {
                keys == [CHANT_COMPONENT, CHANT_DIGEST, CHANT_ENV, IMAGE]
                    && fly_image_ok(&get(settings, FLY_APP), &get(&r.to, IMAGE))
                    && get(&r.to, CHANT_ENV) == get(settings, CHANT_ENV)
            }
            _ => keys.is_empty(),
        }
}

/// The plan `driver` makes for deploying (or, with `roll_back`, rolling
/// back to) a source in an environment: [`crate::deploy::plan_text`], then
/// its settings, the roll back, and what the driver does, in words.
pub fn plan_text(
    driver: &str,
    environment_name: &str,
    source_kind: &str,
    source_ref: &str,
    source_where: &str,
    settings: &Settings,
    roll_back: Option<&RollBack>,
) -> String {
    let mut t = crate::deploy::plan_text(driver, environment_name, source_kind, source_ref, source_where);
    for (k, v) in settings {
        t.push_str(&format!("setting {k} {v}\n"));
    }
    if let Some(r) = roll_back {
        t.push_str(&format!("roll back to {}\n", r.deploy));
        for (k, v) in &r.to {
            t.push_str(&format!("back {k} {v}\n"));
        }
    }
    if driver == CHANT_FLY {
        t.push_str(&chant_fly_does(source_ref, source_where, settings, roll_back));
    }
    // Arugula's own release (M221) never rolls back: a roll back's plan
    // says nothing it'd do, and nothing applies it.
    if driver == arugula_release::DRIVER && roll_back.is_none() {
        t.push_str(&arugula_release::plan_lines(source_kind, source_ref, source_where, settings));
    }
    // planter's (M233): only Go live rolls back (a revert commit, as
    // apps-platform's), and its plan says so either way.
    if planter::is_planter(driver) && (roll_back.is_none() || driver == planter::GO_LIVE) {
        t.push_str(&planter::plan_lines(driver, source_kind, source_ref, source_where, settings));
    }
    t
}

/// What chant→Fly does, in words: the part of its plan a person reads.
fn chant_fly_does(source_ref: &str, source_where: &str, s: &Settings, roll_back: Option<&RollBack>) -> String {
    let get = |k: &str| s.get(k).map_or("", String::as_str);
    let (app, dir, op, env) = (get(FLY_APP), get(CHANT_DIR), get(CHANT_OP), get(CHANT_ENV));
    let config = s.get(FLY_CONFIG).map(|c| format!(" --config {c}")).unwrap_or_default();
    let npm = if get(NPM_SCRIPTS) == "true" {
        "npm ci (its packages' install scripts run, without the tokens)"
    } else {
        "npm ci --ignore-scripts (without the tokens)"
    };
    let mut t =
        format!("does: check out commit {source_ref} from {source_where}, exactly, on a box made for this deploy\n");
    t.push_str(&format!(
        "does: check FLY_API_TOKEN is a Fly deploy token for one app that expires within {} days (flyctl tokens \
         debug), or stop\n",
        FLY_TOKEN_MAX_HOURS / 24
    ));
    match roll_back {
        None => {
            t.push_str(&format!("does: in {dir}, {npm}, then chant run {op}\n"));
            t.push_str(&format!("does: the repo's chant op {op} runs with FLY_API_TOKEN in its environment\n"));
            t.push_str(if get(CHANT_GITHUB) == "true" {
                "does: the repo's chant op runs with GITHUB_TOKEN in its environment too\n"
            } else {
                "does: the repo's chant op runs without GITHUB_TOKEN (only the driver's own git fetch and push use it)\n"
            });
            let gates = match s.get(CHANT_GATES) {
                Some(g) => format!(
                    "only gate{} {}; any other gate stops the deploy",
                    if g.contains(',') { "s" } else { "" },
                    g.replace(',', ", ")
                ),
                None => "each gate of the op it stops at".into(),
            };
            t.push_str(&format!(
                "does: approve {gates} with chant approve, as the approver (relayed by arugula, noting this \
                 approval), then chant run {op} again\n"
            ));
            let digest = if get(CHANT_DIGEST_IS) == "image" {
                "its digest the digest of the image Fly runs after"
            } else {
                "and Fly running another image than before"
            };
            t.push_str(&format!(
                "does: link chant's new release record for this commit in ledger environment {env} \
                 (chant/lifecycle, as origin has it), {digest}\n"
            ));
        }
        Some(r) => {
            let to = |k: &str| r.to.get(k).map_or("", String::as_str);
            t.push_str(&format!(
                "does: flyctl deploy --app {app} --image {}{config} (Fly's own roll back to the image {} ran)\n",
                to(IMAGE),
                r.deploy
            ));
            t.push_str(&format!(
                "does: in {dir}, {npm}, then chant components release {} --component {} --digest {} --git-sha \
                 {source_ref}, as the approver, without the tokens: chant's ledger says it runs again, and the run \
                 links that entry\n",
                to(CHANT_ENV),
                to(CHANT_COMPONENT),
                to(CHANT_DIGEST)
            ));
        }
    }
    t.push_str(&format!(
        "does: check Fly app {app} is healthy (flyctl status: every started machine's checks passing, on one image), \
         and keep the image it runs\n"
    ));
    t.push_str(&format!(
        "with: the environment's FLY_API_TOKEN (fly tokens create deploy -a {app} -x 720h), and GITHUB_TOKEN: the \
         environment's own, or else one the platform's GitHub App mints for this run that writes {source_where}'s \
         contents only, revoked when the run ends; each for this run only\n"
    ));
    t
}

/// A setting a driver takes, as a page asks for it when an environment is
/// made (M252): its name, whether it must be given, the default
/// [`settings`] fills in, the only values it takes (when it's a choice),
/// and what it is, in words.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct SettingField {
    pub name: &'static str,
    pub required: bool,
    pub default: Option<&'static str>,
    pub choices: &'static [&'static str],
    pub about: &'static str,
}

const fn field(
    name: &'static str,
    required: bool,
    default: Option<&'static str>,
    choices: &'static [&'static str],
    about: &'static str,
) -> SettingField {
    SettingField { name, required, default, choices, about }
}

const CHANT_FLY_FIELDS: &[SettingField] = &[
    field(FLY_APP, true, None, &[], "The Fly app it deploys."),
    field(CHANT_ENV, true, None, &[], "The environment chant's release ledger records its releases under."),
    field(CHANT_DIR, false, Some("."), &[], "The chant member, from the repository's root."),
    field(CHANT_OP, false, Some("release"), &[], "The chant op it runs."),
    field(FLY_CONFIG, false, None, &[], "The fly.toml a roll back deploys with."),
    field(CHANT_GATES, false, None, &[], "The only gates it approves, comma separated: any other stops the deploy."),
    field(NPM_SCRIPTS, false, None, &["true"], "npm ci runs the packages' install scripts."),
    field(CHANT_GITHUB, false, None, &["true"], "chant's op gets GITHUB_TOKEN too (an op that pushes)."),
    field(
        CHANT_DIGEST_IS,
        false,
        Some("plan"),
        &["plan", "image"],
        "What the release record's digest is: the plan's, or the image Fly runs after.",
    ),
];

const ARUGULA_RELEASE_FIELDS: &[SettingField] = &[
    field(arugula_release::BUMP, false, Some("patch"), &["patch", "minor", "major"], "The version it bumps."),
    field(
        arugula_release::LANE,
        false,
        Some("daemon"),
        &["daemon", "app"],
        "daemon (a v* tag) or app (an app-v* tag).",
    ),
];

/// The settings `driver` takes, in the order a page asks for them (M252).
/// A driver without settings takes none.
pub fn setting_fields(driver: &str) -> &'static [SettingField] {
    match driver {
        CHANT_FLY => CHANT_FLY_FIELDS,
        arugula_release::DRIVER => ARUGULA_RELEASE_FIELDS,
        d if planter::is_planter(d) => planter::setting_fields(d),
        _ => &[],
    }
}

/// SHA-256 of [`plan_text`], hex.
pub fn plan_digest(driver: &str, op: &Operation, settings: &Settings, roll_back: Option<&RollBack>) -> String {
    hex::encode(Sha256::digest(
        plan_text(driver, &op.environment_name, &op.source_kind, &op.source_ref, &op.source_where, settings, roll_back)
            .as_bytes(),
    ))
}

/// Whether the operation's plan is `driver`'s plan for it, with these
/// settings and this roll back (a box's check before it applies).
pub fn planned(driver: &str, op: &Operation, settings: &Settings, roll_back: Option<&RollBack>) -> bool {
    op.plan == plan_digest(driver, op, settings, roll_back)
}

#[cfg(test)]
mod tests {
    use super::*;

    const SHA: &str = "0123456789abcdef0123456789abcdef01234567";
    const BUMP_: &str = arugula_release::BUMP;
    const LANE_: &str = arugula_release::LANE;

    fn op(plan: String) -> Operation {
        Operation {
            project: "prj_1".into(),
            environment: "env_1".into(),
            environment_name: "production".into(),
            plan,
            source_kind: "commit".into(),
            source_ref: SHA.into(),
            source_where: "acme/app".into(),
            planned: "h".into(),
        }
    }

    fn s(kv: &[(&str, &str)]) -> Settings {
        kv.iter().map(|(k, v)| ((*k).into(), (*v).into())).collect()
    }

    fn image(n: char) -> String {
        format!("registry.fly.io/acme-web:deployment-1@sha256:{}", n.to_string().repeat(64))
    }

    /// What a page asks for is what [`settings`] takes (M252): every
    /// setting, once; the required ones alone are enough; each default is
    /// the one filled in; each choice is taken.
    #[test]
    fn setting_fields_are_the_settings_a_driver_takes() {
        assert!(setting_fields("fake").is_empty() && setting_fields("apps-platform").is_empty());
        for (driver, taken) in [(CHANT_FLY, CHANT_FLY_SETTINGS), (arugula_release::DRIVER, &[BUMP_, LANE_][..])] {
            let fields = setting_fields(driver);
            let mut names: Vec<&str> = fields.iter().map(|f| f.name).collect();
            names.sort_unstable();
            let mut want = taken.to_vec();
            want.sort_unstable();
            assert_eq!(names, want, "{driver}");
            let given: Settings = fields
                .iter()
                .filter(|f| f.required)
                .map(|f| (f.name.to_owned(), if f.name == CHANT_ENV { "github" } else { "acme-web" }.to_owned()))
                .collect();
            let filled = settings(driver, &given).unwrap();
            for f in fields {
                assert_eq!(
                    filled.get(f.name).map(String::as_str),
                    f.default.or(given.get(f.name).map(String::as_str)),
                    "{driver} {}",
                    f.name
                );
                for c in f.choices {
                    let mut with = given.clone();
                    with.insert(f.name.to_owned(), (*c).to_owned());
                    assert!(settings(driver, &with).is_ok(), "{driver} {}={c}", f.name);
                }
            }
        }
    }

    #[test]
    fn a_driver_without_settings_plans_as_before() {
        let t = plan_text("fake", "production", "commit", "abc", "acme/app", &Settings::new(), None);
        assert_eq!(t, crate::deploy::plan_text("fake", "production", "commit", "abc", "acme/app"));
        assert!(settings("fake", &Settings::new()).unwrap().is_empty());
        assert!(settings("fake", &[("x".to_owned(), "y".to_owned())].into()).is_err());
        assert!(source_ok("fake", "commit", "abc"));
    }

    #[test]
    fn chant_fly_settings_are_checked_and_filled_in() {
        assert!(settings(CHANT_FLY, &s(&[])).unwrap_err().contains("fly_app"));
        assert!(settings(CHANT_FLY, &s(&[("fly_app", "acme-web")])).unwrap_err().contains("chant_env"));
        assert_eq!(
            settings(CHANT_FLY, &s(&[("fly_app", "acme-web"), ("chant_env", "github")])).unwrap(),
            s(&[
                ("fly_app", "acme-web"),
                ("chant_env", "github"),
                ("chant_dir", "."),
                ("chant_op", "release"),
                ("chant_digest", "plan")
            ])
        );
        let ok = [("fly_app", "a"), ("chant_env", "github")];
        let with = |more: &[(&str, &str)]| s(&[ok.as_slice(), more].concat());
        assert!(
            settings(CHANT_FLY, &with(&[("chant_gates", "approve-release,ship"), ("npm_scripts", "true")])).is_ok()
        );
        for bad in [
            s(&[("fly_app", "Acme"), ("chant_env", "github")]),
            with(&[("chant_dir", "../x")]),
            with(&[("chant_dir", "/etc")]),
            with(&[("chant_dir", "-x")]),
            with(&[("fly_config", "a b")]),
            with(&[("chant_op", "re;lease")]),
            with(&[("chant_op", "-release")]),
            with(&[("token", "x")]),
            with(&[("chant_gates", "a,,b")]),
            with(&[("npm_scripts", "yes")]),
            with(&[("chant_digest", "both")]),
            s(&[("fly_app", "a"), ("chant_env", "_x")]),
        ] {
            assert!(settings(CHANT_FLY, &bad).is_err(), "{bad:?}");
        }
    }

    #[test]
    fn chant_fly_deploys_a_full_commit_id_only() {
        assert!(source_ok(CHANT_FLY, "commit", SHA));
        for (kind, r) in
            [("commit", "main"), ("commit", &SHA[..12]), ("commit", &SHA.to_uppercase()), ("artifact", SHA)]
        {
            assert!(!source_ok(CHANT_FLY, kind, r), "{kind} {r}");
        }
    }

    #[test]
    fn evidence_is_checked_by_shape() {
        assert!(fly_image_ok("acme-web", &image('a')));
        assert!(fly_image_ok("acme-web", &format!("registry.fly.io/acme-web@sha256:{}", "b".repeat(64))));
        for bad in [
            image('a').replace("acme-web", "acme-webx"),
            image('a').replace("registry.fly.io", "evil.io"),
            image('a').replace("@sha256:", ":"),
            format!("{}0", image('a')),
            image('a').replace(":deployment-1", ":a b"),
        ] {
            assert!(!fly_image_ok("acme-web", &bad), "{bad}");
        }
        let link = format!("https://github.com/acme/app/blob/{SHA}/github/releases.jsonl#L3");
        assert!(ledger_link_ok("acme/app", &link));
        for bad in [
            link.replace("https://", "javascript://"),
            link.replace("acme/app", "acme/other"),
            link.replace(SHA, "main"),
            link.replace("github/releases", "../releases"),
            link.replace("#L3", "#L0"),
            link.replace("#L3", "#Lx"),
            link.replace(".jsonl", ".html"),
            link.replace("github/", "git hub/"),
            format!("{link}\"><script>"),
        ] {
            assert!(!ledger_link_ok("acme/app", &bad), "{bad}");
        }
    }

    /// The plan binds every setting and the roll back: change one, and the
    /// operation's plan isn't the driver's any more.
    #[test]
    fn the_plan_binds_settings_and_the_roll_back() {
        let s =
            settings(CHANT_FLY, &s(&[("fly_app", "acme-web"), ("chant_dir", "ops"), ("chant_env", "github")])).unwrap();
        let digest = plan_digest(CHANT_FLY, &op(String::new()), &s, None);
        let o = op(digest);
        assert!(planned(CHANT_FLY, &o, &s, None));
        assert!(!planned("fake", &o, &Settings::new(), None), "another driver");
        let mut other = s.clone();
        other.insert(FLY_APP.into(), "acme-other".into());
        assert!(!planned(CHANT_FLY, &o, &other, None), "another app");
        let mut scripts = s.clone();
        scripts.insert(NPM_SCRIPTS.into(), "true".into());
        assert!(!planned(CHANT_FLY, &o, &scripts, None), "install scripts are in the plan");
        let r = RollBack {
            deploy: format!("dep_{}", "a".repeat(32)),
            to: [
                (IMAGE, image('0')),
                (CHANT_COMPONENT, "arugula".into()),
                (CHANT_ENV, "github".into()),
                (CHANT_DIGEST, "sha256:11".into()),
            ]
            .into_iter()
            .map(|(k, v)| (k.to_owned(), v))
            .collect(),
        };
        assert!(roll_back_ok(CHANT_FLY, &s, &r) && !roll_back_ok("fake", &Settings::new(), &r));
        let mut elsewhere = r.clone();
        elsewhere.to.insert(IMAGE.into(), image('0').replace("acme-web", "acme-other"));
        assert!(!roll_back_ok(CHANT_FLY, &s, &elsewhere), "another app's image");
        assert!(!planned(CHANT_FLY, &o, &s, Some(&r)), "a roll back is another plan");
        let text = plan_text(CHANT_FLY, "production", "commit", SHA, "acme/app", &s, Some(&r));
        assert!(text.contains(&format!("flyctl deploy --app acme-web --image {}", image('0'))), "{text}");
        assert!(text.contains("chant components release github --component arugula --digest sha256:11"), "{text}");
        let apply = plan_text(CHANT_FLY, "production", "commit", SHA, "acme/app", &s, None);
        assert!(apply.starts_with("arugula deploy plan v1\ndriver chant-fly\n"), "{apply}");
        for says in [
            "in ops, npm ci --ignore-scripts (without the tokens), then chant run release",
            "the repo's chant op release runs with FLY_API_TOKEN in its environment",
            "runs without GITHUB_TOKEN",
            "a Fly deploy token for one app that expires within 30 days",
            "fly tokens create deploy -a acme-web -x 720h",
            "in ledger environment github",
        ] {
            assert!(apply.contains(says), "{says:?} in {apply}");
        }
    }
}
