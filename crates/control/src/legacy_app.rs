//! The desktop app's windows from before it had a kind of its own,
//! grandfathered (Jake, 2026-10-23).
//!
//! Before #1343 the app's window enrolled as a `browser` device it named
//! ("Arugula app on …", "illogical app" before the rename), and it approved
//! browsers and phones like any browser. #1343 (v0.59.0) took those
//! certificates out of "a browser all the way back to the root"
//! (`Cert::is_browser`), which left accounts whose every browser an app
//! window approved, directly or on the way back, with none that may sign
//! for a passkey, approve a deploy or approve a new browser.
//!
//! So such a certificate counts as a link on the way again
//! (`arugula_e2e::cert::legacy_app_link`) when control stored it before
//! #1343 shipped, by control's own record of when it stored it
//! (`devices.created`, control's clock at the row's first insert, which a
//! later put keeps: [`Db::put_device`]). The certificate's own `created` is
//! the signer's claim, so a stolen key could backdate it; the row's can't
//! be. One stored after never counts, and the window itself never signs
//! what needs a browser: it's only ever a link before the device that
//! signs.

use crate::db::Db;

/// When control v0.59.0, which carries #1343, was deployed: the `control`
/// job of the v0.59.0 release run (arugula-salad/arugula Actions run
/// 39845854046) completed at 2026-10-22T20:18:06Z; rounded down to the
/// minute, 2026-10-22T20:18:00Z. An app window's `browser` certificate
/// control stored before this is grandfathered.
pub const APP_KIND_SHIPPED_MS: u64 = 1_792_700_280_000;

/// `account`'s grandfathered app windows: its approved `browser`
/// certificates the desktop app named ([`arugula_e2e::Cert::from_app`]),
/// whose rows control stored before [`APP_KIND_SHIPPED_MS`], and that no
/// revocation names.
pub fn devices(db: &Db, account: &str) -> anyhow::Result<Vec<String>> {
    let revoked = db.revocations(account)?;
    Ok(db
        .browsers_stored_before(account, APP_KIND_SHIPPED_MS)?
        .into_iter()
        .filter(|c| {
            c.account == account
                && c.kind == arugula_e2e::Kind::Browser
                && c.from_app()
                && !revoked.iter().any(|r| r.device == c.device)
        })
        .map(|c| c.device)
        .collect())
}

#[cfg(test)]
mod tests {
    use arugula_e2e::{Cert, DeviceKeys, Kind, Revocation};

    use super::*;

    #[test]
    fn only_app_windows_stored_before_the_cutoff_approved_and_not_revoked() {
        let db = Db::memory();
        let phone = DeviceKeys::generate();
        let mut root = Cert::new(&phone, "a", Kind::Browser, "Pixel");
        root.sign_with(&phone);
        db.put_device(&root, true, 1).unwrap();
        let cert = |name: &str, kind| {
            let k = DeviceKeys::generate();
            let mut c = Cert::new(&k, "a", kind, name);
            c.sign_with(&phone);
            c
        };
        let before = APP_KIND_SHIPPED_MS - 60_000;
        let old = cert("Arugula app on jake-air", Kind::Browser);
        db.put_device(&old, true, before).unwrap();
        let older = cert("illogical app", Kind::Browser);
        db.put_device(&older, true, 2).unwrap();
        let after = cert("Arugula app on jake-mini", Kind::Browser);
        db.put_device(&after, true, APP_KIND_SHIPPED_MS).unwrap();
        // Its own time says long ago; control stored it after.
        let backdated = Cert { created: 1, ..cert("Arugula app on box", Kind::Browser) };
        db.put_device(&backdated, true, APP_KIND_SHIPPED_MS + 1).unwrap();
        let firefox = cert("Firefox", Kind::Browser);
        db.put_device(&firefox, true, before).unwrap();
        let app = cert("Arugula app on jake-air", Kind::App);
        db.put_device(&app, true, before).unwrap();
        let waiting = cert("Arugula app on geek", Kind::Browser);
        db.put_device(&waiting, false, before).unwrap();
        let gone = cert("Arugula app on old-mac", Kind::Browser);
        db.put_device(&gone, true, before).unwrap();
        db.add_revocation(&Revocation::new("a", &gone.device, &phone)).unwrap();
        let mut got = devices(&db, "a").unwrap();
        got.sort();
        let mut want = vec![old.device.clone(), older.device.clone()];
        want.sort();
        assert_eq!(got, want);

        // Stored again later (asked again, approved): still when first stored.
        db.put_device(&old, true, APP_KIND_SHIPPED_MS + 5).unwrap();
        db.put_device(&after, true, 3).unwrap();
        let mut got = devices(&db, "a").unwrap();
        got.sort();
        assert_eq!(got, want, "a later put keeps the row's first time");
        assert!(devices(&db, "b").unwrap().is_empty());
    }
}
