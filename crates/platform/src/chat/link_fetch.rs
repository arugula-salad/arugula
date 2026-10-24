//! Fetching a web page for a link preview (#108): server-side, so a
//! reader's browser never contacts the site, and guarded so a message can't
//! aim control at its own network.
//!
//! **The guard.** Every hop (the link, then each redirect) is resolved
//! here, refused unless every address it names is public ([`public`]:
//! not private, loopback, link-local, ULA, CGNAT, multicast, documentation
//! or a v6 form of any of those), and then requested at the address that
//! was checked, pinned for the request, so a second DNS answer can't
//! swap in another. Redirects are followed by hand for that reason. A hop
//! refused is never connected to. Proxy settings in the environment are
//! ignored (a proxy would resolve for us).
//!
//! **The limits.** 5 seconds a request and 10 for the whole fetch, 1 MB
//! of body (a longer one is cut there for a page, refused for an image),
//! 4 redirects, `text/html` for a page and a raster `image/*` for an image
//! (no SVG: it runs scripts when opened on its own).

use std::{
    collections::HashMap,
    net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr},
    time::Duration,
};

use url::{Host, Url};

pub const MAX_BODY: usize = 1 << 20;
const REDIRECTS: usize = 4;
const HOP: Duration = Duration::from_secs(5);
const WHOLE: Duration = Duration::from_secs(10);
const AGENT: &str = "ArugulaLinkPreview/1 (+https://arugula.io)";

/// Why a fetch gave nothing. Readers see none of these: a link that can't
/// be read is the plain link.
#[derive(Debug, PartialEq, Eq)]
pub enum Refused {
    /// Not an http(s) URL with a host.
    Scheme,
    /// The host is, or resolves to, an address that isn't public.
    Private,
    Unresolved,
    TooManyRedirects,
    /// Not the kind of content that was asked for.
    Kind,
    TooBig,
    Status(u16),
    Failed,
}

/// How hosts are resolved and which addresses are allowed. Production's
/// is [`Net::real`]; tests name hosts and allow loopback to see what a
/// guard that *did* allow it would have done.
pub struct Net {
    hosts: Option<HashMap<String, Vec<IpAddr>>>,
    allow: fn(IpAddr) -> bool,
}

impl Net {
    pub fn real() -> Net {
        Net { hosts: None, allow: public }
    }

    #[cfg(test)]
    fn with(hosts: &[(&str, IpAddr)], allow: fn(IpAddr) -> bool) -> Net {
        Net { hosts: Some(hosts.iter().map(|(h, ip)| ((*h).to_owned(), vec![*ip])).collect()), allow }
    }

    async fn resolve(&self, host: &Host<&str>, port: u16) -> Result<Vec<IpAddr>, Refused> {
        let ips = match host {
            Host::Ipv4(ip) => vec![IpAddr::V4(*ip)],
            Host::Ipv6(ip) => vec![IpAddr::V6(*ip)],
            Host::Domain(d) => match &self.hosts {
                Some(h) => h.get(*d).cloned().unwrap_or_default(),
                None => tokio::net::lookup_host((*d, port))
                    .await
                    .map_err(|_| Refused::Unresolved)?
                    .map(|a| a.ip())
                    .collect(),
            },
        };
        if ips.is_empty() {
            return Err(Refused::Unresolved);
        }
        // Every answer, not the first: one private address among public
        // ones is still a way in.
        if !ips.iter().all(|ip| (self.allow)(*ip)) {
            return Err(Refused::Private);
        }
        Ok(ips)
    }
}

/// Whether `ip` is a public address: one a page on the internet could be at.
pub fn public(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(a) => public4(a),
        IpAddr::V6(a) => public6(a),
    }
}

fn public4(a: Ipv4Addr) -> bool {
    let [o0, o1, o2, _] = a.octets();
    !(a.is_unspecified()
        || a.is_loopback()
        || a.is_private()
        || a.is_link_local()
        || a.is_broadcast()
        || a.is_multicast()
        || a.is_documentation()
        || o0 == 0
        // 100.64.0.0/10 (carrier-grade NAT), 192.0.0.0/24 (IETF), 198.18.0.0/15
        // (benchmarks), 240.0.0.0/4 (reserved).
        || (o0 == 100 && (64..128).contains(&o1))
        || (o0 == 192 && o1 == 0 && o2 == 0)
        || (o0 == 198 && (o1 == 18 || o1 == 19))
        || o0 >= 240)
}

