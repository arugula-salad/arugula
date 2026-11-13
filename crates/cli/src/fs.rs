//! `arugula fs ls|stat|cat|watch|recent` and `arugula cd`: files on a
//! host, read-only (M7). A path is this host's (relative to the current
//! directory), `%N:PATH` on whatever pane %N runs on (its VM, say), or
//! `mN:PATH` on machine N; with `--host`, another daemon's.

use std::io::{BufRead, BufReader, Write};

use anyhow::Context;
use arugula_proto::{
    fs::{CdRequest, FsEntry, FsKind, FsQuery},
    op::ops::{FsListGet, FsRecentGet, FsStatGet, PaneCd},
};
use clap::Subcommand;
use serde_json::Value;

use crate::http::{Target, call, call_raw, enc, request};

#[derive(Subcommand)]
pub enum FsCmd {
    /// A directory's entries.
    Ls {
        /// `PATH` or `%N:PATH` [default: the home directory].
        path: Option<String>,
        /// Only directories.
        #[arg(short, long)]
        dirs: bool,
        /// Size, mode and time too.
        #[arg(short, long)]
        long: bool,
    },
    /// One file or directory (symlinks resolved).
    Stat { path: String },
    /// A file's contents (in 1 MiB ranges; `--offset`/`--len` for part).
    Cat {
        path: String,
        #[arg(long, default_value_t = 0)]
        offset: u64,
        /// Bytes [default: to the end].
        #[arg(long)]
        len: Option<u64>,
    },
    /// Changes to a directory or file, as NDJSON, until interrupted.
    Watch { path: String },
    /// Directories used lately on a host (`%N`; default this one).
    Recent { on: Option<String> },
}

/// `(query for where, path)`: `%N:p` is pane N's host, `mN:p` machine N.
fn place(spec: &str, remote: bool) -> anyhow::Result<(String, String)> {
    if let Some((on, path)) = spec.split_once(':') {
        if let Some(n) = on.strip_prefix('%') {
            let n: u32 = n.parse().with_context(|| format!("not a pane: {on}"))?;
            return Ok((format!("pane={n}"), path.to_owned()));
        }
        if let Some(n) = on.strip_prefix('m')
            && let Ok(n) = n.parse::<u32>()
        {
            return Ok((format!("machine={n}"), path.to_owned()));
        }
    }
    // Ours: relative to where we are (another daemon's: to its home).
    let path = match std::env::current_dir() {
        Ok(cwd) if !remote && !spec.starts_with('/') && !spec.starts_with('~') => cwd.join(spec).display().to_string(),
        _ => spec.to_owned(),
    };
    Ok((String::new(), path))
}

fn query(on: &str, path: &str, extra: &[String]) -> String {
    let mut q: Vec<String> = Vec::new();
    if !on.is_empty() {
        q.push(on.to_owned());
    }
    q.push(format!("path={}", enc(path)));
    q.extend(extra.iter().cloned());
    q.join("&")
}

/// `place`'s query for where (`pane=N`, `machine=N` or none), and a path,
/// as the request the file routes take.
fn fs_query(on: &str, path: Option<&str>) -> FsQuery {
    let n = |k: &str| on.strip_prefix(k).and_then(|n| n.parse().ok());
    FsQuery { pane: n("pane="), machine: n("machine="), path: path.map(str::to_owned), ..Default::default() }
}

fn print_json(v: &Value) {
    println!("{}", serde_json::to_string_pretty(v).unwrap_or_default());
}

fn kind_char(e: &FsEntry) -> char {
    match e.kind {
        FsKind::Directory => 'd',
        FsKind::Symlink => 'l',
        FsKind::File => '-',
        FsKind::Other => '?',
    }
}

pub fn run(sock: &Target, cmd: FsCmd, json_out: bool, remote: bool) -> anyhow::Result<i32> {
    match cmd {
        FsCmd::Ls { path, dirs, long } => {
            let (on, path) = place(path.as_deref().unwrap_or("~"), remote)?;
            let q = FsQuery { dirs: dirs.then_some(1), ..fs_query(&on, Some(&path)) };
            let (list, v) = call_raw::<FsListGet>(sock, &(), &q)?;
            if json_out {
                print_json(&v);
                return Ok(0);
            }
            for e in &list.entries {
                let name = if e.name.is_empty() { "?" } else { &e.name };
                let slash = if e.is_dir() { "/" } else { "" };
                if long {
                    println!("{}{:04o} {:>12}  {name}{slash}", kind_char(e), e.mode, e.size);
                } else {
                    println!("{name}{slash}");
                }
            }
            if list.truncated {
                eprintln!("arugula: (more entries than one listing holds)");
            }
        }
        FsCmd::Stat { path } => {
            let (on, path) = place(&path, remote)?;
            let (e, v) = call_raw::<FsStatGet>(sock, &(), &fs_query(&on, Some(&path)))?;
            if json_out {
                print_json(&v);
            } else {
                let path = if e.path.is_empty() { "?" } else { &e.path };
                println!("{path} {}{:04o} {} bytes", kind_char(&e), e.mode, e.size);
            }
        }
        FsCmd::Cat { path, offset, len } => {
            let (on, path) = place(&path, remote)?;
            let mut out = std::io::stdout().lock();
            let (start, end) = (offset, len.map(|l| offset + l));
            let mut at = start;
            let pages = cat_pages(
                end,
                &mut at,
                |at, want| {
                    let extra = [format!("offset={at}"), format!("len={want}")];
                    let res =
                        request(sock, "GET", &format!("/api/fs/read?{}", query(&on, &path, &extra)), None)?.ok()?;
                    // Not every route says it (control's older daemons don't).
                    let size = res.header("x-arugula-size").and_then(|s| s.parse().ok());
                    Ok((res.bytes()?, size))
                },
                |bytes| out.write_all(bytes),
            );
            let flushed = out.flush();
            if let Err(e) = pages.and(flushed.map_err(Into::into)) {
                // Never a short copy that looks whole (#933).
                eprintln!("arugula: {e:#}; wrote {} bytes, resume with --offset {at}", at - start);
                return Ok(1);
            }
        }
        FsCmd::Watch { path } => {
            let (on, path) = place(&path, remote)?;
            let res = request(sock, "GET", &format!("/api/fs/watch?{}", query(&on, &path, &[])), None)?.ok()?;
            let mut out = std::io::stdout().lock();
            for line in BufReader::new(res).lines() {
                writeln!(out, "{}", line?)?;
                out.flush()?;
            }
        }
        FsCmd::Recent { on } => {
            let q = match on.as_deref() {
                None => String::new(),
                Some(o) => place(&format!("{o}:"), remote)?.0,
            };
            let (dirs, v) = call_raw::<FsRecentGet>(sock, &(), &fs_query(&q, None))?;
            if json_out {
                print_json(&v);
            } else {
                for d in &dirs {
                    println!("{d}");
                }
            }
        }
    }
    Ok(0)
}

