//! The platform page stays in the window (M157, #1183).
//!
//! The platform page (projects, chat, Cloud workspaces) is on an origin of
//! its own, served by control's hub: `https://app.arugula.io`, or
//! `http://app.localhost:<port>` for a local control (aru-079). The window
//! sends every page that isn't the app's, the daemon's or control's to the
//! person's browser (`on_navigation` in `main.rs`), so without this the
//! platform page would open there, away from the shell around it.
//!
//! - **Its origin is control's word**: the `platform` of control's
//!   `/control.json` (`cloud::platform`), never a guess from control's
//!   host name. A control that says none (an older one, or one without a
//!   platform page) leaves the window as it was: its links go to the
//!   browser.
//! - **Exactly that origin** stays in the window: its scheme, host and
//!   port, any path. Not a name under it, nor a lookalike.
//! - **Its window controls work** (#290's lesson): its origin gets the same
//!   run-time window permissions as control's page (`allow_control` in
//!   `main.rs`), or on Linux the client's bar couldn't move, minimize,
//!   maximize or close the window.

use tauri::Url;

/// The platform page's origin as control's `/control.json` says it: an
/// `http` or `https` origin, with nothing after it but a `/`. Anything else
/// (a path, a query, credentials, another scheme) isn't an origin, and
/// nothing is trusted for it.
pub fn origin(raw: &str) -> Option<Url> {
    let u: Url = raw.trim().parse().ok()?;
    let plain = matches!(u.scheme(), "http" | "https")
        && u.host_str().is_some_and(|h| !h.is_empty())
        && u.username().is_empty()
        && u.password().is_none()
        && u.path() == "/"
        && u.query().is_none()
        && u.fragment().is_none();
    plain.then_some(u)
}

/// Whether `url` is on the platform page's origin, when control said one.
pub fn is_platform(url: &Url, platform: Option<&Url>) -> bool {
    platform.is_some_and(|p| p.origin() == url.origin())
}

/// The origins whose pages get the window permissions the client's bar
/// needs: control's, and the platform page's when control said one. Each
/// once, as `scheme://host[:port]`.
pub fn window_origins(control: Option<&str>, platform: Option<&Url>) -> Vec<String> {
    let control = control.and_then(|c| c.parse::<Url>().ok());
    let mut out: Vec<String> = Vec::new();
    for u in control.iter().chain(platform) {
        let o = u.origin().ascii_serialization();
        if o != "null" && !out.contains(&o) {
            out.push(o);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn url(s: &str) -> Url {
        s.parse().unwrap()
    }

    #[test]
    fn the_platform_origin_stays_in_the_window() {
        let p = origin("https://app.arugula.io").unwrap();
        let on = |s: &str| is_platform(&url(s), Some(&p));
        assert!(on("https://app.arugula.io"));
        assert!(on("https://app.arugula.io/"));
        assert!(on("https://app.arugula.io/p/abc-my-project/chat?x=1#k=secret"));
        assert!(on("https://APP.arugula.io/"), "host names aren't case sensitive");
        assert!(on("https://app.arugula.io:443/"), "the default port is the same origin");
        // Not its subdomains, nor its parent or siblings.
        assert!(!on("https://x.app.arugula.io/"));
        assert!(!on("https://arugula.io/"));
        assert!(!on("https://control.arugula.io/"));
        // Not on another scheme or port.
        assert!(!on("http://app.arugula.io/"));
        assert!(!on("https://app.arugula.io:8443/"));
        assert!(!on("wss://app.arugula.io/"));
        // Not a lookalike.
        assert!(!on("https://app.arugula.io.evil.example/"));
        assert!(!on("https://evilapp.arugula.io/"));
        assert!(!on("https://app-arugula.io/"));
        assert!(!on("https://app.arugula.iox/"));
        assert!(!on("https://app.arugula.io@evil.example/"));
    }

    #[test]
    fn a_local_controls_platform_is_on_its_port() {
        let p = origin("http://app.localhost:7690").unwrap();
        let on = |s: &str| is_platform(&url(s), Some(&p));
        assert!(on("http://app.localhost:7690/"));
        assert!(on("http://app.localhost:7690/settings"));
        assert!(!on("http://app.localhost:7691/"));
        assert!(!on("https://app.localhost:7690/"));
        assert!(!on("http://app.localhost/"));
        assert!(!on("http://localhost:7690/"));
    }

    #[test]
    fn no_platform_origin_trusts_nothing() {
        assert!(!is_platform(&url("https://app.arugula.io/"), None));
        assert!(!is_platform(&url("http://app.localhost:7690/"), None));
    }

    #[test]
    fn only_an_origin_is_taken() {
        assert_eq!(origin("https://app.arugula.io").map(String::from), Some("https://app.arugula.io/".into()));
        assert_eq!(origin(" https://app.arugula.io/ ").map(String::from), Some("https://app.arugula.io/".into()));
        assert!(origin("http://app.localhost:7690").is_some());
        for bad in [
            "",
            "app.arugula.io",
            "https://",
            "https://app.arugula.io/p",
            "https://app.arugula.io/?x=1",
            "https://app.arugula.io/#x",
            "https://someone@app.arugula.io",
            "https://a:b@app.arugula.io",
            "ftp://app.arugula.io",
            "file:///etc/passwd",
            "javascript:alert(1)",
            "data:text/html,x",
        ] {
            assert!(origin(bad).is_none(), "{bad}");
        }
    }

    #[test]
    fn the_platform_gets_the_window_permissions_controls_page_does() {
        let p = origin("https://app.arugula.io").unwrap();
        assert_eq!(
            window_origins(Some("https://control.arugula.io"), Some(&p)),
            ["https://control.arugula.io", "https://app.arugula.io"]
        );
        // No platform page: control's alone, as before.
        assert_eq!(window_origins(Some("https://control.arugula.io"), None), ["https://control.arugula.io"]);
        // A local control's, on its ports.
        let local = origin("http://app.localhost:7690").unwrap();
        assert_eq!(
            window_origins(Some("http://localhost:7690"), Some(&local)),
            ["http://localhost:7690", "http://app.localhost:7690"]
        );
        // Not joined: nothing.
        assert!(window_origins(None, None).is_empty());
        // Once each, when they are the same origin.
        let same = origin("https://control.arugula.io").unwrap();
        assert_eq!(window_origins(Some("https://control.arugula.io"), Some(&same)), ["https://control.arugula.io"]);
        // A control that isn't a URL with an origin adds nothing.
        assert!(window_origins(Some("not a url"), None).is_empty());
        assert!(window_origins(Some("data:text/html,x"), None).is_empty());
    }
}
