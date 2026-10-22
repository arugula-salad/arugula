//! Fresh randomness for a new box (M180), before it makes any key.
//!
//! Every box made from one snapshot starts with the same memory, the
//! kernel's random state included. E2B's kernel (6.1, `CONFIG_VMGENID`)
//! gets a new VM generation id from Firecracker at each restore and
//! reseeds itself ("crng reseeded due to virtual machine fork"), and boxes
//! read different bytes from their first moment (checked 2026-10-21). We
//! don't control that kernel, so a new box also forces a reseed of its
//! own before the daemon starts over and makes its keys:
//!
//! `arugulad _reseed <sandbox id>`, as root (`sudo -n`, which the template
//! allows for this one command), adds 64 bytes from the CPU's RDSEED (else
//! RDRAND), 32 from the kernel, the sandbox id and three clocks to the
//! kernel's pool, credited for the CPU's bytes (`RNDADDENTROPY`), and then
//! has it reseed now (`RNDRESEEDCRNG`, Linux 5.18+ and backported). Without
//! root it writes the same bytes to `/dev/random`, which mixes them in
//! uncredited for the kernel's next reseed, and says so.
//!
//! The daemon starts over (`exec`) right after, so no random state of the
//! old process carries on, and every key and token it makes comes after.

use std::{io::Write, path::Path, process::Command, time::Duration};

use tracing::{info, warn};

/// What goes into the pool, and how many of its bits came from the CPU's
/// generator.
pub fn mix(sandbox: &str) -> (Vec<u8>, u32) {
    let cpu = cpu_random(64);
    let mut out = cpu.clone();
    let mut kernel = [0u8; 32];
    let _ = getrandom::fill(&mut kernel);
    out.extend_from_slice(&kernel);
    out.extend_from_slice(sandbox.as_bytes());
    #[cfg(target_os = "linux")]
    for clock in [nix::libc::CLOCK_REALTIME, nix::libc::CLOCK_MONOTONIC, nix::libc::CLOCK_BOOTTIME] {
        let mut t = nix::libc::timespec { tv_sec: 0, tv_nsec: 0 };
        // SAFETY: `t` is a timespec to write into.
        if unsafe { nix::libc::clock_gettime(clock, &mut t) } == 0 {
            out.extend_from_slice(&t.tv_sec.to_le_bytes());
            out.extend_from_slice(&t.tv_nsec.to_le_bytes());
        }
    }
    #[cfg(not(target_os = "linux"))]
    out.extend_from_slice(&arugula_e2e::now_ms().to_le_bytes());
    (out, (cpu.len() * 8) as u32)
}

/// Up to `n` bytes from RDSEED, else RDRAND; none on a CPU without them.
fn cpu_random(n: usize) -> Vec<u8> {
    #[cfg(target_arch = "x86_64")]
    {
        if std::arch::is_x86_feature_detected!("rdseed") {
            // SAFETY: the CPU has RDSEED, checked just above.
            return unsafe { draw(n, rdseed) };
        }
        if std::arch::is_x86_feature_detected!("rdrand") {
            // SAFETY: the CPU has RDRAND, checked just above.
            return unsafe { draw(n, rdrand) };
        }
    }
    let _ = n;
    Vec::new()
}

#[cfg(target_arch = "x86_64")]
unsafe fn draw(n: usize, step: unsafe fn() -> Option<u64>) -> Vec<u8> {
    let mut out = Vec::with_capacity(n);
    while out.len() < n {
        // Either may run dry for a moment: try a while, then give up.
        let Some(v) = (0..1000).find_map(|_| unsafe { step() }) else { break };
        out.extend_from_slice(&v.to_le_bytes());
    }
    out.truncate(n);
    out
}

#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "rdseed")]
unsafe fn rdseed() -> Option<u64> {
    let mut v = 0u64;
    (std::arch::x86_64::_rdseed64_step(&mut v) == 1).then_some(v)
}

#[cfg(target_arch = "x86_64")]
#[target_feature(enable = "rdrand")]
unsafe fn rdrand() -> Option<u64> {
    let mut v = 0u64;
    (std::arch::x86_64::_rdrand64_step(&mut v) == 1).then_some(v)
}

