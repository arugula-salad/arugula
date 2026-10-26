//! A session's push (M259, S42 q2): at the end of a turn that left commits,
//! the box pushes the session's branch, `arugula/<8 hex>-<slug>`, and no
//! other ref.
//!
//! The session's checkout is its user's, and its agent controls everything
//! in it, `.git/config` and hooks included. So the daemon never runs git
//! there as itself, nor pushes from it:
//! 1. The session user writes a git bundle of exactly its one ref, in its
//!    own checkout, as itself (what it planted runs as it, harming
//!    nothing).
//! 2. The daemon reads the bundle's bytes (as the session user) into a
//!    file of its own, and `git bundle list-heads` must name that one ref
//!    and nothing else.
//! 3. It fetches the bundle into a bare repository of its own, which the
//!    agent never touches, and pushes from there with one explicit refspec,
//!    `refs/heads/B:refs/heads/B`, where B was checked first against the
//!    session's own branch ([`may_push`]): `main` and other sessions'
//!    branches are refused before git runs. Its git runs with no hooks
//!    (`core.hooksPath=/dev/null`), no system or global config, and none of
//!    the agent's.
//! 4. The token comes from control for this push alone (`WORK_SESSION_PUSH`:
//!    the session's repo, `contents: write` and `workflows: write`, since
//!    GitHub refuses a branch that began before main changed CI without
//!    it: aru-115, amended), reaches git
//!    through a credential helper reading one process's environment (never
//!    a file, never argv), and is revoked once the push is done.
//!
//! The push goes to `https://github.com/<owner>/<name>.git`, the session's
//! repo on GitHub (tests name another base, `ARUGULA_GITHUB_GIT`), never to
//! a URL from the workspace's spec or the checkout. A push that isn't a
//! fast-forward is refused by GitHub, and the turn says it didn't push.

use std::{
    path::{Path, PathBuf},
    time::Duration,
};

use arugula_control_wire::work::{PushAsk, PushGiven, WORK_SESSION_PUSH};
use tracing::{info, warn};

use super::{AsUser, Session, good_branch};
use crate::server::App;

/// The most a whole push may take: the platform marks a session whose box
/// says nothing for a minute unresponsive.
const PUSH_MAX: Duration = Duration::from_secs(45);
/// The biggest bundle taken.
const BUNDLE_MAX: usize = 512 * 1024 * 1024;

/// The credential helper git asks: the token from its own environment.
const HELPER: &str =
    "!f() { test \"$1\" = get || exit 0; echo username=x-access-token; echo \"password=$ARUGULA_PUSH_TOKEN\"; }; f";

/// Whether session `session`, whose own branch is `own`, may push
/// `branch`: its own, exactly, and a session's branch by its shape.
pub fn may_push(session: &str, own: &str, branch: &str) -> bool {
    branch == own && good_branch(session, branch)
}

/// The one head a bundle's `git bundle list-heads` names, if that's
/// `refs/heads/<branch>` and nothing else: its commit.
pub fn only_head(listing: &str, branch: &str) -> Option<String> {
    let want = format!("refs/heads/{branch}");
    let lines: Vec<&str> = listing.lines().map(str::trim).filter(|l| !l.is_empty()).collect();
    let [line] = lines.as_slice() else { return None };
    let (sha, name) = line.split_once(' ')?;
    (name == want && sha.len() >= 40 && sha.bytes().all(|b| b.is_ascii_hexdigit())).then(|| sha.to_owned())
}

/// Where the session's repo is pushed.
pub fn remote_for(repo: &str) -> Option<String> {
    let base = std::env::var("ARUGULA_GITHUB_GIT")
        .ok()
        .filter(|b| !b.is_empty())
        .unwrap_or_else(|| "https://github.com".into());
    let mut parts = repo.split('/');
    let (Some(owner), Some(name), None) = (parts.next(), parts.next(), parts.next()) else { return None };
    let plain = |s: &str| {
        !s.is_empty() && !s.starts_with('.') && s.bytes().all(|b| b.is_ascii_alphanumeric() || b"-_.".contains(&b))
    };
    (plain(owner) && plain(name)).then(|| format!("{}/{owner}/{name}.git", base.trim_end_matches('/')))
}

/// The daemon's own git: no system or global config, no prompts, no
/// hooks, in `dir`.
fn git(dir: &Path) -> tokio::process::Command {
    let mut c = tokio::process::Command::new("git");
    c.env_clear()
        .env("PATH", std::env::var_os("PATH").unwrap_or_else(|| "/usr/bin:/bin".into()))
        .env("HOME", "/nonexistent")
        .env("LANG", "C")
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_TERMINAL_PROMPT", "0")
        .arg("-C")
        .arg(dir)
        .args(["-c", "core.hooksPath=/dev/null"])
        .stdin(std::process::Stdio::null())
        .kill_on_drop(true);
    crate::agent::hosted::unprotect_oom(&mut c);
    c
}

