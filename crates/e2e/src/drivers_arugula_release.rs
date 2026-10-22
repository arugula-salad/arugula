//! The arugula-release driver's plan (M221, platform step 7, native
//! Arugula #17): Arugula's own release as a deploy, approved by an owner's
//! device, instead of `just release` from Jake's shell.
//!
//! A deploy's source is a commit of [`REPO`]: one on its `main` that passed
//! CI (its `ci/green/<sha>` tag, and no `ci/revoked/<sha>`). The box runs
//! `scripts/release bump` as that commit has it, which is what `just
//! release` runs: it bumps the version on top of the commit, tags it, and
//! pushes the tag and `main` (the bump merged into it) in one atomic push.
//! The tag's release workflow then builds, signs and publishes: its
//! secrets (the updater key, Apple's, the marketplaces') stay in GitHub
//! Actions. The box follows that workflow to its end, for at most
//! [`FOLLOW_MINUTES`].
//!
//! Its credentials are installation tokens of the platform's GitHub App,
//! minted for the run: `contents: write` on [`REPO`] alone ([`TOKEN`]),
//! revoked right after the push, and one that only reads its Actions runs
//! and contents ([`READ_TOKEN`]), to follow the workflow. The environment
//! holds no secret of its own.
//!
//! **No roll back.** A release is never taken back: a bad one is fixed by a
//! new patch release. The plan says so.

use super::Settings;

/// The driver's name, as environments name it.
pub const DRIVER: &str = "arugula-release";

/// The repository it releases, and the only one its token writes.
pub const REPO: &str = "arugula-salad/arugula";

/// The branch a release starts from and pushes to.
pub const BRANCH: &str = "main";

/// The name the run gets its minted token under.
pub const TOKEN: &str = "RELEASE_TOKEN";

/// The name the run gets its second, read-only token under: the
/// repository's Actions runs and contents, to follow the release's
/// workflow (GitHub's API without one allows 60 asks an hour an address).
pub const READ_TOKEN: &str = "RELEASE_READ_TOKEN";

/// Its settings: `bump` (`patch`, `minor` or `major`; default `patch`) and
/// `lane` (`daemon` for a `v*` tag, `app` for an `app-v*` one; default
/// `daemon`), as `just release [patch|minor|major] [--app]` takes them.
pub const BUMP: &str = "bump";
pub const LANE: &str = "lane";
const SETTINGS: &[&str] = &[BUMP, LANE];

/// The longest the box waits for the tag's release workflow to end. A
/// release takes 12 to 20 minutes; its read token lasts an hour from when
/// the run starts.
pub const FOLLOW_MINUTES: u64 = 45;

/// The release workflow's job that publishes the release: the release is
/// out once it succeeded, whatever later jobs (deploying control, the
/// hosted template, the editor extension) did.
pub const PUBLISH_JOB: &str = "publish";

/// A release's lane: the daemon's (`v*` tags) or the desktop app's
/// (`app-v*`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Lane {
    Daemon,
    App,
}

impl Lane {
    /// The lane `settings` name (the daemon's unless they say `app`).
    pub fn of(settings: &Settings) -> Lane {
        if settings.get(LANE).map(String::as_str) == Some("app") { Lane::App } else { Lane::Daemon }
    }

    /// What its tags start with.
    pub fn prefix(self) -> &'static str {
        match self {
            Lane::Daemon => "v",
            Lane::App => "app-v",
        }
    }

    /// The workflow a tag of this lane runs.
    pub fn workflow(self) -> &'static str {
        match self {
            Lane::Daemon => ".github/workflows/release.yml",
            Lane::App => ".github/workflows/app-release.yml",
        }
    }

    /// Where its release notes are for version `v`, on `main`.
    pub fn notes(self, v: &str) -> String {
        match self {
            Lane::Daemon => format!("docs/releases/{v}.md"),
            Lane::App => format!("docs/releases/app-{v}.md"),
        }
    }
}

/// Whether `v` is a version: `X.Y.Z`, digits only.
pub fn version_ok(v: &str) -> bool {
    let parts: Vec<&str> = v.split('.').collect();
    parts.len() == 3 && parts.iter().all(|p| (1..=6).contains(&p.len()) && p.bytes().all(|b| b.is_ascii_digit()))
}

/// The version a tag of `lane` names, if it's one.
pub fn tag_version(lane: Lane, tag: &str) -> Option<&str> {
    tag.strip_prefix(lane.prefix()).filter(|v| version_ok(v))
}

/// The release's page on GitHub.
pub fn release_url(tag: &str) -> String {
    format!("https://github.com/{REPO}/releases/tag/{tag}")
}

