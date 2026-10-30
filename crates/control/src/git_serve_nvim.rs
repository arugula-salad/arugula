//! `/git/nvim` as a plugin manager meets it: real `git` clones it over HTTP
//! from a control served on a local port, and a real headless nvim loads the
//! result from its `pack/*/start/` directory (#1748). lazy.nvim's and
//! vim-plug's own install lines are not run: both come from GitHub, and CI
//! downloads nothing for them. Skipped, saying so, without `git` or an nvim
//! of 0.10 or later on PATH (CI's setup action installs one).

use std::{
    net::SocketAddr,
    path::{Path, PathBuf},
    process::Output,
};

use tokio::process::Command;

use crate::App;

/// `name --version` ran: the tool is on PATH.
fn on_path(name: &str) -> bool {
    std::process::Command::new(name).arg("--version").output().is_ok_and(|o| o.status.success())
}

/// nvim's `(major, minor)`: `NVIM v0.11.6`.
fn nvim_version() -> Option<(u32, u32)> {
    let out = std::process::Command::new("nvim").arg("--version").output().ok()?;
    let first = String::from_utf8_lossy(&out.stdout).lines().next()?.to_owned();
    let v = first.split_whitespace().nth(1)?.trim_start_matches('v').to_owned();
    let mut n = v.split('.').map(|p| p.parse::<u32>().ok());
    Some((n.next()??, n.next()??))
}

/// A fresh directory for one test, in the temp directory, named for this
/// process so runs don't meet each other's files.
fn scratch(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("arugula-git-nvim-{}-{name}", std::process::id()));
    std::fs::create_dir_all(&d).unwrap();
    d
}

async fn run(cmd: &mut Command) -> Output {
    cmd.output().await.unwrap_or_else(|e| panic!("can't run {cmd:?}: {e}"))
}

async fn git(dir: &Path, args: &[&str]) -> String {
    let o = run(Command::new("git")
        .current_dir(dir)
        // Nothing from the machine's own git config (a url rewrite, a
        // helper) reaches the clone.
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_SYSTEM", "/dev/null")
        .env("GIT_TERMINAL_PROMPT", "0")
        .args(args))
    .await;
    assert!(
        o.status.success(),
        "git {args:?} failed: {}{}",
        String::from_utf8_lossy(&o.stdout),
        String::from_utf8_lossy(&o.stderr)
    );
    String::from_utf8_lossy(&o.stdout).trim().to_owned()
}

/// Control on a local port, serving the router as `main` does.
async fn control() -> String {
    let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", l.local_addr().unwrap());
    let app = App::for_tests(&base).start();
    let svc = crate::router(app).into_make_service_with_connect_info::<SocketAddr>();
    tokio::spawn(async move { axum::serve(l, svc).await.unwrap() });
    base
}

/// Runs the Lua in a headless `nvim --clean` whose packpath is `site`, with
/// no daemon to find, and returns what the script wrote to its `out` file.
async fn nvim(scratch: &Path, site: &Path, lua: &str) -> String {
    let script = scratch.join("check.lua");
    let out = scratch.join("check.out");
    std::fs::write(&out, "").unwrap();
    std::fs::write(&script, format!("local out = {:?}\n{lua}", out.to_string_lossy())).unwrap();
    let o = run(Command::new("nvim")
        .arg("--headless")
        .arg("--clean")
        .arg("--cmd")
        .arg(format!("set packpath^={}", site.display()))
        .arg("-c")
        .arg(format!("luafile {}", script.display()))
        .arg("-c")
        .arg("qa!")
        .env("XDG_DATA_HOME", scratch.join("xdg-data"))
        .env("XDG_STATE_HOME", scratch.join("xdg-state"))
        .env("XDG_CONFIG_HOME", scratch.join("xdg-config"))
        // No daemon is there: the health check must say so, not hang or crash.
        .env("ARUGULA_SOCK", scratch.join("no-daemon.sock"))
        .env_remove("ARUGULA_PANE"))
    .await;
    let report = std::fs::read_to_string(&out).unwrap();
    assert!(
        !report.is_empty(),
        "nvim wrote nothing: {}{}",
        String::from_utf8_lossy(&o.stdout),
        String::from_utf8_lossy(&o.stderr)
    );
    report
}

