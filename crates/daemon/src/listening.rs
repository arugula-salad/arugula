//! What listens on a machine (#150): the TCP ports *Open a port…* offers,
//! so picking the dev server is a tap rather than typing a number from an
//! example. A port shows when it answers on loopback (bound to it, or to
//! every address), since that's where a port block dials; the pane's own
//! (its process or one under it) come first.
//!
//! This host: `/proc` on Linux, `lsof` on macOS, nothing elsewhere. A
//! machine: one shell script through its provider, reading its `/proc`.

use std::{
    collections::{BTreeMap, HashMap, HashSet},
    net::IpAddr,
    time::Duration,
};

use arugula_proto::{Machine, MachineState, api::ListeningPort};

use crate::server::App;

/// A process, as far as ranking ports needs it.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct Proc {
    pub pid: u32,
    pub ppid: u32,
    pub comm: String,
    /// Carries the pane's `ARUGULA_EXEC` tag (a machine's pane).
    pub tagged: bool,
    /// Its sockets' inodes.
    pub sockets: Vec<u64>,
}

/// The listening sockets (port, inode) and the processes that may hold them.
#[derive(Debug, Default)]
pub struct Snapshot {
    pub listens: Vec<(u16, u64)>,
    pub procs: Vec<Proc>,
}

/// The sockets in a `/proc/net/tcp{,6}` table that listen where loopback
/// reaches: (port, inode).
pub fn listens(table: &str) -> Vec<(u16, u64)> {
    table
        .lines()
        .filter_map(|line| {
            let f: Vec<&str> = line.split_whitespace().collect();
            // LISTEN is state 0A.
            if f.get(3) != Some(&"0A") {
                return None;
            }
            let at = crate::localauth::parse_addr(f.get(1)?)?;
            let ip = at.ip().to_canonical();
            (ip.is_loopback() || ip.is_unspecified()).then_some(())?;
            Some((at.port(), f.get(9)?.parse().ok()?))
        })
        .collect()
}

/// The ports to offer: the pane's first (a process under `root`, or one
/// carrying its tag), then up to [`OTHERS`] more by number: the account's
/// own programs' above 1024, not Arugula's. Never `skip`'s (this daemon's).
pub fn rank(snap: &Snapshot, root: Option<u32>, skip: Option<u32>) -> Vec<ListeningPort> {
    let parent: HashMap<u32, &Proc> = snap.procs.iter().map(|p| (p.pid, p)).collect();
    let pane_owns = |mut pid: u32| {
        for _ in 0..64 {
            let Some(p) = parent.get(&pid) else { return false };
            if p.tagged || Some(pid) == root {
                return true;
            }
            if p.ppid == 0 || p.ppid == pid {
                return false;
            }
            pid = p.ppid;
        }
        false
    };
    let wanted: HashSet<u64> = snap.listens.iter().map(|(_, i)| *i).collect();
    let mut holder: HashMap<u64, &Proc> = HashMap::new();
    for p in &snap.procs {
        for s in p.sockets.iter().filter(|s| wanted.contains(s)) {
            holder.entry(*s).or_insert(p);
        }
    }
    let mut by_port: BTreeMap<u16, ListeningPort> = BTreeMap::new();
    for (port, inode) in &snap.listens {
        let held = holder.get(inode);
        if *port == 0 || held.is_some_and(|p| Some(p.pid) == skip) {
            continue;
        }
        let pane = held.is_some_and(|p| pane_owns(p.pid));
        let e = by_port.entry(*port).or_insert(ListeningPort { port: *port, process: None, pane: false });
        e.pane |= pane;
        if e.process.is_none() {
            e.process = held.map(|p| p.comm.clone()).filter(|c| !c.is_empty());
        }
    }
    // The rest are what the account runs: a port whose program can't be
    // read is another account's (a system service, a cluster's), and
    // Arugula's own (another daemon, control) is no dev server.
    let mut out: Vec<ListeningPort> = by_port
        .into_values()
        .filter(|p| p.pane || (p.port >= 1024 && p.process.as_deref().is_some_and(|c| !c.starts_with("arugula"))))
        .collect();
    out.sort_by_key(|p| (!p.pane, p.port));
    let mine = out.iter().filter(|p| p.pane).count();
    out.truncate(mine + OTHERS);
    out
}

/// At most this many ports besides the pane's: a tap away, not a wall.
const OTHERS: usize = 12;

