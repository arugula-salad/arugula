//! illogical is now Arugula (#509). Every install is on Arugula 0.26 or
//! later, so names only cross between versions under Arugula's (#534).
//! What was made under the old name and stays there: the state, config,
//! data and cache directories (used where they are, [`kept`]).
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

/// `old` where only it exists, else `new`: a file or directory made under
/// the old name stays where it is, and anything new gets the new name.
pub fn kept(new: std::path::PathBuf, old: std::path::PathBuf) -> std::path::PathBuf {
    if !new.exists() && old.exists() { old } else { new }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kept_is_the_old_path_only_while_it_alone_exists() {
        let d = std::env::temp_dir().join(format!("arugula-kept-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        let (new, old) = (d.join("arugula"), d.join("illogical"));
        assert_eq!(kept(new.clone(), old.clone()), new, "neither: the new");
        std::fs::create_dir_all(&old).unwrap();
        assert_eq!(kept(new.clone(), old.clone()), old, "only the old");
        std::fs::create_dir_all(&new).unwrap();
        assert_eq!(kept(new.clone(), old.clone()), new, "both: the new");
        let _ = std::fs::remove_dir_all(&d);
    }
}
