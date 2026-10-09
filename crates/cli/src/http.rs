//! Just enough HTTP/1.1: one request per connection, with fixed-length or
//! chunked (streamed) responses. Over the daemon's Unix socket by default,
//! or to another daemon's URL (`--host`), with TLS for `https://`, or to a
//! host reached through the local daemon (on its socket, under `/h/<host>`
//! for a dial-out host, `/tunnel/<host>` for a provider host), or to a box
//! over ssh (one ssh channel per connection; `crate::ssh`).

#[cfg(unix)]
use std::os::{
    fd::{AsFd, BorrowedFd},
    unix::net::UnixStream,
};
use std::{
    io::{BufRead, BufReader, Read, Write},
    net::TcpStream,
    path::PathBuf,
    sync::Arc,
};

use anyhow::{Context, bail};

/// A daemon to talk to.
#[derive(Debug, Clone)]
pub enum Target {
    /// The local daemon's socket.
    Socket(PathBuf),
    /// Another daemon, over HTTP(S).
    Url(Url),
    /// A host reached through the local daemon (its home daemon): the local
    /// socket, with every path under this prefix: `/h/NAME` for a dial-out
    /// host, `/tunnel/NAME` for a provider host (M4b).
    Via(PathBuf, String),
    /// A box's daemon over ssh (M51): each connection is a channel running
    /// `arugula bridge` there.
    Ssh(crate::ssh::Remote),
    /// A machine in control's directory (M49), over an end-to-end channel
    /// from this CLI's `cli` device key, direct or through control's relay.
    Control(std::sync::Arc<crate::control::Link>),
}

/// `http(s)://host[:port]`, taken apart.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Url {
    pub tls: bool,
    /// Without brackets, for connecting and for TLS.
    pub host: String,
    pub port: u16,
    /// As written (`host[:port]`), for the Host header.
    pub authority: String,
}

impl Url {
    pub fn parse(s: &str) -> anyhow::Result<Self> {
        let (tls, rest) = if let Some(r) = s.strip_prefix("https://") {
            (true, r)
        } else if let Some(r) = s.strip_prefix("http://") {
            (false, r)
        } else {
            bail!("not an http(s) URL: {s}");
        };
        let authority = rest.split('/').next().unwrap_or_default().to_ascii_lowercase();
        let (host, port) = match authority.strip_prefix('[') {
            Some(v6) => {
                let (h, after) = v6.split_once(']').context("bad IPv6 address")?;
                (h.to_owned(), after.strip_prefix(':'))
            }
            None => match authority.rsplit_once(':') {
                Some((h, p)) => (h.to_owned(), Some(p)),
                None => (authority.clone(), None),
            },
        };
        let port = match port {
            Some(p) => p.parse().with_context(|| format!("bad port in {s}"))?,
            None if tls => 443,
            None => 80,
        };
        if host.is_empty() {
            bail!("no host in {s}");
        }
        Ok(Self { tls, host, port, authority })
    }
}

impl Target {
    /// The Host header, and the WebSocket URL's authority.
    pub fn authority(&self) -> &str {
        match self {
            Target::Socket(_) | Target::Via(..) | Target::Ssh(_) | Target::Control(_) => "localhost",
            Target::Url(u) => &u.authority,
        }
    }

    /// What every request path is put under.
    pub fn prefix(&self) -> &str {
        match self {
            Target::Via(_, p) => p,
            _ => "",
        }
    }

    /// A daemon on this machine over TCP (`--host http://127.0.0.1:…`)
    /// wants the local token, as any loopback caller does: the one in the
    /// default state directory's `local-token` (or
    /// $ARUGULA_LOCAL_TOKEN_FILE), when it's readable.
    pub fn local_token(&self) -> Option<String> {
        let Target::Url(u) = self else { return None };
        let host = u.host.parse::<std::net::IpAddr>().map(|ip| ip.is_loopback()).unwrap_or(u.host == "localhost");
        if !host {
            return None;
        }
        let file = std::env::var_os("ARUGULA_LOCAL_TOKEN_FILE")
            .filter(|v| !v.is_empty())
            .map(PathBuf::from)
            .or_else(|| Some(arugula_proto::dirs::default_state_dir()?.join("local-token")))?;
        std::fs::read_to_string(file).ok().map(|t| t.trim().to_owned()).filter(|t| !t.is_empty())
    }

