//! A socket to control, with TLS for `https://`, and just enough HTTP/1.1 for
//! one request on one connection: what the requests in `control` need, and the
//! socket `link` opens its WebSocket on.

use std::{
    io::{self, BufRead, BufReader, Read, Write},
    net::TcpStream,
    os::fd::{AsRawFd, RawFd},
    sync::{Arc, OnceLock},
    time::Duration,
};

use anyhow::{Context, Result, bail};
use rustls_platform_verifier::BuilderVerifierExt;

const CONNECT: Duration = Duration::from_secs(10);
const ANSWER: Duration = Duration::from_secs(30);

/// Where control is.
#[derive(Clone, Debug)]
pub(crate) struct Site {
    pub tls: bool,
    pub host: String,
    /// `host:port`, or `host` on the scheme's own port.
    pub authority: String,
}

impl Site {
    pub fn parse(url: &str) -> Result<Self> {
        let (tls, rest) = match url.split_once("://") {
            Some(("https", r)) => (true, r),
            Some(("http", r)) => (false, r),
            _ => bail!("control's URL is http:// or https://: {url}"),
        };
        let authority = rest.trim_end_matches('/').to_owned();
        if authority.is_empty() || authority.contains('/') {
            bail!("control's URL has no path: {url}");
        }
        let host = authority.rsplit_once(':').map_or(authority.as_str(), |(h, _)| h).to_owned();
        Ok(Self { tls, host, authority })
    }

    fn port(&self) -> u16 {
        self.authority.rsplit_once(':').and_then(|(_, p)| p.parse().ok()).unwrap_or(if self.tls { 443 } else { 80 })
    }

    pub fn connect(&self) -> Result<Sock> {
        use std::net::ToSocketAddrs;
        let addr = (self.host.as_str(), self.port())
            .to_socket_addrs()
            .with_context(|| format!("finding {}", self.host))?
            .next()
            .with_context(|| format!("no address for {}", self.host))?;
        let tcp = TcpStream::connect_timeout(&addr, CONNECT).with_context(|| format!("connecting to {}", self.host))?;
        tcp.set_nodelay(true)?;
        tcp.set_read_timeout(Some(ANSWER))?;
        tcp.set_write_timeout(Some(ANSWER))?;
        if !self.tls {
            return Ok(Sock::Plain(tcp));
        }
        let name = rustls::pki_types::ServerName::try_from(self.host.clone())?;
        let conn = rustls::ClientConnection::new(tls()?, name)?;
        Ok(Sock::Tls(Box::new(rustls::StreamOwned::new(conn, tcp))))
    }

    /// One request, its whole answer. Control's JSON and the like: not a
    /// stream, which has no end.
    pub fn request(&self, method: &str, path: &str, headers: &[(&str, &str)], body: &[u8]) -> Result<(u16, Vec<u8>)> {
        let mut s = self.connect()?;
        let mut head = format!("{method} {path} HTTP/1.1\r\nHost: {}\r\nConnection: close\r\n", self.authority);
        for (k, v) in headers {
            head.push_str(&format!("{k}: {v}\r\n"));
        }
        head.push_str(&format!("Content-Length: {}\r\n\r\n", body.len()));
        s.write_all(head.as_bytes())?;
        s.write_all(body)?;
        s.flush()?;
        read_answer(BufReader::new(s))
    }
}

/// The platform's verifier: on an iPhone, the system's trust store.
fn tls() -> Result<Arc<rustls::ClientConfig>> {
    static CONFIG: OnceLock<Arc<rustls::ClientConfig>> = OnceLock::new();
    if let Some(c) = CONFIG.get() {
        return Ok(c.clone());
    }
    let c = rustls::ClientConfig::builder().with_platform_verifier()?.with_no_client_auth();
    Ok(CONFIG.get_or_init(|| Arc::new(c)).clone())
}