/// The pages of a `cat`: `read(at, want)` gives a page and the file's size
/// when the route says it, `write` takes each page, and `at` is how far it
/// got. It stops at `end`, at the size, or at a page shorter than asked, and
/// a missing size is never zero (#933).
fn cat_pages(
    end: Option<u64>,
    at: &mut u64,
    mut read: impl FnMut(u64, u64) -> anyhow::Result<(Vec<u8>, Option<u64>)>,
    mut write: impl FnMut(&[u8]) -> std::io::Result<()>,
) -> anyhow::Result<()> {
    const PAGE: u64 = 1 << 20;
    loop {
        let want = end.map_or(PAGE, |e| e.saturating_sub(*at).min(PAGE));
        if want == 0 {
            return Ok(());
        }
        let (bytes, size) = read(*at, want)?;
        write(&bytes)?;
        *at += bytes.len() as u64;
        if (bytes.len() as u64) < want || size.is_some_and(|s| *at >= s) {
            return Ok(());
        }
    }
}

/// `arugula cd %N DIR`: typed into pane N's shell if it's idle at its
/// prompt; refused (with why) otherwise.
pub fn cd(sock: &Target, pane: u32, path: &str) -> anyhow::Result<i32> {
    call::<PaneCd>(sock, &pane, &CdRequest { path: path.to_owned() })?;
    Ok(0)
}

#[cfg(test)]
mod tests {
    /// `place`'s query for where, read back as the request, writes the URL
    /// `query` wrote.
    #[test]
    fn the_request_is_the_query_as_it_was() {
        use arugula_proto::op::ops::{FsListGet, FsStatGet};
        let (on, path) = super::place("%3:src dir", false).unwrap();
        let q = arugula_proto::fs::FsQuery { dirs: Some(1), ..super::fs_query(&on, Some(&path)) };
        let old = format!("/api/fs/list?{}", super::query(&on, &path, &["dirs=1".to_owned()]));
        assert_eq!(crate::http::route::<FsListGet>(&(), &q).unwrap(), old);
        let (on, path) = super::place("m2:/etc", false).unwrap();
        let old = format!("/api/fs/stat?{}", super::query(&on, &path, &[]));
        assert_eq!(crate::http::route::<FsStatGet>(&(), &super::fs_query(&on, Some(&path))).unwrap(), old);
    }

    /// A route that doesn't send the size is read to a short page, not
    /// stopped after the first (#933); one that fails says how far it got.
    #[test]
    fn cat_reads_on_without_a_size() {
        const PAGE: usize = 1 << 20;
        let file = vec![7u8; 3 * PAGE];
        let (mut got, mut at) = (0usize, 0u64);
        let page = |at: u64, want: u64| {
            let from = (at as usize).min(file.len());
            Ok((file[from..(from + want as usize).min(file.len())].to_vec(), None))
        };
        super::cat_pages(None, &mut at, page, |b| {
            got += b.len();
            Ok(())
        })
        .unwrap();
        assert_eq!((got, at), (3 * PAGE, 3 * PAGE as u64));
        // The second page fails: the first is written, and `at` is the resume point.
        let mut at = 0u64;
        let mut n = 0;
        let r = super::cat_pages(
            None,
            &mut at,
            |_, want| {
                n += 1;
                if n == 2 { anyhow::bail!("the channel closed") } else { Ok((vec![0; want as usize], None)) }
            },
            |_| Ok(()),
        );
        assert!(r.is_err());
        assert_eq!(at, PAGE as u64);
    }

    #[test]
    fn places() {
        assert_eq!(super::place("%3:src", false).unwrap(), ("pane=3".into(), "src".into()));
        assert_eq!(super::place("m2:/etc", false).unwrap(), ("machine=2".into(), "/etc".into()));
        assert_eq!(super::place("/etc", false).unwrap(), (String::new(), "/etc".into()));
        assert_eq!(super::place("~/x", true).unwrap(), (String::new(), "~/x".into()));
        assert_eq!(super::place("rel", true).unwrap(), (String::new(), "rel".into()));
        let here = std::env::current_dir().unwrap().join("rel").display().to_string();
        assert_eq!(super::place("rel", false).unwrap().1, here);
        // A colon in an ordinary name isn't a place.
        assert_eq!(super::place("/a:b", false).unwrap().1, "/a:b");
    }
}
