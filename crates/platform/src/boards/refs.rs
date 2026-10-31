//! Which issue a PR says it is for (aru-125): the references in a PR's title,
//! head branch and (when the content cache has it) body, and whether one
//! names an issue that is a board's row. A board holds one row for a piece
//! of work, so a PR that names the issue of a row is attached to that row
//! rather than added beside it ([`super::attach`]), and the one-time move to
//! lanes does the same for the rows it finds ([`crate::db::boards_lanes`]).
//!
//! A reference is `Arugula #N`, `Closes #N` (or `Fixes`, `Resolves` and
//! their other forms), `(#N)`, a branch segment that starts with the number
//! (`feat/357-plane`), or `IDENT-N` for a Plane item. A bare `#N` in prose
//! is not one. The PR's own number is never a reference to an issue.

/// What a text names.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Refs {
    /// Issue numbers: a native issue's, or a GitHub issue's of the PR's repo.
    pub numbers: Vec<u64>,
    /// Plane items as (key, number): `ARUGU-12` is (`ARUGU`, 12).
    pub plane: Vec<(String, u64)>,
}

impl Refs {
    fn add_number(&mut self, n: u64) {
        if !self.numbers.contains(&n) {
            self.numbers.push(n);
        }
    }

    fn add_plane(&mut self, key: &str, n: u64) {
        if !self.plane.iter().any(|(k, m)| k == key && *m == n) {
            self.plane.push((key.to_owned(), n));
        }
    }

    pub fn is_empty(&self) -> bool {
        self.numbers.is_empty() && self.plane.is_empty()
    }
}

/// Where an issue row's issue lives, for matching a reference.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Source {
    /// A native issue of the project.
    Native,
    /// A GitHub issue of this repo.
    Repo(u64),
    /// A Plane item of the connection with this key.
    Plane(String),
}

/// An issue as a reference names it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IssueKey {
    pub source: Source,
    pub number: u64,
}

impl Refs {
    /// Whether these name `issue`, as written in a PR of `repo` (a GitHub
    /// issue is named only from a PR of its own repo; a native issue from
    /// any; a Plane item by its key).
    pub fn name(&self, repo: Option<u64>, issue: &IssueKey) -> bool {
        match &issue.source {
            Source::Native => self.numbers.contains(&issue.number),
            Source::Repo(r) => repo == Some(*r) && self.numbers.contains(&issue.number),
            Source::Plane(key) => self.plane.iter().any(|(k, n)| k.eq_ignore_ascii_case(key) && *n == issue.number),
        }
    }
}

impl Refs {
    /// Whether these name `issue` loosely, as a row's note does: a number is
    /// a native issue's or any repo's.
    pub fn name_any(&self, issue: &IssueKey) -> bool {
        match &issue.source {
            Source::Native | Source::Repo(_) => self.numbers.contains(&issue.number),
            Source::Plane(_) => self.name(None, issue),
        }
    }
}

const CLOSING: [&str; 9] = ["closes", "closed", "close", "fixes", "fixed", "fix", "resolves", "resolved", "resolve"];

/// The references in a PR's `title`, `branch` and `body` (any may be empty).
/// `own` is the PR's number, which names itself in `(#1844)`.
pub fn references(title: &str, branch: &str, body: &str, own: Option<u64>) -> Refs {
    let mut r = Refs::default();
    for text in [title, body] {
        hashes(text, &mut r);
        plane(text, &mut r);
    }
    segments(branch, &mut r);
    plane(&branch.to_uppercase(), &mut r);
    if let Some(own) = own {
        r.numbers.retain(|n| *n != own);
    }
    r
}

/// What a row's `note` (or a block's branch) names: any `#N`, and a branch's
/// leading number, besides the forms of a PR's title.
pub fn references_in(text: &str) -> Refs {
    let mut r = references(text, text, "", None);
    for (i, _) in text.match_indices('#') {
        if let Some((n, _)) = number(&text[i + 1..]) {
            r.add_number(n);
        }
    }
    r
}

/// The digits that start `s`, as a number, and what follows them.
fn number(s: &str) -> Option<(u64, &str)> {
    let end = s.find(|c: char| !c.is_ascii_digit()).unwrap_or(s.len());
    if end == 0 || end > 9 {
        return None;
    }
    Some((s[..end].parse().ok()?, &s[end..]))
}