fn public6(a: Ipv6Addr) -> bool {
    let s = a.segments();
    let embedded = |hi: u16, lo: u16| Ipv4Addr::new((hi >> 8) as u8, hi as u8, (lo >> 8) as u8, lo as u8);
    if let Some(v4) = a.to_ipv4_mapped() {
        return public4(v4);
    }
    if a.is_unspecified() || a.is_loopback() || a.is_multicast() {
        return false;
    }
    // ::/96 (v4-compatible, deprecated) is refused outright; 64:ff9b::/96
    // (NAT64) and 2002::/16 (6to4) carry a v4 address that is the one reached.
    if s[..6].iter().all(|x| *x == 0) {
        return false;
    }
    if s[0] == 0x64 && s[1] == 0xff9b && s[2..6].iter().all(|x| *x == 0) {
        return public4(embedded(s[6], s[7]));
    }
    if s[0] == 0x2002 {
        return public4(embedded(s[1], s[2]));
    }
    !((s[0] & 0xfe00) == 0xfc00 // fc00::/7, unique local
        || (s[0] & 0xffc0) == 0xfe80 // fe80::/10, link-local
        || (s[0] & 0xffc0) == 0xfec0 // fec0::/10, site-local (deprecated)
        || (s[0] == 0x2001 && s[1] == 0) // 2001::/32, Teredo
        || (s[0] == 0x2001 && s[1] == 0xdb8) // documentation
        || (s[0] == 0x64 && s[1] == 0xff9b && s[2] == 1)) // local-use NAT64
}

/// What a fetch got.
#[derive(Debug)]
pub struct Got {
    /// Where the last hop was.
    pub url: Url,
    pub content_type: String,
    pub body: Vec<u8>,
}

/// Which content a fetch asks for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Want {
    Page,
    Image,
}

fn type_ok(want: Want, ct: &str) -> bool {
    let ct = ct.split(';').next().unwrap_or("").trim().to_ascii_lowercase();
    match want {
        Want::Page => ct == "text/html" || ct == "application/xhtml+xml",
        Want::Image => matches!(ct.as_str(), "image/png" | "image/jpeg" | "image/gif" | "image/webp" | "image/avif"),
    }
}

/// GET `url`, hop by hop, as the module says.
pub async fn get(net: &Net, url: &str, want: Want) -> Result<Got, Refused> {
    tokio::time::timeout(WHOLE, hops(net, url, want)).await.map_err(|_| Refused::Failed)?
}

async fn hops(net: &Net, url: &str, want: Want) -> Result<Got, Refused> {
    let mut url = Url::parse(url).map_err(|_| Refused::Scheme)?;
    for _ in 0..=REDIRECTS {
        if !matches!(url.scheme(), "http" | "https") || !url.username().is_empty() || url.password().is_some() {
            return Err(Refused::Scheme);
        }
        let host = url.host().ok_or(Refused::Scheme)?;
        let port = url.port_or_known_default().ok_or(Refused::Scheme)?;
        let ips = net.resolve(&host, port).await?;
        let mut client = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .timeout(HOP)
            .user_agent(AGENT)
            .no_proxy();
        if let Host::Domain(d) = host {
            client = client.resolve(d, SocketAddr::new(ips[0], port));
        }
        let accept = match want {
            Want::Page => "text/html,application/xhtml+xml",
            Want::Image => "image/png,image/jpeg,image/gif,image/webp,image/avif",
        };
        let mut resp = client
            .build()
            .map_err(|_| Refused::Failed)?
            .get(url.clone())
            .header("accept", accept)
            .send()
            .await
            .map_err(|_| Refused::Failed)?;
        let status = resp.status();
        if status.is_redirection() {
            let next = resp
                .headers()
                .get("location")
                .and_then(|v| v.to_str().ok())
                .and_then(|l| url.join(l).ok())
                .ok_or(Refused::Failed)?;
            url = next;
            continue;
        }
        if !status.is_success() {
            return Err(Refused::Status(status.as_u16()));
        }
        let content_type = resp.headers().get("content-type").and_then(|v| v.to_str().ok()).unwrap_or("").to_owned();
        if !type_ok(want, &content_type) {
            return Err(Refused::Kind);
        }
        if want == Want::Image && resp.content_length().is_some_and(|n| n > MAX_BODY as u64) {
            return Err(Refused::TooBig);
        }
        let mut body = Vec::new();
        while let Some(chunk) = resp.chunk().await.map_err(|_| Refused::Failed)? {
            if body.len() + chunk.len() > MAX_BODY {
                // A page's tags are at its top: keep what fits.
                if want == Want::Image {
                    return Err(Refused::TooBig);
                }
                body.extend_from_slice(&chunk[..MAX_BODY - body.len()]);
                break;
            }
            body.extend_from_slice(&chunk);
        }
        return Ok(Got { url, content_type, body });
    }
    Err(Refused::TooManyRedirects)
}

