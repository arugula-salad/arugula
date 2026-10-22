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

/// A fake chant, after `@intentius/chant` 0.124.40 as the M220 rig ran it
/// (arugula-salad/deploy-smoke): each command first reads chant's ledger
/// (`chant/lifecycle`) from origin, and refuses when it isn't here and
/// origin can't be reached (chant's `requireLifecycleLedger`); origin, as
/// GitHub answers for a private repository, answers only a git that has
/// GITHUB_TOKEN, and any other remote (the driver's stand-in) answers.
/// `run release --json` stops at its gate (exit 3) until a resolution of
/// this commit's plan is in the ledger: it writes a pending fact
/// (`_gates/release.jsonl`, its `planDigest`) and prints its run record
/// (`gate: {name, since}`, `approve`), as chant does; then it deploys (a
/// new image) and records the release. `approve` resolves the gate for the
/// plan given (`--plan` must be the pending one's); `components release`
/// records a release again. Each push goes to origin when it's reachable,
/// a warning otherwise. Each call goes in $FAKE_LOG with which tokens it
/// saw.
const FAKE_CHANT: &str = r#"#!/bin/bash
set -e
fly=no; [ "$FLY_API_TOKEN" = "$FAKE_TOKEN" ] && fly=yes
gh=no; [ -n "$GITHUB_TOKEN" ] && gh=yes
echo "chant $* | fly:$fly gh:$gh user:$USER" >> "$FAKE_LOG"
export LC_ALL=C
B=refs/heads/chant/lifecycle
now() { date -u +%Y-%m-%dT%H:%M:%S.000Z; }
reachable() { [ -n "$GITHUB_TOKEN" ] || [ "$(git remote get-url origin)" != "$FAKE_ORIGIN" ]; }
ledger() {
  local had err
  had=$(git rev-parse -q --verify $B || true)
  if reachable; then
    err=$(git fetch -q origin chant/lifecycle:chant/lifecycle 2>&1) && return 0
    case "$err" in *"couldn't find remote ref"*) return 0 ;; esac
  fi
  [ -n "$had" ] && return 0
  echo "the chant/lifecycle ledger branch is not in this checkout and could not be fetched from \"origin\", so what is on it cannot be read. Refusing rather than treating an unreadable ledger as an empty one." >&2
  return 1
}
append() {
  local file=$1 line=$2 parent old blob tree commit
  export GIT_INDEX_FILE=$FAKE_DIR/index.$$
  parent=$(git rev-parse -q --verify $B || true)
  rm -f "$GIT_INDEX_FILE"
  [ -n "$parent" ] && git read-tree "$parent"
  old=$( [ -n "$parent" ] && git show "$parent:$file" 2>/dev/null || true )
  blob=$( { [ -n "$old" ] && printf '%s\n' "$old"; printf '%s\n' "$line"; } | git hash-object -w --stdin)
  git update-index --add --cacheinfo "100644,$blob,$file"
  tree=$(git write-tree)
  commit=$(git commit-tree "$tree" ${parent:+-p "$parent"} -m "chant: $file")
  git update-ref $B "$commit"
  rm -f "$GIT_INDEX_FILE"
  unset GIT_INDEX_FILE
}
push() {
  if reachable && git push -q origin chant/lifecycle:chant/lifecycle 2>/dev/null; then return 0; fi
  echo "warning: The push to the remote did not land: the push of chant/lifecycle to \"origin\" failed" >&2
}
sha=$(git rev-parse HEAD)
plan="sha256:$(printf 'plan %s %s' "$sha" "$(cat "$FAKE_DIR/plan-salt" 2>/dev/null)" | sha256sum | cut -c1-64)"
case "$1 $2" in
  "run release")
    ledger || { echo '{"version":1,"op":"release","status":"fail"}'; exit 1; }
    op=$(cat "$FAKE_DIR/gate-op" 2>/dev/null || echo release)
    gate=approve-release
    if ! git show $B:_gates/release.jsonl 2>/dev/null | grep '"resolvedBy"' | grep -F "\"gitSha\":\"$sha\"" | grep -qF "\"planDigest\":\"$plan\""; then
      id="local-$(date +%s%N | cut -c1-13)"; t=$(now)
      if [ -e "$FAKE_DIR/no-plan" ]; then pd=; hint="chant approve $op $gate"
      else pd="\"planDigest\":\"$plan\","; hint="chant approve $op $gate --plan $plan"; fi
      [ -e "$FAKE_DIR/other-hint" ] && hint="chant approve $op $gate --plan sha256:$(printf '%064d' 0)"
      append _gates/$op.jsonl "{\"description\":\"Approve deploying this commit to acme-web\",\"expiresAt\":\"$t\",\"gate\":\"$gate\",\"gitSha\":\"$sha\",\"kind\":\"pending\",\"neverOverMcp\":true,\"op\":\"$op\",\"origin\":\"cli\",$pd\"runId\":\"$id\",\"timestamp\":\"$t\",\"version\":1}"
      push
      echo "{\"version\":1,\"id\":\"$id\",\"op\":\"$op\",\"env\":\"local\",\"started\":\"$t\",\"ended\":\"$t\",\"status\":\"gated\",\"labels\":{},\"outcomes\":{},\"phases\":[{\"name\":\"Plan\",\"status\":\"ok\",\"steps\":[{\"fn\":\"shellCmd\",\"status\":\"ok\",\"durationMs\":23}],\"durationMs\":23},{\"name\":\"Approve\",\"status\":\"skipped\",\"steps\":[{\"fn\":\"gate:$gate\",\"status\":\"skipped\",\"durationMs\":118}],\"durationMs\":119}],\"gate\":{\"name\":\"$gate\",\"since\":\"$t\"},\"gitSha\":\"$sha\",\"approve\":\"$hint\"}"
      exit 3
    fi
    [ -e "$FAKE_DIR/no-deploy" ] || cp "$FAKE_DIR/next-image" "$FAKE_DIR/image"
    [ -e "$FAKE_DIR/no-record" ] && exit 0
    id="local-$(date +%s%N | cut -c1-13)"; env=${FAKE_ENV:-github}
    append $env/releases.jsonl "{\"actor\":\"$USER\",\"component\":\"acme\",\"digest\":\"sha256:d1\",\"env\":\"$env\",\"gitSha\":\"$sha\",\"runId\":\"$id\",\"timestamp\":\"$(now)\",\"version\":1}"
    push
    echo "{\"version\":1,\"id\":\"$id\",\"op\":\"release\",\"env\":\"local\",\"status\":\"ok\",\"gitSha\":\"$sha\"}" ;;
  "approve release")
    ledger || exit 1
    gate=$3; by=; relayed=; note=; given=; prev=
    for a in "$@"; do case "$prev" in --approver) by=$a;; --relayed-by) relayed=$a;; --note) note=$a;; --plan) given=$a;; esac; prev=$a; done
    pending=$(git show $B:_gates/release.jsonl 2>/dev/null | grep '"kind":"pending"' | grep -F "\"gitSha\":\"$sha\"" | tail -1)
    [ -n "$pending" ] || { echo "no pending gate $gate for $sha" >&2; exit 1; }
    case "$pending" in *"\"planDigest\":\"$given\""*) ;; *) echo "that plan isn't the pending one's" >&2; exit 1 ;; esac
    runid=$(printf '%s' "$pending" | sed 's/.*"runId":"\([^"]*\)".*/\1/')
    append _gates/release.jsonl "{\"approver\":{\"kind\":\"human\"},\"gate\":\"$gate\",\"gitSha\":\"$sha\",\"note\":\"$note\",\"op\":\"release\",\"origin\":\"cli\",\"planDigest\":\"$given\",\"relayedBy\":\"$relayed\",\"resolvedBy\":\"$by\",\"runId\":\"$runid\",\"timestamp\":\"$(now)\",\"version\":1}"
    push
    echo "Gate \"$gate\" on \"release\" resolved by $by, relayed by $relayed" ;;
  "components release")
    ledger || exit 1
    gsha=; run=; actor=; by=; prev=
    for a in "$@"; do case "$prev" in --git-sha) gsha=$a;; --run-id) run=$a;; --actor) actor=$a;; --approver) by=$a;; esac; prev=$a; done
    append "$3/releases.jsonl" "{\"actor\":\"$actor\",\"approver\":\"$by\",\"component\":\"acme\",\"digest\":\"sha256:d1\",\"env\":\"$3\",\"gitSha\":\"$gsha\",\"runId\":\"$run\",\"timestamp\":\"$(now)\",\"version\":1}"
    push
    echo "Recorded release: acme@$3 -> sha256:d1" ;;
  *) echo "fake chant: $*?" >&2; exit 2 ;;
