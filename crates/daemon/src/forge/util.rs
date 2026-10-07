//! What the forge adapters share, kept in core so GitHub does not need
//! Forgejo's module (which is in Labs): timestamps, the number of timeline
//! events kept, a short form of an HTTP error, and the two ways a link says
//! it is GitLab's, which the refusal for a build or machine without Labs
//! needs as much as the adapter does.

/// How many timeline events are kept in the state (the rest are in the
/// block's log).
pub const EVENTS: usize = 50;

/// `2026-10-02T23:37:59Z` or `2026-10-02T15:41:18+02:00` (fractions too)
/// as UTC ms.
pub fn time(s: &str) -> Option<i64> {
    let b = s.as_bytes();
    if b.len() < 19 || b[4] != b'-' || b[7] != b'-' || !(b[10] == b'T' || b[10] == b' ') || b[13] != b':' {
        return None;
    }
    let n = |r: std::ops::Range<usize>| s.get(r)?.parse::<i64>().ok();
    let (y, m, d) = (n(0..4)?, n(5..7)?, n(8..10)?);
    let (hh, mm, ss) = (n(11..13)?, n(14..16)?, n(17..19)?);
    let mut rest = &s[19..];
    let mut ms = 0;
    if let Some(f) = rest.strip_prefix('.') {
        let digits = f.find(|c: char| !c.is_ascii_digit()).unwrap_or(f.len());
        ms = format!("{:0<3}", &f[..digits.min(3)]).parse::<i64>().ok()?;
        rest = &f[digits..];
    }
    let offset = match rest {
        "Z" | "" => 0,
        o if o.len() == 6 && (o.starts_with('+') || o.starts_with('-')) => {
            let h = o.get(1..3)?.parse::<i64>().ok()?;
            let mi = o.get(4..6)?.parse::<i64>().ok()?;
            let v = (h * 60 + mi) * 60_000;
            if o.starts_with('-') { -v } else { v }
        }
        _ => return None,
    };
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146_097 + doe - 719_468;
    Some(((days * 24 + hh) * 60 + mm) * 60_000 + ss * 1000 + ms - offset)
}

pub fn short(e: &reqwest::Error) -> String {
    let mut s = e.to_string();
    let mut src = std::error::Error::source(e);
    while let Some(x) = src {
        s = x.to_string();
        src = x.source();
    }
    s
}

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
    fn times_with_offsets() {
        assert_eq!(time("1970-01-01T00:00:00Z"), Some(0));
        assert_eq!(time("2026-10-02T15:41:18+02:00"), time("2026-10-02T13:41:18Z"));
        assert_eq!(time("2026-10-02T13:41:18.25-01:30"), Some(time("2026-10-02T15:11:18Z").unwrap() + 250));
        assert_eq!(time("soon"), None);
    }

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