/// What a page says about itself.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Meta {
    pub site: String,
    pub title: String,
    pub description: String,
    pub image: Option<String>,
}

/// Open Graph (then Twitter's, then plain) tags from a page's head. A
/// tolerant scan, not a parser: it reads `<meta>` and `<title>` up to
/// `</head>` or `<body`.
pub fn meta(html: &[u8], base: &Url) -> Meta {
    let html = String::from_utf8_lossy(html);
    let lower = html.to_ascii_lowercase();
    let end = [lower.find("</head"), lower.find("<body")].into_iter().flatten().min().unwrap_or(lower.len());
    let (head, head_lower) = (&html[..end], &lower[..end]);
    let mut tags: HashMap<String, String> = HashMap::new();
    let mut at = 0;
    while let Some(i) = head_lower[at..].find("<meta") {
        let start = at + i + 5;
        let Some(len) = head[start..].find('>') else { break };
        let attrs = attributes(&head[start..start + len]);
        at = start + len;
        let key = attrs.get("property").or_else(|| attrs.get("name"));
        if let (Some(k), Some(v)) = (key, attrs.get("content")) {
            tags.entry(k.to_ascii_lowercase()).or_insert_with(|| v.clone());
        }
    }
    let title_tag = head_lower.find("<title").and_then(|i| {
        let from = head[i..].find('>')? + i + 1;
        let to = head_lower[from..].find("</title")? + from;
        Some(head[from..to].to_owned())
    });
    let pick = |keys: &[&str]| keys.iter().find_map(|k| tags.get(*k)).map(|s| tidy(s, 400)).filter(|s| !s.is_empty());
    let image = pick(&["og:image", "og:image:url", "twitter:image"])
        .and_then(|i| base.join(&i).ok())
        .filter(|u| matches!(u.scheme(), "http" | "https"))
        .map(String::from);
    Meta {
        site: pick(&["og:site_name"]).unwrap_or_default(),
        title: pick(&["og:title", "twitter:title"])
            .or_else(|| title_tag.map(|t| tidy(&t, 400)).filter(|s| !s.is_empty()))
            .unwrap_or_default(),
        description: pick(&["og:description", "twitter:description", "description"]).unwrap_or_default(),
        image,
    }
}

/// `name="value"` pairs in a tag's text, names lowercased, values decoded.
fn attributes(s: &str) -> HashMap<String, String> {
    let mut out = HashMap::new();
    let b = s.as_bytes();
    let mut i = 0;
    while i < b.len() {
        while i < b.len() && (b[i].is_ascii_whitespace() || b[i] == b'/') {
            i += 1;
        }
        let from = i;
        while i < b.len() && !b[i].is_ascii_whitespace() && b[i] != b'=' && b[i] != b'/' {
            i += 1;
        }
        let name = s[from..i].to_ascii_lowercase();
        while i < b.len() && b[i].is_ascii_whitespace() {
            i += 1;
        }
        if i >= b.len() || b[i] != b'=' {
            continue;
        }
        i += 1;
        while i < b.len() && b[i].is_ascii_whitespace() {
            i += 1;
        }
        let value = match b.get(i) {
            Some(q @ (b'"' | b'\'')) => {
                let v0 = i + 1;
                let v1 = s[v0..].find(*q as char).map_or(s.len(), |n| v0 + n);
                i = (v1 + 1).min(s.len());
                &s[v0..v1]
            }
            _ => {
                let v0 = i;
                while i < b.len() && !b[i].is_ascii_whitespace() {
                    i += 1;
                }
                &s[v0..i]
            }
        };
        if !name.is_empty() {
            out.entry(name).or_insert_with(|| decode(value));
        }
    }
    out
}

