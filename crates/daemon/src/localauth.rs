//! Who on this machine may use the daemon's TCP port.
//!
//! Loopback is shared by every account and every program on the machine,
//! so a connection from it says nothing about who is asking. The Unix
//! socket does (its directory is the owner's, 0700); TCP needs a secret:
//!
//! - **The local token**, `local-token` in the state directory (0600,
//!   made at the first start and kept, so browsers stay signed in across
//!   restarts). Programs on loopback that tailscaled didn't vouch for show
//!   it as `Authorization: Bearer …`.
//! - **Browsers show the page key** ([`page_key`], derived from the token,
//!   and not accepted as a bearer) in `X-Arugula-Key`, and in the
//!   WebSocket's URL (`?arugula_key=`, which a browser can't send as a
//!   header there). The page keeps it in its own origin's storage, which
//!   is per port. It is never a cookie: cookies aren't kept apart by port,
//!   so every server on 127.0.0.1 a browser visited would get one.
//! - **Browsers get the key from a sign-in link**,
//!   `http://127.0.0.1:PORT/auth?token=…`: `arugula web` asks the daemon
//!   for it over the socket and opens it, and the desktop app reads the
//!   file. The link's page stores the key and moves on to the app, so the
//!   token leaves the address bar at once. A browser still holding an
//!   older link's cookie (the token) is signed in the same way from it,
//!   once, and the cookie is removed: only for [`LEGACY_DAYS`] after the
//!   first start that found a token made before (an older daemon's,
//!   whose links set that cookie). Then, and on a new install, a cookie
//!   counts for nothing anywhere.
//! - **The app's own files** (`/`, `/assets/…`) need neither: they're the
//!   same for everyone, and a page load can't carry a header.
//! - **`tailscale serve`'s identity header** is only believed from
//!   tailscaled: the connection's other end must belong to root
//!   (tailscaled) or to the owner's own account (`loopback_uid`), so
//!   another account can't claim to be the owner by sending the header.
//!   Linux reads that from `/proc/net/tcp`, macOS from the kernel's TCP
//!   table (`net.inet.tcp.pcblist_n`, what `netstat` reads) and Windows
//!   from its TCP table. Where the other end can't be told, the header
//!   isn't believed: those machines reach the tailnet through a direct
//!   tailnet address (WhoIs) or the local token, never serve's header.

use std::{
    io::{self, Read, Write},
    net::SocketAddr,
    path::{Path, PathBuf},
};

/// The token's file in the state directory.
pub const FILE: &str = "local-token";
/// Another file instead (tests: every daemon of a run shares one).
pub const ENV_FILE: &str = "ARUGULA_LOCAL_TOKEN_FILE";
/// The sign-in link's path.
pub const AUTH_PATH: &str = "/auth";

/// Where the token is for a daemon with this state directory.
pub fn path(state_dir: &Path) -> PathBuf {
    match std::env::var_os(ENV_FILE).filter(|v| !v.is_empty()) {
        Some(p) => PathBuf::from(p),
        None => state_dir.join(FILE),
    }
}

/// How long after an upgrade a browser holding an older sign-in link's
/// cookie is signed in from it once (`server::signin`).
pub const LEGACY_DAYS: u64 = 14;
/// When that ends, as Unix seconds (0: never began), in the state directory.
const LEGACY_FILE: &str = "legacy-signin-until";

/// Until when (Unix seconds) an older sign-in link's cookie may sign a
/// browser in once: [`LEGACY_DAYS`] from the first start that found a token
/// made before (`token_existed`), kept in the state directory so restarts
/// don't extend it. None for a token this version made, and once it's past.
pub fn legacy_until(state_dir: &Path, token_existed: bool) -> Option<u64> {
    let file = state_dir.join(LEGACY_FILE);
    let until = match std::fs::read_to_string(&file) {
        Ok(s) => s.trim().parse().unwrap_or(0),
        Err(_) => {
            let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs());
            let until = if token_existed { now + LEGACY_DAYS * 86_400 } else { 0 };
            let _ = std::fs::write(&file, until.to_string());
            until
        }
    };
    (until > 0).then_some(until)
}

