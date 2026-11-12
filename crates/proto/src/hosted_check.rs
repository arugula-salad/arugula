//! What a rebuild of a hosted box would lose (Arugula #932): the script
//! that looks, run with bash on the box, and what it says, read back. The
//! box's daemon runs it for `GET /api/hosted/setup`; `arugula cloud
//! rebuild` runs it in a pane on a box whose daemon is older than that.

use crate::api::{HostedBranch, HostedCheck, HostedMissed, HostedRepo};

/// The script (hosted_check.sh): its arguments are the homes to look
/// through, none for the box's.
pub const SCRIPT: &str = include_str!("hosted_check.sh");

/// A repo (or linked worktree) the script found.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Found {
    pub repo: HostedRepo,
    /// The user it was read as, when not the one the script ran as.
    pub owner: String,
    /// Its repo's git directory (shared by its worktrees).
    pub git_dir: String,
}

/// What the script said.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Checked {
    pub check: HostedCheck,
    pub found: Vec<Found>,
}

/// Read what [`SCRIPT`] printed: its `arugula-check` lines, anything else
/// skipped.
pub fn parse(out: &str) -> Checked {
    let mut got = Checked::default();
    for line in out.lines() {
        let line = line.trim_end_matches('\r');
        let Some(rest) = line.strip_prefix("arugula-check\t") else { continue };
        let f: Vec<&str> = rest.split('\t').collect();
        let n = |i: usize| f.get(i).and_then(|v| v.trim().parse::<u32>().ok()).unwrap_or(0);
        match f.as_slice() {
            ["home", dir] => got.check.homes.push((*dir).to_owned()),
            ["missed", dir, why] => {
                got.check.missed.push(HostedMissed { dir: (*dir).to_owned(), why: (*why).to_owned() })
            }
            ["repo", dir, owner, git_dir, branch, ..] if f.len() == 8 => got.found.push(Found {
                repo: HostedRepo {
                    dir: (*dir).to_owned(),
                    branch: (*branch).to_owned(),
                    changed: n(5),
                    unpushed: n(6),
                    stashes: n(7),
                    ..HostedRepo::default()
                },
                owner: (*owner).to_owned(),
                git_dir: (*git_dir).to_owned(),
            }),
            ["branch", dir, name, _] => {
                if let Some(r) = got.found.last_mut().filter(|r| r.repo.dir == *dir) {
                    r.repo.branches.push(HostedBranch { name: (*name).to_owned(), commits: n(3) });
                }
            }
            _ => {}
        }
    }
    got
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn it_reads_what_the_script_says() {
        let out = "motd\r\n\
            arugula-check\thome\t/home/user\r\n\
            arugula-check\tmissed\t/home/ses2\tcan't read it as ses2: sudo failed\n\
            arugula-check\trepo\t/home/user/app\t\t/home/user/app/.git\tmain\t2\t3\t1\n\
            arugula-check\tbranch\t/home/user/app\tfeature\t3\n\
            arugula-check\trepo\t/home/ses1/work/x\tses1\t/home/ses1/work/x/.git\t\t0\t1\t0\n\
            arugula-check\tbranch\t/home/ses1/work/x\tHEAD\t1\n\
            arugula-check\trepo\ttoo\tfew\n";
        let got = parse(out);
        assert_eq!(got.check.homes, ["/home/user"]);
        assert_eq!(got.check.missed[0].dir, "/home/ses2");
        assert_eq!(got.found.len(), 2);
        let app = &got.found[0];
        assert_eq!((app.repo.changed, app.repo.unpushed, app.repo.stashes), (2, 3, 1));
        assert_eq!(app.repo.branches, [HostedBranch { name: "feature".into(), commits: 3 }]);
        assert_eq!((app.owner.as_str(), app.repo.branch.as_str()), ("", "main"));
        let x = &got.found[1];
        assert_eq!((x.owner.as_str(), x.git_dir.as_str()), ("ses1", "/home/ses1/work/x/.git"));
        assert_eq!(x.repo.branches[0].name, "HEAD");
    }
}
