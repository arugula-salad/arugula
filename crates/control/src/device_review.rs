//! `arugula-control device-review`: an operator's read-only look for
//! accounts and devices to review now that only browsers make browsers
//! (the desktop app has its own `app` kind):
//!
//! - accounts whose first device (root) is a CLI or the desktop app's (an
//!   `app` one, or a `browser` one the app named before it had a kind):
//!   they make no new browsers or recovery codes now;
//! - `browser` devices whose sign-ins said, by their User-Agent, that they
//!   run in a webview (WebKitGTK, WebView2, Tauri, or Apple's WebKit with
//!   no Safari): perhaps the desktop app's, under a name it didn't give.
//!
//! It prints ids and names only, and opens the database read-only, as
//! `census` does, so it can run beside the live control.

use std::path::PathBuf;

use arugula_e2e::{Cert, Kind};
use clap::Parser;
use rusqlite::Connection;

#[derive(Parser, Debug)]
#[command(
    name = "arugula-control device-review",
    about = "Accounts whose root is a CLI or the app's, and browsers that signed in from a webview (read-only)"
)]
pub struct Args {
    /// Control's database, opened read-only.
    #[arg(long, default_value = "control.db", env = "ARUGULA_CONTROL_DB")]
    db: PathBuf,
}

/// An account, or a device of one, to review.
#[derive(Debug, PartialEq, Eq)]
pub struct Seen {
    pub account: String,
    pub device: String,
    pub name: String,
}

/// What to review.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Review {
    /// Accounts whose root is a CLI's or the app's device.
    pub roots: Vec<Seen>,
    /// `browser` devices whose sessions' User-Agent looks like a webview.
    pub webviews: Vec<Seen>,
}

/// Whether a User-Agent looks like a webview's.
pub fn webview(ua: &str) -> bool {
    ["WebKitGTK", "WebView2", "Tauri", "wry"].iter().any(|w| ua.contains(w))
        || (ua.contains("AppleWebKit") && !ua.contains("Safari") && !ua.contains("Chrome"))
}

pub fn review(c: &Connection) -> anyhow::Result<Review> {
    let mut out = Review::default();
    let mut q = c.prepare(
        "SELECT a.id, d.cert FROM accounts a JOIN devices d ON d.account = a.id AND d.id = a.root ORDER BY a.id",
    )?;
    for row in q.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))? {
        let (account, cert) = row?;
        let Ok(cert) = serde_json::from_str::<Cert>(&cert) else { continue };
        if cert.kind == Kind::Cli || cert.from_app() {
            out.roots.push(Seen { account, device: cert.device, name: cert.name });
        }
    }
    let mut q = c.prepare(
        "SELECT DISTINCT d.account, d.cert, s.agent FROM devices d
         JOIN sessions s ON s.account = d.account AND s.device = d.id
         WHERE d.kind = 'browser' AND s.agent IS NOT NULL ORDER BY d.account, d.id",
    )?;
    for row in q.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get::<_, String>(2)?)))? {
        let (account, cert, agent) = row?;
        let Ok(cert) = serde_json::from_str::<Cert>(&cert) else { continue };
        if webview(&agent) && !out.webviews.iter().any(|s| s.device == cert.device) {
            out.webviews.push(Seen { account, device: cert.device, name: cert.name });
        }
    }
    Ok(out)
}

pub fn render(r: &Review) -> String {
    let mut out = format!("Accounts whose root is a CLI or the desktop app's: {}\n", r.roots.len());
    for s in &r.roots {
        out += &format!("  {}  {}  {}\n", s.account, s.device, s.name);
    }
    out += &format!("\nBrowser devices that signed in from a webview: {}\n", r.webviews.len());
    for s in &r.webviews {
        out += &format!("  {}  {}  {}\n", s.account, s.device, s.name);
    }
    out
}

/// `arugula-control device-review [--db PATH]`.
pub fn run(args: impl IntoIterator<Item = String>) -> anyhow::Result<()> {
    let a = Args::parse_from(std::iter::once("arugula-control device-review".to_owned()).chain(args));
    let c = crate::census::open(&a.db)?;
    print!("{}", render(&review(&c)?));
    Ok(())
}

#[cfg(test)]
mod tests {
    use arugula_e2e::DeviceKeys;

    use super::*;
    use crate::db::Db;

    #[test]
    fn it_finds_cli_and_app_roots_and_webview_browsers() {
        let db = Db::memory();
        let root = |account: &str, kind, name: &str| {
            db.account_for("github", account, account, account, 1).unwrap();
            let k = DeviceKeys::generate();
            let mut c = Cert::new(&k, account, kind, name);
            c.sign_with(&k);
            db.put_device(&c, true, 1).unwrap();
            (k, c)
        };
        let (_, cli) = root("a-cli", Kind::Cli, "Arugula CLI");
        let (_, old_app) = root("a-app", Kind::Browser, "Arugula app on geek");
        let (bk, _) = root("a-browser", Kind::Browser, "Chrome on Mac");
        let mut tauri = Cert::new(&DeviceKeys::generate(), "a-browser", Kind::Browser, "Safari on Mac");
        tauri.sign_with(&bk);
        db.put_device(&tauri, true, 1).unwrap();
        for (t, device, ua) in [
            ("s1", &tauri.device, "Mozilla/5.0 (Macintosh) AppleWebKit/605.1.15 (KHTML, like Gecko)"),
            ("s2", &bk.id(), "Mozilla/5.0 (Macintosh) AppleWebKit/537.36 Chrome/140 Safari/537.36"),
        ] {
            db.add_session(t, "a-browser", 1, i64::MAX as u64).unwrap();
            db.set_session_device(t, "a-browser", device).unwrap();
            db.set_session_agent(t, ua).unwrap();
        }
        let r = review(&db.conn()).unwrap();
        let ids = |v: &[Seen]| v.iter().map(|s| s.device.clone()).collect::<Vec<_>>();
        assert_eq!(ids(&r.roots), [old_app.device.clone(), cli.device.clone()], "by account id");
        assert_eq!(ids(&r.webviews), [tauri.device.clone()]);
        let text = render(&r);
        assert!(text.contains("Arugula CLI") && text.contains("webview: 1"), "{text}");
        assert!(!text.contains("AppleWebKit"), "names and ids only");
    }
}
