//! The chant→Fly driver's apply (M220), against a fake chant, a fake
//! flyctl and a fake npm that record what they're run with, and a local
//! origin: it checks the token, runs chant's op, approves the op's own gate
//! as the approver, ties chant's new ledger entry to what Fly runs, and
//! rolls back through Fly and chant. Each program gets only the credentials
//! it's meant to, and only in its environment; the result comes on fd 3.

use std::{
    io::Read as _,
    os::{fd::AsRawFd as _, unix::process::CommandExt as _},
    path::Path,
    process::Command,
};

use super::{CHANT_FLY, RESULT_FD, result_of};

const TOKEN: &str = "fly-token-m220-secret"; // gitleaks:allow
const GH: &str = "gh-token-m220-secret"; // gitleaks:allow

fn image(n: char) -> String {
    format!("registry.fly.io/acme-web:deployment-{n}@sha256:{}", n.to_string().repeat(64))
}

/// A fake chant: `run` stops at a gate (of $FAKE_DIR/gate-op's op, or its
/// own) until it's approved, then deploys (a new image) and writes a
/// release record to chant's ledger and pushes it; `approve` records the
/// approval; `components release` records a release again. Each call goes
/// in $FAKE_LOG with which tokens it saw.
const FAKE_CHANT: &str = r#"#!/bin/bash
set -e
fly=no; [ "$FLY_API_TOKEN" = "$FAKE_TOKEN" ] && fly=yes
gh=no; [ -n "$GITHUB_TOKEN" ] && gh=yes
echo "chant $* | fly:$fly gh:$gh user:$USER" >> "$FAKE_LOG"
append() {
  local env=$1 line=$2 parent old blob tree commit
  export GIT_INDEX_FILE=$FAKE_DIR/index.$$
  parent=$(git rev-parse -q --verify refs/heads/chant/lifecycle || true)
  rm -f "$GIT_INDEX_FILE"
  [ -n "$parent" ] && git read-tree "$parent"
  old=$( [ -n "$parent" ] && git show "$parent:$env/releases.jsonl" 2>/dev/null || true )
  blob=$( { [ -n "$old" ] && printf '%s\n' "$old"; printf '%s\n' "$line"; } | git hash-object -w --stdin)
  git update-index --add --cacheinfo "100644,$blob,$env/releases.jsonl"
  tree=$(git write-tree)
  commit=$(git commit-tree "$tree" ${parent:+-p "$parent"} -m release)
  git update-ref refs/heads/chant/lifecycle "$commit"
  rm -f "$GIT_INDEX_FILE"
  git push -q origin chant/lifecycle 2>/dev/null || true
}
case "$1 $2" in
  "run release")
    if [ ! -e "$FAKE_DIR/approved" ]; then
      gop=$(cat "$FAKE_DIR/gate-op" 2>/dev/null || echo release)
      echo "{\"status\":\"gated\",\"gate\":{\"op\":\"$gop\",\"gate\":\"approve-release\",\"planDigest\":\"sha256:abc\"}}"
      exit 3
    fi
    [ -e "$FAKE_DIR/no-deploy" ] || cp "$FAKE_DIR/next-image" "$FAKE_DIR/image"
    [ -e "$FAKE_DIR/no-record" ] && exit 0
    sha=$(git rev-parse HEAD)
    append "${FAKE_ENV:-github}" "{\"version\":1,\"component\":\"acme\",\"env\":\"${FAKE_ENV:-github}\",\"digest\":\"sha256:d1\",\"gitSha\":\"$sha\",\"runId\":\"local-1\",\"timestamp\":\"2026-10-22T00:00:01Z\",\"actor\":\"$USER\",\"approver\":\"acct1\"}"
    echo '{"status":"ok"}' ;;
  "approve release") touch "$FAKE_DIR/approved" ;;
  "components release")
    sha=; run=; prev=
    for a in "$@"; do case "$prev" in --git-sha) sha=$a;; --run-id) run=$a;; esac; prev=$a; done
    append "$3" "{\"version\":1,\"component\":\"acme\",\"env\":\"$3\",\"digest\":\"sha256:d1\",\"gitSha\":\"$sha\",\"runId\":\"$run\",\"timestamp\":\"2026-10-22T00:00:09Z\",\"actor\":\"$USER\",\"approver\":\"acct1\"}" ;;
  *) echo "fake chant: $*?" >&2; exit 2 ;;