fn read_answer(mut r: impl BufRead) -> Result<(u16, Vec<u8>)> {
    let mut line = String::new();
    r.read_line(&mut line).context("reading control's answer")?;
    let status: u16 = line.split_whitespace().nth(1).and_then(|s| s.parse().ok()).context("not an HTTP answer")?;
    let (mut length, mut chunked) = (None, false);
    loop {
        line.clear();
        if r.read_line(&mut line)? == 0 {
            bail!("control's answer ended in its headers");
        }
        let l = line.trim_end();
        if l.is_empty() {
            break;
        }
        if let Some((k, v)) = l.split_once(':') {
            let v = v.trim();
            match k.to_ascii_lowercase().as_str() {
                "content-length" => length = v.parse::<usize>().ok(),
                "transfer-encoding" => chunked = v.eq_ignore_ascii_case("chunked"),
                _ => {}
            }
        }
    }
    let mut body = Vec::new();
    if chunked {
        loop {
            line.clear();
            r.read_line(&mut line)?;
            let n = usize::from_str_radix(line.trim().split(';').next().unwrap_or(""), 16).context("a chunk's size")?;
            if n == 0 {
                break;
            }
            let at = body.len();
            body.resize(at + n, 0);
            r.read_exact(&mut body[at..])?;
            line.clear();
            r.read_line(&mut line)?;
        }
    } else if let Some(n) = length {
        body.resize(n, 0);
        r.read_exact(&mut body)?;
    } else {
        r.read_to_end(&mut body)?;
    }
    Ok((status, body))
}

/// A socket, plain or under TLS.
pub(crate) enum Sock {
    Plain(TcpStream),
    Tls(Box<rustls::StreamOwned<rustls::ClientConnection, TcpStream>>),
}

impl Sock {
    fn tcp(&self) -> &TcpStream {
        match self {
            Sock::Plain(s) => s,
            Sock::Tls(s) => &s.sock,
        }
    }

    pub fn nonblocking(&self) -> io::Result<()> {
        self.tcp().set_nonblocking(true)
    }

    pub fn fd(&self) -> RawFd {
        self.tcp().as_raw_fd()
    }
}

impl Read for Sock {
    fn read(&mut self, b: &mut [u8]) -> io::Result<usize> {
        match self {
            Sock::Plain(s) => s.read(b),
            Sock::Tls(s) => s.read(b),
        }
    }
}

impl Write for Sock {
    fn write(&mut self, b: &[u8]) -> io::Result<usize> {
        match self {
            Sock::Plain(s) => s.write(b),
            Sock::Tls(s) => s.write(b),
        }
    }

    fn flush(&mut self) -> io::Result<()> {
        match self {
            Sock::Plain(s) => s.flush(),
            Sock::Tls(s) => s.flush(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_answer_with_a_length_chunks_or_neither() {
        let plain = b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\nhi and more".as_slice();
        assert_eq!(read_answer(plain).unwrap(), (200, b"hi".to_vec()));
        let chunks =
            b"HTTP/1.1 201 Created\r\nTransfer-Encoding: chunked\r\n\r\n2\r\nhi\r\n3\r\nyou\r\n0\r\n\r\n".as_slice();
        assert_eq!(read_answer(chunks).unwrap(), (201, b"hiyou".to_vec()));
        let until_closed = b"HTTP/1.1 404 Not Found\r\n\r\nno".as_slice();
        assert_eq!(read_answer(until_closed).unwrap(), (404, b"no".to_vec()));
    }

    #[test]
    fn sites() {
        let s = Site::parse("https://control.arugula.io/").unwrap();
        assert!(s.tls);
        assert_eq!((s.host.as_str(), s.port()), ("control.arugula.io", 443));
        let s = Site::parse("http://10.0.0.1:8080").unwrap();
        assert_eq!((s.host.as_str(), s.port()), ("10.0.0.1", 8080));
        assert!(Site::parse("ftp://x").is_err());
        assert!(Site::parse("https://x/y").is_err());
    }
}
