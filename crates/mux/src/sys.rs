//! The bits of systemd the daemon uses without linking libsystemd. On
//! Windows there's no systemd: notifying does nothing and nothing is kept.

#[cfg(unix)]
use std::{
    collections::HashMap,
    io::IoSlice,
    os::fd::{AsRawFd, BorrowedFd, FromRawFd, OwnedFd, RawFd},
};
use std::{path::PathBuf, process::Command};

#[cfg(unix)]
use nix::{
    fcntl::{FcntlArg, FdFlag, fcntl},
    sys::socket::{AddressFamily, ControlMessage, MsgFlags, SockFlag, SockType, UnixAddr, sendmsg, socket},
};

/// What systemd tells the daemon's service about itself. Not for the
/// programs in its panes: a daemon started in one would take itself for the
/// service (scopes, the FD store).
pub const SERVICE_ENV: &[&str] = &[
    "NOTIFY_SOCKET",
    "LISTEN_FDS",
    "LISTEN_PID",
    "LISTEN_FDNAMES",
    "LISTEN_PIDFDID",
    "INVOCATION_ID",
    "JOURNAL_STREAM",
    "WATCHDOG_PID",
    "WATCHDOG_USEC",
    "MEMORY_PRESSURE_WATCH",
    "MEMORY_PRESSURE_WRITE",
    "SYSTEMD_EXEC_PID",
];

/// Tell systemd about the daemon's state (`READY=1`, `STOPPING=1`) when it
/// runs as a `Type=notify` service; a no-op otherwise.
pub fn notify(state: &str) {
    #[cfg(unix)]
    notify_with_fds(state, &[]);
    #[cfg(not(unix))]
    let _ = state;
}

/// Whether systemd is listening (a `Type=notify` service). Without it there
/// is no FD store, so panes can't outlive the daemon.
pub fn under_systemd() -> bool {
    std::env::var_os("NOTIFY_SOCKET").is_some()
}

#[cfg(unix)]
fn notify_with_fds(state: &str, fds: &[RawFd]) -> bool {
    let Some(path) = std::env::var_os("NOTIFY_SOCKET") else { return false };
    let path = path.to_string_lossy().into_owned();
    let addr = match path.strip_prefix('@') {
        #[cfg(any(target_os = "linux", target_os = "android"))]
        Some(name) => UnixAddr::new_abstract(name.as_bytes()),
        #[cfg(not(any(target_os = "linux", target_os = "android")))]
        Some(_) => return false,
        None => UnixAddr::new(path.as_str()),
    };
    let Ok(addr) = addr else { return false };
    // systemd only exists on Linux, where the socket can be close-on-exec
    // from the start.
    #[cfg(any(target_os = "linux", target_os = "android"))]
    let flags = SockFlag::SOCK_CLOEXEC;
    #[cfg(not(any(target_os = "linux", target_os = "android")))]
    let flags = SockFlag::empty();
    let Ok(sock) = socket(AddressFamily::Unix, SockType::Datagram, flags, None) else {
        return false;
    };
    let iov = [IoSlice::new(state.as_bytes())];
    let rights = [ControlMessage::ScmRights(fds)];
    let cmsgs: &[ControlMessage] = if fds.is_empty() { &[] } else { &rights };
    sendmsg(sock.as_raw_fd(), &iov, cmsgs, MsgFlags::empty(), Some(&addr)).is_ok()
}

/// Keep a pane's PTY master in systemd's FD store, so the terminal stays
/// open (and its programs running) while the daemon restarts. `FDPOLL=0`:
/// keep it even if it hangs up.
#[cfg(unix)]
pub fn store_fd(name: &str, fd: RawFd) -> bool {
    notify_with_fds(&format!("FDSTORE=1\nFDNAME={name}\nFDPOLL=0"), &[fd])
}

pub fn remove_fd(name: &str) {
    notify(&format!("FDSTOREREMOVE=1\nFDNAME={name}"));
}

