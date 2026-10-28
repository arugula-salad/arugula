//! What the forge adapters share, kept in core so GitHub does not need
//! Forgejo's module (which is in Labs): timestamps, the number of timeline
//! events kept and a short form of an HTTP error (the GitHub crate's, M160),
//! and the two ways a link says it is GitLab's, which the refusal for a
//! build or machine without Labs needs as much as the adapter does.

// Forgejo's and GitLab's (Labs) use them; GitHub's are the crate's own.
pub use arugula_forge_github::util::{EVENTS, short, time};

/// A host that's GitLab by its name alone (the rest are GitLab when `glab`
/// knows them, or when a link says so).
pub fn known_host(host: &str) -> bool {
    let h = host.split(':').next().unwrap_or(host).to_ascii_lowercase();
    h == "gitlab.com" || h.starts_with("gitlab.")
}

/// `https://H[/prefix]/G/[SUB/…]P/-/merge_requests/N[/…]` → (host, project
/// path, N, API base). `None` when it isn't a merge request's address.
pub fn parse_mr_url(u: &str) -> Option<(String, String, u64, String)> {
    let url = url::Url::parse(u).ok()?;
    if !matches!(url.scheme(), "http" | "https") {
        return None;
    }
    let host = super::login::url_host(u)?;
    let segs: Vec<&str> = url.path_segments()?.filter(|x| !x.is_empty()).collect();
    let at = segs.windows(2).position(|w| w == ["-", "merge_requests"])?;
    let n: u64 = segs.get(at + 2)?.parse().ok()?;
    if at < 2 {
        return None; // a project needs a namespace and a name
    }
    let repo = segs[..at].join("/");
    Some((host.clone(), repo, n, format!("{}://{host}/api/v4", url.scheme())))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gitlab_links_and_hosts() {
        let p = parse_mr_url("https://gitlab.com/gitlab-org/cli/-/merge_requests/3941").unwrap();
        assert_eq!(p, ("gitlab.com".into(), "gitlab-org/cli".into(), 3941, "https://gitlab.com/api/v4".into()));
        let p = parse_mr_url("http://127.0.0.1:8080/group/sub/proj/-/merge_requests/7/diffs?x=1").unwrap();
        assert_eq!((p.1.as_str(), p.2, p.3.as_str()), ("group/sub/proj", 7, "http://127.0.0.1:8080/api/v4"));
        assert!(parse_mr_url("https://gitlab.com/gitlab-org/cli/-/issues/3").is_none());
        assert!(parse_mr_url("https://gitlab.com/cli/-/merge_requests/3").is_none());
        assert!(parse_mr_url("https://git.example/o/r/pulls/3").is_none());
        assert!(known_host("gitlab.com"));
        assert!(known_host("gitlab.example.org"));
        assert!(!known_host("git.inevitable.fyi"));
    }
}
