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
/// BASE` or `err WHY` last.
pub const SCRIPT: &str = r#"dir=$1; branch=$2; base=$3; repo=$4
case $dir in "~") dir=$HOME ;; "~/"*) dir=$HOME/${dir#"~/"} ;; esac
cd -- "$dir" 2>/dev/null || { printf 'err no such directory: %s\n' "$dir"; exit 0; }
top=$(git rev-parse --show-toplevel 2>/dev/null) || { printf 'err not a git repository: %s\n' "$dir"; exit 0; }
cd -- "$top" || exit 0
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
            .args(["-c", super::SCRIPT, "sh", &clone, "i3-x", "", ""])
            .output()
            .unwrap();
        let out = String::from_utf8_lossy(&out.stdout);
        assert_eq!(out.lines().last(), Some(format!("ok {clone}/.arugula/worktrees/i3-x main").as_str()), "{out}");
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