/// `pid (comm) state ppid …`, a `/proc/PID/stat` line.
fn stat_line(line: &str) -> Option<Proc> {
    let (head, rest) = line.rsplit_once(')')?;
    let (pid, comm) = head.split_once(" (")?;
    let ppid = rest.split_whitespace().nth(1)?.parse().ok()?;
    Some(Proc { pid: pid.trim().parse().ok()?, ppid, comm: comm.to_owned(), ..Default::default() })
}

/// `socket:[123]`'s inode.
fn socket_inode(link: &str) -> Option<u64> {
    link.strip_prefix("socket:[")?.strip_suffix(']')?.parse().ok()
}

/// `/proc/123/…`'s pid.
fn proc_pid(path: &str) -> Option<u32> {
    path.strip_prefix("/proc/")?.split('/').next()?.parse().ok()
}

/// Reads a machine's `/proc`: its TCP tables, every process's stat line,
/// their sockets, and (given a tag, `$1`) which carry the pane's.
const GUEST_SCRIPT: &str = r#"
exec 2>/dev/null
cat /proc/net/tcp /proc/net/tcp6
echo @procs
for f in /proc/[0-9]*/stat; do read -r l <"$f" && echo "$l"; done
echo @sockets
find /proc/[0-9]*/fd -lname 'socket:*' -printf '%h %l\n'
echo @tagged
[ -n "$1" ] && grep -lzx "ARUGULA_EXEC=$1" /proc/[0-9]*/environ
exit 0
"#;

/// [`GUEST_SCRIPT`]'s output, read.
pub fn parse_guest(out: &str) -> Snapshot {
    let mut section = "";
    let mut table = String::new();
    let mut procs: Vec<Proc> = Vec::new();
    let mut sockets: HashMap<u32, Vec<u64>> = HashMap::new();
    let mut tagged: HashSet<u32> = HashSet::new();
    for line in out.lines() {
        if let Some(s) = line.strip_prefix('@') {
            section = match s {
                "procs" | "sockets" | "tagged" => s,
                _ => section,
            };
            continue;
        }
        match section {
            "" => {
                table.push_str(line);
                table.push('\n');
            }
            "procs" => procs.extend(stat_line(line)),
            "sockets" => {
                if let Some((dir, link)) = line.split_once(' ')
                    && let (Some(pid), Some(inode)) = (proc_pid(dir), socket_inode(link))
                {
                    sockets.entry(pid).or_default().push(inode);
                }
            }
            _ => tagged.extend(proc_pid(line)),
        }
    }
    for p in &mut procs {
        p.sockets = sockets.remove(&p.pid).unwrap_or_default();
        p.tagged = tagged.contains(&p.pid);
    }
    Snapshot { listens: listens(&table), procs }
}

/// The ports listening on a machine: its pane's first when `tag` is the
/// pane's exec tag. Nothing when it's gone, or doesn't answer in time.
pub async fn guest(app: &App, machine: &Machine, tag: Option<String>) -> Vec<ListeningPort> {
    let Some(provider) = app.mux.provider.clone() else { return Vec::new() };
    if machine.state == MachineState::Gone {
        return Vec::new();
    }
    let tag = tag.unwrap_or_default();
    let argv = ["bash", "-c", GUEST_SCRIPT, "arugula-ports", &tag];
    match tokio::time::timeout(Duration::from_secs(8), provider.run(&machine.sprite, &argv)).await {
        Ok(Ok((out, _))) => rank(&parse_guest(&String::from_utf8_lossy(&out)), None, None),
        _ => Vec::new(),
    }
}

/// The ports listening on this host, those of the process tree under
/// `root` (a pane's) first. Blocking: run it off the runtime.
pub fn local(root: Option<u32>) -> Vec<ListeningPort> {
    rank(&local_snapshot(), root, Some(std::process::id()))
}

#[cfg(any(target_os = "linux", target_os = "android"))]
fn local_snapshot() -> Snapshot {
    let table: String =
        ["/proc/net/tcp", "/proc/net/tcp6"].iter().filter_map(|f| std::fs::read_to_string(f).ok()).collect();
    let listens = listens(&table);
    if listens.is_empty() {
        return Snapshot::default();
    }
    let wanted: HashSet<u64> = listens.iter().map(|(_, i)| *i).collect();
    let mut procs = Vec::new();
    for e in std::fs::read_dir("/proc").into_iter().flatten().flatten() {
        let Some(pid) = e.file_name().to_str().and_then(|s| s.parse::<u32>().ok()) else { continue };
        let Some(mut p) = std::fs::read_to_string(format!("/proc/{pid}/stat")).ok().as_deref().and_then(stat_line)
        else {
            continue;
        };
        // Other accounts' fds can't be read: their ports show unnamed.
        for fd in std::fs::read_dir(format!("/proc/{pid}/fd")).into_iter().flatten().flatten() {
            if let Some(inode) = std::fs::read_link(fd.path()).ok().and_then(|l| socket_inode(&l.to_string_lossy()))
                && wanted.contains(&inode)
            {
                p.sockets.push(inode);
            }
        }
        procs.push(p);
    }
    Snapshot { listens, procs }
}