esac
"#;

/// A fake flyctl: `tokens debug` decodes the token as $FAKE_DIR/token says
/// (app-scoped for 2 hours, or `org`, or `forever`); `status` says the
/// app's machines (started, on $FAKE_DIR/image, checks passing unless
/// $FAKE_DIR/unhealthy); `deploy --image` deploys one.
const FAKE_FLYCTL: &str = r#"#!/bin/bash
fly=no; [ "$FLY_API_TOKEN" = "$FAKE_TOKEN" ] && fly=yes
gh=no; [ -n "$GITHUB_TOKEN" ] && gh=yes
echo "flyctl $* | fly:$fly gh:$gh" >> "$FAKE_LOG"
img=$(cat "$FAKE_DIR/image")
case "$1" in
  version) echo "flyctl v0.4.104" ;;
  tokens)
    end=$(( $(date +%s) + 7200 ))
    case "$(cat "$FAKE_DIR/token" 2>/dev/null)" in
      org) apps='' ;;
      forever) apps='{"type":"Apps","body":{"apps":{"123":"rwcdC"}}},'; end=$(( end + 20 * 365 * 86400 )) ;;
      *) apps='{"type":"Apps","body":{"apps":{"123":"rwcdC"}}},' ;;
    esac
    echo "[{\"location\":\"https://api.fly.io/v1\",\"caveats\":[{\"type\":\"Organization\",\"body\":{\"id\":1,\"mask\":\"*\"}},$apps{\"type\":\"ValidityWindow\",\"body\":{\"not_before\":1,\"not_after\":$end}}]}]" ;;
  status)
    check=passing; [ -e "$FAKE_DIR/unhealthy" ] && check=critical
    reg=${img%%/*}; rest=${img#*/}; repo=${rest%%:*}; rest=${rest#*:}; tag=${rest%%@*}; dig=${rest#*@}
    printf '{"Machines":[{"id":"m1","state":"started","image_ref":{"registry":"%s","repository":"%s","tag":"%s","digest":"%s"},"checks":[{"status":"%s"}]},{"id":"m2","state":"stopped","image_ref":{}}]}\n' "$reg" "$repo" "$tag" "$dig" "$check" ;;
  deploy)
    prev=; for a in "$@"; do [ "$prev" = --image ] && echo "$a" > "$FAKE_DIR/image"; prev=$a; done ;;
  *) exit 2 ;;
esac
"#;

/// A fake npm: records its arguments and which tokens it saw.
const FAKE_NPM: &str = r#"#!/bin/bash
fly=no; [ -n "$FLY_API_TOKEN" ] && fly=yes
gh=no; [ -n "$GITHUB_TOKEN" ] && gh=yes
echo "npm $* | fly:$fly gh:$gh" >> "$FAKE_LOG"
"#;

struct Rig {
    dir: std::path::PathBuf,
    sha: String,
}

fn sh(dir: &Path, script: &str) -> String {
    let out = Command::new("bash").arg("-c").arg(script).current_dir(dir).output().unwrap();
    assert!(out.status.success(), "{script}: {}", String::from_utf8_lossy(&out.stderr));
    String::from_utf8_lossy(&out.stdout).trim().to_owned()
}

fn exe(p: &Path, text: &str) {
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    std::fs::write(p, text).unwrap();
    use std::os::unix::fs::PermissionsExt as _;
    std::fs::set_permissions(p, std::fs::Permissions::from_mode(0o755)).unwrap();
}

