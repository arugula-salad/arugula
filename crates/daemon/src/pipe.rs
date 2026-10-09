//! The daemon's local socket on Windows (M56, #219): a named pipe, as S29
//! found. Its DACL admits only this user (by SID; `OW` would be the
//! Administrators group under an elevated token) and SYSTEM, and each client
//! is checked again by the user its process runs as, as `SO_PEERCRED` is on
//! Unix.

use std::{ffi::c_void, io, os::windows::io::AsRawHandle, ptr, time::Duration};

pub use arugula_mux::sys::my_sid;
use arugula_mux::sys::{Sa, same_user, sid_of, token_user};
use tokio::net::windows::named_pipe::{NamedPipeServer, ServerOptions};
use windows_sys::Win32::{
    Foundation::{CloseHandle, HANDLE, HLOCAL, LocalFree},
    Security::Authorization::ConvertSidToStringSidW,
    System::Threading::{OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION},
};

fn instance(name: &str, first: bool, sa: &mut Sa) -> io::Result<NamedPipeServer> {
    // SAFETY: `sa` is a valid SECURITY_ATTRIBUTES for the call.
    unsafe {
        ServerOptions::new()
            .first_pipe_instance(first)
            .reject_remote_clients(true)
            .create_with_security_attributes_raw(name, &mut sa.0 as *mut _ as *mut c_void)
    }
}

/// A named pipe as an axum listener: a fresh instance waits while the last
/// one serves, as a socket's backlog would.
pub struct PipeListener {
    name: String,
    next: NamedPipeServer,
    sa: Sa,
}

impl PipeListener {
    /// The first instance of `name`: fails if another process has it (a
    /// daemon already running, or someone squatting the name).
    pub fn bind(name: &str) -> io::Result<Self> {
        let mut sa = Sa::mine()?;
        let next = instance(name, true, &mut sa)?;
        Ok(Self { name: name.to_owned(), next, sa })
    }
}

impl axum::serve::Listener for PipeListener {
    type Io = NamedPipeServer;
    type Addr = ();

    async fn accept(&mut self) -> (Self::Io, Self::Addr) {
        loop {
            if let Err(e) = self.next.connect().await {
                tracing::debug!(error = %e, "pipe: a client went before it was served");
                tokio::time::sleep(Duration::from_millis(20)).await;
                continue;
            }
            let fresh = match instance(&self.name, false, &mut self.sa) {
                Ok(p) => p,
                Err(e) => {
                    tracing::warn!(error = %e, "pipe: can't make the next instance");
                    tokio::time::sleep(Duration::from_millis(50)).await;
                    continue;
                }
            };
            let conn = std::mem::replace(&mut self.next, fresh);
            match same_user(conn.as_raw_handle() as HANDLE) {
                Ok(true) => return (conn, ()),
                other => tracing::warn!(?other, "pipe: refused a client that isn't this user"),
            }
        }
    }

    fn local_addr(&self) -> io::Result<()> {
        Ok(())
    }
}

/// Something is serving `name`.
pub fn answering(name: &str) -> bool {
    // Busy (every instance taken) still means it's there.
    match std::fs::OpenOptions::new().read(true).write(true).open(name) {
        Ok(_) => true,
        Err(e) => e.raw_os_error() == Some(231),
    }
}

/// The user a process runs as, as a SID string; `None` if it can't be told.
fn user_of(pid: u32) -> Option<String> {
    // SAFETY: Win32 calls on a handle we open and close here.
    unsafe {
        let p = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
        if p.is_null() {
            return None;
        }
        let user = token_user(p);
        CloseHandle(p);
        let user = user.ok()?;
        let mut s: *mut u16 = ptr::null_mut();
        if ConvertSidToStringSidW(sid_of(&user), &mut s) == 0 {
            return None;
        }
        let len = (0..).take_while(|&i| *s.add(i) != 0).count();
        let out = String::from_utf16_lossy(std::slice::from_raw_parts(s, len));
        LocalFree(s as HLOCAL);
        Some(out)
    }
}

/// The process at the client end of a loopback TCP connection to our
/// `port`, from Windows' TCP table (both ends, and the owning pid).
fn loopback_pid(peer: std::net::SocketAddr, port: u16) -> Option<u32> {
    use windows_sys::Win32::NetworkManagement::IpHelper::{
        GetExtendedTcpTable, MIB_TCP6ROW_OWNER_PID, MIB_TCPROW_OWNER_PID, TCP_TABLE_OWNER_PID_ALL,
    };
    const AF_INET: u32 = 2;
    const AF_INET6: u32 = 23;
    let family = if peer.is_ipv4() { AF_INET } else { AF_INET6 };
    let mut size = 0u32;
    // SAFETY: the first call sizes the buffer, the second fills it.
    let table = unsafe {
        GetExtendedTcpTable(ptr::null_mut(), &mut size, 0, family, TCP_TABLE_OWNER_PID_ALL, 0);
        let mut buf = vec![0u64; (size as usize).div_ceil(8) + 1];
        if GetExtendedTcpTable(buf.as_mut_ptr() as *mut c_void, &mut size, 0, family, TCP_TABLE_OWNER_PID_ALL, 0) != 0 {
            return None;
        }
        buf
    };
    let n = table[0] as u32 as usize;
    // Ports are in network order in the low 16 bits.
    let p = |v: u32| u16::from_be(v as u16);
    // SAFETY: rows follow the u32 count, as GetExtendedTcpTable laid them out.
    unsafe {
        let base = (table.as_ptr() as *const u8).add(std::mem::size_of::<u32>());
        match peer {
            std::net::SocketAddr::V4(v4) => {
                let rows = std::slice::from_raw_parts(base as *const MIB_TCPROW_OWNER_PID, n);
                rows.iter()
                    .find(|r| {
                        std::net::Ipv4Addr::from(r.dwLocalAddr.to_ne_bytes()) == *v4.ip()
                            && p(r.dwLocalPort) == v4.port()
                            && p(r.dwRemotePort) == port
                    })
                    .map(|r| r.dwOwningPid)
            }
            std::net::SocketAddr::V6(v6) => {
                let rows = std::slice::from_raw_parts(base as *const MIB_TCP6ROW_OWNER_PID, n);
                rows.iter()
                    .find(|r| {
                        std::net::Ipv6Addr::from(r.ucLocalAddr) == *v6.ip()
                            && p(r.dwLocalPort) == v6.port()
                            && p(r.dwRemotePort) == port
                    })
                    .map(|r| r.dwOwningPid)
            }
        }
    }
}

/// Whether a loopback connection to our `port` comes from a process of this
/// user's, or SYSTEM's (tailscaled is a service): `tailscale serve`'s, or
/// the owner's own. Another account's process isn't.
pub fn loopback_peer_ok(peer: std::net::SocketAddr, port: u16) -> bool {
    let Some(pid) = loopback_pid(peer, port) else { return false };
    let Some(theirs) = user_of(pid) else { return false };
    theirs == "S-1-5-18" || my_sid().is_ok_and(|me| me == theirs)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn our_own_loopback_connection_is_ours() {
        let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = l.local_addr().unwrap().port();
        let c = std::net::TcpStream::connect(("127.0.0.1", port)).unwrap();
        let (_s, peer) = l.accept().unwrap();
        assert_eq!(loopback_pid(peer, port), Some(std::process::id()));
        assert!(loopback_peer_ok(peer, port));
        drop(c);
    }
}
