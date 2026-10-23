//! What the desktop app's windows from before it had a kind of its own
//! approved, grandfathered (Jake, 2026-10-23 (UTC)).
//!
//! Before #1343 the app's window enrolled as a `browser` device it named
//! ("Arugula app on …", "illogical app" before the rename), and it approved
//! browsers and phones like any browser. #1343 (v0.59.0) took those
//! certificates out of "a browser all the way back to the root"
//! (`Cert::is_browser`), which left accounts whose every browser an app
//! window approved, directly or on the way back, with none that may sign
//! for a passkey, approve a deploy or approve a new browser.
//!
//! So what such a window approved back then counts again: a browser or a
//! phone (`Cert::is_browser`) whose certificate a window signed, when
//! control stored and approved both rows before #1343 shipped. Then, in a
//! chain back to the root, the window counts as the link just above that
//! browser, and nowhere else (`arugula_e2e::cert::legacy_app_link`): not as
//! the device that signs, not above anything it certifies now, not above a
//! recovery code it made.
//!
//! "Before" is control's own record, never a certificate's time (its
//! signer's claim, which a stolen key could backdate): `devices.created`,
//! control's clock when the row was first stored (a later put keeps it),
//! and `devices.approved_at`, its clock when the row became approved. A row
//! asked for before the cutoff and approved after has an `approved_at`
//! after it, and doesn't count. Rows approved before `approved_at` came
//! have none; they were approved before this shipped, under #1343's rule
//! when after the cutoff, which refused a window approving a browser, so
//! none of those is a window's child.
//!
//! Revocations don't change the set, as they don't change a chain's links
//! anywhere else (`Trust::chained`): what a removed device approved before
//! it was removed stands. They count where they always do: the device that
//! signs must be trusted now (`Trust::evaluate`), and a removed device
//! approves nothing new, a window included.

use std::collections::HashMap;

use arugula_e2e::{Cert, Kind};

use crate::db::Db;

/// When control v0.59.0, which carries #1343, was deployed: the `control`
/// job of the v0.59.0 release run (arugula-salad/arugula Actions run
/// 39845854046) completed at 2026-10-22T20:18:06Z; rounded down to the
/// minute, 2026-10-22T20:18:00Z.
pub const APP_KIND_SHIPPED_MS: u64 = 1_792_700_280_000;

/// `account`'s browsers and phones a grandfathered app window approved:
/// approved `browser` certificates that aren't the app's
/// ([`Cert::is_browser`]), signed by an approved `browser` certificate the
/// app named ([`Cert::from_app`]), both rows stored and approved before
/// [`APP_KIND_SHIPPED_MS`]. Revoked or not: see the module's doc.
pub fn children(db: &Db, account: &str) -> anyhow::Result<Vec<String>> {
    let ok = |c: &Cert| c.account == account;
    let old = db.browsers_stored_before(account, APP_KIND_SHIPPED_MS)?;
    let windows: HashMap<&str, &Cert> = old
        .iter()
        .filter(|c| c.kind == Kind::Browser && c.from_app() && ok(c))
        .map(|c| (c.device.as_str(), c))
        .collect();
    Ok(old
        .iter()
        .filter(|c| c.is_browser() && ok(c) && windows.contains_key(c.approver.as_str()))
        .map(|c| c.device.clone())
        .collect())
}

#[cfg(test)]
mod tests {
    use arugula_e2e::{Cert, DeviceKeys, Kind, Revocation};

    use super::*;