esac
"#;

/// A fake flyctl, after flyctl v0.4.104: `tokens debug` decodes the token
/// as `fly tokens create deploy -a <app>` makes it (its permission
/// macaroon's Organization, 3P and IfPresent caveats, the IfPresent's
/// `ifs` the app and Fly's features, `else` read; its discharge's
/// ValidityWindow, 2 hours), or as $FAKE_DIR/token says otherwise: `org`
/// (no app), `forever`, `broad` (IfPresent `else` is everything),
/// `org-in-ifs` (IfPresent holds an Organization caveat), `inner-window`
/// (its only window in the IfPresent); `status` says the app's machines
/// (started, on $FAKE_DIR/image, checks passing unless
/// $FAKE_DIR/unhealthy); `deploy --image` deploys one.
const FAKE_FLYCTL: &str = r#"#!/bin/bash
fly=no; [ "$FLY_API_TOKEN" = "$FAKE_TOKEN" ] && fly=yes
gh=no; [ -n "$GITHUB_TOKEN" ] && gh=yes
echo "flyctl $* | fly:$fly gh:$gh" >> "$FAKE_LOG"
img=$(cat "$FAKE_DIR/image")
case "$1" in
  version) echo "flyctl v0.4.104 linux/amd64 Commit: 815f54c0 BuildDate: 2026-09-15T17:40:56Z" ;;
  tokens)
    now=$(date +%s); end=$(( now + 7200 ))
    apps='{"type":"Apps","body":{"apps":{"5678":"rwcdC"}}},'; else=r; more=; inner=
    case "$(cat "$FAKE_DIR/token" 2>/dev/null)" in
      org) apps= ;;
      forever) end=$(( end + 20 * 365 * 86400 )) ;;
      broad) else=rwcdC ;;
      org-in-ifs) more='{"type":"Organization","body":{"id":1234,"mask":"rwcdC"}},' ;;
      inner-window) inner="{\"type\":\"ValidityWindow\",\"body\":{\"not_before\":$now,\"not_after\":$end}}," ;;
    esac
    window="{\"type\":\"ValidityWindow\",\"body\":{\"not_before\":$now,\"not_after\":$end}},"
    [ -n "$inner" ] && window=
    cat <<EOF