    /// A WebSocket handshake request for this target. (Only the terminal
    /// front ends use WebSockets, and they're Unix only for now.)
    pub fn ws_request(&self) -> anyhow::Result<tungstenite::handshake::client::Request> {
        use tungstenite::client::IntoClientRequest;
        let mut req = self.ws_url().into_client_request()?;
        if let Some(t) = self.local_token() {
            req.headers_mut().insert("authorization", format!("Bearer {t}").parse()?);
        }
        Ok(req)
    }

    pub fn ws_url(&self) -> String {
        match self {
            Target::Url(u) if u.tls => format!("wss://{}/ws", u.authority),
            _ => format!("ws://{}{}/ws", self.authority(), self.prefix()),
        }
    }

    pub fn connect(&self) -> anyhow::Result<Box<dyn Stream>> {
        match self {
            #[cfg(unix)]
            Target::Socket(path) | Target::Via(path, _) => Ok(Box::new(
                UnixStream::connect(path)
                    .with_context(|| format!("can't reach arugulad at {} (is it running?)", path.display()))?,
            )),
            // Windows: the daemon's named pipe (M56).
            #[cfg(windows)]
            Target::Socket(path) | Target::Via(path, _) => Ok(Box::new(
                crate::pipe::PipeStream::connect(path)
                    .with_context(|| format!("can't reach arugulad at {} (is it running?)", path.display()))?,
            )),
            Target::Ssh(r) => Ok(Box::new(r.channel()?)),
            Target::Control(l) => Ok(Box::new(l.stream()?)),
            Target::Url(u) => u.connect(None),
        }
    }
}

impl Url {
    /// A connection to it, TLS for `https://`; within `timeout` if given
    /// (a machine that's away shouldn't hold a command up for a minute).
    pub fn connect(&self, timeout: Option<std::time::Duration>) -> anyhow::Result<Box<dyn Stream>> {
        let cant = || format!("can't reach {}:{}", self.host, self.port);
        let tcp = match timeout {
            None => TcpStream::connect((self.host.as_str(), self.port)).with_context(cant)?,
            Some(t) => {
                use std::net::ToSocketAddrs;
                let mut last = None;
                let mut got = None;
                for a in (self.host.as_str(), self.port).to_socket_addrs().with_context(cant)? {
                    match TcpStream::connect_timeout(&a, t) {
                        Ok(s) => {
                            got = Some(s);
                            break;
                        }
                        Err(e) => last = Some(e),
                    }
                }
                match (got, last) {
                    (Some(s), _) => s,
                    (None, Some(e)) => return Err(anyhow::Error::from(e).context(cant())),
                    (None, None) => bail!("{}: no address", cant()),
                }
            }
        };
        tcp.set_nodelay(true)?;
        if !self.tls {
            return Ok(Box::new(tcp));
        }
        let name = rustls::pki_types::ServerName::try_from(self.host.clone())?;
        let conn = rustls::ClientConnection::new(tls_config()?, name)?;
        Ok(Box::new(Tls(rustls::StreamOwned::new(conn, tcp))))
    }
}

/// The platform's roots first (they carry a company's own CA); the bundled
/// Mozilla roots on a machine with none (no `ca-certificates`), with a warning.
fn tls_config() -> anyhow::Result<Arc<rustls::ClientConfig>> {
    use rustls_platform_verifier::ConfigVerifierExt;
    match rustls::ClientConfig::with_platform_verifier() {
        Ok(c) => Ok(Arc::new(c)),
        Err(e) => {
            eprintln!("arugula: no system CA certificates ({e}): trusting the bundled Mozilla roots");
            let mut roots = rustls::RootCertStore::empty();
            roots.add_parsable_certificates(webpki_root_certs::TLS_SERVER_ROOT_CERTS.iter().cloned());
            Ok(Arc::new(rustls::ClientConfig::builder().with_root_certificates(roots).with_no_client_auth()))
        }
    }
}

