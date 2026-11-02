//! The chant→Fly driver (M220, #1985), against a fake chant, a fake flyctl
//! and a fake npm that record what they're run with, and a local origin:
//! its plan runs chant's op to its gate with no Fly token and pushes the
//! pending fact; its apply checks the token, asks chant whether the plan is
//! stale, hands chant the passkey's signature over chant's own statement,
//! runs the op, ties chant's new ledger entry to what Fly runs, and rolls
//! back through Fly and chant. Each program gets only the credentials it's
//! meant to, and only in its environment; the result comes on fd 3.

use std::{
    io::Read as _,
    os::{fd::AsRawFd as _, unix::process::CommandExt as _},
    path::Path,
    process::Command,
};

use arugula_control_wire::work::{DeployWork, PasskeyAssertion};
use arugula_e2e::deploy::{Approval, Operation, chant::Gate};

use super::{CHANT_FLY, RESULT_FD, chant_relay, result_of};

const TOKEN: &str = "fly-token-m220-secret"; // gitleaks:allow
const GH: &str = "gh-token-m220-secret"; // gitleaks:allow

fn image(n: char) -> String {
    format!("registry.fly.io/acme-web:deployment-{n}@sha256:{}", n.to_string().repeat(64))
}

/// A fake chant, after `@intentius/chant` 0.124.67 (and, with
/// $FAKE_CHANT_VERSION, an older one, which the driver refuses): `--version`
/// says which. Each command first reads chant's ledger (`chant/lifecycle`)
/// from its remote, and refuses when it isn't here and the remote can't be
/// reached (chant's `requireLifecycleLedger`). The remote is the one
/// `CHANT_LIFECYCLE_REMOTE` names (chant#4647), else origin, and must be one
/// of the repository's. origin, as GitHub answers for a private repository,
/// answers only a git that has GITHUB_TOKEN, and any other remote (the
/// driver's stand-in) answers.
/// `run release --json` stops at its gate (exit 3) until a resolution of
/// this commit's plan is in the ledger: it writes a pending fact
/// (`_gates/release.jsonl`, its `planDigest`) and prints its run record
/// (`gate: {name, since}`, `approve`), as chant does; then it deploys (a
/// new image) and records the release. With $FAKE_DIR/needs-fly it fails
/// before its gate without the Fly token. `approve` with `--statement`
/// prints chant's gate statement (its lines, as chant's `gateSealPayload`,
/// and their challenge); with `--signature -` it reads a WebAuthn
/// assertion on stdin, takes it only when its client data's challenge is
/// that statement's (a real chant checks the signature too, against
/// `.chant/trust.json`), and resolves the gate for the plan given (`--plan`
/// must be the pending one's). `plan stale <digest>` says it's current, or
/// stale with $FAKE_DIR/stale, or unknown (exit 1) when no pending fact
/// records it. `components release` records a release again. Each push
/// goes to the remote when it's reachable, a warning otherwise. Each call
/// goes in $FAKE_LOG with which tokens it saw and which remote it used.
const FAKE_CHANT: &str = r#"#!/bin/bash
set -e
fly=no; [ "$FLY_API_TOKEN" = "$FAKE_TOKEN" ] && fly=yes
gh=no; [ -n "$GITHUB_TOKEN" ] && gh=yes
fd3=no; (: >&3) 2>/dev/null && fd3=yes
version=${FAKE_CHANT_VERSION:-0.124.67}
remote=origin
[ -n "$CHANT_LIFECYCLE_REMOTE" ] && remote=$CHANT_LIFECYCLE_REMOTE
echo "chant $* | fly:$fly gh:$gh fd3:$fd3 user:$USER remote:$remote" >> "$FAKE_LOG"
[ "$1" = --version ] && { echo "$version"; exit 0; }
git remote get-url "$remote" >/dev/null 2>&1 || { echo "CHANT_LIFECYCLE_REMOTE names the remote \"$remote\", which this repository doesn't have" >&2; exit 1; }
export LC_ALL=C
B=refs/heads/chant/lifecycle
now() { date -u +%Y-%m-%dT%H:%M:%S.000Z; }
reachable() { [ -n "$GITHUB_TOKEN" ] || [ "$(git remote get-url "$remote")" != "$FAKE_ORIGIN" ]; }
ledger() {
  local had err
  had=$(git rev-parse -q --verify $B || true)
  if reachable; then
    err=$(git fetch -q "$remote" chant/lifecycle:chant/lifecycle 2>&1) && return 0
    case "$err" in *"couldn't find remote ref"*) return 0 ;; esac
  fi
  [ -n "$had" ] && return 0
  echo "the chant/lifecycle ledger branch is not in this checkout and could not be fetched from \"$remote\", so what is on it cannot be read. Refusing rather than treating an unreadable ledger as an empty one." >&2
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
  if reachable && git push -q "$remote" chant/lifecycle:chant/lifecycle 2>/dev/null; then return 0; fi
  echo "warning: The push to the remote did not land: the push of chant/lifecycle to \"$remote\" failed" >&2
}
# macOS may have no sha256sum (its Tart VMs don't): shasum there.
sha256() { if command -v sha256sum >/dev/null; then sha256sum; else shasum -a 256; fi; }
b64url() { openssl dgst -sha256 -binary | base64 | tr '+/' '-_' | tr -d '=\n'; }
sha=$(git rev-parse HEAD)
plan="jcs1-sha256:$(printf 'plan %s %s' "$sha" "$(cat "$FAKE_DIR/plan-salt" 2>/dev/null)" | sha256 | cut -c1-64)"
case "$1 $2" in
  "run release")
    ledger || { echo '{"version":1,"op":"release","status":"fail"}'; exit 1; }
    [ -e "$FAKE_DIR/needs-fly" ] && [ "$fly" = no ] && { echo "the op's plan step needs FLY_API_TOKEN" >&2; exit 1; }
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
    gate=$3; by=; relayed=; note=; given=; envn=; binding=; at=; statement=no; signature=; prev=
    for a in "$@"; do case "$prev" in --actor) by=$a;; --relayed-by) relayed=$a;; --note) note=$a;; --plan) given=$a;; --env) envn=$a;; --binding) binding=$a;; --at) at=$a;; --signature) signature=$a;; esac; [ "$a" = --statement ] && statement=yes; prev=$a; done
    pending=$(git show $B:_gates/release.jsonl 2>/dev/null | grep '"kind":"pending"' | grep -F "\"gitSha\":\"$sha\"" | tail -1)
    [ -n "$pending" ] || { echo "no pending gate $gate for $sha" >&2; exit 1; }
    case "$pending" in *"\"planDigest\":\"$given\""*) ;; *) echo "that plan isn't the pending one's" >&2; exit 1 ;; esac
    text=$(printf 'release\n%s\n%s\n%s\n%s\n%s\nrelayed-by %s\nbinding %s' "$gate" "$envn" "$given" "$by" "$at" "$relayed" "$binding")
    challenge=$(printf '%s' "$text" | b64url)
    if [ $statement = yes ]; then
      printf '{"schema":"chant-gate-statement/v1","chantVersion":"%s","op":"release","gate":"%s","planDigest":"%s","resolvedBy":"%s","relayedBy":"%s","timestamp":"%s","binding":"%s","sealVersion":3,"challenge":"%s"}\n' "$version" "$gate" "$given" "$by" "$relayed" "$at" "$binding" "$challenge"
      exit 0
    fi
    [ "$signature" = - ] || { echo "a gate under identity.gates needs --signature" >&2; exit 1; }
    assertion=$(cat)
    cd_b64=$(printf '%s' "$assertion" | sed 's/.*"clientDataJSON":"\([^"]*\)".*/\1/' | tr '_-' '/+')
    while [ $(( ${#cd_b64} % 4 )) -ne 0 ]; do cd_b64="$cd_b64="; done
    case "$(printf '%s' "$cd_b64" | base64 -d 2>/dev/null)" in
      *"\"challenge\":\"$challenge\""*) ;;
      *) echo "Gate \"$gate\" on \"release\" was not approved: the signature does not verify for $by: its challenge is not this approval's statement digest" >&2; exit 1 ;;
    esac
    runid=$(printf '%s' "$pending" | sed 's/.*"runId":"\([^"]*\)".*/\1/')
    append _gates/release.jsonl "{\"approver\":{\"kind\":\"human\"},\"binding\":\"$binding\",\"gate\":\"$gate\",\"gitSha\":\"$sha\",\"note\":\"$note\",\"op\":\"release\",\"origin\":\"cli\",\"planDigest\":\"$given\",\"relayedBy\":\"$relayed\",\"resolvedBy\":\"$by\",\"runId\":\"$runid\",\"seal\":{\"kind\":\"webauthn\",\"version\":3},\"timestamp\":\"$at\",\"version\":1}"
    push
    echo "Gate \"$gate\" on \"release\" resolved by $by, relayed by $relayed" ;;
  "plan stale")
    ledger || exit 1
    git show $B:_gates/release.jsonl 2>/dev/null | grep '"kind":"pending"' | grep -qF "\"planDigest\":\"$3\"" || { echo '{"contract":1,"plan":null,"stale":null,"applies":[]}'; exit 1; }
    if [ -e "$FAKE_DIR/stale" ]; then echo '{"contract":1,"stale":true,"applies":[{"source":"deploys"}]}'; exit 2; fi
    echo '{"contract":1,"stale":false,"applies":[]}' ;;
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
fd3=no; (: >&3) 2>/dev/null && fd3=yes
echo "flyctl $* | fly:$fly gh:$gh fd3:$fd3" >> "$FAKE_LOG"
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
fd3=no; (: >&3) 2>/dev/null && fd3=yes
echo "npm $* | fly:$fly gh:$gh fd3:$fd3" >> "$FAKE_LOG"
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
    /// deploy's and its secrets, its result pipe as fd 3. A plan gets
    /// GITHUB_TOKEN only; an apply both tokens and (unless it's a roll
    /// back) `relay` as `DEPLOY_CHANT`.
    fn drive(
        &self,
        phase: &str,
        settings: &str,
        roll_back: Option<String>,
        relay: Option<String>,
        extra: &[(&str, &str)],
    ) -> Ran {
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
            .env("DEPLOY_ID", DEPLOY)
            .env("DEPLOY_RUN", "run_1")
            .env("DEPLOY_PHASE", phase)
            .env("DEPLOY_ENVIRONMENT", "production")
            .env("DEPLOY_SOURCE_KIND", "commit")
            .env("DEPLOY_SOURCE_REF", &self.sha)
            .env("DEPLOY_SOURCE_WHERE", "acme/app")
            .env("DEPLOY_SETTINGS", settings)
            .env("DEPLOY_ROLL_BACK", roll_back.unwrap_or_default())
            .env("ARUGULA_CHANT_FLY_TEST", "1")
            .env("DEPLOY_GIT_BASE", format!("{}/origin/", self.dir.display()))
            .env("DEPLOY_FLY_CHECKS", "2x0")
            .env("GITHUB_TOKEN", GH)
            .env("FAKE_TOKEN", TOKEN)
            .env("FAKE_LOG", self.dir.join("log"))
            .env("FAKE_DIR", &self.dir)
            .env("FAKE_ORIGIN", self.dir.join("origin/acme/app.git"));
        if phase == "apply" {
            c.env("FLY_API_TOKEN", TOKEN)
                .env("DEPLOY_APPROVER", "acct1")
                .env("DEPLOY_APPROVED_BY", "")
                .env("DEPLOY_APPROVAL", "apv_1")
                .env("DEPLOY_OP_HASH", "ab12")
                .env("DEPLOY_CHANT", relay.unwrap_or_default());
        }
        c.envs(extra.iter().copied());
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

    /// Its plan, in a home of its own (`plan-home`): a box of its own.
    fn plan(&self, settings: &str, extra: &[(&str, &str)]) -> Ran {
        let home = self.dir.join("plan-home");
        std::fs::create_dir_all(&home).unwrap();
        let mut extra = extra.to_vec();
        extra.push(("HOME", home.to_str().unwrap()));
        self.drive("plan", settings, None, None, &extra)
    }

    /// Its plan, then its apply with the passkey's signature over chant's
    /// statement for the gate the plan found (`challenge` says the
    /// assertion's, when it isn't the statement's own).
    fn release(&self, settings: &str, extra: &[(&str, &str)]) -> Ran {
        self.release_signed(settings, extra, None)
    }

    fn release_signed(&self, settings: &str, extra: &[(&str, &str)], challenge: Option<&str>) -> Ran {
        let planned = self.plan(settings, extra);
        assert_eq!(planned.code, 0, "the plan: {}", planned.out);
        let gate: Gate = serde_json::from_value(planned.result.unwrap()["chant_gate"].clone()).unwrap();
        self.drive("apply", settings, None, Some(relay(&gate, challenge)), extra)
    }

    /// A roll back: an apply with no gate.
    fn roll_back(&self, settings: &str, to: String) -> Ran {
        self.drive("apply", settings, Some(to), None, &[])
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

const DEPLOY: &str = "dep_11111111111111111111111111111111";
const AT: &str = "2099-01-01T00:00:00.000Z";

/// What arugulad hands the driver for a v3 approval of `gate` by `acct1`
/// (`chant_relay`): its assertion's client data names `challenge`, or the
/// statement's own (a real passkey signs what its client data names).
fn relay(gate: &Gate, challenge: Option<&str>) -> String {
    use base64::Engine as _;
    let op = Operation {
        project: "prj_1".into(),
        environment: "env_1".into(),
        environment_name: "production".into(),
        plan: "ab".repeat(32),
        source_kind: "commit".into(),
        source_ref: "c".repeat(40),
        source_where: "acme/app".into(),
        planned: "cd".repeat(32),
    };
    // A time no earlier than the pending fact's: the fake's are now.
    let approval = Approval::v3(DEPLOY, &op, "acct1", 1, AT, gate);
    let b64 = base64::engine::general_purpose::URL_SAFE_NO_PAD;
    let ch = challenge.map_or_else(|| approval.passkey_challenge(), str::to_owned);
    let cd = serde_json::json!({ "type": "webauthn.get", "challenge": ch, "origin": "https://control.test" });
    let d = DeployWork {
        deploy: DEPLOY.into(),
        operation: op,
        approval,
        passkey: Some(PasskeyAssertion {
            id: "pk1".into(),
            client_data_json: b64.encode(cd.to_string()),
            authenticator_data: "AAAA".into(),
            signature: "AAAA".into(),
        }),
        passkey_log: None,
        driver: "chant-fly".into(),
        secrets: vec![],
        settings: Default::default(),
        roll_back: None,
        approval_id: None,
    };
    chant_relay(&d).expect("a v3's relay")
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

/// #1985: the plan runs chant's op to its gate with no Fly token and only
/// GitHub's for the driver's own git, and pushes the gate's pending fact;
/// its result is that gate, which the passkey signs chant's statement of.
#[test]
fn chant_fly_plans_without_fly_and_says_which_gate() {
    needs_node!();
    let r = rig("chant-fly-plan");
    let ran = r.plan(SETTINGS, &[]);
    assert_eq!(ran.code, 0, "{}", ran.out);
    let calls = r.calls();
    assert!(!calls.iter().any(|c| c.starts_with("flyctl")), "a plan runs no flyctl: {calls:?}");
    assert!(!calls.iter().any(|c| c.starts_with("chant approve")), "nobody approves at planning: {calls:?}");
    assert_eq!(calls.iter().filter(|c| c.starts_with("chant run release --json")).count(), 1, "{calls:?}");
    for l in r.log() {
        assert!(l.contains("fly:no gh:no"), "a plan's programs get no token: {l}");
        assert!(!l.contains("fd3:yes"), "{l}");
    }
    let gate = &ran.result.expect("its result, on fd 3")["chant_gate"];
    let tip = r.origin("git rev-parse refs/heads/chant/lifecycle");
    let facts = r.origin(&format!("git show {tip}:_gates/release.jsonl"));
    let pending = facts.lines().find(|l| l.contains("\"kind\":\"pending\"")).expect("its pending fact reached origin");
    assert_eq!(gate["op"], "release");
    assert_eq!(gate["gate"], "approve-release");
    assert_eq!(gate["chant"], "0.124.67");
    let plan = gate["plan"].as_str().unwrap();
    assert!(plan.starts_with("jcs1-sha256:") && pending.contains(&format!("\"planDigest\":\"{plan}\"")), "{pending}");
    assert!(pending.contains(&format!("\"runId\":\"{}\"", gate["run"].as_str().unwrap())), "{pending}");
    assert!(pending.contains(&format!("\"timestamp\":\"{}\"", gate["recorded"].as_str().unwrap())), "{pending}");
    let g: Gate = serde_json::from_value(gate.clone()).unwrap();
    assert!(g.well_formed(), "{g:?}");
    // Handed a Fly token, or with none of GitHub's: nothing runs.
    let r = rig("chant-fly-plan-fly");
    let ran = r.plan(SETTINGS, &[("FLY_API_TOKEN", TOKEN)]);
    assert_eq!(ran.code, 1, "{}", ran.out);
    assert!(ran.out.contains("runs without FLY_API_TOKEN"), "{}", ran.out);
    assert!(r.calls().is_empty(), "{:?}", r.calls());
    // An op that needs Fly before its gate fails at planning.
    let r = rig("chant-fly-plan-needs-fly");
    std::fs::write(r.dir.join("needs-fly"), "").unwrap();
    let ran = r.plan(SETTINGS, &[]);
    assert_eq!(ran.code, 1, "{}", ran.out);
    assert!(ran.out.contains("before its gate"), "{}", ran.out);
}

/// The apply: chant says the plan is current, chant's statement for the
/// approval is the one the passkey signed, chant takes the signature, and
/// the op runs through its gate; chant's new ledger entry is linked.
#[test]
fn chant_fly_releases_through_chants_gate_and_links_its_ledger() {
    needs_node!();
    let r = rig("chant-fly-apply");
    let ran = r.release(SETTINGS, &[]);
    assert_eq!(ran.code, 0, "{}", ran.out);
    let calls = r.calls();
    let applied = calls.iter().position(|c| c.starts_with("flyctl version")).expect("the apply's calls");
    let calls = &calls[applied..];
    let at = |prefix: &str| {
        calls.iter().position(|c| c.starts_with(prefix)).unwrap_or_else(|| panic!("{prefix}: {calls:?}"))
    };
    assert!(at("flyctl tokens debug") < at("npm ci"), "the token's checked before anything of the repo's runs");
    assert_eq!(calls[at("npm ci")], "npm ci --no-fund --no-audit --ignore-scripts");
    let stale = at("chant plan stale ");
    let plan = calls[stale].split(' ').nth(3).unwrap().to_owned();
    assert!(plan.starts_with("jcs1-sha256:") && calls[stale].ends_with(" --json"), "{}", calls[stale]);
    let approve = format!(
        "chant approve release approve-release --actor arugula:acct1 --relayed-by arugula --plan {plan} --binding \
         arugula deploy {DEPLOY} op "
    );
    assert!(calls[stale + 1].starts_with(&approve) && calls[stale + 1].ends_with(&format!(" --at {AT} --statement")));
    assert!(calls[stale + 2].starts_with(&approve) && calls[stale + 2].ends_with(" --signature -"));
    assert!(calls[stale + 2].contains("--note arugula deploy dep_1111"), "{}", calls[stale + 2]);
    assert_eq!(calls[stale + 3], "chant run release --json");
    assert_eq!(calls.last().unwrap(), "flyctl status --app acme-web --json");
    // Who saw which token: chant's op and flyctl the Fly token, nobody
    // GitHub's but the driver's own git; npm, chant's own reads and chant
    // approve neither.
    for l in r.log().iter().skip(applied) {
        let (call, saw) = l.split_once(" | ").unwrap();
        let want = if call.starts_with("npm")
            || call.starts_with("chant approve")
            || call.starts_with("chant plan")
            || call == "flyctl version"
            || call == "chant --version"
        {
            "fly:no gh:no"
        } else {
            "fly:yes gh:no"
        };
        assert!(saw.starts_with(want), "{l}");
        assert!(!call.contains(TOKEN) && !call.contains(GH), "on a command line: {l}");
        assert!(!saw.contains("fd3:yes"), "fd 3 reached a program the driver ran: {l}");
    }
    assert!(r.log().iter().filter(|l| l.starts_with("chant")).all(|l| l.contains(" user:arugula ")));
    assert!(!ran.out.contains(TOKEN) && !ran.out.contains(GH), "{}", ran.out);
    assert!(!ran.out.contains("\"chant_ledger\""), "the result isn't on its output: {}", ran.out);

    let result = ran.result.expect("its result, on fd 3");
    let tip = r.origin("git rev-parse refs/heads/chant/lifecycle");
    assert_eq!(result["chant_ledger"], format!("https://github.com/acme/app/blob/{tip}/github/releases.jsonl#L1"));
    assert_eq!(result["chant_record"]["gitSha"], r.sha);
    assert_eq!(result["fly"], serde_json::json!({ "app": "acme-web", "image": image('2'), "healthy": true }));
    // chant's gate facts reached origin: what the plan waited for, and the
    // passkey's approval of that plan, with its binding.
    let gates = r.origin(&format!("git show {tip}:_gates/release.jsonl"));
    let resolved = gates.lines().find(|l| l.contains("\"resolvedBy\"")).unwrap_or_default();
    for want in [
        "\"resolvedBy\":\"arugula:acct1\"",
        "\"relayedBy\":\"arugula\"",
        &format!("\"planDigest\":\"{plan}\""),
        &format!("\"binding\":\"arugula deploy {DEPLOY} op "),
        "\"kind\":\"webauthn\"",
    ] {
        assert!(resolved.contains(want), "{want} in {gates}");
    }
    // chant had no GitHub token: it read and wrote its ledger through the
    // driver's stand-in, by name, and origin stayed origin.
    assert!(!ran.out.contains("could not be fetched") && !ran.out.contains("did not land"), "{}", ran.out);
    let home = r.dir.join("home");
    assert_eq!(remotes(&home), ["arugula-ledger", "origin"]);
    for l in r.log().iter().filter(|l| l.starts_with("chant") && !l.starts_with("chant --version")) {
        assert!(l.ends_with(" remote:arugula-ledger"), "{l}");
    }
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
    for h in ["home", "plan-home"] {
        std::fs::remove_dir_all(r.dir.join(h)).unwrap();
        std::fs::create_dir_all(r.dir.join(h)).unwrap();
    }
    let ran = r.release(SETTINGS, &[]);
    assert_eq!(ran.code, 0, "{}", ran.out);
    let tip2 = r.origin("git rev-parse refs/heads/chant/lifecycle");
    assert_eq!(r.origin(&format!("git merge-base --is-ancestor {tip} {tip2} && echo yes")), "yes");
    assert_eq!(
        ran.result.unwrap()["chant_ledger"],
        format!("https://github.com/acme/app/blob/{tip2}/github/releases.jsonl#L2")
    );
}

/// The remotes of the box's checkout (`~/src`).
fn remotes(home: &Path) -> Vec<String> {
    let mut v: Vec<String> = sh(&home.join("src"), "git remote").lines().map(str::to_owned).collect();
    v.sort();
    v
}

/// Nothing is approved when chant says the plan is stale, when chant's
/// statement isn't the one the passkey signed, or with no approval over
/// chant's statement at all.
#[test]
fn chant_fly_approves_only_a_current_plan_its_passkey_signed() {
    needs_node!();
    let r = rig("chant-fly-stale");
    let planned = r.plan(SETTINGS, &[]);
    let gate: Gate = serde_json::from_value(planned.result.unwrap()["chant_gate"].clone()).unwrap();
    std::fs::write(r.dir.join("stale"), "").unwrap();
    let ran = r.drive("apply", SETTINGS, None, Some(relay(&gate, None)), &[]);
    assert_eq!(ran.code, 1, "{}", ran.out);
    assert!(ran.out.contains("is stale") && ran.out.contains("(1 since)"), "{}", ran.out);
    assert!(!r.calls().iter().any(|c| c.starts_with("chant approve")), "{:?}", r.calls());

    // What Arugula says the passkey signed isn't chant's statement for
    // this approval: the driver stops before chant records anything.
    let r = rig("chant-fly-other-statement");
    let planned = r.plan(SETTINGS, &[]);
    let gate: Gate = serde_json::from_value(planned.result.unwrap()["chant_gate"].clone()).unwrap();
    let mut other: serde_json::Value = serde_json::from_str(&relay(&gate, None)).unwrap();
    other["challenge"] = "another-statements-challenge".into();
    let ran = r.drive("apply", SETTINGS, None, Some(other.to_string()), &[]);
    assert_eq!(ran.code, 1, "{}", ran.out);
    assert!(ran.out.contains("isn't the one the passkey signed"), "{}", ran.out);
    assert!(!r.calls().iter().any(|c| c.ends_with("--signature -")), "{:?}", r.calls());

    // A signature over something else: chant refuses it, and nothing runs.
    let r = rig("chant-fly-bad-signature");
    let ran = r.release_signed(SETTINGS, &[], Some("another-statements-challenge"));
    assert_eq!(ran.code, 1, "{}", ran.out);
    assert!(ran.out.contains("the signature does not verify for arugula:acct1"), "{}", ran.out);
    assert_eq!(r.calls().iter().filter(|c| c.starts_with("chant run")).count(), 1, "only the plan's: {:?}", r.calls());

    let r = rig("chant-fly-no-relay");
    let ran = r.drive("apply", SETTINGS, None, None, &[]);
    assert_eq!(ran.code, 1, "{}", ran.out);
    assert!(ran.out.contains("approval v3"), "{}", ran.out);
    assert!(r.calls().is_empty(), "{:?}", r.calls());
}

/// chant older than 0.124.64 can't check a binding line: refused, in a
/// plan or an apply, before anything of chant's runs but `--version`.
#[test]
fn chant_fly_refuses_a_chant_that_checks_no_binding() {
    needs_node!();
    let r = rig("chant-fly-old-chant");
    let ran = r.plan(SETTINGS, &[("FAKE_CHANT_VERSION", "0.124.63")]);
    assert_eq!(ran.code, 1, "{}", ran.out);
    assert!(ran.out.contains("pins chant 0.124.63, and chant-fly needs 0.124.64 or newer"), "{}", ran.out);
    assert_eq!(r.calls().iter().filter(|c| c.starts_with("chant")).collect::<Vec<_>>(), ["chant --version"]);
    assert!(ran.result.is_none());
}

/// chant's gate is planned from the pending fact its run recorded: with
/// no plan digest, or when the run names another, nothing is planned.
#[test]
fn chant_fly_plans_only_the_plan_chant_recorded() {
    needs_node!();
    for (kind, says) in [("no-plan", "has no plan digest"), ("other-hint", "says to approve plan sha256:000")] {
        let r = rig(&format!("chant-fly-{kind}"));
        std::fs::write(r.dir.join(kind), "").unwrap();
        let ran = r.plan(SETTINGS, &[]);
        assert_eq!(ran.code, 1, "{}", ran.out);
        assert!(ran.out.contains(says), "{}", ran.out);
        assert!(ran.result.is_none());
    }
}

/// The settings the plan names change what runs: install scripts, chant's
/// GitHub token (an apply's op only), the gates it plans.
#[test]
fn chant_fly_does_what_its_settings_say() {
    needs_node!();
    let r = rig("chant-fly-settings");
    let ran = r.release(&with(r#""npm_scripts":"true","chant_github":"true","chant_gates":"approve-release""#), &[]);
    assert_eq!(ran.code, 0, "{}", ran.out);
    assert!(r.calls().contains(&"npm ci --no-fund --no-audit".to_owned()), "{:?}", r.calls());
    let runs: Vec<String> = r.log().into_iter().filter(|l| l.starts_with("chant run")).collect();
    assert!(runs[0].contains("fly:no gh:no"), "the plan's op gets no token: {runs:?}");
    assert!(runs.last().unwrap().contains("fly:yes gh:yes"), "{runs:?}");
    // With its own token, chant keeps an apply's ledger on origin: no
    // stand-in. (The plan, with none, had one.)
    assert_eq!(remotes(&r.dir.join("home")), ["origin"]);
    assert_eq!(remotes(&r.dir.join("plan-home")), ["arugula-ledger", "origin"]);

    // A gate the plan doesn't name, or another op's: never planned.
    for (name, settings, gate_op, says) in [
        ("chant-fly-gates", with(r#""chant_gates":"ship""#), None, "which the plan doesn't name"),
        ("chant-fly-gate-op", SETTINGS.to_owned(), Some("other"), "another op's gate"),
    ] {
        let r = rig(name);
        if let Some(o) = gate_op {
            std::fs::write(r.dir.join("gate-op"), o).unwrap();
        }
        let ran = r.plan(&settings, &[]);
        assert_eq!(ran.code, 1, "{}", ran.out);
        assert!(ran.out.contains(says), "{}", ran.out);
        assert!(ran.result.is_none());
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
        let ran = r.release(SETTINGS, &[]);
        assert_eq!(ran.code, 1, "{}", ran.out);
        assert!(
            ran.out.contains(says) && ran.out.contains("fly tokens create deploy -a acme-web -x 720h"),
            "{}",
            ran.out
        );
        let applied = r.calls().iter().position(|c| c.starts_with("flyctl")).unwrap();
        assert!(
            !r.calls()[applied..].iter().any(|c| c.starts_with("npm") || c.starts_with("chant")),
            "{:?}",
            r.calls()
        );
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
    let ran = r.release(SETTINGS, &[]);
    assert_eq!(ran.code, 5, "{}", ran.out);
    let result = ran.result.unwrap();
    assert_eq!(result["fly"]["healthy"], false);
    assert!(result["chant_ledger"].is_string(), "the release went out: its entry is linked still");

    // The release didn't reach Fly (it runs the image it ran before).
    let r = rig("chant-fly-no-deploy");
    std::fs::write(r.dir.join("no-deploy"), "").unwrap();
    let ran = r.release(SETTINGS, &[]);
    assert_eq!(ran.code, 6, "{}", ran.out);
    // The record's digest must be the image's, when the plan says so.
    let r = rig("chant-fly-digest");
    let ran = r.release(&with(r#""chant_digest":"image""#).replace(r#""chant_digest":"plan","#, ""), &[]);
    assert_eq!(ran.code, 6, "{}", ran.out);
    assert!(ran.out.contains("not the image Fly runs"), "{}", ran.out);

    // No record in this run, or one in another ledger environment.
    for (name, extra) in [("chant-fly-no-record", None), ("chant-fly-other-env", Some(("FAKE_ENV", "staging")))] {
        let r = rig(name);
        if extra.is_none() {
            std::fs::write(r.dir.join("no-record"), "").unwrap();
        }
        let ran = r.release(SETTINGS, extra.as_slice());
        assert_eq!(ran.code, 4, "{}", ran.out);
        assert!(ran.result.unwrap()["chant_ledger"].is_null());
    }

    // An origin that won't take a push (as GitHub refuses one without a
    // token that can write): nothing runs, a plan as an apply.
    let r = rig("chant-fly-readonly");
    let refuse =
        [("GIT_CONFIG_COUNT", "1"), ("GIT_CONFIG_KEY_0", "remote.origin.receivepack"), ("GIT_CONFIG_VALUE_0", "false")];
    let ran = r.plan(SETTINGS, &refuse);
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
    let ran = short.plan(SETTINGS, &[]);
    assert_eq!(ran.code, 1, "{}", ran.out);
    assert!(ran.out.contains("by its full id"), "{}", ran.out);
    short.sha = "f".repeat(40);
    let ran = short.plan(SETTINGS, &[]);
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
    let ran = r.roll_back(SETTINGS, back.to_string());
    assert_eq!(ran.code, 0, "{}", ran.out);
    let calls = r.calls();
    let deploy = calls.iter().position(|c| c.starts_with("flyctl deploy")).unwrap();
    assert_eq!(calls[deploy], format!("flyctl deploy --app acme-web --image {} --yes", image('1')));
    assert_eq!(calls[deploy + 1], "flyctl status --app acme-web --json");
    assert_eq!(
        calls[deploy + 2],
        format!(
            "chant components release github --component acme --digest sha256:d1 --git-sha {} --run-id run_1 \
             --actor arugula --approver arugula:acct1",
            r.sha
        )
    );
    assert!(!calls.iter().any(|c| c.starts_with("chant run") || c.starts_with("chant approve")), "no gate");
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