[
  {
    "location": "https://api.fly.io/v1",
    "caveats": [
      {"type": "Organization", "body": {"id": 1234, "mask": "rwcdC"}},
      {"type": "3P", "body": {"Location": "https://api.fly.io/aaa/v1", "VerifierKey": "AAAA", "Ticket": "AAAA"}},
      {"type": "IfPresent", "body": {"ifs": [$apps$more$inner{"type": "FeatureSet", "body": {"features": {"wg": "rw"}}}], "else": "$else"}}
    ]
  },
  {
    "location": "https://api.fly.io/aaa/v1",
    "caveats": [
      $window
      {"type": "FlyioUserID", "body": 42},
      {"type": "IsUser", "body": {"uint64": 42}},
      {"type": "BindToParentToken", "body": "AAAA"}
    ]
  }
]
EOF
    echo "Warning: Metrics send issue: metrics send failed with status 401" >&2 ;;
  status)
    check=passing; [ -e "$FAKE_DIR/unhealthy" ] && check=critical
    reg=${img%%/*}; rest=${img#*/}; repo=${rest%%:*}; rest=${rest#*:}; tag=${rest%%@*}; dig=${rest#*@}
    printf '{"ID":"acme-web","Name":"acme-web","Machines":[{"id":"m1","state":"started","region":"iad","image_ref":{"registry":"%s","repository":"%s","tag":"%s","digest":"%s","labels":{}},"checks":[{"name":"servicecheck-00-http-8080","status":"%s"}]},{"id":"m2","state":"stopped","image_ref":{}}]}\n' "$reg" "$repo" "$tag" "$dig" "$check" ;;
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
        let (r, w) = crate::agent::pipe_cloexec().unwrap();
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
            .env("FAKE_ORIGIN", self.dir.join("origin/acme/app.git"))
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
    // The plan chant's run recorded (its pending fact's), and only that.
    let plan = calls[gated + 1].rsplit_once(" --plan ").map(|(_, p)| p.to_owned()).unwrap_or_default();
    assert!(plan.starts_with("sha256:") && plan.len() == 71, "{}", calls[gated + 1]);
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
    assert!(line.contains("\"actor\":\"arugula\"") && !line.contains(TOKEN), "{line}");
    // chant's gate facts reached origin too: what it waited for, and who
    // resolved it, for which plan.
    let gates = r.origin(&format!("git show {tip}:_gates/release.jsonl"));
    let resolved = gates.lines().find(|l| l.contains("\"resolvedBy\"")).unwrap_or_default();
    for want in ["\"resolvedBy\":\"acct1\"", "\"relayedBy\":\"arugula\"", &format!("\"planDigest\":\"{plan}\"")] {
        assert!(resolved.contains(want), "{want} in {gates}");
    }
    // chant had no GitHub token, and origin (like a private repository on
    // GitHub) answers none without one: on this first deploy there was no
    // chant/lifecycle to read, and chant read and wrote its ledger through
    // the driver's stand-in, which takes that branch only.
    assert!(!ran.out.contains("could not be fetched") && !ran.out.contains("did not land"), "{}", ran.out);
    let home = r.dir.join("home");
    assert!(
        !Command::new("git")
            .args(["-C", "src", "push", "-q", "../ledger.git", "HEAD:refs/heads/main"])
            .current_dir(&home)
            .stderr(std::process::Stdio::null())
            .status()
            .unwrap()
            .success(),
        "the stand-in took another branch"
    );

    // The next deploy reads origin's ledger and adds to it.
    std::fs::write(r.dir.join("next-image"), image('3')).unwrap();
    std::fs::write(r.dir.join("plan-salt"), "2").unwrap();
    let next = r.dir.join("home2");
    std::fs::create_dir_all(&next).unwrap();
    let ran = r.apply(SETTINGS, None, &[("HOME", next.to_str().unwrap())]);
    assert_eq!(ran.code, 0, "{}", ran.out);
    let tip2 = r.origin("git rev-parse refs/heads/chant/lifecycle");
    assert_eq!(r.origin(&format!("git merge-base --is-ancestor {tip} {tip2} && echo yes")), "yes");
    assert_eq!(
        ran.result.unwrap()["chant_ledger"],
        format!("https://github.com/acme/app/blob/{tip2}/github/releases.jsonl#L2")
    );
}