/// The few entities pages use in meta text.
fn decode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(i) = rest.find('&') {
        out.push_str(&rest[..i]);
        rest = &rest[i..];
        let Some(semi) = rest.find(';').filter(|n| *n <= 10) else {
            out.push('&');
            rest = &rest[1..];
            continue;
        };
        let name = &rest[1..semi];
        let ch = match name {
            "amp" => Some('&'),
            "lt" => Some('<'),
            "gt" => Some('>'),
            "quot" => Some('"'),
            "apos" => Some('\''),
            "nbsp" => Some(' '),
            _ => name
                .strip_prefix("#x")
                .or_else(|| name.strip_prefix("#X"))
                .and_then(|h| u32::from_str_radix(h, 16).ok())
                .or_else(|| name.strip_prefix('#').and_then(|d| d.parse().ok()))
                .and_then(char::from_u32),
        };
        match ch {
            Some(c) => {
                out.push(c);
                rest = &rest[semi + 1..];
            }
            None => {
                out.push('&');
                rest = &rest[1..];
            }
        }
    }
    out.push_str(rest);
    out
}

/// Whitespace collapsed, at most `max` characters.
fn tidy(s: &str, max: usize) -> String {
    let one = decode(s).split_whitespace().collect::<Vec<_>>().join(" ");
    if one.chars().count() <= max { one } else { format!("{}…", one.chars().take(max - 1).collect::<String>()) }
}