/// An origin (`origin/acme/app.git`) holding one commit with the chant
/// member `ops` (its chant the fake, as `node_modules/.bin/chant`, and a
/// lock file), and flyctl and npm on a PATH of their own. Fly runs image 1;
/// a release deploys image 2.
fn rig(name: &str) -> Rig {
    let dir = crate::hosted::scratch(name);
    let repo = dir.join("repo");
    exe(&repo.join("ops/node_modules/.bin/chant"), FAKE_CHANT);
    std::fs::write(repo.join("ops/package-lock.json"), "{}\n").unwrap();
    exe(&dir.join("bin/flyctl"), FAKE_FLYCTL);
    exe(&dir.join("bin/npm"), FAKE_NPM);
    std::fs::write(dir.join("image"), image('1')).unwrap();
    std::fs::write(dir.join("next-image"), image('2')).unwrap();
    std::fs::write(repo.join("fly.toml"), "app = \"acme-web\"\n").unwrap();
    sh(
        &dir,
        "git init -q --bare origin/acme/app.git && cd repo && git init -q -b main && git add -A \
         && git -c user.name=t -c user.email=t@t commit -q -m one && git push -q ../origin/acme/app.git main",
    );
    let sha = sh(&repo, "git rev-parse HEAD");
    std::fs::create_dir_all(dir.join("home")).unwrap();
    Rig { dir, sha }
}

fn node() -> Option<String> {
    let o = Command::new("bash").args(["-c", "command -v node"]).output().ok()?;
    o.status.success().then(|| String::from_utf8_lossy(&o.stdout).trim().to_owned())
}

/// What a run of the driver did: its exit, its output, its result (fd 3).
struct Ran {
    code: i32,
    out: String,
    result: Option<serde_json::Value>,
}

impl Rig {
    /// Run the driver as arugulad would: an empty environment but the
    /// deploy's and its secrets, its result pipe as fd 3.
    fn apply(&self, settings: &str, roll_back: Option<String>, extra: &[(&str, &str)]) -> Ran {
        let node = node().unwrap();
        let node_dir = Path::new(&node).parent().unwrap().to_string_lossy().into_owned();
        let path = format!("{}:{node_dir}:/usr/bin:/bin", self.dir.join("bin").display());
        let (r, w) = nix::unistd::pipe2(nix::fcntl::OFlag::O_CLOEXEC).unwrap();
        let wfd = w.as_raw_fd();
        let mut c = Command::new(&node);
        c.env_clear()
            .args(["--input-type=module", "-e", CHANT_FLY])
            .current_dir("/")
            .env("PATH", path)
            .env("HOME", self.dir.join("home"))
            .env("USER", "runner")
            .env("DEPLOY_ID", format!("dep_{}", "1".repeat(32)))
            .env("DEPLOY_RUN", "run_1")
            .env("DEPLOY_ENVIRONMENT", "production")
            .env("DEPLOY_SOURCE_KIND", "commit")
            .env("DEPLOY_SOURCE_REF", &self.sha)
            .env("DEPLOY_SOURCE_WHERE", "acme/app")
            .env("DEPLOY_APPROVER", "acct1")
            .env("DEPLOY_APPROVED_BY", "dev1")
            .env("DEPLOY_APPROVAL", "apv_1")
            .env("DEPLOY_OP_HASH", "ab12")
            .env("DEPLOY_SETTINGS", settings)
            .env("DEPLOY_ROLL_BACK", roll_back.unwrap_or_default())
            .env("ARUGULA_CHANT_FLY_TEST", "1")
            .env("DEPLOY_GIT_BASE", format!("{}/origin/", self.dir.display()))
            .env("DEPLOY_FLY_CHECKS", "2x0")
            .env("FLY_API_TOKEN", TOKEN)
            .env("GITHUB_TOKEN", GH)
            .env("FAKE_TOKEN", TOKEN)
            .env("FAKE_LOG", self.dir.join("log"))
            .env("FAKE_DIR", &self.dir)
            .envs(extra.iter().copied());
        // SAFETY: dup2 only, between fork and exec.
        unsafe {
            c.pre_exec(move || {
                if nix::libc::dup2(wfd, RESULT_FD) < 0 {
                    return Err(std::io::Error::last_os_error());
                }
                Ok(())
            });
        }
        let out = c.output().unwrap();
        drop(w);
        let mut said = Vec::new();
        std::fs::File::from(r).read_to_end(&mut said).unwrap();
        let text = format!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
        Ran { code: out.status.code().unwrap_or(-1), out: text, result: result_of(&said) }
    }