/// A connection to a daemon, whatever it runs over.
pub trait Stream: Read + Write + Send {
    /// For polling (the terminal front ends, Unix only for now).
    #[cfg(unix)]
    fn fd(&self) -> BorrowedFd<'_>;
    /// Windows: whether a read now would return something (data, or the
    /// end), for loops that can't poll (`crate::wake`). Asked while the
    /// stream is non-blocking.
    #[cfg(windows)]
    fn readable(&self) -> bool;
    fn set_nonblocking(&self, on: bool) -> std::io::Result<()>;
    fn set_timeout(&self, t: Option<std::time::Duration>) -> std::io::Result<()>;
}

#[cfg(unix)]
impl Stream for UnixStream {
    fn fd(&self) -> BorrowedFd<'_> {
        self.as_fd()
    }
    fn set_nonblocking(&self, on: bool) -> std::io::Result<()> {
        UnixStream::set_nonblocking(self, on)
    }
    fn set_timeout(&self, t: Option<std::time::Duration>) -> std::io::Result<()> {
        self.set_read_timeout(t)?;
        self.set_write_timeout(t)
    }
}

/// A non-blocking socket's `peek`: something, or the end, is there.
#[cfg(windows)]
fn peekable(s: &TcpStream) -> bool {
    !matches!(s.peek(&mut [0u8; 1]), Err(e) if e.kind() == std::io::ErrorKind::WouldBlock)
}

impl Stream for TcpStream {
    #[cfg(unix)]
    fn fd(&self) -> BorrowedFd<'_> {
        self.as_fd()
    }
    #[cfg(windows)]
    fn readable(&self) -> bool {
        peekable(self)
    }
    fn set_nonblocking(&self, on: bool) -> std::io::Result<()> {
        TcpStream::set_nonblocking(self, on)
    }
    fn set_timeout(&self, t: Option<std::time::Duration>) -> std::io::Result<()> {
        self.set_read_timeout(t)?;
        self.set_write_timeout(t)
    }
}

struct Tls(rustls::StreamOwned<rustls::ClientConnection, TcpStream>);

impl Read for Tls {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        self.0.read(buf)
    }
}

impl Write for Tls {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        self.0.write(buf)
    }
    fn flush(&mut self) -> std::io::Result<()> {
        self.0.flush()
    }
}

impl Stream for Tls {
    #[cfg(unix)]
    fn fd(&self) -> BorrowedFd<'_> {
        self.0.sock.as_fd()
    }
    #[cfg(windows)]
    fn readable(&self) -> bool {
        // What TLS already decrypted is read before WouldBlock comes back,
        // so only the socket's bytes are left to wait for.
        peekable(&self.0.sock)
    }
    fn set_nonblocking(&self, on: bool) -> std::io::Result<()> {
        self.0.sock.set_nonblocking(on)
    }
    fn set_timeout(&self, t: Option<std::time::Duration>) -> std::io::Result<()> {
        self.0.sock.set_read_timeout(t)?;
        self.0.sock.set_write_timeout(t)
    }
}

pub struct Response {
    pub status: u16,
    reader: BufReader<Box<dyn Stream>>,
    chunked: bool,
    length: Option<usize>,
    chunk_left: usize,
    done: bool,
    /// Lowercase names.
    headers: Vec<(String, String)>,
    /// `GET /api/panes`, for an error that names the route.
    route: String,
}

