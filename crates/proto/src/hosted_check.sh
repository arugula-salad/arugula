# What a rebuild of a hosted box would lose (Arugula #932). Read-only.
#
# Looks through each home it's given (none: this user's, and each other
# home under /home but the run user's, whose copies each run makes again)
# for git repos, and in each repo, at every worktree: files changed or not
# tracked (ignored ones aside), commits on no remote branch (on each local
# branch, and on a detached HEAD), and stashes. A home owned by someone
# else is read as them (`sudo -n -u`), as a session user's is.
#
# Says, one record a line, its fields after `arugula-check`, tab-separated:
#   home DIR                looked through DIR
#   missed DIR WHY          couldn't look at DIR
#   repo DIR OWNER GITDIR BRANCH CHANGED UNPUSHED STASHES
#                           a worktree (OWNER empty: this user); its repo's
#                           branches and stashes on the first of them
#   branch DIR NAME COMMITS a branch of the worktree before it with commits
#                           on no remote branch (HEAD: a detached one's on
#                           no branch at all)
# Each repo once, however many of its worktrees are under the homes.
# Bash 3 and BSD tools will do: tests run it on macOS too.
set -u -o pipefail
export GIT_TERMINAL_PROMPT=0 GIT_OPTIONAL_LOCKS=0 LC_ALL=C
me=$(id -un)
out() {
  local IFS=$'\t'
  printf 'arugula-check\t%s\n' "$*"
}
flat() { printf '%s' "$*" | tr '\t\r\n' '   '; }
count() { local n; n=$(wc -l); echo $((n)); }

if [ $# -eq 0 ]; then
  set -- "$HOME"
  for d in /home/*; do
    [ -d "$d" ] && [ "$d" != "$HOME" ] && [ "$d" != "/home/${ARUGULA_HOSTED_RUN_USER-runner}" ] && set -- "$@" "$d"
  done
fi

seen='
'
as=() owner_=
# A command as the home's owner.
run() { ${as[@]+"${as[@]}"} "$@" </dev/null; }
g() { run git -C "$@"; }

# One repo: each of its worktrees, its branches and its stashes.
repo() {
  local dir=$1 common wts w first=1 branch detached changed unpushed stashes ref n st e i inner=
  common=$(g "$dir" rev-parse --path-format=absolute --git-common-dir 2>&1) || {
    out missed "$dir" "$(flat "$common")"
    return
  }
  case "$seen" in *"
$common
"*) return ;; esac
  seen="$seen$common
"
  wts=$(g "$dir" worktree list --porcelain 2>/dev/null | sed -n 's/^worktree //p')
  [ -n "$wts" ] || wts=$dir
  while IFS= read -r w; do
    [ -d "$w" ] || continue
    # A bare repo's has no work tree.
    [ "$(g "$w" rev-parse --is-inside-work-tree 2>/dev/null)" = true ] || continue
    if ! st=$(g "$w" status --porcelain -z --no-renames --untracked-files=all 2>&1 | tr '\0' '\n'); then
      out missed "$w" "$(flat "$st")"
      continue
    fi
    # Every untracked file is listed on its own: a directory is a repo
    # inside this one (one of its worktrees, or a clone), looked at as a
    # repo of its own.
    changed=0
    while IFS= read -r e; do
      case "$e" in
        '') ;;
        '?? '*/) inner="$inner$w/${e#?? }
" ;;
        *) changed=$((changed + 1)) ;;
      esac
    done <<EOF
$st
EOF
    branch=$(g "$w" symbolic-ref --short -q HEAD 2>/dev/null) || branch=
    detached=0
    # A detached HEAD's commits on no branch, local or remote.
    if [ -z "$branch" ] && g "$w" rev-parse -q --verify HEAD >/dev/null 2>&1; then
      detached=$(g "$w" rev-list --count HEAD --not --branches --remotes 2>/dev/null || echo 0)
    fi
    unpushed=$detached
    stashes=0
    if [ $first = 1 ]; then
      n=$(g "$w" rev-list --count --branches --not --remotes 2>/dev/null || echo 0)
      unpushed=$((unpushed + n))
      stashes=$(g "$w" stash list 2>/dev/null | count)
    fi
    out repo "$w" "$owner_" "$common" "$branch" "$changed" "$unpushed" "$stashes"
    [ "$detached" -gt 0 ] && out branch "$w" HEAD "$detached"
    if [ $first = 1 ]; then
      g "$w" for-each-ref --format='%(refname)' refs/heads/ 2>/dev/null | while IFS= read -r ref; do
        n=$(g "$w" rev-list --count "$ref" --not --remotes 2>/dev/null || echo 0)
        [ "$n" -gt 0 ] && out branch "$w" "${ref#refs/heads/}" "$n"
      done
    fi
    first=0
  done <<EOF
$wts
EOF
  while IFS= read -r i; do
    [ -n "$i" ] && repo "${i%/}"
  done <<EOF
$inner
EOF
}

for home in "$@"; do
  [ -d "$home" ] || continue
  # shellcheck disable=SC2012 # stat takes other flags on BSD
  owner=$(ls -ld -- "$home" 2>/dev/null | awk '{print $3}')
  as=() owner_=
  if [ -n "$owner" ] && [ "$owner" != "$me" ]; then
    owner_=$owner
    as=(sudo -n -H -u "$owner" --)
    if ! why=$(run true 2>&1); then
      out missed "$home" "$(flat "can't read it as $owner: ${why:-sudo failed}")"
      continue
    fi
  fi
  out home "$home"
  # Not into hidden directories (state, caches, tools' own clones) or
  # build output: a repo's linked worktrees there are found from the repo.
  found=$(run find "$home" -maxdepth 6 -name .git -print -prune -o \( -name '.*' -o -name node_modules -o -name target \) -prune 2>/dev/null)
  while IFS= read -r git; do
    [ -n "$git" ] && repo "${git%/.git}"
  done <<EOF
$found
EOF
done
exit 0