/// The page key for `token`: what this daemon's page shows instead of the
/// token. A hash of it, so a page that leaks its key leaks nothing a
/// program could use as a bearer, and a new token makes a new key.
pub fn page_key(token: &str) -> String {
    use sha2::{Digest, Sha256};
    let mut h = Sha256::new();
    h.update(b"arugula page key\0");
    h.update(token.trim().as_bytes());
    format!("ilp_{}", hex::encode(h.finalize()))
}

/// The token in `path`, made (0600, owner only) if there is none. One
/// another account could read or swap is refused.
pub fn load_or_create(path: &Path) -> anyhow::Result<String> {
    match std::fs::symlink_metadata(path) {
        Ok(m) => {
            if !m.file_type().is_file() {
                anyhow::bail!("{} isn't a plain file", path.display());
            }
            if !crate::perm::mine(&m) {
                anyhow::bail!("{} belongs to another account", path.display());
            }
            if crate::perm::mode(&m) & 0o077 != 0 {
                crate::perm::set(path, 0o600)?;
            }
            let mut s = String::new();
            std::fs::File::open(path)?.read_to_string(&mut s)?;
            let s = s.trim().to_owned();
            if s.len() < 32 {
                anyhow::bail!("{} is too short to be a token: delete it and restart", path.display());
            }
            Ok(s)
        }
        Err(e) if e.kind() == io::ErrorKind::NotFound => {
            if let Some(dir) = path.parent() {
                crate::store::private_dir(dir)?;
            }
            let token = format!("ilt_{}", hex::encode(crate::push::random::<32>()));
            // Written aside and linked in: whoever starts beside us (two
            // daemons sharing a file) reads a whole token, never half of one.
            let tmp = path.with_extension(format!("tmp{}", std::process::id()));
            let mut f =
                crate::perm::open_mode(std::fs::OpenOptions::new().write(true).create_new(true), 0o600).open(&tmp)?;
            f.write_all(token.as_bytes())?;
            f.sync_all()?;
            let mut linked = std::fs::hard_link(&tmp, path);
            // Android's SELinux refuses hard links (S31). Renaming in loses
            // the "theirs wins" race, which a phone's one daemon never runs.
            if cfg!(target_os = "android")
                && linked.as_ref().is_err_and(|e| e.kind() == io::ErrorKind::PermissionDenied)
            {
                linked = std::fs::rename(&tmp, path);
            }
            let _ = std::fs::remove_file(&tmp);
            match linked {
                Ok(()) => Ok(token),
                // Someone else made it first: theirs.
                Err(e) if e.kind() == io::ErrorKind::AlreadyExists => load_or_create(path),
                Err(e) => Err(e.into()),
            }
        }
        Err(e) => Err(e.into()),
    }
}

/// Whether a loopback connection carrying `tailscale serve`'s identity
/// header can be serve's: its other end is tailscaled's (root) or the
/// owner's own. Where that can't be told, it isn't.
#[cfg(unix)]
pub fn serve_peer_ok(peer: SocketAddr, port: u16) -> bool {
    match loopback_uid(peer, port) {
        Some(uid) => uid == 0 || uid == nix::unistd::geteuid().as_raw(),
        None => false,
    }
}

/// Windows: its TCP table names the client's process, and so its user
/// (tailscaled runs as SYSTEM).
#[cfg(windows)]
pub fn serve_peer_ok(peer: SocketAddr, port: u16) -> bool {
    crate::pipe::loopback_peer_ok(peer, port)
}

#[cfg(any(target_os = "linux", target_os = "android"))]
/// The account that owns the client end of a loopback TCP connection to
/// our `port` (Linux: `/proc/net/tcp{,6}`, which has both ends).
pub fn loopback_uid(peer: SocketAddr, port: u16) -> Option<u32> {
    let tables: Vec<String> =
        ["/proc/net/tcp", "/proc/net/tcp6"].iter().filter_map(|f| std::fs::read_to_string(f).ok()).collect();
    one_uid(tables.iter().flat_map(|t| uids_in(t, peer, port)))
}

#[cfg(target_os = "macos")]
/// The account that owns the client end of a loopback TCP connection to
/// our `port` (macOS: the kernel's TCP table, `net.inet.tcp.pcblist_n`,
/// which any account may read, as `netstat` does).
pub fn loopback_uid(peer: SocketAddr, port: u16) -> Option<u32> {
    let table = pcblist()?;
    one_uid(pcb_uids(&table, peer, port))
}