/// `_reseed`: add `bytes` to the kernel's pool, credited `bits`, and reseed
/// it now. Needs root (CAP_SYS_ADMIN).
#[cfg(target_os = "linux")]
pub fn reseed_kernel(bytes: &[u8], bits: u32) -> anyhow::Result<()> {
    use std::os::fd::AsRawFd;
    // `struct rand_pool_info { int entropy_count; int buf_size; __u32 buf[]; }`
    let mut info = Vec::with_capacity(8 + bytes.len().next_multiple_of(4));
    info.extend_from_slice(&(bits.min(bytes.len() as u32 * 8) as i32).to_ne_bytes());
    info.extend_from_slice(&(bytes.len() as i32).to_ne_bytes());
    info.extend_from_slice(bytes);
    info.resize(8 + bytes.len().next_multiple_of(4), 0);
    let f = std::fs::OpenOptions::new().write(true).open("/dev/random")?;
    // RNDADDENTROPY = _IOW('R', 0x03, int[2]); RNDRESEEDCRNG = _IO('R', 0x07).
    const RNDADDENTROPY: u64 = 0x4008_5203;
    const RNDRESEEDCRNG: u64 = 0x5207;
    // SAFETY: `info` is a rand_pool_info of `bytes.len()` bytes, alive for
    // the call; RNDRESEEDCRNG takes no argument.
    unsafe {
        if nix::libc::ioctl(f.as_raw_fd(), RNDADDENTROPY as _, info.as_ptr()) != 0 {
            anyhow::bail!("RNDADDENTROPY: {}", std::io::Error::last_os_error());
        }
        if nix::libc::ioctl(f.as_raw_fd(), RNDRESEEDCRNG as _) != 0 {
            anyhow::bail!("RNDRESEEDCRNG: {}", std::io::Error::last_os_error());
        }
    }
    Ok(())
}

#[cfg(not(target_os = "linux"))]
pub fn reseed_kernel(_bytes: &[u8], _bits: u32) -> anyhow::Result<()> {
    anyhow::bail!("only on Linux")
}

/// `arugulad _reseed <sandbox id>`: run as root by [`fresh`].
pub fn cli(sandbox: &str) -> ! {
    let (bytes, bits) = mix(sandbox);
    match reseed_kernel(&bytes, bits) {
        Ok(()) => {
            eprintln!("reseeded the kernel's random generator ({} bytes, {bits} bits from the CPU)", bytes.len());
            std::process::exit(0)
        }
        Err(e) => {
            eprintln!("can't reseed the kernel's random generator: {e:#}");
            std::process::exit(1)
        }
    }
}

/// A new box: reseed the kernel's generator as root, else mix fresh bytes
/// in as this user. True when it reseeded.
pub fn fresh(sandbox: &str) -> bool {
    if nix::unistd::geteuid().is_root() {
        let (bytes, bits) = mix(sandbox);
        return report(reseed_kernel(&bytes, bits));
    }
    if let Ok(exe) = std::env::current_exe()
        && run_for(Command::new("sudo").arg("-n").arg(exe).arg("_reseed").arg(sandbox), Duration::from_secs(5))
    {
        info!("hosted: reseeded the kernel's random generator for the new box");
        return true;
    }
    let (bytes, _) = mix(sandbox);
    let wrote =
        std::fs::OpenOptions::new().write(true).open(Path::new("/dev/random")).and_then(|mut f| f.write_all(&bytes));
    warn!(
        mixed_in = wrote.is_ok(),
        "hosted: can't reseed the kernel's random generator (no `sudo -n arugulad _reseed`): fresh bytes mixed in for its next reseed"
    );
    false
}

fn report(r: anyhow::Result<()>) -> bool {
    match r {
        Ok(()) => {
            info!("hosted: reseeded the kernel's random generator for the new box");
            true
        }
        Err(e) => {
            warn!(error = %format!("{e:#}"), "hosted: can't reseed the kernel's random generator");
            false
        }
    }
}

/// Run `cmd`, true if it exits 0 within `limit`.
fn run_for(cmd: &mut Command, limit: Duration) -> bool {
    let Ok(mut child) = cmd.stdin(std::process::Stdio::null()).stdout(std::process::Stdio::null()).spawn() else {
        return false;
    };
    let until = std::time::Instant::now() + limit;
    loop {
        match child.try_wait() {
            Ok(Some(s)) => return s.success(),
            Ok(None) if std::time::Instant::now() < until => std::thread::sleep(Duration::from_millis(20)),
            _ => {
                let _ = child.kill();
                return false;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_mix_is_new_each_time_and_names_the_box() {
        let (a, bits) = mix("sbx1");
        let (b, _) = mix("sbx1");
        assert_ne!(a, b);
        assert!(a.windows(4).any(|w| w == b"sbx1"));
        // This machine's CPU, if it has RDSEED or RDRAND, gave 64 bytes.
        assert!(bits == 0 || bits == 512, "{bits}");
    }

    #[test]
    fn reseeding_needs_root() {
        if nix::unistd::geteuid().is_root() {
            return;
        }
        let (bytes, bits) = mix("sbx1");
        assert!(reseed_kernel(&bytes, bits).is_err());
    }
}