async fn run(mut c: tokio::process::Command, what: &str) -> Result<String, String> {
    let out = c.output().await.map_err(|e| format!("can't run git to {what}: {e}"))?;
    if out.status.success() {
        Ok(String::from_utf8_lossy(&out.stdout).into_owned())
    } else {
        let said = String::from_utf8_lossy(&out.stderr);
        Err(format!("git couldn't {what}: {}", said.trim().chars().take(400).collect::<String>()))
    }
}

/// From the bundle at `bundle`, into the daemon's own bare repo `mirror`,
/// then to `remote`: session `session`'s `branch`, if it's `own` and the
/// bundle names it alone. The pushed commit.
pub async fn push_bundle(
    mirror: &Path,
    bundle: &Path,
    session: &str,
    own: &str,
    branch: &str,
    remote: &str,
    token: Option<&str>,
) -> Result<String, String> {
    if !may_push(session, own, branch) {
        return Err(format!(
            "a session pushes its own branch and nothing else, not {}",
            super::super::printable(branch)
        ));
    }
    if !mirror.join("HEAD").exists() {
        std::fs::create_dir_all(mirror).map_err(|e| format!("{}: {e}", mirror.display()))?;
        run(
            {
                let mut c = git(mirror);
                c.args(["init", "-q", "--bare"]);
                c
            },
            "make the push's repo",
        )
        .await?;
    }
    let mut c = git(mirror);
    c.args(["bundle", "list-heads"]).arg(bundle);
    let heads = run(c, "read the session's bundle").await?;
    let Some(sha) = only_head(&heads, branch) else {
        return Err(format!(
            "the session's bundle names more than its branch: {}",
            super::super::printable(heads.trim())
        ));
    };
    let spec = format!("refs/heads/{branch}");
    let mut c = git(mirror);
    c.args(["fetch", "-q", "--no-tags", "--no-write-fetch-head"]).arg(bundle).arg(format!("+{spec}:{spec}"));
    run(c, "take the session's commits").await?;
    let mut c = git(mirror);
    c.args(["-c", "credential.helper=", "-c", &format!("credential.helper={HELPER}")])
        .args(["-c", "credential.useHttpPath=false", "-c", "push.followTags=false"])
        .args(["push", "--porcelain", "--no-follow-tags", "--no-verify", remote])
        .arg(format!("{spec}:{spec}"));
    if let Some(t) = token {
        c.env("ARUGULA_PUSH_TOKEN", t);
    }
    run(c, &format!("push {branch}")).await?;
    Ok(sha)
}

/// Bytes the session user's `argv` printed.
async fn as_user_bytes(u: &AsUser, argv: &[&str]) -> Result<Vec<u8>, String> {
    let argv: Vec<String> = argv.iter().map(|s| (*s).to_owned()).collect();
    let out = u
        .command(&[], &argv)
        .stdin(std::process::Stdio::null())
        .kill_on_drop(true)
        .output()
        .await
        .map_err(|e| format!("can't run {} as {}: {e}", argv[0], u.user))?;
    if out.status.success() {
        Ok(out.stdout)
    } else {
        Err(String::from_utf8_lossy(&out.stderr).trim().chars().take(400).collect())
    }
}

/// Revoke the push's token, the token as its own bearer: best effort (it
/// lasts an hour at most).
async fn revoke(url: &str, token: &str) {
    if !(url.starts_with("https://") || url.starts_with("http://127.0.0.1:")) {
        return;
    }
    let Ok(http) = crate::roots::http().timeout(Duration::from_secs(20)).build() else { return };
    let r = http
        .delete(url)
        .bearer_auth(token)
        .header("Accept", "application/vnd.github+json")
        .header("X-GitHub-Api-Version", "2022-11-28")
        .header("User-Agent", "arugulad")
        .send()
        .await;
    if !r.is_ok_and(|r| r.status().is_success() || r.status() == reqwest::StatusCode::UNAUTHORIZED) {
        warn!("hosted: a session's push token wasn't revoked; it expires within the hour");
    }
}

/// The ledger's place for session `l`'s push: its bundle and the daemon's
/// own repo.
fn place(dir: &Path, l: &Session) -> PathBuf {
    dir.join(super::super::DIR).join(super::DIR).join(format!("{}.push", l.session))
}