/// Whether `url` is one of [`REPO`]'s workflow runs on GitHub.
pub fn run_url_ok(url: &str) -> bool {
    url.strip_prefix(&format!("https://github.com/{REPO}/actions/runs/"))
        .is_some_and(|id| (1..=20).contains(&id.len()) && id.bytes().all(|b| b.is_ascii_digit()))
}

/// Its settings as an environment keeps them, defaults filled in: `Err` why
/// not.
pub fn settings(given: &Settings) -> Result<Settings, String> {
    if let Some(k) = given.keys().find(|k| !SETTINGS.contains(&k.as_str())) {
        return Err(format!("{DRIVER} has no setting {k}: it takes {}", SETTINGS.join(", ")));
    }
    let mut s = given.clone();
    s.entry(BUMP.into()).or_insert_with(|| "patch".into());
    s.entry(LANE.into()).or_insert_with(|| "daemon".into());
    if !["patch", "minor", "major"].contains(&s[BUMP].as_str()) {
        return Err("bump is patch, minor or major".into());
    }
    if !["daemon", "app"].contains(&s[LANE].as_str()) {
        return Err("lane is daemon (a v* tag) or app (an app-v* tag)".into());
    }
    Ok(s)
}

/// Whether `reference` is a full commit id: 40 lowercase hex.
fn full_commit(reference: &str) -> bool {
    reference.len() == 40 && reference.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
}

/// Why this driver can't release a source, before anything is planned:
/// `None` when it can.
pub fn refuses(source_kind: &str, source_ref: &str, source_where: &str) -> Option<String> {
    if source_kind != "commit" {
        return Some(format!("the {DRIVER} driver releases a commit, not an artifact"));
    }
    if !full_commit(source_ref) {
        return Some(format!("the {DRIVER} driver releases a commit by its full id (40 lowercase hex)"));
    }
    if source_where != REPO {
        return Some(format!("the {DRIVER} driver releases {REPO} only"));
    }
    None
}