/// `lsof`'s fields (`-F pcn`): `p<pid>`, `c<command>`, `n<address:port>`
/// per socket. Each socket gets an inode of our own.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
fn parse_lsof(out: &str, ppid: impl Fn(u32) -> Option<u32>) -> Snapshot {
    let mut snap = Snapshot::default();
    let mut next = 1u64;
    for line in out.lines() {
        let (k, v) = line.split_at_checked(1).unwrap_or(("", ""));
        match k {
            "p" => {
                if let Ok(pid) = v.parse() {
                    snap.procs.push(Proc { pid, ppid: ppid(pid).unwrap_or(0), ..Default::default() });
                }
            }
            "c" => {
                if let Some(p) = snap.procs.last_mut() {
                    p.comm = v.to_owned();
                }
            }
            "n" => {
                let Some((host, port)) = v.rsplit_once(':') else { continue };
                let host = host.trim_start_matches('[').trim_end_matches(']');
                let here = host == "*"
                    || host == "localhost"
                    || host.parse::<IpAddr>().is_ok_and(|ip| ip.is_loopback() || ip.is_unspecified());
                if let (true, Ok(port), Some(p)) = (here, port.parse(), snap.procs.last_mut()) {
                    p.sockets.push(next);
                    snap.listens.push((port, next));
                    next += 1;
                }
            }
            _ => {}
        }
    }
    snap
}

#[cfg(target_os = "macos")]
fn local_snapshot() -> Snapshot {
    let out = std::process::Command::new("/usr/sbin/lsof")
        .args(["-nP", "-iTCP", "-sTCP:LISTEN", "-F", "pcn"])
        .stderr(std::process::Stdio::null())
        .output();
    match out {
        Ok(o) => parse_lsof(&String::from_utf8_lossy(&o.stdout), arugula_mux::procinfo::ppid),
        Err(_) => Snapshot::default(),
    }
}

#[cfg(not(any(target_os = "linux", target_os = "android", target_os = "macos")))]
fn local_snapshot() -> Snapshot {
    Snapshot::default()
}

#[cfg(test)]
mod tests {
    use super::*;

    const HEAD: &str =
        "  sl  local_address rem_address   st tx_queue rx_queue tr tm->when retrnsmt   uid  timeout inode\n";

    #[test]
    fn listeners_on_loopback_or_everywhere() {
        if cfg!(target_endian = "big") {
            return;
        }
        // 127.0.0.1:5317, 0.0.0.0:3000, 100.64.0.1:8080 (not loopback), a
        // connection on :5317, and port 22 everywhere.
        let v4 = format!(
            "{HEAD}   0: 0100007F:14C5 00000000:0000 0A 0:0 0:0 0 1000 0 11 1\n   \
             1: 00000000:0BB8 00000000:0000 0A 0:0 0:0 0 1000 0 12 1\n   \
             2: 01004064:1F90 00000000:0000 0A 0:0 0:0 0 1000 0 13 1\n   \
             3: 0100007F:14C5 0100007F:9C40 01 0:0 0:0 0 1000 0 14 1\n   \
             4: 00000000:0016 00000000:0000 0A 0:0 0:0 0 0 0 15 1\n"
        );
        assert_eq!(listens(&v4), vec![(5317, 11), (3000, 12), (22, 15)]);
        // ::1:5173 (Vite's "localhost") and :: on 3000.
        let v6 = format!(
            "{HEAD}   0: 00000000000000000000000001000000:1435 00000000000000000000000000000000:0000 0A 0:0 0:0 0 1000 0 21 1\n   \
             1: 00000000000000000000000000000000:0BB8 00000000000000000000000000000000:0000 0A 0:0 0:0 0 1000 0 22 1\n"
        );
        assert_eq!(listens(&v6), vec![(5173, 21), (3000, 22)]);
    }

    fn proc(pid: u32, ppid: u32, comm: &str, sockets: &[u64]) -> Proc {
        Proc { pid, ppid, comm: comm.into(), tagged: false, sockets: sockets.to_vec() }
    }