/// What a push of session `l` left, gone once the session ends: the
/// daemon's own repo and bundle, and a bundle a cut-off push left in its
/// user's home.
pub async fn clean(dir: &Path, l: &Session) {
    let _ = std::fs::remove_dir_all(place(dir, l));
    let bundle = Path::new(&l.user.home).join(format!(".arugula-push-{}.bundle", l.session)).display().to_string();
    let _ = super::as_user(&l.user, &["rm", "-f", "--", &bundle]).await;
}

/// Once a push of session `l` is back: an `end` that came while it ran
/// cleaned the session's slot then, and the push may have made its place
/// again since (its bundle, the daemon's repo). So an ended session's is
/// cleaned again here. Whether it had ended.
pub async fn clean_if_ended(dir: &Path, l: &Session) -> bool {
    let ended = super::read(dir, &l.session).ok().flatten().is_none_or(|now| now.ended.is_some());
    if ended {
        clean(dir, l).await;
    }
    ended
}

/// Push session `l`'s branch at the end of a turn (module docs): the commit
/// that's on GitHub now, as the checkout has it (pushed now, or before:
/// `l.pushed_head`), `None` when there's nothing to push (no repo, or no
/// commits of its own), or why it didn't push.
pub async fn push_turn(app: &App, l: &Session) -> Result<Option<String>, String> {
    let Some(repo) = l.repo.as_deref() else { return Ok(None) };
    let remote =
        remote_for(repo).ok_or_else(|| format!("{} isn't a GitHub repo's name", super::super::printable(repo)))?;
    let own = format!("refs/heads/{}", l.branch);
    let head = super::as_user(&l.user, &["git", "-C", &l.checkout, "rev-parse", "--verify", "-q", &own])
        .await
        .map(|h| h.trim().to_owned())
        .map_err(|e| format!("its branch: {e}"))?;
    if l.pushed_head.as_deref() == Some(head.as_str()) {
        return Ok(Some(head));
    }
    // Only its own commits: none past the repo's own branch, nothing to
    // push.
    let ahead = super::as_user(
        &l.user,
        &["git", "-C", &l.checkout, "rev-list", "--count", &format!("refs/remotes/origin/HEAD..{own}")],
    )
    .await
    .map(|n| n.trim() != "0")
    .unwrap_or(true);
    if !ahead {
        return Ok(None);
    }
    let dir = app.control.state_dir().to_owned();
    let place = place(&dir, l);
    crate::store::private_dir(&place).map_err(|e| format!("{}: {e}", place.display()))?;
    let bundle = Path::new(&l.user.home).join(format!(".arugula-push-{}.bundle", l.session)).display().to_string();
    let made = super::as_user(&l.user, &["git", "-C", &l.checkout, "bundle", "create", "-q", &bundle, &own]).await;
    let bytes = match made {
        Ok(_) => as_user_bytes(&l.user, &["cat", "--", &bundle]).await,
        Err(e) => Err(e),
    };
    let _ = super::as_user(&l.user, &["rm", "-f", "--", &bundle]).await;
    let bytes = bytes.map_err(|e| format!("its bundle: {e}"))?;
    if bytes.len() > BUNDLE_MAX {
        return Err("its bundle is over 512 MiB".into());
    }
    let mine = place.join("in.bundle");
    std::fs::write(&mine, &bytes).map_err(|e| format!("{}: {e}", mine.display()))?;
    // The token, for this push alone.
    let ask = PushAsk { session: l.session.clone(), branch: l.branch.clone() };
    let given: PushGiven =
        app.control.post_signed(WORK_SESSION_PUSH, &ask).await.map_err(|e| format!("asking control to push: {e:#}"))?;
    let (token, revoke_at) = match given {
        PushGiven::Given { token, revoke, .. } => (token, revoke),
        PushGiven::Refused { why } => {
            let _ = std::fs::remove_file(&mine);
            return Err(format!("control won't let it push: {why}"));
        }
    };
    let mirror = place.join("repo.git");
    let pushed = tokio::time::timeout(
        PUSH_MAX,
        push_bundle(&mirror, &mine, &l.session, &l.branch, &l.branch, &remote, Some(&token)),
    )
    .await
    .unwrap_or_else(|_| Err("the push took too long".into()));
    revoke(&revoke_at, &token).await;
    let mut t = token.into_bytes();
    t.iter_mut().for_each(|b| *b = 0);
    std::hint::black_box(&t);
    let _ = std::fs::remove_file(&mine);
    match &pushed {
        Ok(sha) => info!(session = l.session, branch = l.branch, head = sha, "hosted: pushed a session's branch"),
        Err(e) => warn!(session = l.session, branch = l.branch, error = e, "hosted: a session's push failed"),
    }
    let sha = pushed?;
    if sha != head {
        return Err(format!("it pushed {sha}, not {head}: the branch moved under it"));
    }
    Ok(Some(head))
}
