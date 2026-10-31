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
//!
//! A merged PR closes the native issues it names and moves the Plane items it
//! names to Done ([`crate::forge::closes`]): `part of`, `refs` or `see` before
//! `Arugula #N` or `KEY-N` only links ([`Refs::soft`], [`Refs::soft_plane`]).
//!
//! `Arugula #N` names a native issue only. Any other `#N` names the GitHub
//! issue N of the PR's repo or, when the board has no row for that, the
//! native issue N ([`rows_named`], which decides it for every caller). A
//! row is named by any issue of its work item (Arugula #405), so a native
//! issue exported to GitHub is one row that either number finds.

/// What a text names.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Refs {
    /// Issue numbers, as `Closes #N` or `(#N)` say them: a GitHub issue's of
    /// the PR's repo, or a native issue's.
    pub numbers: Vec<u64>,
    /// Native issue numbers, as `Arugula #N` says them.
    pub native: Vec<u64>,
    /// Those of `native` written only as `part of Arugula #N`, `refs Arugula
    /// #N` or `see Arugula #N`: linked, but not closed when the PR merges.
    pub soft: Vec<u64>,
    /// Plane items as (key, number): `ARUGU-12` is (`ARUGU`, 12).
    pub plane: Vec<(String, u64)>,
    /// Those of `plane` written only as `part of ARUGU-12`, `refs ARUGU-12`
    /// or `see ARUGU-12`: linked, but not moved to Done when the PR merges.
    pub soft_plane: Vec<(String, u64)>,
}

impl Refs {
    fn add_number(&mut self, n: u64) {
        if !self.numbers.contains(&n) {
            self.numbers.push(n);
        }
    }

    fn add_native(&mut self, n: u64) {
        if !self.native.contains(&n) {
            self.native.push(n);
        }
        self.soft.retain(|s| *s != n);
    }

    /// A native issue named softly: soft unless it is named plainly too,
    /// before or after.
    fn add_soft(&mut self, n: u64) {
        if !self.native.contains(&n) {
            self.native.push(n);
            self.soft.push(n);
        }
    }

    /// The native issues a merge closes: those named plainly.
    pub fn closing_native(&self) -> impl Iterator<Item = u64> + '_ {
        self.native.iter().copied().filter(|n| !self.soft.contains(n))
    }

    /// A Plane item named softly is soft unless it is named plainly too,
    /// before or after.
    fn add_plane(&mut self, key: &str, n: u64, soft: bool) {
        let named = |(k, m): &(String, u64)| k == key && *m == n;
        if !self.plane.iter().any(named) {
            self.plane.push((key.to_owned(), n));
            if soft {
                self.soft_plane.push((key.to_owned(), n));
            }
        } else if !soft {
            self.soft_plane.retain(|p| !named(p));
        }
    }

    /// The Plane items a merge moves to Done: those named plainly.
    pub fn closing_plane(&self) -> impl Iterator<Item = &(String, u64)> + '_ {
        self.plane.iter().filter(|p| !self.soft_plane.contains(p))
    }

    pub fn is_empty(&self) -> bool {
        self.numbers.is_empty() && self.native.is_empty() && self.plane.is_empty()
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
            Source::Native => self.numbers.contains(&issue.number) || self.native.contains(&issue.number),
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
            Source::Native => self.numbers.contains(&issue.number) || self.native.contains(&issue.number),
            Source::Repo(_) => self.numbers.contains(&issue.number),
            Source::Plane(_) => self.name(None, issue),
        }
    }
}

/// A row of a board that a PR's references may name.
#[derive(Debug, Clone, Copy)]
pub struct Candidate<'a> {
    pub row: &'a str,
    /// The row's work item, if it has one: rows of one are one row.
    pub work_item: Option<&'a str>,
    /// Every way a reference names the row's work (its issue, and the other
    /// issues of its work item).
    pub keys: &'a [IssueKey],
}

