//! illogical is now Arugula (#509). Every install is on Arugula 0.26 or
//! later, so names only cross between versions under Arugula's (#534).
//!
//! What was made under the old name is moved to the new one, with the old
//! name left as a link to it (`dirs::renamed`): the state, config, data and
//! cache directories. What a running pane or agent was started with is
//! still found under illogical's name until it ends: its holder socket in
//! the temp dir, its systemd scope, its sandbox (`illogical-eph-`), a
//! resident daemon's service, a VM agent's MCP relay socket and its block
//! token. New ones get Arugula's.
//!
//! **Never renamed**, because something already stored depends on the
//! exact bytes. Each is marked "Frozen (#504)" where it's defined:
//!
//! - the domains signed or hashed into device certificates, join and team
//!   proofs, push and call tokens (`illogical device v1`, `illogical team
//!   invite v1`, …), the Noise prologue `illogical/1`, the sync key's HKDF
//!   info `illogical sync v1`, the key file header `illogical-device-key 1`
//!   and the checkpoint magic `ILLOGICAL-CKPT1`;
//! - the browser's IndexedDB `illogical-device`, which holds its device key.

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