#[cfg(all(unix, not(any(target_os = "linux", target_os = "android", target_os = "macos"))))]
/// Elsewhere there is no table we read: nobody is known.
pub fn loopback_uid(_peer: SocketAddr, _port: u16) -> Option<u32> {
    None
}

/// The one account every live entry for the connection names. None when
/// there's no entry, or entries that disagree (a port reused while an old
/// connection lingers): then nobody is believed.
#[cfg(any(unix, test))]
fn one_uid(mut uids: impl Iterator<Item = u32>) -> Option<u32> {
    let first = uids.next()?;
    uids.all(|u| u == first).then_some(first)
}

#[cfg(target_os = "macos")]
fn pcblist() -> Option<Vec<u8>> {
    let name = c"net.inet.tcp.pcblist_n";
    let mut len = 0usize;
    // SAFETY: a size query (no buffer), then a read into a buffer of the
    // length given; sysctlbyname writes at most `got` bytes.
    unsafe {
        if libc::sysctlbyname(name.as_ptr(), std::ptr::null_mut(), &mut len, std::ptr::null_mut(), 0) != 0 {
            return None;
        }
        // The table can grow between the two calls: room to spare, and again.
        for _ in 0..4 {
            let mut buf = vec![0u8; len + len / 4 + 4096];
            let mut got = buf.len();
            let r = libc::sysctlbyname(name.as_ptr(), buf.as_mut_ptr().cast(), &mut got, std::ptr::null_mut(), 0);
            if r == 0 {
                buf.truncate(got);
                return Some(buf);
            }
            if io::Error::last_os_error().raw_os_error() != Some(libc::ENOMEM) {
                return None;
            }
            len = buf.len() * 2;
        }
    }
    None
}

/// macOS's `pcblist_n`: a `struct xinpgen`, then for each connection a run
/// of records (`xinpcb_n`, `xsocket_n`, buffers, stats, `xtcpcb_n`), each
/// starting with its length and kind and padded to 8 bytes, then a closing
/// `xinpgen`. The owners (`so_uid`) of the live sockets whose local end is
/// `peer` and whose other end is our `port`. Offsets are xnu's (`in_pcb.h`,
/// `socketvar.h`, both `#pragma pack(4)`).
#[cfg(any(target_os = "macos", test))]
fn pcb_uids(buf: &[u8], peer: SocketAddr, port: u16) -> impl Iterator<Item = u32> + '_ {
    const XSO_SOCKET: u32 = 0x001;
    const XSO_INPCB: u32 = 0x010;
    const XINPGEN_LEN: usize = 24;
    let u32_at = |b: &[u8], o: usize| b.get(o..o + 4).map(|s| u32::from_ne_bytes(s.try_into().unwrap()));
    let round = |n: usize| (n + 7) & !7;
    let want = (peer.ip().to_canonical(), peer.port());
    let mut at = u32_at(buf, 0).map_or(buf.len(), |l| round(l as usize));
    // The current connection's ends, once its xinpcb_n is read.
    let mut ends: Option<(SocketAddr, u16)> = None;
    std::iter::from_fn(move || {
        loop {
            let len = u32_at(buf, at)? as usize;
            if len <= XINPGEN_LEN {
                return None;
            }
            let rec = buf.get(at..at + len)?;
            at += round(len);
            match u32_at(rec, 4)? {
                XSO_INPCB => ends = pcb_ends(rec),
                XSO_SOCKET => {
                    // A connection lingering without its socket (TIME_WAIT)
                    // has an empty record: no owner to name.
                    let live = rec.get(8..16).is_some_and(|h| h.iter().any(|&b| b != 0));
                    let Some((local, fport)) = ends.take() else { continue };
                    if live && (local.ip().to_canonical(), local.port()) == want && fport == port {
                        return u32_at(rec, 64);
                    }
                }
                _ => {}
            }
        }
    })
}