    fn log(&self) -> Vec<String> {
        std::fs::read_to_string(self.dir.join("log")).unwrap_or_default().lines().map(str::to_owned).collect()
    }

    /// The programs it ran, without what they saw.
    fn calls(&self) -> Vec<String> {
        self.log().iter().map(|l| l.split(" | ").next().unwrap().to_owned()).collect()
    }

    fn origin(&self, script: &str) -> String {
        sh(&self.dir.join("origin/acme/app.git"), script)
    }
}

const SETTINGS: &str =
    r#"{"chant_dir":"ops","chant_env":"github","chant_op":"release","chant_digest":"plan","fly_app":"acme-web"}"#;

fn with(more: &str) -> String {
    format!("{},{more}}}", SETTINGS.trim_end_matches('}'))
}

macro_rules! needs_node {
    () => {
        if node().is_none() {
            eprintln!("skipped: no node here");
            return;
        }
    };
}

#[test]
fn chant_fly_releases_through_chants_gate_and_links_its_ledger() {
    needs_node!();
    let r = rig("chant-fly-apply");
    let ran = r.apply(SETTINGS, None, &[]);
    assert_eq!(ran.code, 0, "{}", ran.out);
    let calls = r.calls();
    let at = |prefix: &str| {
        calls.iter().position(|c| c.starts_with(prefix)).unwrap_or_else(|| panic!("{prefix}: {calls:?}"))
    };
    assert!(at("flyctl tokens debug") < at("npm ci"), "the token's checked before anything of the repo's runs");
    assert_eq!(calls[at("npm ci")], "npm ci --no-fund --no-audit --ignore-scripts");
    let gated = at("chant run release --json");
    assert!(
        calls[gated + 1]
            .starts_with("chant approve release approve-release --approver acct1 --relayed-by arugula --note ")
    );
    assert!(calls[gated + 1].contains("approval apv_1 by acct1 from device dev1, operation sha256:ab12, run run_1"));
    assert!(calls[gated + 1].ends_with("--plan sha256:abc"), "{}", calls[gated + 1]);
    assert_eq!(calls[gated + 2], "chant run release --json");
    assert_eq!(calls.last().unwrap(), "flyctl status --app acme-web --json");
    // Who saw which token: chant's op and flyctl the Fly token, nobody
    // GitHub's but the driver's own git; npm and chant approve neither.
    for l in r.log() {
        let (call, saw) = l.split_once(" | ").unwrap();
        let want = if call.starts_with("npm") || call.starts_with("chant approve") || call == "flyctl version" {
            "fly:no gh:no"
        } else {
            "fly:yes gh:no"
        };
        assert!(saw.starts_with(want), "{l}");
        assert!(!call.contains(TOKEN) && !call.contains(GH), "on a command line: {l}");
    }
    assert!(r.log().iter().filter(|l| l.starts_with("chant")).all(|l| l.ends_with("user:arugula")));
    assert!(!ran.out.contains(TOKEN) && !ran.out.contains(GH), "{}", ran.out);
    assert!(!ran.out.contains("\"chant_ledger\""), "the result isn't on its output: {}", ran.out);

    let result = ran.result.expect("its result, on fd 3");
    let tip = r.origin("git rev-parse refs/heads/chant/lifecycle");
    assert_eq!(result["chant_ledger"], format!("https://github.com/acme/app/blob/{tip}/github/releases.jsonl#L1"));
    assert_eq!(result["chant_record"]["gitSha"], r.sha);
    assert_eq!(result["fly"], serde_json::json!({ "app": "acme-web", "image": image('2'), "healthy": true }));
    let line = r.origin(&format!("git show {tip}:github/releases.jsonl"));
    assert!(line.contains("\"approver\":\"acct1\"") && !line.contains(TOKEN), "{line}");
}

