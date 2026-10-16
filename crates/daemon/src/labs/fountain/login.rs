//! The person's Managoat login (M43), as their `managoat` CLI has it.
//!
//! - **The key** is the `api_key` of the profile picked on the block, if it
//!   has one; else `$MANAGOAT_API_KEY`; else the `api_key` of a profile in
//!   `~/.managoat/credentials` (TOML: `[profile]` tables of `api_key` and
//!   `base_url`). Which profile: the block's, else `$MANAGOAT_PROFILE`,
//!   else `default`.
//! - **The base URL** is `$MANAGOAT_BASE_URL`, else the profile's
//!   `base_url`, else hosted Managoat's (for a profile picked on the block,
//!   its own `base_url` comes first).
//! - **The names before the rename** (Managoat was Fountain; DECISIONS.md
//!   "Fountain is Managoat"), as the CLI reads them: each `FOUNTAIN_*`
//!   variable when its `MANAGOAT_*` one is unset or empty, and
//!   `~/.fountain/credentials` when there's no `~/.managoat/credentials`.
//!   The daemon's log says so once per name ([`deprecated`]). They go when
//!   Managoat drops them (managoat/managoat#2776).
//! - Both are read on the block's host with the user's shell environment
//!   (#74, [`Runner::user`]), as M36 reads `tea`'s logins, and the key is
//!   held in memory only: never logged, saved, or sent to a client.
//! - `ARUGULA_MANAGOAT_CREDENTIALS` (or `ARUGULA_FOUNTAIN_CREDENTIALS`)
//!   names another credentials file, for tests (the e2e daemon runs with
//!   the real HOME).

use std::collections::BTreeMap;

use crate::review::Runner;

/// Hosted Managoat, the CLI's default.
pub const DEFAULT_BASE: &str = "https://managoat.com";

/// Where the key came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum From {
    Env,
    File,
    /// This daemon's own sign-in ([`super::account`]).
    Arugula,
}

/// A login: the account's base URL and key.
#[derive(Clone, PartialEq, Eq)]
pub struct Login {
    pub profile: String,
    pub base_url: String,
    pub key: String,
    pub from: From,
}

impl std::fmt::Debug for Login {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Login")
            .field("profile", &self.profile)
            .field("base_url", &self.base_url)
            .field("from", &self.from)
            .finish_non_exhaustive()
    }
}

/// What the host said: the variables, and the credentials file's text.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Found {
    pub env_key: String,
    pub env_base: String,
    pub env_profile: String,
    pub file: Option<String>,
    /// Which credentials file, as said to a person (`~/.managoat/credentials`).
    pub file_name: String,
    /// The names before the rename it was read by: `FOUNTAIN_*` variables,
    /// `~/.fountain/credentials`.
    pub old: Vec<String>,
}

/// The credentials file, as said to a person.
pub const CREDENTIALS: &str = "~/.managoat/credentials";
/// Its path before the rename, read when there's no new one.
pub const OLD_CREDENTIALS: &str = "~/.fountain/credentials";

impl Found {
    fn file_name(&self) -> &str {
        if self.file_name.is_empty() { CREDENTIALS } else { &self.file_name }
    }
}

/// Says once in the daemon's log that a name from before the rename was
/// read, and what to use instead.
pub fn deprecated(old: &[String]) {
    static SAID: std::sync::Mutex<Vec<String>> = std::sync::Mutex::new(Vec::new());
    let mut said = SAID.lock().unwrap();
    for name in old {
        if said.contains(name) {
            continue;
        }
        said.push(name.clone());
        let now = match name.strip_prefix("FOUNTAIN_") {
            Some(rest) => format!("MANAGOAT_{rest}"),
            None => CREDENTIALS.to_owned(),
        };
        tracing::warn!(old = %name, new = %now, "the Managoat login is read from {name}, its name before Fountain became Managoat: use {now}; Managoat stops reading it in a later release");
    }
}

/// A credentials file's profiles: `[name]` → its keys (quotes taken off).
pub fn parse_credentials(text: &str) -> BTreeMap<String, BTreeMap<String, String>> {
    let mut out: BTreeMap<String, BTreeMap<String, String>> = BTreeMap::new();
    let mut at: Option<String> = None;
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if let Some(name) = line.strip_prefix('[').and_then(|l| l.strip_suffix(']')) {
            let name = unquote(name.trim()).to_owned();
            out.entry(name.clone()).or_default();
            at = Some(name);
            continue;
        }
        let (Some(p), Some((k, v))) = (&at, line.split_once('=')) else { continue };
        let v = v.trim();
        // A trailing comment after a bare value.
        let v = if v.starts_with('"') || v.starts_with('\'') { v } else { v.split(" #").next().unwrap_or(v) };
        out.entry(p.clone()).or_default().insert(k.trim().to_owned(), unquote(v.trim()).to_owned());
    }
    out
}