pub fn request(
    target: &Target,
    method: &str,
    path: &str,
    body: Option<&serde_json::Value>,
) -> anyhow::Result<Response> {
    let body = body.map(|b| b.to_string()).unwrap_or_default();
    if agent() {
        // M36: a forge block makes an agent's writes drafts.
        return send(
            target,
            method,
            path,
            &[("Content-Type", "application/json"), ("x-arugula-agent", "1")],
            body.as_bytes(),
        );
    }
    send(target, method, path, &[("Content-Type", "application/json")], body.as_bytes())
}

/// An operation (`arugula_proto::op`): its method and path from its
/// declaration, its request as the body (none where it takes none; a GET's
/// is its query string), and the response to read as the caller likes.
pub fn send_op<O: arugula_proto::op::Op>(target: &Target, path: &O::Path, req: &O::Req) -> anyhow::Result<Response> {
    use arugula_proto::op::{Method, Request};
    let (mut route, mut body) = (O::path(path), None);
    if O::Req::without_body().is_none() {
        if O::METHOD == Method::Get {
            route.push('?');
            route.push_str(&query(req)?);
        } else {
            body = Some(serde_json::to_value(req)?);
        }
    }
    request(target, O::METHOD.as_str(), &route, body.as_ref())
}

/// A request as a query string: its fields in order, a `None` left out,
/// each value percent-encoded as `enc` does (not as a form, which writes a
/// space as `+`) and a float as `Display` writes it (`30`, where the form
/// encoder would write `30.0`).
fn query(req: &impl serde::Serialize) -> anyhow::Result<String> {
    let pairs: Vec<(String, String)> = serde_urlencoded::from_str(&serde_urlencoded::to_string(req)?)?;
    let json = serde_json::to_value(req)?;
    let value = |k: &str, form: &str| match json.get(k) {
        Some(serde_json::Value::Number(n)) if n.is_f64() => {
            n.as_f64().map_or_else(|| form.to_owned(), |f| f.to_string())
        }
        _ => form.to_owned(),
    };
    Ok(pairs.iter().map(|(k, v)| format!("{}={}", enc(k), enc(&value(k, v)))).collect::<Vec<_>>().join("&"))
}

/// An operation with a JSON body of the caller's own, where its typed
/// request would refuse what the daemon words better (`mouse`'s button, an
/// attention state typed on the command line).
pub fn request_op<O: arugula_proto::op::Op>(
    target: &Target,
    path: &O::Path,
    body: &serde_json::Value,
) -> anyhow::Result<Response> {
    request(target, O::METHOD.as_str(), &O::path(path), Some(body))
}

/// An operation's answer, with the body as the daemon sent it too (`parse_raw`).
pub fn call_raw<O: arugula_proto::op::Op>(
    target: &Target,
    path: &O::Path,
    req: &O::Req,
) -> anyhow::Result<(O::Res, serde_json::Value)> {
    send_op::<O>(target, path, req)?.parse_raw::<O::Res>()
}

/// `call_raw`, told apart by where it failed: the outer error is the
/// connection's (refused, reset, closed before a status line or mid-body),
/// which a daemon that is restarting causes; the inner is the daemon's
/// answer, a status like 404 or 410 included.
pub fn call_raw_conn<O: arugula_proto::op::Op>(
    target: &Target,
    path: &O::Path,
    req: &O::Req,
) -> anyhow::Result<anyhow::Result<(O::Res, serde_json::Value)>> {
    let mut resp = send_op::<O>(target, path, req)?;
    let (status, route) = (resp.status, resp.route.clone());
    let mut text = String::new();
    resp.read_to_string(&mut text)?;
    Ok(parse_body(status, &route, &text))
}

/// An operation's answer (`parse`).
pub fn call<O: arugula_proto::op::Op>(target: &Target, path: &O::Path, req: &O::Req) -> anyhow::Result<O::Res> {
    Ok(call_raw::<O>(target, path, req)?.0)
}

/// `request` with a typed body (a request type from `arugula_proto::api`),
/// which goes out as the same JSON the type serializes to.
pub fn request_as(target: &Target, method: &str, path: &str, body: &impl serde::Serialize) -> anyhow::Result<Response> {
    request(target, method, path, Some(&serde_json::to_value(body)?))
}

