//! What a turn left in the block's git repo (AR-1, #1475): the branch,
//! whether HEAD is pushed, the uncommitted files and the commits since the
//! turn began. Git runs through the runner
//! the worktree code uses, on the block's host, off the agent's lock, and
//! under a short timeout; any failure is `None`, never an error.

use std::time::Duration;

use arugula_proto::api::{TurnResult, Work};

use crate::review::Runner;

const TIMEOUT: Duration = Duration::from_secs(8);
/// Uncommitted paths listed.
const PATHS_MAX: usize = 50;
/// Lines of `git diff --stat` kept: files, then its summary line.
const STAT_MAX: usize = 20;

/// The script: `$1` the directory, `$2` the turn's start in unix seconds
/// (or none). It prints tagged lines, and exits 3 outside a repo.
///
/// HEAD at the turn's start comes from HEAD's reflog, which is on disk (so
/// it survives a restart) and can't miss a quick commit the way reading
/// HEAD after the prompt could. Entries are newest first, each with the
/// time it was made: the start is the newest entry made strictly before
/// the turn's second. (Strictly: the reflog has one-second resolution, and
/// an entry in the turn's own second may be the agent's, so counting it as
/// the turn's can only over-report.) If every entry is newer, as when the
/// worktree was made during the turn, the start is the oldest entry's old
/// value: its parent when that entry is a commit (a root commit has none:
/// then every commit is the turn's), else the entry itself (a checkout or
/// branch creation, whose result is where the work began).
/// No reflog, or nothing that resolves: no commits or diffstat.
const SCRIPT: &str = r#"
cd "$1" 2>/dev/null && git rev-parse --git-dir >/dev/null 2>&1 || exit 3
echo "b $(git symbolic-ref --short -q HEAD || echo '(detached)')"
if git rev-parse -q --verify HEAD >/dev/null 2>&1; then
  echo "h $(git rev-parse --short HEAD)"
  if [ -n "$(git branch -r --contains HEAD 2>/dev/null | head -n 1)" ]; then echo "p"; fi
fi
git status --porcelain -uall 2>/dev/null | sed 's/^/s /'
start=
all=
if [ -n "$2" ]; then
  pick=$(git reflog --date=unix --format='%H %gd %gs' HEAD 2>/dev/null | awk -v t="$2" '
    { n = $2; sub(/^HEAD@\{/, "", n); sub(/\}$/, "", n) }
    !found && n + 0 < t + 0 { print "at " $1; found = 1 }
    { last = $1 " " $3 }
    END { if (!found && last != "") print "first " last }')
  set -- "$1" "$2" $pick
  case "$3" in
    at) start=$4 ;;
    first) case "$5" in commit*) start=$(git rev-parse -q --verify "$4^" 2>/dev/null) || all=1 ;; *) start=$4 ;; esac ;;
  esac
fi
if [ -n "$all" ]; then
  echo "k"
  git log --no-decorate --format='c %h %s' HEAD 2>/dev/null
  git diff --stat 4b825dc642cb6eb9a060e54bf8d69288fbee4904 HEAD 2>/dev/null | sed 's/^/d /'
elif [ -n "$start" ] && git cat-file -e "$start^{commit}" 2>/dev/null && git rev-parse -q --verify HEAD >/dev/null 2>&1; then
  echo "k"
  git log --no-decorate --format='c %h %s' "$start..HEAD" 2>/dev/null
  git diff --stat "$start" HEAD 2>/dev/null | sed 's/^/d /'
fi
"#;

/// The turn's result: its `message`, and the work in `cwd`'s repo, with
/// the commits since the turn began (`started_ms`) when the reflog says.
pub async fn result(runner: &Runner, cwd: &str, started_ms: Option<u64>, message: String) -> TurnResult {
    let message = cut(message);
    let secs = started_ms.map(|ms| (ms / 1000).to_string()).unwrap_or_default();
    let work = match timed(runner, SCRIPT, &[cwd.to_owned(), secs]).await {
        Some((out, Some(0))) => parse(&String::from_utf8_lossy(&out)),
        _ => None,
    };
    TurnResult { message, work }
}

async fn timed(runner: &Runner, script: &str, args: &[String]) -> Option<(Vec<u8>, Option<i32>)> {
    tokio::time::timeout(TIMEOUT, runner.sh(script, args)).await.ok()?.ok()
}

/// `message` cut to its last [`TurnResult::MESSAGE_MAX`] characters, and
/// marked: a final message ends with its summary and caveats (Arugula
/// #215).
fn cut(message: String) -> String {
    let n = message.chars().count();
    if n <= TurnResult::MESSAGE_MAX {
        return message;
    }
    let more = n - TurnResult::MESSAGE_MAX;
    let at = message.char_indices().nth(more).map_or(0, |(at, _)| at);
    format!("[cut: {more} characters before this]\n\n…{}", &message[at..])
}