/// The rows of one board that a PR of `repo` naming `refs` is attached to:
/// at most one for each issue it names.
/// - `Arugula #N`: the row of native issue N.
/// - `#N` in any other form: the row of GitHub issue N of the PR's own repo;
///   only when the board has none, the row of native issue N.
/// - `KEY-N`: the row of that Plane item.
///
/// A row is found by any of its work's keys, so a native issue exported to
/// GitHub is one row whichever number names it. This is the only place the
/// rule is written: the platform's automatic attach and a hand add both ask
/// it, so they can't disagree.
pub fn rows_named<'a>(refs: &Refs, repo: Option<u64>, rows: &[Candidate<'a>]) -> Vec<&'a str> {
    let first = |source: &dyn Fn(&Source) -> bool, number: u64| {
        rows.iter().find(|c| c.keys.iter().any(|k| k.number == number && source(&k.source))).map(|c| c.row)
    };
    let mut out: Vec<&'a str> = vec![];
    let mut take = |row: Option<&'a str>| {
        if let Some(row) = row
            && !out.contains(&row)
        {
            out.push(row);
        }
    };
    for n in &refs.native {
        take(first(&|s| *s == Source::Native, *n));
    }
    for n in &refs.numbers {
        let own = repo.and_then(|r| first(&|s| *s == Source::Repo(r), *n));
        take(own.or_else(|| first(&|s| *s == Source::Native, *n)));
    }
    for (key, n) in &refs.plane {
        take(first(&|s| matches!(s, Source::Plane(k) if k.eq_ignore_ascii_case(key)), *n));
    }
    // Rows of one work item are one row, whichever named it.
    let mut seen: Vec<&str> = vec![];
    out.retain(|row| match rows.iter().find(|c| c.row == *row).and_then(|c| c.work_item) {
        Some(w) if seen.contains(&w) => false,
        Some(w) => {
            seen.push(w);
            true
        }
        None => true,
    });
    out
}

const CLOSING: [&str; 9] = ["closes", "closed", "close", "fixes", "fixed", "fix", "resolves", "resolved", "resolve"];

/// The references in a PR's `title`, `branch` and `body` (any may be empty).
/// `own` is the PR's number, which names itself in `(#1844)`.
pub fn references(title: &str, branch: &str, body: &str, own: Option<u64>) -> Refs {
    let mut r = Refs::default();
    for text in [title, body] {
        hashes(text, &mut r);
        plane(text, &mut r, true);
    }
    segments(branch, &mut r);
    plane(&branch.to_uppercase(), &mut r, false);
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
        if spaced && last == "arugula" {
            if soft_before(&word[..word.len() - last.len()]) {
                r.add_soft(n);
            } else {
                r.add_native(n);
            }
        } else if before.ends_with('(') || (spaced && CLOSING.contains(&last)) {
            r.add_number(n);
        }
    }
}