/// Whether an agent runs this (Claude Code sets CLAUDECODE; others
/// AI_AGENT): a courtesy the daemon honours, not a boundary.
pub fn agent() -> bool {
    ["CLAUDECODE", "AI_AGENT"].iter().any(|k| std::env::var_os(k).is_some_and(|v| !v.is_empty() && v != "0"))
}

/// A request with headers of the caller's own (MCP's bridge).
pub fn send(
    target: &Target,
    method: &str,
    path: &str,
    headers: &[(&str, &str)],
    body: &[u8],
) -> anyhow::Result<Response> {
    let stream = target.connect()?;
    let token;
    let mut all: Vec<(&str, &str)> = headers.to_vec();
    if !headers.iter().any(|(k, _)| k.eq_ignore_ascii_case("authorization"))
        && let Some(t) = target.local_token()
    {
        token = format!("Bearer {t}");
        all.push(("Authorization", &token));
    }
    send_on(stream, method, &format!("{}{path}", target.prefix()), target.authority(), &all, body)
}

/// One request on a connection of the caller's, with exactly these headers
/// (no local token): control's API from the CLI (M49).
pub fn send_on(
    mut stream: Box<dyn Stream>,
    method: &str,
    path: &str,
    authority: &str,
    headers: &[(&str, &str)],
    body: &[u8],
) -> anyhow::Result<Response> {
    let mut head = format!(
        "{method} {path} HTTP/1.1\r\nHost: {authority}\r\nConnection: close\r\nContent-Length: {}\r\n",
        body.len()
    );
    for (k, v) in headers {
        head.push_str(&format!("{k}: {v}\r\n"));
    }
    head.push_str("\r\n");
    stream.write_all(head.as_bytes())?;
    stream.write_all(body)?;
    stream.flush()?;
    let mut reader = BufReader::new(stream);
    let mut line = String::new();
    reader.read_line(&mut line)?;
    let status: u16 = line.split_whitespace().nth(1).and_then(|s| s.parse().ok()).context("bad HTTP response")?;
    let (mut chunked, mut length, mut headers) = (false, None, Vec::new());
    loop {
        line.clear();
        reader.read_line(&mut line)?;
        let l = line.trim_end();
        if l.is_empty() {
            break;
        }
        let (k, v) = l.split_once(':').unwrap_or((l, ""));
        headers.push((k.trim().to_ascii_lowercase(), v.trim().to_owned()));
        match k.trim().to_ascii_lowercase().as_str() {
            "transfer-encoding" => chunked = v.to_ascii_lowercase().contains("chunked"),
            "content-length" => length = v.trim().parse().ok(),
            _ => {}
        }
    }
    let route = format!("{method} {}", path.split('?').next().unwrap_or(path));
    Ok(Response { status, reader, chunked, length, chunk_left: 0, done: false, headers, route })
}

impl Read for Response {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        if self.done {
            return Ok(0);
        }
        if !self.chunked {
            let want = match self.length {
                Some(0) => return Ok(0),
                Some(n) => buf.len().min(n),
                None => buf.len(),
            };
            let n = self.reader.read(&mut buf[..want])?;
            if let Some(len) = &mut self.length {
                *len -= n;
            }
            return Ok(n);
        }
        if self.chunk_left == 0 {
            let mut size = String::new();
            self.reader.read_line(&mut size)?;
            if size.trim().is_empty() {
                // The CRLF after a chunk.
                size.clear();
                self.reader.read_line(&mut size)?;
            }
            let n = usize::from_str_radix(size.trim().split(';').next().unwrap_or("0"), 16).unwrap_or(0);
            if n == 0 {
                self.done = true;
                return Ok(0);
            }
            self.chunk_left = n;
        }
        let want = buf.len().min(self.chunk_left);
        let n = self.reader.read(&mut buf[..want])?;
        self.chunk_left -= n;
        Ok(n)
    }
}

