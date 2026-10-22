//! The apps-platform driver's plan (M222, platform step 7, native Arugula
//! #18; aru-104): what both the platform and the deploy's box need to
//! agree on, so the plan an owner approves names the repository, the file
//! and the change, and the box recomputes it before it applies.
//!
//! home-cloud's app platform runs an app from a **pointer**: a Flux
//! `GitRepository` and `Kustomization` in a list repository, one file per
//! app (`apps/<name>.yaml`), which Flux applies. The app's own repository
//! holds its `app.yaml` and the `k8s/` its CI renders from it. A deploy
//! pins the pointer's `GitRepository` at the approved commit of the app's
//! repository, so what runs is that commit's `app.yaml` and `k8s/`, and
//! Flux applies it. The list is the one GitHub list,
//! [`LIST_REPO`]; apps are its organization's repositories, named as
//! their pointer is.

/// The driver's name, as environments name it.
pub const DRIVER: &str = "apps-platform";

/// The list repository the driver commits to: home-cloud's list for apps
/// on GitHub. The token the driver holds writes this repository only.
pub const LIST_REPO: &str = "arugula-salad/planter-apps";

/// The list's branch Flux applies.
pub const LIST_BRANCH: &str = "main";

/// Where an app's public address is: `https://<name>.<APPS_DOMAIN>`.
pub const APPS_DOMAIN: &str = "apps.arugula.io";

/// The organization whose repositories are apps here: the list's.
pub fn list_owner() -> &'static str {
    LIST_REPO.split('/').next().unwrap_or_default()
}

/// An app's name, as home-cloud takes it: 1 to 50 lowercase letters,
/// digits and dashes, starting and ending with a letter or a digit.
pub fn valid_app_name(name: &str) -> bool {
    (1..=50).contains(&name.len())
        && name.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
        && !name.starts_with('-')
        && !name.ends_with('-')
}

/// Names no app may have: the list itself, and what else Flux keeps
/// beside the apps' pointers in `planter-apps-system`.
pub const RESERVED_APP_NAMES: &[&str] = &["planter-apps", "jhgaylor-apps", "flux-system", "fleet", "arugula-fleet"];

/// The app a source's `where` names: `<list owner>/<name>`, the name an
/// app's, and not one [`RESERVED_APP_NAMES`] holds (the list's own
/// repository among them). `None` for any other repository.
pub fn app_name(source_where: &str) -> Option<&str> {
    let (owner, name) = source_where.split_once('/')?;
    (owner == list_owner() && valid_app_name(name) && !RESERVED_APP_NAMES.contains(&name)).then_some(name)
}

/// A commit this driver pins: a full sha-1, lowercase hex.
pub fn valid_commit(sha: &str) -> bool {
    sha.len() == 40 && sha.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
}

/// The pointer's path in the list.
pub fn pointer_path(name: &str) -> String {
    format!("apps/{name}.yaml")
}

/// Why this driver can't deploy a source, before anything is planned:
/// `None` when it can.
pub fn refuses(source_kind: &str, source_ref: &str, source_where: &str) -> Option<String> {
    if source_kind != "commit" {
        return Some("the apps-platform driver deploys a commit of the app's repository, not an artifact".into());
    }
    if !valid_commit(source_ref) {
        return Some("the apps-platform driver pins a full commit sha (40 lowercase hex)".into());
    }
    if app_name(source_where).is_none() {
        return Some(format!(
            "the apps-platform driver deploys {}/<app> repositories, named as an app is (lowercase letters, digits and dashes)",
            list_owner()
        ));
    }
    None
}

/// The lines the driver adds to its plan ([`super::plan_text`]): the
/// repository, the file, the change and how it's checked.
pub fn plan_lines(source_kind: &str, source_ref: &str, source_where: &str) -> String {
    if let Some(why) = refuses(source_kind, source_ref, source_where) {
        return format!("repo {LIST_REPO}\nrefused {why}\n");
    }
    let name = app_name(source_where).unwrap_or_default();
    format!(
        "repo {LIST_REPO} (branch {LIST_BRANCH})\n\
         path {path}\n\
         change pin the GitRepository {name} (https://github.com/{source_where}) at commit {source_ref}; \
         a new pointer if there's none; a revert commit when it rolls back the last deploy\n\
         verify https://{name}.{APPS_DOMAIN} answers 200 at its app.yaml's health path; when the answer \
         reports a revision (JSON `revision`, `commit`, `version` or `sha`, or an `X-Revision` header), it \
         must be this commit's or its pinned image's; an app that reports none is ready when it answers, \
         which an old version still serving also does\n",
        path = pointer_path(name),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_the_lists_own_apps_at_a_full_commit() {
        let sha = "a".repeat(40);
        assert_eq!(app_name("arugula-salad/beds-smoke"), Some("beds-smoke"));
        for bad in [
            "acme/app",
            "arugula-salad/planter-apps",
            "arugula-salad/flux-system",
            "arugula-salad/Caps",
            "arugula-salad/-x",
            "arugula-salad/",
            "arugula-salad/a/b",
            "x",
        ] {
            assert_eq!(app_name(bad), None, "{bad}");
            assert!(refuses("commit", &sha, bad).is_some(), "{bad}");
        }
        assert!(refuses("commit", &sha, "arugula-salad/app").is_none());
        assert!(refuses("artifact", &sha, "arugula-salad/app").is_some());
        assert!(refuses("commit", "abc123", "arugula-salad/app").is_some(), "a short sha");
        assert!(refuses("commit", &"A".repeat(40), "arugula-salad/app").is_some(), "uppercase");
        assert!(valid_app_name(&"a".repeat(50)) && !valid_app_name(&"a".repeat(51)));
    }

    #[test]
    fn the_plan_names_the_repo_the_file_and_the_change() {
        let sha = "b".repeat(40);
        let p = plan_lines("commit", &sha, "arugula-salad/beds-smoke");
        assert!(p.contains("repo arugula-salad/planter-apps"), "{p}");
        assert!(p.contains("path apps/beds-smoke.yaml\n"), "{p}");
        assert!(p.contains(&format!("at commit {sha}")), "{p}");
        assert!(p.contains("https://beds-smoke.apps.arugula.io"), "{p}");
        let refused = plan_lines("commit", &sha, "acme/app");
        assert!(refused.contains("refused ") && !refused.contains("path "), "{refused}");
    }
}
