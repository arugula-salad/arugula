//! The git worktree an agent on an issue works in: a GitHub issue's block
//! makes it in the daemon (`arugula issue agent %N`), and the CLI makes it
//! for a native issue on the platform (`arugula issue agent #N`, M199).

/// `$1` the clone, `$2` the branch, `$3` the base branch (empty: the
/// remote's HEAD), `$4` owner/name (empty: none). Fetches the base from the
/// remote whose URL names the repo (else `origin`), and makes a worktree in
/// `.claude/worktrees/BRANCH` (where the repo keeps its worktrees) or
/// `.arugula/worktrees/BRANCH` on a new branch from it that tracks
/// nothing (or on the branch, if it's there already). An existing
/// worktree is left as it is, and one an illogical daemon made in
/// `.illogical/worktrees` is used where it is. Says `ok WORKTREE
/// BASE` or `err WHY` last, after a `top CLONE` line. `$5` `new`
/// (`start_agent`'s `worktree`, AR-2): refuse a branch or a worktree
/// that's there already, rather than use it.
pub const SCRIPT: &str = r#"dir=$1; branch=$2; base=$3; repo=$4; fresh=$5
case $dir in "~") dir=$HOME ;; "~/"*) dir=$HOME/${dir#"~/"} ;; esac
cd -- "$dir" 2>/dev/null || { printf 'err no such directory: %s\n' "$dir"; exit 0; }
top=$(git rev-parse --show-toplevel 2>/dev/null) || { printf 'err not a git repository: %s\n' "$dir"; exit 0; }
cd -- "$top" || exit 0
printf 'top %s\n' "$top"
if [ "$fresh" = new ]; then
  git check-ref-format --branch "$branch" >/dev/null 2>&1 || { printf 'err %s is not a branch name\n' "$branch"; exit 0; }
  ! git show-ref --verify -q "refs/heads/$branch" || { printf 'err branch %s exists already\n' "$branch"; exit 0; }
fi
remote=
[ -z "$repo" ] || for r in $(git remote); do
  case $(git remote get-url "$r" 2>/dev/null) in *"/$repo"|*"/$repo.git"|*":$repo"|*":$repo.git"|*"/$repo/") remote=$r; break ;; esac
done
[ -n "$remote" ] || remote=origin
if [ -z "$base" ]; then
  base=$(git ls-remote --symref "$remote" HEAD 2>/dev/null | awk '$1 == "ref:" { sub("^refs/heads/", "", $2); print $2; exit }')
  [ -n "$base" ] || base=main
fi
if [ -d .claude/worktrees ]; then w=.claude/worktrees/$branch
elif [ -e ".illogical/worktrees/$branch/.git" ]; then w=.illogical/worktrees/$branch; else
  w=.arugula/worktrees/$branch; mkdir -p .arugula/worktrees
  ex=$(git rev-parse --git-common-dir)/info/exclude; mkdir -p "$(dirname "$ex")"
  grep -qx '.arugula/' "$ex" 2>/dev/null || echo '.arugula/' >> "$ex"
fi
if [ "$fresh" = new ] && [ -e "$w" ]; then printf 'err %s exists already\n' "$top/$w"; exit 0; fi
if [ -e "$w/.git" ]; then printf 'ok %s/%s %s\n' "$top" "$w" "$base"; exit 0; fi
out=$(git fetch -q --no-tags "$remote" "+refs/heads/$base:refs/remotes/$remote/$base" 2>&1) ||
  { printf 'err git fetch %s %s failed: %s\n' "$remote" "$base" "$(printf '%s' "$out" | tail -n 1)"; exit 0; }
if git show-ref --verify -q "refs/heads/$branch"; then
  out=$(git worktree add -q "$w" "$branch" 2>&1)
else
  out=$(git worktree add -q --no-track -b "$branch" "$w" "refs/remotes/$remote/$base" 2>&1)