impl Response {
    /// A header's value (`name` in lowercase).
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers.iter().find(|(k, _)| k == name).map(|(_, v)| v.as_str())
    }

    pub fn bytes(mut self) -> anyhow::Result<Vec<u8>> {
        let mut b = Vec::new();
        self.read_to_end(&mut b)?;
        Ok(b)
    }

    pub fn text(mut self) -> anyhow::Result<String> {
        let mut s = String::new();
        self.read_to_string(&mut s)?;
        Ok(s)
    }

    /// The body as JSON, or the API's error as an error.
    pub fn json(self) -> anyhow::Result<serde_json::Value> {
        let status = self.status;
        let text = self.text()?;
        let v: serde_json::Value = serde_json::from_str(&text).unwrap_or(serde_json::Value::String(text));
        if !(200..300).contains(&status) {
            bail!("{}", v.get("error").and_then(|e| e.as_str()).unwrap_or(&v.to_string()));
        }
        Ok(v)
    }

    /// The body as a `T` (a type from `arugula_proto::api`), or the API's
    /// error as an error, as `json` does. A body that isn't a `T` is an error
    /// naming the route. Fields a `T` leaves out are the type's to default:
    /// the daemon may be older or newer than this CLI.
    pub fn parse<T: serde::de::DeserializeOwned>(self) -> anyhow::Result<T> {
        Ok(self.parse_raw::<T>()?.0)
    }

    /// Like `parse`, with the body as the daemon sent it too, for a command
    /// that prints the daemon's answer (`--json`) as it came.
    pub fn parse_raw<T: serde::de::DeserializeOwned>(self) -> anyhow::Result<(T, serde_json::Value)> {
        let (status, route) = (self.status, self.route.clone());
        let text = self.text()?;
        parse_body(status, &route, &text)
    }

    /// Fail with the API's error message on a non-2xx status.
    pub fn ok(self) -> anyhow::Result<Self> {
        if (200..300).contains(&self.status) {
            return Ok(self);
        }
        let status = self.status;
        let text = self.text()?;
        let msg = serde_json::from_str::<serde_json::Value>(&text)
            .ok()
            .and_then(|v| v.get("error").and_then(|e| e.as_str()).map(str::to_owned))
            .unwrap_or(text);
        bail!("{msg} (HTTP {status})")
    }
}

/// `Response::parse_raw`, on a body already read.
pub(crate) fn parse_body<T: serde::de::DeserializeOwned>(
    status: u16,
    route: &str,
    text: &str,
) -> anyhow::Result<(T, serde_json::Value)> {
    let v: serde_json::Value =
        serde_json::from_str(text).unwrap_or_else(|_| serde_json::Value::String(text.to_owned()));
    if !(200..300).contains(&status) {
        bail!("{}", v.get("error").and_then(|e| e.as_str()).unwrap_or(&v.to_string()));
    }
    let typed = serde_json::from_value(v.clone())
        .map_err(|e| anyhow::anyhow!("{route}: the daemon's answer isn't what this arugula expects: {e}"))?;
    Ok((typed, v))
}