fn parse(out: &str) -> Option<Work> {
    let (mut branch, mut head, mut pushed) = (None, String::new(), false);
    let (mut paths, mut total) = (vec![], 0u32);
    let (mut known, mut commits, mut stat) = (false, vec![], vec![]);
    for line in out.lines() {
        let (tag, rest) = line.split_once(' ').unwrap_or((line, ""));
        match tag {
            "b" => branch = Some(rest.to_owned()),
            "h" => head = rest.to_owned(),
            "p" => pushed = true,
            "s" => {
                total += 1;
                if paths.len() < PATHS_MAX {
                    // "XY path": drop the two status columns and the space.
                    paths.push(rest.get(3..).unwrap_or(rest).to_owned());
                }
            }
            "k" => known = true,
            "c" => commits.push(rest.to_owned()),
            "d" => stat.push(rest.trim_end().to_owned()),
            _ => {}
        }
    }
    let diffstat = known.then(|| shorten(stat));
    Some(Work {
        branch: branch?,
        head,
        pushed,
        uncommitted: paths,
        uncommitted_total: total,
        commits: known.then_some(commits),
        diffstat,
    })
}

/// The stat's first lines and its summary, with a count for the rest.
fn shorten(mut lines: Vec<String>) -> String {
    if lines.len() > STAT_MAX + 1 {
        let summary = lines.pop().unwrap_or_default();
        let more = lines.len() - STAT_MAX;
        lines.truncate(STAT_MAX);
        lines.push(format!("… {more} more files"));
        lines.push(summary);
    }
    lines.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_long_message_is_cut_on_a_char_boundary_and_marked() {
        let long = format!("{}{}", "a".repeat(5), "é".repeat(TurnResult::MESSAGE_MAX));
        let c = cut(long);
        assert!(c.starts_with("[cut: 5 characters before this]\n\n…"), "{c}");
        assert!(c.ends_with(&format!("…{}", "é".repeat(TurnResult::MESSAGE_MAX))), "the end is kept");
        assert_eq!(cut("short".into()), "short");
    }

    #[test]
    fn the_script_output_parses() {
        let w = parse("b main\nh abc1234\np\ns  M a.txt\ns ?? b c.txt\nk\nc def5678 one\nd  f.txt | 2 +-\nd  1 file changed, 1 insertion(+), 1 deletion(-)\n")
            .unwrap();
        assert_eq!((w.branch.as_str(), w.head.as_str(), w.pushed), ("main", "abc1234", true));
        assert_eq!(w.uncommitted, vec!["a.txt", "b c.txt"]);
        assert_eq!(w.commits, Some(vec!["def5678 one".to_owned()]));
        assert_eq!(w.line(), "branch main at abc1234 (pushed), UNCOMMITTED: 2 files, 1 commit (+1 -1)");
        assert!(parse("b main\nh abc\n").unwrap().commits.is_none());
    }

    #[tokio::test]
    async fn the_start_is_found_in_the_reflog_and_the_commits_follow() {
        let dir = std::env::temp_dir().join(format!("ilg-turnwork-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let git = |args: &[&str]| {
            let out = std::process::Command::new("git")
                .args(["-c", "user.name=T", "-c", "user.email=t@example.com", "-c", "core.logAllRefUpdates=true"])
                .args(args)
                .current_dir(&dir)
                .output()
                .unwrap();
            assert!(out.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&out.stderr));
        };
        let commit = |f: &str| {
            std::fs::write(dir.join(f), f).unwrap();
            git(&["add", f]);
            git(&["commit", "-q", "-m", &f.to_uppercase()]);
        };
        git(&["init", "-q", "-b", "main"]);
        commit("x");
        // The reflog counts seconds: let X fall before the turn's second.
        tokio::time::sleep(Duration::from_millis(1100)).await;
        let started = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_millis() as u64;
        commit("y");
        commit("z");
        let runner = Runner::Local { env: vec![], home: std::env::temp_dir() };
        let cwd = dir.display().to_string();
        let w = result(&runner, &cwd, Some(started), "m".into()).await.work.unwrap();
        let subjects: Vec<&str> = w.commits.as_ref().unwrap().iter().map(|c| c.split_once(' ').unwrap().1).collect();
        assert_eq!(subjects, ["Z", "Y"]);
        let stat = w.diffstat.unwrap();
        assert!(stat.contains(" y ") && stat.contains(" z ") && !stat.contains(" x "), "{stat}");

        // Every entry is newer than the turn, and the oldest is the first
        // commit, which has no parent: all of them are the turn's.
        let w = result(&runner, &cwd, Some(1000), "m".into()).await.work.unwrap();
        assert_eq!(w.commits.unwrap().len(), 3);
        // No start time: the rest still comes.
        let w = result(&runner, &cwd, None, "m".into()).await.work.unwrap();
        assert!(w.commits.is_none() && w.branch == "main");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