fi
[ $? -eq 0 ] || { printf 'err git worktree add failed: %s\n' "$(printf '%s' "$out" | tail -n 1)"; exit 0; }
printf 'ok %s/%s %s\n' "$top" "$w" "$base"
"#;

/// What closing a block does to its worktree (AR-2): `$1` the clone, `$2`
/// the worktree, `$3` its branch, `$4` the base. Fetches, then removes the
/// worktree (never with `--force`) and the local branch only if nothing
/// is uncommitted, HEAD is on a remote branch, and HEAD and the branch
/// are in `origin/BASE`. A squash merge isn't in `origin/BASE`: for a
/// clean, pushed worktree on a github.com origin, `gh` may say a merged PR
/// from the branch had exactly HEAD as its head, which counts. Never
/// touches a remote branch. Says `removed`, `kept WHY` or `err WHY` last.
pub const CLOSE: &str = r#"repo=$1; w=$2; branch=$3; base=$4
[ -e "$w/.git" ] || { printf 'gone\n'; exit 0; }
why=
n=$(git -C "$w" status --porcelain 2>/dev/null | wc -l | tr -d ' ')
[ "$n" -eq 0 ] || why="$n uncommitted file$([ "$n" -eq 1 ] || echo s)"
head=$(git -C "$w" rev-parse HEAD 2>/dev/null) || { printf 'kept cannot read its HEAD\n'; exit 0; }
[ -n "$(git -C "$w" branch -r --contains "$head" 2>/dev/null)" ] || why="${why:+$why, }commits not pushed"
fetched=1
git -C "$repo" fetch -q --no-tags origin "+refs/heads/$base:refs/remotes/origin/$base" >/dev/null 2>&1 || fetched=
merged=
if [ -n "$fetched" ] && git -C "$repo" merge-base --is-ancestor "$head" "refs/remotes/origin/$base" 2>/dev/null; then
  merged=1
  if git -C "$repo" show-ref --verify -q "refs/heads/$branch"; then
    git -C "$repo" merge-base --is-ancestor "refs/heads/$branch" "refs/remotes/origin/$base" 2>/dev/null || merged=
  fi
fi
# The forge: a squash-merged PR whose head is exactly HEAD (a PR merged at an
# older commit, with more commits since, is not).
if [ -z "$merged" ] && [ -n "$fetched" ] && [ -z "$why" ] && command -v gh >/dev/null 2>&1 \
  && case $(git -C "$repo" config --get remote.origin.url 2>/dev/null) in *github.com[:/]*) true ;; *) false ;; esac; then
  tip=$(git -C "$repo" rev-parse -q --verify "refs/heads/$branch" 2>/dev/null)
  if [ -z "$tip" ] || [ "$tip" = "$head" ]; then
    if command -v timeout >/dev/null 2>&1; then t="timeout 10"; else t=; fi
    oids=$(cd "$repo" && $t gh pr list --head "$branch" --state merged --json headRefOid,number --limit 5 -q '.[].headRefOid' 2>/dev/null)
    case "
$oids
" in *"
$head
"*) merged=1 ;; esac
  fi
fi
if [ -z "$merged" ]; then
  if [ -z "$fetched" ]; then why="${why:+$why, }couldn't fetch origin/$base to check it's merged"
  else why="${why:+$why, }not merged into origin/$base"; fi
fi
[ -z "$why" ] || { printf 'kept %s\n' "$why"; exit 0; }
out=$(git -C "$repo" worktree remove "$w" 2>&1) || { printf 'kept git worktree remove: %s\n' "$(printf '%s' "$out" | tail -n 1)"; exit 0; }
# Merged (into origin/BASE, or the PR's head is HEAD) above, so -D (-d would judge it against the clone's HEAD).
if git -C "$repo" show-ref --verify -q "refs/heads/$branch"; then
  out=$(git -C "$repo" branch -q -D "$branch" 2>&1) || { printf 'removed worktree, kept branch: %s\n' "$(printf '%s' "$out" | tail -n 1)"; exit 0; }