/// chant's gate is approved for the plan its run recorded, from the
/// pending fact in its ledger: with none, or when the run names another,
/// nothing is approved.
#[test]
fn chant_fly_approves_only_the_plan_chant_recorded() {
    needs_node!();
    for (kind, says) in [("no-plan", "has no plan digest"), ("other-hint", "says to approve plan sha256:000")] {
        let r = rig(&format!("chant-fly-{kind}"));
        std::fs::write(r.dir.join(kind), "").unwrap();
        let ran = r.apply(SETTINGS, None, &[]);
        assert_eq!(ran.code, 1, "{}", ran.out);
        assert!(ran.out.contains(says), "{}", ran.out);
        assert!(!r.calls().iter().any(|c| c.starts_with("chant approve")), "{:?}", r.calls());
        assert!(ran.result.is_none());
    }
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
/// the repo's runs. (Every other test's token is shaped as `fly tokens
/// create deploy` makes one, its app in an IfPresent caveat.)
#[test]
fn chant_fly_wants_an_app_token_that_expires() {
    needs_node!();
    for (kind, says) in [
        ("org", "isn't scoped to one app"),
        ("forever", "longer than 30 days"),
        // An IfPresent caveat naming one app whose `else` lets every other
        // access do anything: the org's, not the app's.
        ("broad", "lets more than read past the app"),
        ("org-in-ifs", "its IfPresent caveat holds Apps, Organization, FeatureSet"),
        // A window that binds only some accesses doesn't end the token.
        ("inner-window", "doesn't expire"),
    ] {
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