    #[test]
    fn the_panes_own_come_first_then_by_number() {
        let snap = Snapshot {
            listens: vec![(8080, 1), (5317, 2), (5317, 3), (22, 4), (7681, 5), (9000, 6), (443, 7), (7690, 8)],
            procs: vec![
                proc(1, 0, "init", &[]),
                proc(10, 1, "bash", &[]),      // the pane's shell
                proc(11, 10, "npm", &[]),      //
                proc(12, 11, "node", &[2, 3]), // its dev server, v4 and v6
                proc(13, 10, "caddy", &[7]),   // a pane's on 443 stays
                proc(20, 1, "python3", &[1]),  // the account's other one
                proc(30, 1, "sshd", &[4]),     // well-known: left out
                proc(40, 1, "arugulad", &[5]), // this daemon
                proc(50, 1, "arugula-control", &[8]), // Arugula's own
                                               // 9000: another account's (unreadable): left out
            ],
        };
        let got = rank(&snap, Some(10), Some(40));
        let lp = |port, process: &str, pane| ListeningPort { port, process: Some(process.into()), pane };
        assert_eq!(got, vec![lp(443, "caddy", true), lp(5317, "node", true), lp(8080, "python3", false)]);
        // No pane to favour: by number.
        assert_eq!(rank(&snap, None, Some(40)).iter().map(|p| p.port).collect::<Vec<_>>(), vec![5317, 8080]);
        // A wall of others is cut short; the pane's all stay.
        let many = Snapshot {
            listens: (0..30).map(|i| (3000 + i as u16, i)).collect(),
            procs: (0..30)
                .map(|i| proc(100 + i as u32, if i == 29 { 10 } else { 1 }, "node", &[i]))
                .chain([proc(10, 1, "bash", &[])])
                .collect(),
        };
        let got = rank(&many, Some(10), None);
        assert_eq!(got.len(), 1 + OTHERS);
        assert_eq!((got[0].port, got[0].pane), (3029, true));
    }

    #[test]
    fn a_machines_proc_read_through_its_provider() {
        if cfg!(target_endian = "big") {
            return;
        }
        let out = format!(
            "{HEAD}   0: 00000000:14C5 00000000:0000 0A 0:0 0:0 0 1000 0 31 1\n   \
             1: 0100007F:1F90 00000000:0000 0A 0:0 0:0 0 1000 0 32 1\n\
             @procs\n\
             1 (init) S 0 1 1 0\n\
             50 (bash) S 1 50 50 0\n\
             51 (my server) S 50 50 50 0\n\
             60 (python3) S 1 60 60 0\n\
             @sockets\n\
             /proc/51/fd socket:[31]\n\
             /proc/60/fd socket:[32]\n\
             /proc/60/fd socket:[99]\n\
             @tagged\n\
             /proc/50/environ\n"
        );
        let snap = parse_guest(&out);
        assert_eq!(snap.procs.len(), 4);
        let got = rank(&snap, None, None);
        assert_eq!(
            got,
            vec![
                ListeningPort { port: 5317, process: Some("my server".into()), pane: true },
                ListeningPort { port: 8080, process: Some("python3".into()), pane: false },
            ]
        );
        // Nothing read: nothing offered.
        assert!(rank(&parse_guest(""), None, None).is_empty());
    }

    #[test]
    fn lsof_on_macos() {
        let out = "p100\ncnode\nn*:5317\nn[::1]:5317\np200\ncpostgres\nn127.0.0.1:5432\nn192.168.1.4:9999\n";
        let snap = parse_lsof(out, |pid| (pid == 100).then_some(10));
        assert_eq!(snap.listens, vec![(5317, 1), (5317, 2), (5432, 3)]);
        let got =
            rank(&Snapshot { procs: [vec![proc(10, 1, "zsh", &[])], snap.procs].concat(), ..snap }, Some(10), None);
        assert_eq!(got.iter().map(|p| (p.port, p.pane)).collect::<Vec<_>>(), vec![(5317, true), (5432, false)]);
    }

    #[cfg(any(target_os = "linux", target_os = "android"))]
    #[test]
    fn a_real_listener_here() {
        let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = l.local_addr().unwrap().port();
        // This process is the daemon in a test: offered under a pane, not
        // as the daemon's own.
        let snap = local_snapshot();
        let got = rank(&snap, Some(std::process::id()), None);
        let mine = got.iter().find(|p| p.port == port).expect("listed");
        assert!(mine.pane, "{mine:?}");
        assert!(local(None).iter().all(|p| p.port != port), "the daemon's own ports aren't offered");
    }
}