/// Descriptors systemd handed back from the FD store (or socket
/// activation), by name. Each call after the first returns nothing: the
/// environment variables are cleared so children don't inherit them.
#[cfg(unix)]
pub fn take_listen_fds() -> HashMap<String, OwnedFd> {
    let mut out = HashMap::new();
    let ours = std::env::var("LISTEN_PID").ok().and_then(|p| p.parse::<u32>().ok()) == Some(std::process::id());
    let n: i32 = std::env::var("LISTEN_FDS").ok().and_then(|n| n.parse().ok()).unwrap_or(0);
    let names = std::env::var("LISTEN_FDNAMES").unwrap_or_default();
    // SAFETY: single-threaded at this point (called before the runtime
    // spawns anything that reads the environment).
    unsafe {
        std::env::remove_var("LISTEN_PID");
        std::env::remove_var("LISTEN_FDS");
        std::env::remove_var("LISTEN_FDNAMES");
    }
    if !ours {
        return out;
    }
    let names: Vec<&str> = names.split(':').collect();
    for i in 0..n {
        let fd = 3 + i;
        let _ = fcntl(unsafe { BorrowedFd::borrow_raw(fd) }, FcntlArg::F_SETFD(FdFlag::FD_CLOEXEC));
        // SAFETY: systemd passed these descriptors to us; we own them now.
        let owned = unsafe { OwnedFd::from_raw_fd(fd) };
        out.insert(names.get(i as usize).copied().unwrap_or("").to_owned(), owned);
    }
    out
}

/// The systemd user manager's environment, read fresh for each new pane.
///
/// At boot (lingering) the daemon starts before anyone logs in, so its own
/// environment lacks the graphical session's `WAYLAND_DISPLAY`, `DISPLAY` and
/// `SSH_AUTH_SOCK`. The session imports those into the manager when it
/// starts, so panes started afterwards get them.
pub fn manager_env() -> Vec<(String, String)> {
    let Ok(out) = Command::new("systemctl").args(["--user", "show-environment"]).output() else {
        return vec![];
    };
    if !out.status.success() {
        return vec![];
    }
    String::from_utf8_lossy(&out.stdout).lines().filter_map(parse_line).collect()
}

/// `KEY=value` or `KEY=$'escaped value'`, as `systemctl show-environment`
/// prints them.
fn parse_line(line: &str) -> Option<(String, String)> {
    let (key, value) = line.split_once('=')?;
    if key.is_empty() || !key.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_') {
        return None;
    }
    let value = match value.strip_prefix("$'").and_then(|v| v.strip_suffix('\'')) {
        Some(quoted) => unescape(quoted),
        None => value.to_owned(),
    };
    Some((key.to_owned(), value))
}

fn unescape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('n') => out.push('\n'),
            Some('t') => out.push('\t'),
            Some('e') => out.push('\x1b'),
            Some(other) => out.push(other),
            None => out.push('\\'),
        }
    }
    out
}

/// A command line from words, quoted as Windows programs split them (cmd's
/// `/k` takes one). On Unix, the words joined (never used there).
pub fn conpty_command_line(argv: &[String]) -> String {
    #[cfg(windows)]
    return argv.split_first().map(|(p, rest)| crate::conpty::command_line(p, rest)).unwrap_or_default();
    #[cfg(not(windows))]
    argv.join(" ")
}

/// `$HOME`, or `%USERPROFILE%` on Windows.
pub fn home() -> PathBuf {
    std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from)
        .unwrap_or_else(|| "/".into())
}

// The pipe server's security helpers (M56), which the pane host (`host.rs`)
// shares: whose process is at the other end, and who may open the pipe.
#[cfg(windows)]
pub use user::{Sa, my_sid, same_user, sid_of, token_user};

#[cfg(windows)]
mod user {
    use std::{ffi::c_void, io, ptr};

    use windows_sys::Win32::{
        Foundation::{CloseHandle, HANDLE, HLOCAL, LocalFree},
        Security::{
            Authorization::{
                ConvertSidToStringSidW, ConvertStringSecurityDescriptorToSecurityDescriptorW, SDDL_REVISION_1,
            },
            EqualSid, GetTokenInformation, PSECURITY_DESCRIPTOR, SECURITY_ATTRIBUTES, TOKEN_QUERY, TOKEN_USER,
            TokenUser,
        },
        System::{
            Pipes::GetNamedPipeClientProcessId,
            Threading::{GetCurrentProcess, OpenProcess, OpenProcessToken, PROCESS_QUERY_LIMITED_INFORMATION},
        },
    };

    fn last() -> io::Error {
        io::Error::last_os_error()
    }

