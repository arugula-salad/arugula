//! Block frames stay in the window (#150).
//!
//! The window's navigation handler (`on_navigation` in `main.rs`) sends
//! every page that isn't the app's, the daemon's or control's to the
//! person's browser. On macOS (WKWebView) and Linux (WebKitGTK) wry asks
//! that handler about subframe navigations too, with only the URL: wry's
//! `decidePolicyForNavigationAction` never looks at `targetFrame`, and
//! WebKitGTK's `decide-policy` navigation actions are "in either the main
//! frame or a subframe" and say nothing of which. (WebView2's
//! `NavigationStarting` is the main frame's only, so Windows never had
//! this.) A port or editor block's frame on its own origin was cancelled
//! and opened in the browser, where it can't work: its page needs the
//! control page around it for its credentials.
//!
//! The handler can't tell a frame from the window, so a block's origin
//! counts as a place the window may go, and only a block's:
//!
//! - **On control's block domain** (M50): `b-<site key>.<domain>`, the
//!   site key 16 to 61 hex digits, as control's `blocks::host` takes it,
//!   on control's own scheme and port (control serves block origins on its
//!   page's listener). The domain is control's, from its `/control.json`
//!   (`cloud::block_domain`), never a guess.
//! - **On the daemon's own block listener, dev scheme**:
//!   `http://b-<id>-<key>.localhost:<port>`, always loopback.
//!
//! A block's origin opened top-level only says it can't load (its page
//! asks the page around it for credentials), so nothing is lost by letting
//! the window stay. Popups
//! (`target=_blank`) aren't changed: those still go to the browser unless
//! they're the app's own pages.

use tauri::Url;

/// Whether `url` is a block's origin the window's frames load: on
/// `control`'s block `domain`, or on the daemon's dev block scheme.
pub fn is_block(url: &Url, control: Option<&Url>, domain: Option<&str>) -> bool {
    dev_block(url) || control.zip(domain).is_some_and(|(c, d)| control_block(url, c, d))
}

/// `b-<16..61 hex>.<domain>`, on control's scheme and port.
fn control_block(url: &Url, control: &Url, domain: &str) -> bool {
    if url.scheme() != control.scheme() || url.port_or_known_default() != control.port_or_known_default() {
        return false;
    }
    let Some(host) = url.host_str() else { return false };
    let Some(label) = host.strip_suffix(domain).and_then(|l| l.strip_suffix('.')) else { return false };
    label
        .strip_prefix("b-")
        .is_some_and(|key| (16..=61).contains(&key.len()) && key.bytes().all(|b| b.is_ascii_hexdigit()))
}

/// `http://b-<id>-<key>.localhost:<port>`: the daemon's own block sites
/// without a domain (`sites::Scheme::Dev`), loopback by name.
fn dev_block(url: &Url) -> bool {
    if url.scheme() != "http" {
        return false;
    }
    let Some(label) = url.host_str().and_then(|h| h.strip_suffix(".localhost")) else { return false };
    let Some((id, key)) = label.strip_prefix("b-").and_then(|l| l.split_once('-')) else { return false };
    !id.is_empty()
        && id.bytes().all(|b| b.is_ascii_digit())
        && !key.is_empty()
        && key.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit())
}

/// A block domain as control's `/control.json` says it, as control's own
/// `--block-domain` check takes one: lower case, no dots at the ends, at
/// least two labels, nothing but a name.
pub fn block_domain(raw: &str) -> Option<String> {
    let d = raw.trim().trim_matches('.').to_ascii_lowercase();
    let ok = d.contains('.')
        && d.split('.').all(|l| !l.is_empty() && l.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-'));
    ok.then_some(d)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn url(s: &str) -> Url {
        s.parse().unwrap()
    }

    const KEY: &str = "0123456789abcdef0123456789abcdef";

    #[test]
    fn a_block_on_controls_domain_is_one() {
        let c = url("https://control.arugula.io");
        let block = |s: &str| is_block(&url(s), Some(&c), Some("arugula.run"));
        assert!(block(&format!("https://b-{KEY}.arugula.run/")));
        assert!(block(&format!("https://b-{KEY}.arugula.run/some/path?x=1#y")));
        assert!(block(&format!("https://b-{}.arugula.run/", KEY.to_uppercase())));
        assert!(block(&format!("https://b-{}.arugula.run/", &KEY[..16])));
        // Not a block: the domain itself, other names under it, a key too
        // short or not hex, another domain, a deeper name.
        assert!(!block("https://arugula.run/"));
        assert!(!block("https://www.arugula.run/"));
        assert!(!block(&format!("https://b-{}.arugula.run/", &KEY[..15])));
        assert!(!block(&format!("https://b-{}.arugula.run/", "a".repeat(62))));
        assert!(!block("https://b-0123456789abcdeg.arugula.run/"));
        assert!(!block(&format!("https://b-{KEY}.evil.example/")));
        assert!(!block(&format!("https://b-{KEY}.arugula.run.evil.example/")));
        assert!(!block(&format!("https://x.b-{KEY}.arugula.run/")));
        assert!(!block(&format!("https://b-{KEY}evilarugula.run/")));
        // Control's scheme and port only.
        assert!(!block(&format!("http://b-{KEY}.arugula.run/")));
        assert!(!block(&format!("https://b-{KEY}.arugula.run:8443/")));
        // Not without a control, or a domain it said.
        assert!(!is_block(&url(&format!("https://b-{KEY}.arugula.run/")), None, Some("arugula.run")));
        assert!(!is_block(&url(&format!("https://b-{KEY}.arugula.run/")), Some(&c), None));
    }

    #[test]
    fn a_local_controls_blocks_are_on_its_scheme_and_port() {
        let c = url("http://localhost:7690");
        let block = |s: &str| is_block(&url(s), Some(&c), Some("blocks.localhost"));
        assert!(block(&format!("http://b-{KEY}.blocks.localhost:7690/")));
        assert!(!block(&format!("http://b-{KEY}.blocks.localhost:7691/")));
        assert!(!block(&format!("https://b-{KEY}.blocks.localhost:7690/")));
    }

    #[test]
    fn the_daemons_dev_blocks_are_blocks() {
        let block = |s: &str| is_block(&url(s), None, None);
        assert!(block("http://b-42-abcdefghij0123456789.localhost:7701/"));
        assert!(block("http://b-7-k3y.localhost:51234/x?y"));
        assert!(!block("https://b-42-abc.localhost:7701/"));
        assert!(!block("http://b-42.localhost:7701/"));
        assert!(!block("http://b-x-abc.localhost:7701/"));
        assert!(!block("http://b-42-ab.c.localhost:7701/"));
        assert!(!block("http://b-42-abc.localhost.evil.example/"));
        assert!(!block("http://localhost:7701/"));
        assert!(!block("http://b-42-abc.example.com/"));
    }

    #[test]
    fn a_block_domain_is_a_name() {
        assert_eq!(block_domain("arugula.run"), Some("arugula.run".into()));
        assert_eq!(block_domain(" .Arugula.RUN. "), Some("arugula.run".into()));
        assert_eq!(block_domain("blocks.localhost"), Some("blocks.localhost".into()));
        for bad in ["", "run", "arugula..run", "arugula.run/x", "arugula.run:443", "*.arugula.run", "a b.run"] {
            assert_eq!(block_domain(bad), None, "{bad}");
        }
    }
}