/// The settings the plan names change what runs: install scripts, chant's
/// GitHub token, the gates it approves.
#[test]
fn chant_fly_does_what_its_settings_say() {
    needs_node!();
    let r = rig("chant-fly-settings");
    let ran =
        r.apply(&with(r#""npm_scripts":"true","chant_github":"true","chant_gates":"approve-release""#), None, &[]);
    assert_eq!(ran.code, 0, "{}", ran.out);
    assert!(r.calls().contains(&"npm ci --no-fund --no-audit".to_owned()), "{:?}", r.calls());
    assert!(r.log().iter().any(|l| l.starts_with("chant run") && l.contains("fly:yes gh:yes")), "{:?}", r.log());

    // A gate the plan doesn't name, or another op's: never approved.
    for (name, settings, gate_op, says) in [
        ("chant-fly-gates", with(r#""chant_gates":"ship""#), None, "which the plan doesn't name"),
        ("chant-fly-gate-op", SETTINGS.to_owned(), Some("other"), "another op's gate"),
    ] {
        let r = rig(name);
        if let Some(o) = gate_op {
            std::fs::write(r.dir.join("gate-op"), o).unwrap();
        }
        let ran = r.apply(&settings, None, &[]);
        assert_eq!(ran.code, 1, "{}", ran.out);
        assert!(ran.out.contains(says), "{}", ran.out);
        assert!(!r.calls().iter().any(|c| c.starts_with("chant approve")), "{:?}", r.calls());
    }
}

/// A token for the whole org, or one that doesn't expire soon: nothing of
/// the repo's runs.
#[test]
fn chant_fly_wants_an_app_token_that_expires() {
    needs_node!();
    for (kind, says) in [("org", "isn't scoped to one app"), ("forever", "longer than 30 days")] {
        let r = rig(&format!("chant-fly-token-{kind}"));
        std::fs::write(r.dir.join("token"), kind).unwrap();
        let ran = r.apply(SETTINGS, None, &[]);
        assert_eq!(ran.code, 1, "{}", ran.out);
        assert!(
            ran.out.contains(says) && ran.out.contains("fly tokens create deploy -a acme-web -x 720h"),
            "{}",
            ran.out
        );
        assert!(!r.calls().iter().any(|c| c.starts_with("npm") || c.starts_with("chant")), "{:?}", r.calls());
        assert!(ran.result.is_none());
    }
}

/// What it links is this release's: a record new in this run, in the
/// plan's ledger environment, and Fly running what it released.
#[test]
fn chant_fly_ties_its_evidence_to_this_release() {
    needs_node!();
    // Fly unhealthy: failed, its entry reported still.
    let r = rig("chant-fly-unhealthy");
    std::fs::write(r.dir.join("unhealthy"), "").unwrap();
    let ran = r.apply(SETTINGS, None, &[]);
    assert_eq!(ran.code, 5, "{}", ran.out);
    let result = ran.result.unwrap();
    assert_eq!(result["fly"]["healthy"], false);
    assert!(result["chant_ledger"].is_string(), "the release went out: its entry is linked still");

    // The release didn't reach Fly (it runs the image it ran before).
    let r = rig("chant-fly-no-deploy");
    std::fs::write(r.dir.join("no-deploy"), "").unwrap();
    let ran = r.apply(SETTINGS, None, &[]);
    assert_eq!(ran.code, 6, "{}", ran.out);
    // The record's digest must be the image's, when the plan says so.
    let r = rig("chant-fly-digest");
    let ran = r.apply(&with(r#""chant_digest":"image""#).replace(r#""chant_digest":"plan","#, ""), None, &[]);
    assert_eq!(ran.code, 6, "{}", ran.out);
    assert!(ran.out.contains("not the image Fly runs"), "{}", ran.out);

    // No record in this run, or one in another ledger environment.
    for (name, extra) in [("chant-fly-no-record", None), ("chant-fly-other-env", Some(("FAKE_ENV", "staging")))] {
        let r = rig(name);
        if extra.is_none() {
            std::fs::write(r.dir.join("no-record"), "").unwrap();
        }
        let ran = r.apply(SETTINGS, None, extra.as_slice());
        assert_eq!(ran.code, 4, "{}", ran.out);
        assert!(ran.result.unwrap()["chant_ledger"].is_null());
    }

    // An origin that won't take a push (as GitHub refuses one without a
    // token that can write): nothing runs.
    let r = rig("chant-fly-readonly");
    let refuse =
        [("GIT_CONFIG_COUNT", "1"), ("GIT_CONFIG_KEY_0", "remote.origin.receivepack"), ("GIT_CONFIG_VALUE_0", "false")];
    let ran = r.apply(SETTINGS, None, &refuse);
    assert_eq!(ran.code, 1, "{}", ran.out);
    assert!(ran.out.contains("origin won't take a push"), "{}", ran.out);
    assert!(!r.calls().iter().any(|c| c.starts_with("chant")), "{:?}", r.calls());
}

/// A short id, or one that isn't the commit checked out: refused.
#[test]
fn chant_fly_checks_out_a_full_commit_id_exactly() {
    needs_node!();
    let r = rig("chant-fly-short");
    let mut short = Rig { dir: r.dir.clone(), sha: r.sha[..12].to_owned() };
    let ran = short.apply(SETTINGS, None, &[]);
    assert_eq!(ran.code, 1, "{}", ran.out);
    assert!(ran.out.contains("by its full id"), "{}", ran.out);
    short.sha = "f".repeat(40);
    let ran = short.apply(SETTINGS, None, &[]);
    assert_ne!(ran.code, 0, "{}", ran.out);
    assert!(!r.calls().iter().any(|c| c.starts_with("chant")), "{:?}", r.calls());
}

#[test]
fn chant_fly_rolls_back_through_fly_and_chant() {
    needs_node!();
    let r = rig("chant-fly-back");
    std::fs::write(r.dir.join("image"), image('2')).unwrap();
    let back = serde_json::json!({
        "deploy": format!("dep_{}", "0".repeat(32)),
        "to": { "chant_component": "acme", "chant_digest": "sha256:d1", "chant_env": "github", "image": image('1') },
    });
    let ran = r.apply(SETTINGS, Some(back.to_string()), &[]);
    assert_eq!(ran.code, 0, "{}", ran.out);
    let calls = r.calls();
    let deploy = calls.iter().position(|c| c.starts_with("flyctl deploy")).unwrap();
    assert_eq!(calls[deploy], format!("flyctl deploy --app acme-web --image {} --yes", image('1')));
    assert_eq!(calls[deploy + 1], "flyctl status --app acme-web --json");
    assert_eq!(
        calls[deploy + 2],
        format!(
            "chant components release github --component acme --digest sha256:d1 --git-sha {} --run-id run_1 \
             --actor arugula --approver acct1",
            r.sha
        )
    );
    assert!(!calls.iter().any(|c| c.starts_with("chant run")), "no release op on a roll back");
    let release = r.log().into_iter().find(|l| l.starts_with("chant components")).unwrap();
    assert!(release.contains("fly:no gh:no"), "chant's own command gets no token: {release}");
    let result = ran.result.unwrap();
    assert_eq!(
        (result["fly"]["image"].as_str(), result["fly"]["healthy"].as_bool()),
        (Some(image('1').as_str()), Some(true))
    );
    assert_eq!(result["chant_record"]["runId"], "run_1");
    assert!(result["chant_ledger"].as_str().unwrap().ends_with("github/releases.jsonl#L1"), "{result}");
}