    // Public for the daemon's pipe server; the handle goes straight to Win32,
    // which checks it, as it did when this was the daemon's own code.
    #[allow(clippy::not_unsafe_ptr_arg_deref)]
    /// A process's token's user, as the TOKEN_USER buffer it came in.
    pub fn token_user(process: HANDLE) -> io::Result<Vec<u8>> {
        // SAFETY: Win32 calls with valid buffers; the token is closed here.
        unsafe {
            let mut token = ptr::null_mut();
            if OpenProcessToken(process, TOKEN_QUERY, &mut token) == 0 {
                return Err(last());
            }
            let mut len = 0u32;
            GetTokenInformation(token, TokenUser, ptr::null_mut(), 0, &mut len);
            let mut buf = vec![0u8; len as usize];
            let ok = GetTokenInformation(token, TokenUser, buf.as_mut_ptr() as *mut c_void, len, &mut len);
            CloseHandle(token);
            if ok == 0 {
                return Err(last());
            }
            Ok(buf)
        }
    }

    pub fn sid_of(user: &[u8]) -> *mut c_void {
        // SAFETY: `user` is a TOKEN_USER buffer from GetTokenInformation.
        unsafe { (*(user.as_ptr() as *const TOKEN_USER)).User.Sid }
    }

    /// This user's SID as a string (`S-1-5-21-…`).
    pub fn my_sid() -> io::Result<String> {
        let me = token_user(unsafe { GetCurrentProcess() })?;
        // SAFETY: converting a valid SID; the string is freed here.
        unsafe {
            let mut s: *mut u16 = ptr::null_mut();
            if ConvertSidToStringSidW(sid_of(&me), &mut s) == 0 {
                return Err(last());
            }
            let len = (0..).take_while(|&i| *s.add(i) != 0).count();
            let out = String::from_utf16_lossy(std::slice::from_raw_parts(s, len));
            LocalFree(s as HLOCAL);
            Ok(out)
        }
    }

    // Public for the daemon's pipe server; the handle goes straight to Win32,
    // which checks it, as it did when this was the daemon's own code.
    #[allow(clippy::not_unsafe_ptr_arg_deref)]
    /// The client on `pipe` runs as this process's user.
    pub fn same_user(pipe: HANDLE) -> io::Result<bool> {
        // SAFETY: Win32 calls on handles we hold; the client's is closed here.
        unsafe {
            let mut pid = 0u32;
            if GetNamedPipeClientProcessId(pipe, &mut pid) == 0 {
                return Err(last());
            }
            let p = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
            if p.is_null() {
                return Err(last());
            }
            let theirs = token_user(p);
            CloseHandle(p);
            let ours = token_user(GetCurrentProcess())?;
            Ok(EqualSid(sid_of(&theirs?), sid_of(&ours)) != 0)
        }
    }

    /// Security attributes admitting this user and SYSTEM only. (A named
    /// pipe's default DACL gives Everyone read.)
    pub struct Sa(pub SECURITY_ATTRIBUTES);
    // The descriptor is only read, by CreateNamedPipe.
    unsafe impl Send for Sa {}
    unsafe impl Sync for Sa {}

    impl Sa {
        pub fn mine() -> io::Result<Self> {
            let sddl: Vec<u16> =
                format!("D:P(A;;GA;;;{})(A;;GA;;;SY)", my_sid()?).encode_utf16().chain(Some(0)).collect();
            let mut sd: PSECURITY_DESCRIPTOR = ptr::null_mut();
            // SAFETY: a valid SDDL string; the descriptor lives as long as the
            // listener (it's freed in Drop).
            if unsafe {
                ConvertStringSecurityDescriptorToSecurityDescriptorW(
                    sddl.as_ptr(),
                    SDDL_REVISION_1,
                    &mut sd,
                    ptr::null_mut(),
                )
            } == 0
            {
                return Err(last());
            }
            Ok(Sa(SECURITY_ATTRIBUTES {
                nLength: std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32,
                lpSecurityDescriptor: sd,
                bInheritHandle: 0,
            }))
        }
    }

    impl Drop for Sa {
        fn drop(&mut self) {
            // SAFETY: allocated by ConvertStringSecurityDescriptorToSecurityDescriptorW.
            unsafe { LocalFree(self.0.lpSecurityDescriptor as HLOCAL) };
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_show_environment_lines() {
        assert_eq!(parse_line("WAYLAND_DISPLAY=wayland-0"), Some(("WAYLAND_DISPLAY".into(), "wayland-0".into())));
        assert_eq!(parse_line("A=b=c"), Some(("A".into(), "b=c".into())));
        assert_eq!(parse_line(r"X=$'two words\nand it\'s'"), Some(("X".into(), "two words\nand it's".into())));
        assert_eq!(parse_line("not a line"), None);
        assert_eq!(parse_line("BAD-KEY=x"), None);
    }
}