/// Whether `before`, the text ahead of `Arugula` or of a Plane key, ends in `part of`, `refs` or
/// `see` (a colon allowed): the reference only links.
fn soft_before(before: &str) -> bool {
    let words = |s: &'_ str| -> Vec<String> {
        s.split(|c: char| !c.is_ascii_alphabetic()).filter(|w| !w.is_empty()).map(str::to_owned).rev().take(2).collect()
    };
    match words(before).as_slice() {
        [w, ..] if w == "refs" || w == "see" => true,
        [w, p] => w == "of" && p == "part",
        _ => false,
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
/// digits, not part of a longer word. `soft_ok` is whether `part of` before
/// one makes it soft (not in a branch).
fn plane(text: &str, r: &mut Refs, soft_ok: bool) {
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
            r.add_plane(&text[i..key_end], n, soft_ok && soft_before(&text[..i].to_lowercase()));
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
        let named = references("Arugula #365: lanes", "", "", None);
        assert_eq!((named.native, named.numbers), (vec![365], vec![]));
        let soft = references("Part of Arugula #5, see: arugula #6, REFS Arugula #7, Arugula #8", "", "", None);
        assert_eq!((soft.native, soft.soft), (vec![5, 6, 7, 8], vec![5, 6, 7]));
        let both = references("Part of Arugula #5", "", "Closes Arugula #5. Arugula #6, refs Arugula #6", None);
        assert_eq!((both.closing_native().collect::<Vec<_>>(), both.soft), (vec![5, 6], vec![]));
        assert_eq!(references("Closes #12, fixes: #13", "", "", None).numbers, vec![12, 13]);
        assert_eq!(references("Resolved #7", "", "", None).numbers, vec![7]);
        assert_eq!(references("see #44 for why", "", "", None).numbers, Vec::<u64>::new());
        assert_eq!(references("", "fix/1850-tracker-writes", "", None).numbers, vec![1850]);
        assert_eq!(references("", "feat/v2-things", "", None).numbers, Vec::<u64>::new());
        assert_eq!(references("", "", "Closes #9 and (#10)", None).numbers, vec![9, 10]);
        assert_eq!(references("Plane: ARUGU-12 and not xARUGU-13", "", "", None).plane, vec![("ARUGU".into(), 12)]);
        assert_eq!(references("", "feat/arugu-14-x", "", None).plane, vec![("ARUGU".into(), 14)]);
        let soft = references("Part of ARUGU-1, see: ARUGU-2, ARUGU-3", "", "refs ARUGU-1", None);
        assert_eq!((soft.plane.len(), soft.closing_plane().collect::<Vec<_>>()), (3, vec![&("ARUGU".into(), 3)]));
        let both = references("Part of ARUGU-1", "", "Fixes ARUGU-1. ARUGU-2, refs ARUGU-2", None);
        assert_eq!((both.closing_plane().count(), both.soft_plane.len()), (2, 0));
    }

    /// A native issue is named from any PR; a GitHub issue only from a PR of
    /// its repo, and not by `Arugula #N`; a Plane item by its key.
    #[test]
    fn a_reference_names_the_issue_it_says() {
        let r = references("Closes #5 and ARUGU-6", "", "", None);
        let native = IssueKey { source: Source::Native, number: 5 };
        let gh = |repo| IssueKey { source: Source::Repo(repo), number: 5 };
        let plane = |n| IssueKey { source: Source::Plane("ARUGU".into()), number: n };
        assert!(r.name(Some(1), &native) && r.name(None, &native));
        assert!(r.name(Some(1), &gh(1)) && !r.name(Some(2), &gh(1)) && !r.name(None, &gh(1)));
        assert!(r.name(None, &plane(6)) && !r.name(None, &plane(5)));
        let a = references("Arugula #5", "", "", None);
        assert!(a.name(Some(1), &native) && !a.name(Some(1), &gh(1)), "Arugula #5 is never a GitHub issue");
    }

    fn key(source: Source, number: u64) -> IssueKey {
        IssueKey { source, number }
    }

    /// A board with a native #5 (row `n`), an unrelated GitHub #5 of repo 1
    /// (row `g`) and a native issue exported as #9 of repo 1 (row `x`, whose
    /// work item holds both keys).
    fn named(text: &str, repo: u64, only: &[&str]) -> Vec<String> {
        let (n, g, x) =
            ([key(Source::Native, 5)], [key(Source::Repo(1), 5)], [key(Source::Native, 7), key(Source::Repo(1), 9)]);
        let all = [
            Candidate { row: "n", work_item: Some("wi_n"), keys: &n },
            Candidate { row: "g", work_item: Some("wi_g"), keys: &g },
            Candidate { row: "x", work_item: Some("wi_x"), keys: &x },
        ];
        let rows: Vec<Candidate<'_>> = all.into_iter().filter(|c| only.contains(&c.row)).collect();
        rows_named(&references(text, "", "", None), Some(repo), &rows).into_iter().map(str::to_owned).collect()
    }

    /// The rule for `#N`, once: the PR's own repo's GitHub issue wins, the
    /// native issue is for `Arugula #N` and for a repo with no row for it.
    #[test]
    fn a_number_names_the_github_issue_of_the_prs_repo_before_the_native_one() {
        let both = ["n", "g", "x"];
        assert_eq!(named("Closes #5", 1, &both), ["g"], "the PR's own repo's #5");
        assert_eq!(named("Arugula #5", 1, &both), ["n"], "Arugula #5 is the native one");
        assert_eq!(named("Closes #5", 2, &both), ["n"], "another repo's PR: the native #5");
        assert_eq!(named("Closes #5", 1, &["n"]), ["n"], "no GitHub #5 on the board: the native one");
        assert_eq!(named("Arugula #5", 1, &["g"]), Vec::<String>::new(), "never the GitHub one");
        assert_eq!(named("Closes #5 and Arugula #5", 1, &both), ["n", "g"], "two issues named: a row each");
        assert_eq!(named("Arugula #5 (#5)", 2, &both), ["n"], "one issue named twice: one row");
        // An exported issue is one row, found by either of its numbers.
        assert_eq!(named("Closes #9", 1, &both), ["x"]);
        assert_eq!(named("Arugula #7", 2, &both), ["x"]);
        assert_eq!(named("Closes #9", 2, &both), Vec::<String>::new(), "#9 of repo 1 is not repo 2's");
        // Rows that are one work item are one row.
        let k = [key(Source::Native, 3), key(Source::Repo(1), 3)];
        let twins = [
            Candidate { row: "a", work_item: Some("wi"), keys: &k },
            Candidate { row: "b", work_item: Some("wi"), keys: &k },
        ];
        assert_eq!(rows_named(&references("Closes #3 Arugula #3", "", "", None), Some(1), &twins), ["a"]);
        // A Plane item by its key.
        let p = [key(Source::Plane("ARUGU".into()), 12)];
        let plane = [Candidate { row: "p", work_item: None, keys: &p }];
        assert_eq!(rows_named(&references("ARUGU-12: fix", "", "", None), Some(1), &plane), ["p"]);
        assert!(rows_named(&references("ARUGU-13: fix", "", "", None), Some(1), &plane).is_empty());
    }
}