/// An `xinpcb_n`'s local address and port, and its foreign port.
#[cfg(any(target_os = "macos", test))]
fn pcb_ends(rec: &[u8]) -> Option<(SocketAddr, u16)> {
    const INP_IPV4: u8 = 0x1;
    const INP_IPV6: u8 = 0x2;
    let port_at = |o: usize| rec.get(o..o + 2).map(|s| u16::from_be_bytes([s[0], s[1]]));
    let (fport, lport) = (port_at(16)?, port_at(18)?);
    let vflag = *rec.get(44)?;
    // inp_dependladdr: an in_addr_4in6 (12 bytes of padding, then the
    // address) or an in6_addr.
    let ip = if vflag & INP_IPV4 != 0 {
        let b: [u8; 4] = rec.get(76..80)?.try_into().ok()?;
        std::net::IpAddr::from(b)
    } else if vflag & INP_IPV6 != 0 {
        let b: [u8; 16] = rec.get(64..80)?.try_into().ok()?;
        std::net::IpAddr::from(b)
    } else {
        return None;
    };
    Some((SocketAddr::new(ip, lport), fport))
}

/// The owners of the client end in one `/proc/net/tcp` table, skipping
/// connections in TIME_WAIT or CLOSE, which belong to nobody (uid 0).
#[cfg(any(target_os = "linux", target_os = "android", test))]
fn uids_in<'t>(table: &'t str, peer: SocketAddr, port: u16) -> impl Iterator<Item = u32> + 't {
    const TIME_WAIT: &str = "06";
    const CLOSE: &str = "07";
    let want = (peer.ip().to_canonical(), peer.port());
    table.lines().skip(1).filter_map(move |line| {
        let f: Vec<&str> = line.split_whitespace().collect();
        let local = parse_addr(f.get(1)?)?;
        let remote = parse_addr(f.get(2)?)?;
        let state = *f.get(3)?;
        ((local.ip().to_canonical(), local.port()) == want
            && remote.port() == port
            && state != TIME_WAIT
            && state != CLOSE)
            .then(|| f.get(7)?.parse().ok())?
    })
}

#[cfg(test)]
fn uid_in(table: &str, peer: SocketAddr, port: u16) -> Option<u32> {
    one_uid(uids_in(table, peer, port))
}