/// `#N` after `Arugula `, a closing word, or `(`.
fn hashes(text: &str, r: &mut Refs) {
    let lower = text.to_lowercase();
    for (i, _) in lower.match_indices('#') {
        let Some((n, _)) = number(&lower[i + 1..]) else { continue };
        let before = &lower[..i];
        let word = before.trim_end_matches(' ').trim_end_matches(':');
        let spaced = before.len() > word.len();
        let last = word.rsplit(|c: char| !c.is_ascii_alphabetic()).next().unwrap_or("");
        if before.ends_with('(') || (spaced && (last == "arugula" || CLOSING.contains(&last))) {
            r.add_number(n);
        }
    }
}

/// A branch's segments that start with a number and a dash or the end:
/// `feat/365-boards-model` names 365.
fn segments(branch: &str, r: &mut Refs) {
    for seg in branch.split('/') {
        if let Some((n, rest)) = number(seg)
            && (rest.is_empty() || rest.starts_with('-'))
        {
            r.add_number(n);
        }
    }
}

/// `KEY-N` words: capitals and digits starting with a capital, a dash and
/// digits, not part of a longer word.
fn plane(text: &str, r: &mut Refs) {
    let b = text.as_bytes();
    let mut i = 0;
    while i < b.len() {
        let starts = b[i].is_ascii_uppercase() && (i == 0 || !b[i - 1].is_ascii_alphanumeric());
        if !starts {
            i += 1;
            continue;
        }
        let key_end = (i..b.len()).find(|&j| !(b[j].is_ascii_uppercase() || b[j].is_ascii_digit())).unwrap_or(b.len());
        if key_end < b.len()
            && b[key_end] == b'-'
            && let Some((n, rest)) = number(&text[key_end + 1..])
            && rest.chars().next().is_none_or(|c| !c.is_alphanumeric())
        {
            r.add_plane(&text[i..key_end], n);
        }
        i = key_end.max(i + 1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Each form a PR names its issue in, and the ones that don't count.
    #[test]
    fn the_forms_a_pr_names_an_issue_in() {
        let r = references("Boards across projects: /boards (#339) (#1844)", "feat/339-boards", "", Some(1844));
        assert_eq!(r.numbers, vec![339]);
        assert_eq!(references("Arugula #365: lanes", "", "", None).numbers, vec![365]);
        assert_eq!(references("Closes #12, fixes: #13", "", "", None).numbers, vec![12, 13]);
        assert_eq!(references("Resolved #7", "", "", None).numbers, vec![7]);
        assert_eq!(references("see #44 for why", "", "", None).numbers, Vec::<u64>::new());
        assert_eq!(references("", "fix/1850-tracker-writes", "", None).numbers, vec![1850]);
        assert_eq!(references("", "feat/v2-things", "", None).numbers, Vec::<u64>::new());
        assert_eq!(references("", "", "Closes #9 and (#10)", None).numbers, vec![9, 10]);
        assert_eq!(references("Plane: ARUGU-12 and not xARUGU-13", "", "", None).plane, vec![("ARUGU".into(), 12)]);
        assert_eq!(references("", "feat/arugu-14-x", "", None).plane, vec![("ARUGU".into(), 14)]);
    }

    /// A native issue is named from any PR; a GitHub issue only from a PR of
    /// its repo; a Plane item by its key.
    #[test]
    fn a_reference_names_the_issue_it_says() {
        let r = references("Arugula #5 and ARUGU-6", "", "", None);
        let native = IssueKey { source: Source::Native, number: 5 };
        let gh = |repo| IssueKey { source: Source::Repo(repo), number: 5 };
        let plane = |n| IssueKey { source: Source::Plane("ARUGU".into()), number: n };
        assert!(r.name(Some(1), &native) && r.name(None, &native));
        assert!(r.name(Some(1), &gh(1)) && !r.name(Some(2), &gh(1)) && !r.name(None, &gh(1)));
        assert!(r.name(None, &plane(6)) && !r.name(None, &plane(5)));
    }
}
