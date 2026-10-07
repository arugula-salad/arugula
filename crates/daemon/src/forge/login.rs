//! Reading a host and a repository out of a URL or a git remote, which the
//! forge blocks use to pick a login and to match a clone to a link. (The
//! `tea` logins Forgejo reads with are in `labs/tea.rs`.)

/// The host (with a port, if not the scheme's) of a URL.
pub fn url_host(u: &str) -> Option<String> {
    let u = url::Url::parse(u).ok()?;
    let host = u.host_str()?.to_ascii_lowercase();
    Some(match u.port() {
        Some(p) => format!("{host}:{p}"),
        None => host,
    })
}

/// A git remote's host and repository path (`owner/name`, no `.git`):
/// `ssh://git@host[:port]/o/r.git`, `git@host:o/r.git` or
/// `https://host/o/r`.
pub fn parse_remote(remote: &str) -> Option<(String, String)> {
    let remote = remote.trim();
    let path = |p: &str| {
        let p = p.trim_matches('/').trim_end_matches(".git").trim_end_matches('/');
        (p.split('/').count() >= 2).then(|| p.to_owned())
    };
    if remote.contains("://") {
        let u = url::Url::parse(remote).ok()?;
        let host = u.host_str()?.to_ascii_lowercase();
        // An SSH port isn't the web's: match on the name alone.
        let host = match (u.scheme(), u.port()) {
            ("http" | "https", Some(p)) => format!("{host}:{p}"),
            _ => host,
        };
        return Some((host, path(u.path())?));
    }
    // scp-like: [user@]host:path
    let (left, p) = remote.split_once(':')?;
    if left.contains('/') {
        return None; // a local path with a colon in it
    }
    let host = left.rsplit('@').next()?.to_ascii_lowercase();
    Some((host, path(p)?))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn remotes() {
        let r = |s: &str| parse_remote(s).map(|(h, p)| format!("{h} {p}"));
        assert_eq!(
            r("ssh://git@git.tail1234.ts.net/jhgaylor/illogical.git").as_deref(),
            Some("git.tail1234.ts.net jhgaylor/illogical")
        );
        assert_eq!(r("ssh://git@host:2222/o/r.git").as_deref(), Some("host o/r"));
        assert_eq!(r("git@codeberg.org:forgejo/forgejo.git").as_deref(), Some("codeberg.org forgejo/forgejo"));
        assert_eq!(
            r("https://git.inevitable.fyi/jhgaylor/illogical").as_deref(),
            Some("git.inevitable.fyi jhgaylor/illogical")
        );
        assert_eq!(r("http://127.0.0.1:3000/o/r.git").as_deref(), Some("127.0.0.1:3000 o/r"));
        assert_eq!(r("/srv/git/r.git"), None);
        assert_eq!(r("./a:b/c"), None);
    }
}