/// `0100007F:1E01` (IPv4) or 32 hex digits and a port (IPv6): each 32-bit
/// word as the kernel holds it, printed as a native number.
pub(crate) fn parse_addr(s: &str) -> Option<SocketAddr> {
    let (ip, port) = s.split_once(':')?;
    let port = u16::from_str_radix(port, 16).ok()?;
    let word = |h: &str| u32::from_str_radix(h, 16).ok().map(u32::to_ne_bytes);
    let ip = match ip.len() {
        8 => std::net::IpAddr::V4(std::net::Ipv4Addr::from(word(ip)?)),
        32 => {
            let mut b = [0u8; 16];
            for i in 0..4 {
                b[4 * i..4 * i + 4].copy_from_slice(&word(&ip[8 * i..8 * i + 8])?);
            }
            std::net::IpAddr::V6(std::net::Ipv6Addr::from(b))
        }
        _ => return None,
    };
    Some(SocketAddr::new(ip, port))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[cfg(unix)]
    fn the_token_file_is_private_and_kept() {
        use std::os::unix::fs::{MetadataExt, PermissionsExt};
        let dir = std::env::temp_dir().join(format!("ilg-localauth-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let p = dir.join(FILE);
        let t = load_or_create(&p).unwrap();
        assert!(t.starts_with("ilt_") && t.len() == 68, "{t}");
        assert_eq!(std::fs::metadata(&p).unwrap().mode() & 0o777, 0o600);
        assert_eq!(std::fs::metadata(&dir).unwrap().mode() & 0o777, 0o700);
        assert_eq!(load_or_create(&p).unwrap(), t, "kept across restarts");
        // Made readable by others: closed again.
        std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o644)).unwrap();
        assert_eq!(load_or_create(&p).unwrap(), t);
        assert_eq!(std::fs::metadata(&p).unwrap().mode() & 0o777, 0o600);
        // A link isn't followed.
        let link = dir.join("link");
        std::os::unix::fs::symlink(&p, &link).unwrap();
        assert!(load_or_create(&link).is_err());
        std::fs::write(&p, "short").unwrap();
        assert!(load_or_create(&p).is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn an_older_links_cookie_counts_only_after_an_upgrade_and_not_for_long() {
        let dir = std::env::temp_dir().join(format!("ilg-legacy-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        // A new install: never, and not on a later start either.
        assert_eq!(legacy_until(&dir, false), None);
        assert_eq!(legacy_until(&dir, true), None);
        // An upgrade: from the first start, kept across restarts.
        std::fs::remove_file(dir.join(LEGACY_FILE)).unwrap();
        let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs();
        let until = legacy_until(&dir, true).unwrap();
        assert!(until >= now + LEGACY_DAYS * 86_400 && until <= now + LEGACY_DAYS * 86_400 + 5);
        assert_eq!(legacy_until(&dir, true), Some(until));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_page_key_is_the_tokens_and_isnt_it() {
        let t = "ilt_0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
        let k = page_key(t);
        assert!(k.starts_with("ilp_") && k.len() == 68, "{k}");
        assert_ne!(k, t);
        assert_eq!(page_key(&format!(" {t}\n")), k);
        assert_ne!(page_key("ilt_other"), k);
        // web/e2e/local-token.ts computes it the same way.
        assert_eq!(
            k,
            "ilp_".to_owned() + &hex::encode(<sha2::Sha256 as sha2::Digest>::digest(format!("arugula page key\0{t}")))
        );
    }

    #[test]
    fn proc_net_tcp_names_the_client_ends_owner() {
        // 127.0.0.1:40000 -> 127.0.0.1:7681 (0x1E01), and the server's end.
        let v4 = "  sl  local_address rem_address   st tx_queue rx_queue tr tm->when retrnsmt   uid  timeout inode\n   \
            0: 0100007F:1E01 00000000:0000 0A 00000000:00000000 00:00000000 00000000  1000        0 1 1\n   \
            1: 0100007F:9C40 0100007F:1E01 01 00000000:00000000 00:00000000 00000000  1001        0 2 1\n   \
            2: 0100007F:1E01 0100007F:9C40 01 00000000:00000000 00:00000000 00000000  1000        0 3 1\n";
        let peer: SocketAddr = "127.0.0.1:40000".parse().unwrap();
        if cfg!(target_endian = "little") {
            assert_eq!(uid_in(v4, peer, 7681), Some(1001));
            assert_eq!(uid_in(v4, peer, 7682), None);
            assert_eq!(uid_in(v4, "127.0.0.1:40001".parse().unwrap(), 7681), None);
            // ::1 and a v4-mapped peer (a dual-stack listener).
            let v6 = "header\n   0: 00000000000000000000000001000000:9C40 00000000000000000000000001000000:1E01 01 0 0 0 0 0 1\n   \
                1: 0000000000000000FFFF00000100007F:9C41 0000000000000000FFFF00000100007F:1E01 01 0 0 0 42 0 1\n";
            assert_eq!(uid_in(v6, "[::1]:40000".parse().unwrap(), 7681), Some(0));
            assert_eq!(uid_in(v6, "[::ffff:127.0.0.1]:40001".parse().unwrap(), 7681), Some(42));
            assert_eq!(uid_in(v6, "127.0.0.1:40001".parse().unwrap(), 7681), Some(42));
            // A connection lingering in TIME_WAIT (uid 0) beside a live one
            // on the same ports: the live one's owner, not root.
            let (head, rows) = v4.split_once('\n').unwrap();
            let reused = format!(
                "{head}\n   3: 0100007F:9C40 0100007F:1E01 06 00000000:00000000 03:00000000 00000000     0        0 0 3\n{rows}"
            );
            assert_eq!(uid_in(&reused, peer, 7681), Some(1001));
            // Two live entries that disagree: nobody.
            let two = format!(
                "{v4}   3: 0100007F:9C40 0100007F:1E01 01 00000000:00000000 00:00000000 00000000     0        0 9 1\n"
            );
            assert_eq!(uid_in(&two, peer, 7681), None);
        }
    }

    /// One connection's records in macOS's `pcblist_n`, as xnu lays them
    /// out: `xinpcb_n` (104 bytes), `xsocket_n` (104), then a buffer record.
    fn pcb(local: SocketAddr, fport: u16, uid: u32, live: bool) -> Vec<u8> {
        let mut inp = vec![0u8; 104];
        inp[0..4].copy_from_slice(&104u32.to_ne_bytes());
        inp[4..8].copy_from_slice(&0x10u32.to_ne_bytes());
        inp[16..18].copy_from_slice(&fport.to_be_bytes());
        inp[18..20].copy_from_slice(&local.port().to_be_bytes());
        match local.ip() {
            std::net::IpAddr::V4(a) => {
                inp[44] = 0x1;
                inp[76..80].copy_from_slice(&a.octets());
            }
            std::net::IpAddr::V6(a) => {
                inp[44] = 0x2;
                inp[64..80].copy_from_slice(&a.octets());
            }
        }
        let mut so = vec![0u8; 104];
        so[0..4].copy_from_slice(&104u32.to_ne_bytes());
        so[4..8].copy_from_slice(&0x1u32.to_ne_bytes());
        if live {
            so[8..16].copy_from_slice(&0xfeed_u64.to_ne_bytes());
            so[64..68].copy_from_slice(&uid.to_ne_bytes());
        }
        let mut buf = vec![0u8; 32];
        buf[0..4].copy_from_slice(&28u32.to_ne_bytes());
        buf[4..8].copy_from_slice(&0x2u32.to_ne_bytes());
        [inp, so, buf].concat()
    }

    fn pcblist(conns: &[Vec<u8>]) -> Vec<u8> {
        let mut head = vec![0u8; 24];
        head[0..4].copy_from_slice(&24u32.to_ne_bytes());
        [vec![head.clone()], conns.to_vec(), vec![head]].concat().concat()
    }

    #[test]
    fn macos_tcp_table_names_the_client_ends_owner() {
        let peer: SocketAddr = "127.0.0.1:40000".parse().unwrap();
        let server: SocketAddr = "127.0.0.1:7681".parse().unwrap();
        let uids = |t: &[u8], p: SocketAddr, port: u16| one_uid(pcb_uids(t, p, port));
        let t = pcblist(&[pcb(server, 40000, 501, true), pcb(peer, 7681, 502, true)]);
        assert_eq!(uids(&t, peer, 7681), Some(502));
        assert_eq!(uids(&t, peer, 7682), None);
        assert_eq!(uids(&t, "127.0.0.1:40001".parse().unwrap(), 7681), None);
        // ::1, and a v4-mapped peer (a dual-stack listener) of a v4 client.
        let v6: SocketAddr = "[::1]:40002".parse().unwrap();
        let t = pcblist(&[pcb(v6, 7681, 0, true), pcb(peer, 7681, 503, true)]);
        assert_eq!(uids(&t, v6, 7681), Some(0));
        assert_eq!(uids(&t, "[::ffff:127.0.0.1]:40000".parse().unwrap(), 7681), Some(503));
        // A lingering connection without a socket isn't anyone's; two live
        // ones that disagree are nobody's.
        let t = pcblist(&[pcb(peer, 7681, 0, false), pcb(peer, 7681, 504, true)]);
        assert_eq!(uids(&t, peer, 7681), Some(504));
        let t = pcblist(&[pcb(peer, 7681, 0, true), pcb(peer, 7681, 504, true)]);
        assert_eq!(uids(&t, peer, 7681), None);
        // Cut short, or empty: nobody.
        let t = pcblist(&[pcb(peer, 7681, 505, true)]);
        assert_eq!(uids(&t[..100], peer, 7681), None);
        assert_eq!(uids(&[], peer, 7681), None);
    }

    #[cfg(any(target_os = "linux", target_os = "android", target_os = "macos"))]
    #[test]
    fn a_real_connection_is_ours() {
        let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = l.local_addr().unwrap().port();
        let c = std::net::TcpStream::connect(("127.0.0.1", port)).unwrap();
        let (_s, peer) = l.accept().unwrap();
        assert_eq!(peer, c.local_addr().unwrap());
        assert_eq!(loopback_uid(peer, port), Some(nix::unistd::geteuid().as_raw()));
        assert!(serve_peer_ok(peer, port));
    }

    /// A peer no table names (or a machine without one) isn't serve's.
    #[cfg(unix)]
    #[test]
    fn an_unknown_peer_isnt_serve() {
        let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = l.local_addr().unwrap().port();
        assert!(!serve_peer_ok("127.0.0.1:1".parse().unwrap(), port));
    }
}