    #[test]
    fn only_what_a_window_approved_both_stored_and_approved_before_the_cutoff() {
        let db = Db::memory();
        let phone = DeviceKeys::generate();
        let mut root = Cert::new(&phone, "a", Kind::Browser, "Pixel");
        root.sign_with(&phone);
        db.put_device(&root, true, 1).unwrap();
        let before = APP_KIND_SHIPPED_MS - 60_000;
        let cert = |name: &str, kind, by: &DeviceKeys| {
            let k = DeviceKeys::generate();
            let mut c = Cert::new(&k, "a", kind, name);
            c.sign_with(by);
            (k, c)
        };
        let put = |name: &str, kind, by: &DeviceKeys, at: u64| {
            let (k, c) = cert(name, kind, by);
            db.put_device(&c, true, at).unwrap();
            (k, c)
        };
        let (window, _) = put("Arugula app on jake-air", Kind::Browser, &phone, before);
        let (_, firefox) = put("Firefox", Kind::Browser, &window, before);
        let (_, older) = put("Safari", Kind::Browser, &window, 2);
        // Not a window's child: the phone's, a recovery code, an `app`
        // device, another window.
        put("Chrome", Kind::Browser, &phone, before);
        put("recovery 1", Kind::Recovery, &window, before);
        put("Arugula app on jake-mini", Kind::App, &window, before);
        put("Arugula app on geek", Kind::Browser, &window, before);
        // Stored after the cutoff, or backdated by its own time.
        put("Edge", Kind::Browser, &window, APP_KIND_SHIPPED_MS);
        let (_, late) = cert("Opera", Kind::Browser, &window);
        db.put_device(&Cert { created: 1, ..late }, true, APP_KIND_SHIPPED_MS + 1).unwrap();
        // Asked before the cutoff, approved after.
        let (_, waited) = cert("Brave", Kind::Browser, &window);
        db.put_device(&waited, false, before).unwrap();
        db.put_device(&waited, true, APP_KIND_SHIPPED_MS + 2).unwrap();
        // A child removed since: what it approved before stands.
        let (_, gone) = put("Vivaldi", Kind::Browser, &window, before);
        db.add_revocation(&Revocation::new("a", &gone.device, &phone)).unwrap();
        let mut got = children(&db, "a").unwrap();
        got.sort();
        let mut want = vec![firefox.device.clone(), older.device.clone(), gone.device.clone()];
        want.sort();
        assert_eq!(got, want);

        // Stored again later: still when first stored and approved.
        db.put_device(&firefox, true, APP_KIND_SHIPPED_MS + 5).unwrap();
        assert_eq!(children(&db, "a").unwrap().len(), 3, "a later put keeps the row's times");
        // Made to wait and approved again: from then.
        db.put_device(&firefox, false, APP_KIND_SHIPPED_MS + 6).unwrap();
        db.put_device(&firefox, true, APP_KIND_SHIPPED_MS + 7).unwrap();
        let mut got = children(&db, "a").unwrap();
        got.sort();
        let mut want = vec![older.device.clone(), gone.device.clone()];
        want.sort();
        assert_eq!(got, want);
        assert!(children(&db, "b").unwrap().is_empty());
    }

    /// A window approved after the cutoff, though it asked before: nothing
    /// it approved counts, even rows stored before. One removed since keeps
    /// what it approved back then.
    #[test]
    fn a_window_approved_after_the_cutoff_has_no_children() {
        let db = Db::memory();
        let phone = DeviceKeys::generate();
        let mut root = Cert::new(&phone, "a", Kind::Browser, "Pixel");
        root.sign_with(&phone);
        db.put_device(&root, true, 1).unwrap();
        let before = APP_KIND_SHIPPED_MS - 60_000;
        let window = DeviceKeys::generate();
        let mut w = Cert::new(&window, "a", Kind::Browser, "Arugula app on jake-air");
        w.sign_with(&phone);
        db.put_device(&w, false, before).unwrap();
        db.put_device(&w, true, APP_KIND_SHIPPED_MS + 1).unwrap();
        let k = DeviceKeys::generate();
        let mut c = Cert::new(&k, "a", Kind::Browser, "Firefox");
        c.sign_with(&window);
        db.put_device(&c, true, before).unwrap();
        assert!(children(&db, "a").unwrap().is_empty());

        let db = Db::memory();
        db.put_device(&root, true, 1).unwrap();
        db.put_device(&w, true, before).unwrap();
        db.put_device(&c, true, before).unwrap();
        assert_eq!(children(&db, "a").unwrap(), vec![c.device.clone()]);
        db.add_revocation(&Revocation::new("a", &w.device, &phone)).unwrap();
        assert_eq!(children(&db, "a").unwrap(), vec![c.device.clone()], "the window was removed since");
    }
}