#[tokio::test(flavor = "multi_thread")]
async fn a_git_clone_of_nvim_loads_in_a_real_nvim() {
    if !on_path("git") {
        eprintln!("SKIPPED: no git here");
        return;
    }
    match nvim_version() {
        None => return eprintln!("SKIPPED: no nvim here (CI installs it; 0.10 or later)"),
        Some(v) if v < (0, 10) => return eprintln!("SKIPPED: nvim {}.{} is older than 0.10", v.0, v.1),
        Some(_) => {}
    }
    let base = control().await;
    let s = scratch("clone");
    let site = s.join("site");
    let dest = site.join("pack/test/start/arugula.nvim");
    git(&s, &["clone", "--depth", "1", &format!("{base}/git/nvim"), dest.to_str().unwrap()]).await;
    assert!(dest.join("lua/arugula/init.lua").is_file() && dest.join("plugin").is_dir());
    assert_eq!(git(&dest, &["rev-parse", "--is-shallow-repository"]).await, "true");

    let report = nvim(
        &s,
        &site,
        r#"
local lines = {}
local ok, mod = pcall(require, "arugula")
table.insert(lines, "require=" .. tostring(ok) .. ":" .. type(mod))
table.insert(lines, "loaded=" .. tostring(vim.g.loaded_arugula))
table.insert(lines, "command=" .. tostring(vim.fn.exists(":ArugulaStatus")))
local hok, herr = pcall(vim.cmd, "checkhealth arugula")
table.insert(lines, "health=" .. tostring(hok) .. ":" .. tostring(herr))
table.insert(lines, "buf=" .. table.concat(vim.api.nvim_buf_get_lines(0, 0, -1, false), "\n"))
local f = assert(io.open(out, "w"))
f:write(table.concat(lines, "\n"))
f:close()
"#,
    )
    .await;
    assert!(report.contains("require=true:table"), "require(\"arugula\") failed:\n{report}");
    assert!(report.contains("loaded=true"), "the plugin/ file didn't run:\n{report}");
    assert!(report.contains("command=2"), "no :ArugulaStatus:\n{report}");
    // The health check ran. It has no daemon to find; that's the expected answer.
    assert!(report.contains("health=true:"), "`:checkhealth arugula` raised:\n{report}");
    assert!(report.contains("arugula"), "the health report is empty:\n{report}");
}

#[tokio::test(flavor = "multi_thread")]
async fn a_full_clone_updates_with_a_fast_forward_fetch() {
    if !on_path("git") {
        eprintln!("SKIPPED: no git here");
        return;
    }
    let base = control().await;
    let s = scratch("update");
    let url = format!("{base}/git/nvim");
    let dest = s.join("arugula.nvim");
    git(&s, &["clone", &url, dest.to_str().unwrap()]).await;
    let head = git(&dest, &["rev-parse", "HEAD"]).await;
    git(&dest, &["fsck", "--strict"]).await;
    // `nvim.git` is the same repository.
    git(&s, &["ls-remote", "--exit-code", &format!("{base}/git/nvim.git"), "refs/heads/main"]).await;
    if git(&dest, &["rev-list", "--count", "HEAD"]).await == "1" {
        // A build whose checkout has no release tags serves one commit (build.rs).
        eprintln!("SKIPPED the fast-forward: this build serves a single commit");
        return;
    }
    git(&dest, &["reset", "--hard", "HEAD~1"]).await;
    assert_ne!(git(&dest, &["rev-parse", "HEAD"]).await, head);
    git(&dest, &["fetch", "origin"]).await;
    git(&dest, &["merge", "--ff-only", "origin/main"]).await;
    assert_eq!(git(&dest, &["rev-parse", "HEAD"]).await, head);
}
