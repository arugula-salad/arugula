//! illogical is now Arugula (#509). Things made under the old name keep
//! working (#505): installs, panes started before the update, unit files,
//! hooks, and daemons or clients still on illogical 0.25 (#504, the bridge,
//! which accepts both names) or older (which know only the old ones).
//!
//! - **Headers:** read under either name ([`either`]). Send both where the
//!   other end may be older than 0.25 ([`AGENT`], [`PANE`],
//!   [`CLAUDE_CONFIG_DIR`], [`SIZE`]); control is always current, so
//!   [`AUTH`] goes under the new name only.
//! - **Environment variables:** `ILLOGICAL_X` stands in for `ARUGULA_X`
//!   when only the old name is set ([`alias_env`]): a pane started before
//!   the update, or a unit file written by an older install.
//!
//! **Never renamed**, because something already stored or running depends
//! on the exact bytes. Each is marked "Frozen (#504)" where it's defined:
//!
//! - the domains signed or hashed into device certificates, join and team
//!   proofs, push and call tokens (`illogical device v1`, `illogical team
//!   invite v1`, …), the Noise prologue `illogical/1`, the sync key's HKDF
//!   info `illogical sync v1`, the key file header `illogical-device-key 1`
//!   and the checkpoint magic `ILLOGICAL-CKPT1`;
//! - the browser's IndexedDB `illogical-device`, which holds its device key;
//! - the names a running pane is found by after a daemon restart: the
//!   holder socket `illogical-hold-*`, Windows' pane pipe
//!   `illogical-<user>-pane-*`, and the systemd scopes `illogical-pane-*`
//!   and `illogical-agent-*`;
//! - the sandbox prefix `illogical-eph-` and the resident service name,
//!   which daemons of different versions on one account read from each
//!   other's sandboxes.

/// The old name of the product, for what was made under it.
pub const OLD: &str = "illogical";

/// `X-Arugula-Agent`: the request comes from an agent on the owner's CLI,
/// so it gets less than the owner would. Either name counts, and the CLI
/// sends both: a daemon older than 0.25 knows only the old one, and must
/// not take an agent for the owner.
pub const AGENT: [&str; 2] = ["x-arugula-agent", "x-illogical-agent"];
/// The pane an MCP client runs in.
pub const PANE: [&str; 2] = ["x-arugula-pane", "x-illogical-pane"];
/// The Claude Code config directory of the agent calling MCP.
pub const CLAUDE_CONFIG_DIR: [&str; 2] = ["arugula-claude-config-dir", "illogical-claude-config-dir"];
/// A daemon's or CLI's signed request to control.
pub const AUTH: [&str; 2] = ["x-arugula-auth", "x-illogical-auth"];
/// A file's size (`/api/fs/read`).
pub const SIZE: [&str; 2] = ["x-arugula-size", "x-illogical-size"];
/// The offset of a ranged read (`/api/fs/read`).
pub const OFFSET: [&str; 2] = ["x-arugula-offset", "x-illogical-offset"];

/// The first of `names` that `get` finds: `either(AGENT, |n| headers.get(n))`.
pub fn either<T>(names: [&str; 2], get: impl FnMut(&str) -> Option<T>) -> Option<T> {
    names.into_iter().find_map(get)
}

/// For each `ILLOGICAL_X` whose `ARUGULA_X` isn't set: the new name and the
/// value, so the rest of the program (and its children) see it there.
pub fn env_aliases(
    vars: impl IntoIterator<Item = (std::ffi::OsString, std::ffi::OsString)>,
) -> Vec<(String, std::ffi::OsString)> {
    let vars: Vec<_> = vars.into_iter().collect();
    let set = |k: &str| vars.iter().any(|(n, _)| n.to_str() == Some(k));
    vars.iter()
        .filter_map(|(k, v)| {
            let new = format!("ARUGULA_{}", k.to_str()?.strip_prefix("ILLOGICAL_")?);
            (!set(&new)).then(|| (new, v.clone()))
        })
        .collect()
}

/// Set `ARUGULA_X` from `ILLOGICAL_X` where only the old name is set.
///
/// # Safety
///
/// It changes the environment: call it first thing in `main`, before any
/// other thread exists.
pub unsafe fn alias_env() {
    for (k, v) in env_aliases(std::env::vars_os()) {
        // SAFETY: the caller promises there's only this thread.
        unsafe { std::env::set_var(k, v) };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn vars(v: &[(&str, &str)]) -> Vec<(std::ffi::OsString, std::ffi::OsString)> {
        v.iter().map(|(k, v)| (k.into(), v.into())).collect()
    }

    #[test]
    fn old_env_names_stand_in_for_new_ones() {
        let got = env_aliases(vars(&[
            ("ILLOGICAL_SOCK", "/old"),
            ("ILLOGICAL_PANE", "%3"),
            ("ARUGULA_PANE", "%1"),
            ("ILLOGICAL", "x"),
            ("PATH", "/bin"),
        ]));
        // The new name wins when both are set; a bare ILLOGICAL isn't ours.
        assert_eq!(got, vec![("ARUGULA_SOCK".to_string(), "/old".into())]);
    }

    #[test]
    fn either_takes_the_first_name_found() {
        let headers = [("x-illogical-agent", "1")];
        let get = |n: &str| headers.iter().find(|(k, _)| *k == n).map(|(_, v)| *v);
        assert_eq!(either(AGENT, get), Some("1"));
        assert_eq!(either(PANE, get), None);
        let both = [("x-illogical-pane", "1"), ("x-arugula-pane", "2")];
        assert_eq!(either(PANE, |n| both.iter().find(|(k, _)| *k == n).map(|(_, v)| *v)), Some("2"));
    }
}
