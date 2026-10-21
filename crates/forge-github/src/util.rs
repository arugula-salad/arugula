//! What the forge adapters share: timestamps, the number of timeline
//! events kept, a short form of an HTTP error and a URL's host. The daemon's
//! Forgejo and GitLab adapters use these too.

/// How many timeline events a read keeps: the newest.
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

/// An HTTP error as its innermost cause (the part that says what went wrong).
pub fn short(e: &reqwest::Error) -> String {
    let mut s = e.to_string();
    let mut src = std::error::Error::source(e);
    while let Some(x) = src {
        s = x.to_string();
        src = x.source();
    }
    s
}

/// A URL's host, lower case, with its port if it has one.
pub fn url_host(u: &str) -> Option<String> {
    let u = url::Url::parse(u).ok()?;
    let host = u.host_str()?.to_ascii_lowercase();
    Some(match u.port() {
        Some(p) => format!("{host}:{p}"),
        None => host,
    })
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
}