fn unquote(s: &str) -> &str {
    for q in ['"', '\''] {
        if let Some(inner) = s.strip_prefix(q).and_then(|s| s.strip_suffix(q)) {
            return inner;
        }
    }
    s
}

/// The profiles a host has, for the block to offer.
pub fn profiles(found: &Found) -> Vec<String> {
    found.file.as_deref().map(parse_credentials).unwrap_or_default().into_keys().collect()
}

/// The login, per the rules above. `want`: the block's profile.
pub fn resolve(found: &Found, want: Option<&str>) -> Result<Login, String> {
    let profile = want
        .filter(|p| !p.is_empty())
        .or(Some(found.env_profile.as_str()).filter(|p| !p.is_empty()))
        .unwrap_or("default")
        .to_owned();
    let all = found.file.as_deref().map(parse_credentials).unwrap_or_default();
    let section = all.get(&profile);
    let base = Some(found.env_base.trim())
        .filter(|b| !b.is_empty())
        .or(section.and_then(|s| s.get("base_url")).map(|b| b.trim()).filter(|b| !b.is_empty()))
        .unwrap_or(DEFAULT_BASE)
        .trim_end_matches('/')
        .to_owned();
    let file_key = section.and_then(|s| s.get("api_key")).map(|k| k.trim()).filter(|k| !k.is_empty());
    // A profile picked on the block, with a key of its own, is that
    // account: its key and base URL, whatever the environment says.
    if want.is_some_and(|w| !w.is_empty())
        && let Some(key) = file_key
    {
        let base = section
            .and_then(|s| s.get("base_url"))
            .map(|b| b.trim())
            .filter(|b| !b.is_empty())
            .or(Some(found.env_base.trim()).filter(|b| !b.is_empty()))
            .unwrap_or(DEFAULT_BASE)
            .trim_end_matches('/')
            .to_owned();
        return Ok(Login { profile, base_url: base, key: key.to_owned(), from: From::File });
    }
    if !found.env_key.trim().is_empty() {
        return Ok(Login { profile, base_url: base, key: found.env_key.trim().to_owned(), from: From::Env });
    }
    if found.file.is_none() {
        return Err(
            "no Managoat login here: `managoat auth login` (or set MANAGOAT_API_KEY) on this host, then refresh".into(),
        );
    }
    let file = found.file_name();
    let Some(section) = section else {
        let have: Vec<&str> = all.keys().map(String::as_str).collect();
        return Err(if have.is_empty() {
            format!("{file} has no profiles: `managoat auth login`, then refresh")
        } else {
            format!("no profile {profile:?} in {file} (it has {}): pick one", have.join(", "))
        });
    };
    let key =
        section.get("api_key").map(|k| k.trim()).filter(|k| !k.is_empty()).ok_or_else(|| {
            format!("profile {profile:?} in {file} has no api_key: `managoat auth login`, then refresh")
        })?;
    Ok(Login { profile, base_url: base, key: key.to_owned(), from: From::File })
}

/// Prints the variables, new names and old, then which file it reads and
/// the file (if any) after a marker line: `~/.managoat/credentials`, else
/// `~/.fountain/credentials`.
const READ: &str = r#"printf 'MANAGOAT_API_KEY=%s\n' "${MANAGOAT_API_KEY:-}"
printf 'MANAGOAT_BASE_URL=%s\n' "${MANAGOAT_BASE_URL:-}"
printf 'MANAGOAT_PROFILE=%s\n' "${MANAGOAT_PROFILE:-}"
printf 'FOUNTAIN_API_KEY=%s\n' "${FOUNTAIN_API_KEY:-}"
printf 'FOUNTAIN_BASE_URL=%s\n' "${FOUNTAIN_BASE_URL:-}"
printf 'FOUNTAIN_PROFILE=%s\n' "${FOUNTAIN_PROFILE:-}"
f="${ARUGULA_MANAGOAT_CREDENTIALS:-${ARUGULA_FOUNTAIN_CREDENTIALS:-}}"
if [ -z "$f" ]; then
  f="${HOME:-.}/.managoat/credentials"
  [ -r "$f" ] || f="${HOME:-.}/.fountain/credentials"
fi
[ -r "$f" ] || exit 0
printf 'arugula-credentials-file=%s\n' "$f"
echo arugula-credentials
cat "$f""#;