fi
printf 'removed\n'
"#;

#[cfg(all(test, unix))]
mod tests {
    /// With no repo named (a native issue's), the base comes from `origin`.
    #[test]
    fn no_repo_is_origins() {
        let dir = std::env::temp_dir().join(format!("arugula-worktree-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        // git prints the real path: macOS's temp dir is under a symlink
        // (/var is /private/var).
        let dir = dir.canonicalize().unwrap();
        let sh = |script: &str| {
            let out = std::process::Command::new("sh").arg("-c").arg(script).current_dir(&dir).output().unwrap();
            assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
        };
        sh("git init -q -b main up && git -C up -c user.name=t -c user.email=t@t commit -q --allow-empty -m one \
            && git clone -q up clone");
        let clone = dir.join("clone").display().to_string();
        let out = std::process::Command::new("sh")
            .args(["-c", super::SCRIPT, "sh", &clone, "i3-x", "", "", ""])
            .output()
            .unwrap();
        let out = String::from_utf8_lossy(&out.stdout);
        assert_eq!(out.lines().last(), Some(format!("ok {clone}/.arugula/worktrees/i3-x main").as_str()), "{out}");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    /// A squash-merged branch isn't in `origin/main`: with a github.com
    /// origin, a fake `gh` saying a merged PR had HEAD as its head makes it
    /// merged; one that names another commit leaves it kept.
    #[test]
    fn close_asks_the_forge_about_a_squash_merge() {
        let dir = std::env::temp_dir().join(format!("arugula-close-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("bin")).unwrap();
        let dir = dir.canonicalize().unwrap();
        let sh = |script: &str| {
            let out = std::process::Command::new("sh").arg("-c").arg(script).current_dir(&dir).output().unwrap();
            assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
            String::from_utf8_lossy(&out.stdout).trim().to_owned()
        };
        let g = "git -c user.name=t -c user.email=t@t";
        sh(&format!(
            "git init -q --bare -b main up.git && git clone -q up.git clone 2>/dev/null && cd clone \
             && {g} commit -q --allow-empty -m one && git push -q origin main \
             && git remote set-url origin https://github.com/o/r.git && git remote set-url --push origin ../up.git"
        ));
        // The fetch and the push go to the bare repo; the URL is only for the check.
        sh("cd clone && git config url.\"$PWD/../up.git\".insteadOf https://github.com/o/r.git");
        let close = |name: &str, oid: &str| {
            sh(&format!(
                "cd clone && git worktree add -q -b {name} ../{name} && cd ../{name} && {g} commit -q --allow-empty -m w \
                 && git push -q origin {name}"
            ));
            let head = sh(&format!("git -C {name} rev-parse HEAD"));
            let want = if oid.is_empty() { head } else { oid.to_owned() };
            std::fs::write(dir.join("bin/gh"), format!("#!/bin/sh\necho {want}\n")).unwrap();
            sh("chmod +x bin/gh");
            let out = std::process::Command::new("sh")
                .args(["-c", super::CLOSE, "sh"])
                .args([dir.join("clone").to_str().unwrap(), dir.join(name).to_str().unwrap(), name, "main"])
                .env("PATH", format!("{}:{}", dir.join("bin").display(), std::env::var("PATH").unwrap()))
                .output()
                .unwrap();
            String::from_utf8_lossy(&out.stdout).lines().last().unwrap_or_default().to_owned()
        };
        let stale = close("stale", "0000000000000000000000000000000000000001");
        assert!(stale.starts_with("kept ") && stale.contains("not merged into origin/main"), "{stale}");
        assert!(dir.join("stale").exists());
        assert_eq!(close("squashed", ""), "removed");
        assert!(!dir.join("squashed").exists());
        assert!(sh("cd clone && git branch --list squashed").is_empty());
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