/// What it does, in words: the part of its plan a person reads before they
/// approve (after the settings lines `plan_text` writes).
pub fn plan_lines(source_kind: &str, source_ref: &str, source_where: &str, settings: &Settings) -> String {
    if let Some(why) = refuses(source_kind, source_ref, source_where) {
        return format!("refused {why}\n");
    }
    let lane = Lane::of(settings);
    let bump = settings.get(BUMP).map_or("patch", String::as_str);
    let (what, flag) = match lane {
        Lane::Daemon => ("the daemon's version (every arugula crate but the app)", ""),
        Lane::App => ("the desktop app's version (crates/desktop)", " --app"),
    };
    let prefix = lane.prefix();
    let notes = lane.notes("X.Y.Z");
    let workflow = lane.workflow();
    let mut t = format!(
        "does: check commit {source_ref} is on {REPO}'s {BRANCH} and passed CI (its ci/green tag, and no ci/revoked \
         one), on a box made for this deploy, or stop\n"
    );
    t.push_str(&format!(
        "does: run scripts/release bump {bump}{flag} {source_ref}, as that commit has it (what just release runs): \
         a {bump} bump of {what}, from the higher of the commit's version and {BRANCH}'s; the run's log and the \
         deploy's name the script's blob id\n"
    ));
    t.push_str(&format!(
        "does: stop if {BRANCH} has no {notes} for the new version, the tag is there already, or the commit is in a \
         release already\n"
    ));
    t.push_str(&format!(
        "does: commit the bump on top of the commit, tag it {prefix}X.Y.Z, and push the tag and {BRANCH} (the bump \
         merged into it) in one atomic push over https\n"
    ));
    t.push_str(&format!(
        "does: follow the tag's release workflow ({workflow}) for at most {FOLLOW_MINUTES} minutes; the release is \
         out once its {PUBLISH_JOB} job succeeded. Its own secrets (the updater key, Apple's, the marketplaces') stay \
         in GitHub Actions\n"
    ));
    t.push_str(&format!(
        "with: an installation token of the platform's GitHub App that writes {REPO}'s contents and nothing else, \
         minted for this run, and revoked right after the push; and one that only reads its Actions runs and \
         contents, to follow the workflow, revoked when that ends\n"
    ));
    t.push_str(&format!(
        "check: before either token is minted, the platform checks with GitHub that commit {source_ref} is on \
         {BRANCH} and passed CI\n"
    ));
    t.push_str(
        "no roll back: a release is never taken back; a bad release is fixed by a new patch release, deployed the \
         same way\n",
    );
    t
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(kv: &[(&str, &str)]) -> Settings {
        kv.iter().map(|(k, v)| ((*k).into(), (*v).into())).collect()
    }

    #[test]
    fn settings_are_checked_and_filled_in() {
        assert_eq!(settings(&s(&[])).unwrap(), s(&[("bump", "patch"), ("lane", "daemon")]));
        assert_eq!(settings(&s(&[("bump", "minor"), ("lane", "app")])).unwrap()["lane"], "app");
        for bad in [s(&[("bump", "huge")]), s(&[("lane", "web")]), s(&[("token", "x")]), s(&[("bump", "-x")])] {
            assert!(settings(&bad).is_err(), "{bad:?}");
        }
    }

    #[test]
    fn only_a_full_commit_of_arugula() {
        let sha = "a".repeat(40);
        assert!(refuses("commit", &sha, REPO).is_none());
        assert!(refuses("artifact", &sha, REPO).is_some());
        assert!(refuses("commit", &sha[..12], REPO).is_some());
        assert!(refuses("commit", &sha.to_uppercase(), REPO).is_some());
        assert!(refuses("commit", &sha, "arugula-salad/arugula-fork").is_some());
        assert!(refuses("commit", &sha, "acme/arugula").is_some());
    }

    #[test]
    fn tags_versions_and_links_are_checked_by_shape() {
        assert_eq!(tag_version(Lane::Daemon, "v0.59.1"), Some("0.59.1"));
        assert_eq!(tag_version(Lane::App, "app-v0.31.0"), Some("0.31.0"));
        for bad in ["app-v0.1.0", "v0.1", "v0.1.0-rc1", "v0..1", "0.1.0", "v1.2.3\n"] {
            assert_eq!(tag_version(Lane::Daemon, bad), None, "{bad}");
        }
        assert_eq!(tag_version(Lane::App, "v0.1.0"), None);
        assert!(run_url_ok("https://github.com/arugula-salad/arugula/actions/runs/123456"));
        for bad in [
            "https://github.com/arugula-salad/other/actions/runs/1",
            "https://github.com/arugula-salad/arugula/actions/runs/1/x",
            "javascript:alert(1)",
            "https://github.com/arugula-salad/arugula/actions/runs/",
        ] {
            assert!(!run_url_ok(bad), "{bad}");
        }
        assert_eq!(release_url("v1.2.3"), "https://github.com/arugula-salad/arugula/releases/tag/v1.2.3");
    }

    #[test]
    fn the_plan_says_what_it_runs_and_that_it_never_rolls_back() {
        let sha = "b".repeat(40);
        let app = plan_lines("commit", &sha, REPO, &settings(&s(&[("bump", "minor"), ("lane", "app")])).unwrap());
        for says in [
            format!("run scripts/release bump minor --app {sha}"),
            "docs/releases/app-X.Y.Z.md".into(),
            "tag it app-vX.Y.Z".into(),
            ".github/workflows/app-release.yml".into(),
            "no roll back".into(),
            "writes arugula-salad/arugula's contents and nothing else".into(),
        ] {
            assert!(app.contains(&says), "{says:?} in {app}");
        }
        let daemon = plan_lines("commit", &sha, REPO, &settings(&s(&[])).unwrap());
        assert!(daemon.contains(&format!("bump patch {sha}, as")) && daemon.contains("tag it vX.Y.Z"), "{daemon}");
        assert!(plan_lines("commit", &sha, "acme/app", &Settings::new()).starts_with("refused "));
    }

    /// The plan binds the settings: another bump or lane is another plan,
    /// and a roll back plans nothing it would do.
    #[test]
    fn the_plan_binds_the_bump_and_the_lane() {
        let sha = "c".repeat(40);
        let plan = |set: &Settings| super::super::plan_text(DRIVER, "release", "commit", &sha, REPO, set, None);
        let patch = settings(&s(&[])).unwrap();
        let minor = settings(&s(&[("bump", "minor")])).unwrap();
        let app = settings(&s(&[("lane", "app")])).unwrap();
        assert_ne!(plan(&patch), plan(&minor));
        assert_ne!(plan(&patch), plan(&app));
        assert!(plan(&patch).contains("setting bump patch\nsetting lane daemon\n"), "{}", plan(&patch));
        let r = super::super::RollBack { deploy: format!("dep_{}", "d".repeat(32)), to: Settings::new() };
        let back = super::super::plan_text(DRIVER, "release", "commit", &sha, REPO, &patch, Some(&r));
        assert!(!back.contains("does:"), "{back}");
        assert!(super::super::source_ok(DRIVER, "commit", &sha) && !super::super::source_ok(DRIVER, "commit", "main"));
    }
}