/// Parse what [`READ`] printed.
pub fn parse_found(out: &[u8]) -> Found {
    let text = String::from_utf8_lossy(out);
    let mut found = Found::default();
    let mut vars: BTreeMap<String, String> = BTreeMap::new();
    let mut lines = text.split_inclusive('\n');
    for line in lines.by_ref() {
        let l = line.trim_end_matches(['\n', '\r']);
        if l == "arugula-credentials" {
            found.file = Some(String::new());
            break;
        }
        let Some((k, v)) = l.split_once('=') else { continue };
        if k == "arugula-credentials-file" {
            found.file_name = said_path(v);
        } else {
            vars.insert(k.to_owned(), v.to_owned());
        }
    }
    // Each `MANAGOAT_*`, else its `FOUNTAIN_*` (the name before the rename).
    for (name, into) in
        [("API_KEY", &mut found.env_key), ("BASE_URL", &mut found.env_base), ("PROFILE", &mut found.env_profile)]
    {
        let now = vars.get(&format!("MANAGOAT_{name}")).filter(|v| !v.is_empty());
        let old = vars.get(&format!("FOUNTAIN_{name}")).filter(|v| !v.is_empty());
        match (now, old) {
            (Some(v), _) => *into = v.clone(),
            (None, Some(v)) => {
                *into = v.clone();
                found.old.push(format!("FOUNTAIN_{name}"));
            }
            (None, None) => {}
        }
    }
    if found.file_name == OLD_CREDENTIALS {
        found.old.push(OLD_CREDENTIALS.to_owned());
    }
    if let Some(f) = found.file.as_mut() {
        f.extend(lines);
    }
    found
}

/// A credentials file's path as said to a person: under `~` for the CLI's.
fn said_path(path: &str) -> String {
    for (end, said) in [("/.managoat/credentials", CREDENTIALS), ("/.fountain/credentials", OLD_CREDENTIALS)] {
        if path.ends_with(end) {
            return said.to_owned();
        }
    }
    path.to_owned()
}

/// Read the variables and the file on the runner's host.
pub(crate) async fn read(runner: &Runner) -> Result<Found, String> {
    if crate::labs::DIRECT_MANAGOAT_RETIRED {
        return Err(crate::agent::defs::CLOUD_MIGRATION.into());
    }
    let (out, _) = runner.sh(READ, &[]).await?;
    let found = parse_found(&out);
    deprecated(&found.old);
    Ok(found)
}

/// A Managoat login the catalog would find (this module's order): a
/// key in the daemon's or the shell's environment, or the CLI's
/// credentials file.
pub fn here(shell_env: &crate::shellenv::ShellEnv) -> bool {
    if crate::labs::DIRECT_MANAGOAT_RETIRED {
        return false;
    }
    let shell = shell_env.local_now();
    let var = |k: &str| {
        shell
            .as_ref()
            .and_then(|r| r.get(k).map(str::to_owned))
            .or_else(|| std::env::var(k).ok())
            .filter(|v| !v.is_empty())
    };
    if var("MANAGOAT_API_KEY").is_some() {
        return true;
    }
    if var("FOUNTAIN_API_KEY").is_some() {
        deprecated(&["FOUNTAIN_API_KEY".into()]);
        return true;
    }
    if let Some(f) = var("ARUGULA_MANAGOAT_CREDENTIALS").or_else(|| var("ARUGULA_FOUNTAIN_CREDENTIALS")) {
        return std::path::Path::new(&f).is_file();
    }
    let Some(home) = std::env::var_os("HOME").map(std::path::PathBuf::from) else { return false };
    if home.join(".managoat/credentials").is_file() {
        return true;
    }
    let old = home.join(".fountain/credentials").is_file();
    if old {
        deprecated(&[OLD_CREDENTIALS.into()]);
    }
    old
}

#[cfg(test)]
mod tests {
    use super::*;

    const FILE: &str = "[default]\napi_key = \"ftn_test_default\"\nbase_url = \"https://fountain.example\"\n\n# another\n[selfhost]\napi_key = 'ftn_test_self'\nbase_url = http://localhost:4000 # mine\n\n[empty]\n";

    fn found(file: Option<&str>) -> Found {
        Found { file: file.map(str::to_owned), ..Found::default() }
    }

    #[test]
    fn credentials_as_the_cli_writes_them() {
        let p = parse_credentials(FILE);
        assert_eq!(p.keys().collect::<Vec<_>>(), ["default", "empty", "selfhost"]);
        assert_eq!(p["default"]["api_key"], "ftn_test_default");
        assert_eq!(p["selfhost"]["base_url"], "http://localhost:4000");
        assert!(p["empty"].is_empty());
    }