#[cfg(test)]
mod tests {
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };

    use tokio::{
        io::{AsyncReadExt, AsyncWriteExt},
        net::TcpListener,
    };

    use super::*;

    fn ip(s: &str) -> IpAddr {
        s.parse().unwrap()
    }

    #[test]
    fn only_public_addresses_pass() {
        for bad in [
            "127.0.0.1",
            "10.1.2.3",
            "172.16.0.1",
            "192.168.1.1",
            "169.254.169.254",
            "100.64.0.1",
            "0.0.0.0",
            "224.0.0.1",
            "255.255.255.255",
            "192.0.2.1",
            "198.18.0.1",
            "::1",
            "::",
            "fe80::1",
            "fc00::1",
            "fd12:3456::1",
            "ff02::1",
            "::ffff:127.0.0.1",
            "::ffff:169.254.169.254",
            "::127.0.0.1",
            "64:ff9b::7f00:1",
            "2002:a9fe:a9fe::1",
            "2001:0:1::1",
            "2001:db8::1",
            "fec0::1",
        ] {
            assert!(!public(ip(bad)), "{bad}");
        }
        for good in
            ["8.8.8.8", "1.1.1.1", "93.184.216.34", "2606:4700:4700::1111", "::ffff:8.8.8.8", "64:ff9b::808:808"]
        {
            assert!(public(ip(good)), "{good}");
        }
    }

    /// A server that counts the connections it gets, answering each with `reply`.
    async fn server(bind: &str, reply: String) -> Option<(u16, Arc<AtomicUsize>)> {
        let l = TcpListener::bind(bind).await.ok()?;
        let port = l.local_addr().unwrap().port();
        let n = Arc::new(AtomicUsize::new(0));
        let seen = n.clone();
        tokio::spawn(async move {
            loop {
                let Ok((mut s, _)) = l.accept().await else { return };
                seen.fetch_add(1, Ordering::SeqCst);
                let reply = reply.clone();
                tokio::spawn(async move {
                    // Read the request first: closing on unread bytes resets the
                    // connection, and the client would lose the reply's tail.
                    let mut buf = [0u8; 2048];
                    let _ = s.read(&mut buf).await;
                    let _ = s.write_all(reply.as_bytes()).await;
                    let _ = s.shutdown().await;
                });
            }
        });
        Some((port, n))
    }

    fn html(body: &str) -> String {
        format!(
            "HTTP/1.1 200 OK\r\ncontent-type: text/html; charset=utf-8\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
            body.len()
        )
    }

    #[tokio::test]
    async fn a_private_target_is_never_requested() {
        // Listeners on the very addresses a guard must keep control away
        // from. The real guard refuses before connecting, so none sees a thing.
        let (v4, n4) = server("127.0.0.1:0", html("<title>no</title>")).await.unwrap();
        let v6 = server("[::1]:0", html("<title>no</title>")).await;
        let net = Net::real();
        let mut urls = vec![
            "http://169.254.169.254/latest/meta-data/".to_owned(),
            format!("http://localhost:{v4}/"),
            format!("http://127.0.0.1:{v4}/"),
            "http://[::1]/".to_owned(),
            "http://[::ffff:7f00:1]/".to_owned(),
            "http://2130706433/".to_owned(),
            "ftp://example.com/".to_owned(),
            "http://user:pw@example.com/".to_owned(),
        ];
        if let Some((p, _)) = &v6 {
            urls.push(format!("http://[::1]:{p}/"));
        }
        for u in &urls {
            assert!(get(&net, u, Want::Page).await.is_err(), "{u}");
        }
        assert_eq!(n4.load(Ordering::SeqCst), 0);
        if let Some((_, n6)) = v6 {
            assert_eq!(n6.load(Ordering::SeqCst), 0);
        }
    }

    #[tokio::test]
    async fn a_redirect_into_a_private_address_is_not_followed() {
        // "pub.test" is let through (it answers from loopback in this test
        // only); what it redirects to is judged again, by the same rule
        // applied to everything but that one address.
        fn only_127_0_0_2(ip: IpAddr) -> bool {
            ip == "127.0.0.2".parse::<IpAddr>().unwrap()
        }
        let (target, hits) = server("127.0.0.1:0", html("<title>secret</title>")).await.unwrap();
        for to in [
            format!("http://127.0.0.1:{target}/"),
            "http://169.254.169.254/".to_owned(),
            format!("http://localhost:{target}/"),
            "http://[::1]/".to_owned(),
        ] {
            let redirect =
                format!("HTTP/1.1 302 Found\r\nlocation: {to}\r\ncontent-length: 0\r\nconnection: close\r\n\r\n");
            let Some((front, front_hits)) = server("127.0.0.2:0", redirect).await else { return };
            let net = Net::with(&[("pub.test", ip("127.0.0.2")), ("localhost", ip("127.0.0.1"))], only_127_0_0_2);
            let r = get(&net, &format!("http://pub.test:{front}/"), Want::Page).await;
            assert_eq!(front_hits.load(Ordering::SeqCst), 1, "the front page is requested: {to}");
            assert_eq!(r.unwrap_err(), Refused::Private, "{to}");
        }
        assert_eq!(hits.load(Ordering::SeqCst), 0, "the redirect target is never requested");
    }

    #[tokio::test]
    async fn a_page_is_read_and_limits_hold() {
        fn loopback_ok(_: IpAddr) -> bool {
            true
        }
        let page = r#"<html><head><meta property="og:title" content="Docs &amp; more"><meta property="og:image" content="/i.png"></head><body></body></html>"#;
        let Some((port, _)) = server("127.0.0.1:0", html(page)).await else { return };
        let net = Net::with(&[("site.test", ip("127.0.0.1"))], loopback_ok);
        let got = get(&net, &format!("http://site.test:{port}/a"), Want::Page).await.unwrap();
        let m = meta(&got.body, &got.url);
        assert_eq!(m.title, "Docs & more");
        assert_eq!(m.image, Some(format!("http://site.test:{port}/i.png")));
        // Not HTML.
        let json =
            "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: 2\r\nconnection: close\r\n\r\n{}"
                .to_owned();
        let (port, _) = server("127.0.0.1:0", json).await.unwrap();
        let r = get(&net, &format!("http://site.test:{port}/"), Want::Page).await;
        assert_eq!(r.unwrap_err(), Refused::Kind);
        // Too much: a page is cut at the cap, an image refused.
        let big = "x".repeat(MAX_BODY + 10);
        let (port, _) = server("127.0.0.1:0", html(&big)).await.unwrap();
        let got = get(&net, &format!("http://site.test:{port}/"), Want::Page).await.unwrap();
        assert_eq!(got.body.len(), MAX_BODY);
        let img = format!(
            "HTTP/1.1 200 OK\r\ncontent-type: image/png\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{big}",
            big.len()
        );
        let (port, _) = server("127.0.0.1:0", img).await.unwrap();
        let r = get(&net, &format!("http://site.test:{port}/"), Want::Image).await;
        assert_eq!(r.unwrap_err(), Refused::TooBig);
    }

    #[test]
    fn meta_prefers_open_graph_and_decodes() {
        let base = Url::parse("https://example.com/a/b").unwrap();
        let m = meta(
            br#"<head><title> Plain  title </title><meta name="description" content="d1">
            <meta property='og:title' content='OG &#39;title&#39;'><META NAME=twitter:image CONTENT=../x.jpg>
            <meta property="og:site_name" content="Example"></head><body><meta property="og:title" content="late">"#,
            &base,
        );
        assert_eq!(m.title, "OG 'title'");
        assert_eq!(m.site, "Example");
        assert_eq!(m.description, "d1");
        assert_eq!(m.image.as_deref(), Some("https://example.com/x.jpg"));
        let m = meta(b"<html><head><title>Only</title></head>", &base);
        assert_eq!((m.title.as_str(), m.image), ("Only", None));
        // A javascript: image is no image.
        let m = meta(br#"<meta property="og:image" content="javascript:alert(1)">"#, &base);
        assert_eq!(m.image, None);
    }
}