/// Percent-encode a query value.
pub fn enc(s: &str) -> String {
    let mut out = String::new();
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || b"-_.~".contains(&b) {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

#[cfg(windows)]
impl Stream for crate::pipe::PipeStream {
    fn readable(&self) -> bool {
        crate::pipe::PipeStream::readable(self)
    }
    fn set_nonblocking(&self, on: bool) -> std::io::Result<()> {
        crate::pipe::PipeStream::set_nonblocking(self, on);
        Ok(())
    }
    fn set_timeout(&self, t: Option<std::time::Duration>) -> std::io::Result<()> {
        crate::pipe::PipeStream::set_timeout(self, t);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::{Url, enc, query};
    use arugula_proto::api::WaitRequest;

    /// `arugula wait`'s query string as it was built by hand before
    /// `pane.wait` (#573).
    fn by_hand(until: &str, re: Option<&str>, timeout: Option<f64>) -> String {
        let mut q = match re {
            Some(re) => format!("until={until}&re={}", enc(re)),
            None => format!("until={until}"),
        };
        if let Some(t) = timeout {
            q.push_str(&format!("&timeout={t}"));
        }
        q
    }

    #[test]
    fn a_wait_query_is_the_one_written_by_hand() {
        let res = [None, Some("done"), Some("a b*c~d/é&=+%"), Some("")];
        let timeouts = [None, Some(30.0), Some(0.5), Some(2.25), Some(1e-7), Some(1e21), Some(0.0), Some(86400.0)];
        for until in ["command-end", "exit", "match", "idle", "needs-input"] {
            for re in res {
                for timeout in timeouts {
                    let req = WaitRequest { until: until.into(), re: re.map(Into::into), timeout };
                    assert_eq!(query(&req).unwrap(), by_hand(until, re, timeout), "{req:?}");
                }
            }
        }
    }

    /// The pane verbs' routes and bodies as the commands wrote them by hand
    /// before they were operations (#574): the same path, the same JSON, and
    /// no query string on a GET that takes none.
    #[test]
    fn a_pane_verb_goes_out_as_it_was_written_by_hand() {
        use arugula_proto::{
            api::{Empty, KeysRequest, SendRequest},
            op::{Op, Request, ops::*},
        };
        let pane = 7;
        assert_eq!(PaneSend::path(&pane), format!("/api/panes/{pane}/send"));
        assert_eq!(PaneKeys::path(&pane), format!("/api/panes/{pane}/keys"));
        assert_eq!(PaneMouse::path(&pane), format!("/api/panes/{pane}/mouse"));
        assert_eq!(PaneAttention::path(&pane), format!("/api/panes/{pane}/attention"));
        for path in
            [PaneProcess::path(&pane), PaneDetection::path(&pane), PaneDrivers::path(&pane), PaneDiffOf::path(&pane)]
        {
            assert!(path.starts_with("/api/panes/7/") && !path.contains('?'), "{path}");
        }
        assert_eq!(PaneProcess::path(&pane), format!("/api/panes/{pane}/process"));
        assert_eq!(PaneDetection::path(&pane), format!("/api/panes/{pane}/detection"));
        assert_eq!(PaneDrivers::path(&pane), format!("/api/panes/{pane}/drivers"));
        // A GET with no query sends no body and no `?`.
        assert!(Empty::without_body().is_some());
        let send = |text: &str, enter| serde_json::to_value(SendRequest { text: text.into(), enter }).unwrap();
        assert_eq!(send("ls", true).to_string(), r#"{"enter":true,"text":"ls"}"#);
        assert_eq!(send("", false).to_string(), r#"{"enter":false,"text":""}"#);
        let keys = serde_json::to_value(KeysRequest { keys: vec!["C-c".into(), "Up".into()] }).unwrap();
        assert_eq!(keys.to_string(), r#"{"keys":["C-c","Up"]}"#);
    }

    #[test]
    fn urls() {
        let u = Url::parse("https://box.example.ts.net").unwrap();
        assert_eq!(
            (u.tls, u.host.as_str(), u.port, u.authority.as_str()),
            (true, "box.example.ts.net", 443, "box.example.ts.net")
        );
        let u = Url::parse("http://127.0.0.1:7691/").unwrap();
        assert_eq!(
            (u.tls, u.host.as_str(), u.port, u.authority.as_str()),
            (false, "127.0.0.1", 7691, "127.0.0.1:7691")
        );
        let u = Url::parse("http://[::1]:7681").unwrap();
        assert_eq!((u.host.as_str(), u.port, u.authority.as_str()), ("::1", 7681, "[::1]:7681"));
        assert!(Url::parse("ftp://x").is_err());
        assert!(Url::parse("box").is_err());
        assert!(Url::parse("http://x:notaport").is_err());
    }
}