    #[test]
    fn which_key_and_base() {
        let l = resolve(&found(Some(FILE)), None).unwrap();
        assert_eq!(
            (l.profile.as_str(), l.base_url.as_str(), l.key.as_str()),
            ("default", "https://fountain.example", "ftn_test_default")
        );
        assert_eq!(l.from, From::File);
        let l = resolve(&found(Some(FILE)), Some("selfhost")).unwrap();
        assert_eq!(l.base_url, "http://localhost:4000");
        // MANAGOAT_PROFILE picks one when the block doesn't.
        let f = Found { env_profile: "selfhost".into(), ..found(Some(FILE)) };
        assert_eq!(resolve(&f, None).unwrap().key, "ftn_test_self");
        assert_eq!(resolve(&f, Some("default")).unwrap().key, "ftn_test_default");
        // The variables win; the base falls back to the profile's, then hosted.
        let f = Found { env_key: "ftn_test_env".into(), ..found(Some(FILE)) };
        let l = resolve(&f, None).unwrap();
        assert_eq!(
            (l.key.as_str(), l.base_url.as_str(), l.from),
            ("ftn_test_env", "https://fountain.example", From::Env)
        );
        // ...unless the block picked a profile with a key of its own.
        let f = Found { env_key: "ftn_test_env".into(), env_base: "http://127.0.0.1:9".into(), ..found(Some(FILE)) };
        let l = resolve(&f, Some("selfhost")).unwrap();
        assert_eq!(
            (l.key.as_str(), l.base_url.as_str(), l.from),
            ("ftn_test_self", "http://localhost:4000", From::File)
        );
        // A picked profile with no key: the environment's.
        assert_eq!(resolve(&f, Some("empty")).unwrap().key, "ftn_test_env");
        let f = Found { env_key: "ftn_test_env".into(), env_base: "http://127.0.0.1:9/".into(), ..found(None) };
        assert_eq!(resolve(&f, None).unwrap().base_url, "http://127.0.0.1:9");
        let f = Found { env_key: "k".into(), ..found(None) };
        assert_eq!(resolve(&f, None).unwrap().base_url, DEFAULT_BASE);
    }

    #[test]
    fn what_it_says_without_one() {
        assert!(resolve(&found(None), None).unwrap_err().starts_with("no Managoat login here"));
        let e = resolve(&found(Some(FILE)), Some("nope")).unwrap_err();
        assert!(e.contains("no profile \"nope\"") && e.contains("default, empty, selfhost"), "{e}");
        assert!(resolve(&found(Some(FILE)), Some("empty")).unwrap_err().contains("has no api_key"));
        assert!(resolve(&found(Some("")), None).unwrap_err().contains("no profiles"));
    }

    #[test]
    fn what_the_host_printed() {
        let out =
            format!("FOUNTAIN_API_KEY=\nFOUNTAIN_BASE_URL=http://x\nFOUNTAIN_PROFILE=\narugula-credentials\n{FILE}");
        let f = parse_found(out.as_bytes());
        assert_eq!(f.env_base, "http://x");
        assert_eq!(f.file.as_deref(), Some(FILE));
        assert_eq!(profiles(&f), ["default", "empty", "selfhost"]);
        let f = parse_found(b"FOUNTAIN_API_KEY=k\nFOUNTAIN_BASE_URL=\nFOUNTAIN_PROFILE=p\n");
        assert_eq!((f.env_key.as_str(), f.env_profile.as_str(), f.file), ("k", "p", None));
    }

    /// The rename (DECISIONS.md "Fountain is Managoat"): each `MANAGOAT_*`
    /// first, its `FOUNTAIN_*` when it's unset or empty, and which old
    /// names were read, for the log to say once.
    #[test]
    fn the_new_names_first_then_the_old() {
        let f = parse_found(
            b"MANAGOAT_API_KEY=new\nMANAGOAT_BASE_URL=\nMANAGOAT_PROFILE=\nFOUNTAIN_API_KEY=old\nFOUNTAIN_BASE_URL=http://old\nFOUNTAIN_PROFILE=\n",
        );
        assert_eq!((f.env_key.as_str(), f.env_base.as_str()), ("new", "http://old"));
        assert_eq!(f.old, ["FOUNTAIN_BASE_URL"]);
        let f = parse_found(
            format!("MANAGOAT_API_KEY=\narugula-credentials-file=/home/me/.fountain/credentials\narugula-credentials\n{FILE}")
                .as_bytes(),
        );
        assert_eq!(
            (f.file_name.as_str(), f.old.as_slice()),
            (OLD_CREDENTIALS, [OLD_CREDENTIALS.to_owned()].as_slice())
        );
        assert!(resolve(&f, Some("nope")).unwrap_err().contains("~/.fountain/credentials"));
        let f = parse_found(
            format!("arugula-credentials-file=/home/me/.managoat/credentials\narugula-credentials\n{FILE}").as_bytes(),
        );
        assert_eq!((f.file_name.as_str(), f.old.len()), (CREDENTIALS, 0));
        assert_eq!(resolve(&f, None).unwrap().key, "ftn_test_default");
    }
}
