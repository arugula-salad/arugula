//! What a turn left in the block's git repo (AR-1, #1475): HEAD when the
//! turn began, and, when it ended, the branch, whether HEAD is pushed, the
//! uncommitted files and the commits since. Git runs through the runner
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

/// The script: `$1` the directory, `$2` HEAD at the turn's start (or none).
/// It prints tagged lines, and exits 3 outside a repo.
const SCRIPT: &str = r#"
cd "$1" 2>/dev/null && git rev-parse --git-dir >/dev/null 2>&1 || exit 3
echo "b $(git symbolic-ref --short -q HEAD || echo '(detached)')"
if git rev-parse -q --verify HEAD >/dev/null 2>&1; then
  echo "h $(git rev-parse --short HEAD)"
  if [ -n "$(git branch -r --contains HEAD 2>/dev/null | head -n 1)" ]; then echo "p"; fi
fi
git status --porcelain -uall 2>/dev/null | sed 's/^/s /'
if [ -n "$2" ] && git cat-file -e "$2^{commit}" 2>/dev/null && git rev-parse -q --verify HEAD >/dev/null 2>&1; then
  echo "k"
  git log --no-decorate --format='c %h %s' "$2..HEAD" 2>/dev/null
  git diff --stat "$2" HEAD 2>/dev/null | sed 's/^/d /'
fi
"#;

/// HEAD (full) of the repo holding `cwd`, or `None` outside one.
pub async fn head(runner: &Runner, cwd: &str) -> Option<String> {
    let script = r#"cd "$1" 2>/dev/null && git rev-parse -q --verify HEAD"#;
    let (out, code) = timed(runner, script, &[cwd.to_owned()]).await?;
    let sha = String::from_utf8_lossy(&out).trim().to_owned();
    (code == Some(0) && !sha.is_empty()).then_some(sha)
}

/// The turn's result: its `message`, and the work in `cwd`'s repo, with
/// commits since `start` when that is known.
pub async fn result(runner: &Runner, cwd: &str, start: Option<&str>, message: String) -> TurnResult {
    let message = cut(message);
    let work = match timed(runner, SCRIPT, &[cwd.to_owned(), start.unwrap_or_default().to_owned()]).await {
        Some((out, Some(0))) => parse(&String::from_utf8_lossy(&out)),
        _ => None,
    };
    TurnResult { message, work }
}

async fn timed(runner: &Runner, script: &str, args: &[String]) -> Option<(Vec<u8>, Option<i32>)> {
    tokio::time::timeout(TIMEOUT, runner.sh(script, args)).await.ok()?.ok()
}

/// `message` cut to [`TurnResult::MESSAGE_MAX`] characters, and marked.
fn cut(message: String) -> String {
    match message.char_indices().nth(TurnResult::MESSAGE_MAX) {
        Some((at, _)) => {
            let more = message[at..].chars().count();
            format!("{}\n\n[cut: {more} more characters]", &message[..at])
        }
        None => message,
    }
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
        let long = "é".repeat(TurnResult::MESSAGE_MAX + 5);
        let c = cut(long);
        assert!(c.starts_with(&"é".repeat(TurnResult::MESSAGE_MAX)));
        assert!(c.ends_with("[cut: 5 more characters]"));
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
}
