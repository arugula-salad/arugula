// Release builds embed web/dist (rust-embed), and so do debug builds with
// debug-embed (the platform's build.rs does web/dist-platform). rust-embed
// tracks only the files that were there when the crate compiled: one built
// before `just web` kept serving no page through every later build. So
// rebuild when the folder changes, as the daemon's build.rs does. Other
// debug builds read it at run time, and needn't recompile for a new build.
fn main() {
    git_folders();
    let embeds =
        std::env::var("PROFILE").as_deref() == Ok("release") || std::env::var_os("CARGO_FEATURE_DEBUG_EMBED").is_some();
    // There before cargo looks at it: a missing path would rerun this, and
    // rebuild the crate, every time. And a debug build reading it at run
    // time finds nothing if it wasn't there when the crate compiled (the
    // daemon's build.rs says why).
    let _ = std::fs::create_dir_all("../../web/dist");
    if embeds {
        println!("cargo:rerun-if-changed=../../web/dist");
    } else {
        println!("cargo:rerun-if-changed=build.rs");
    }
}

/// The folders `/git/` serves as repositories (#1748), the only place the
/// allowlist is written: (name in the URL, folder from the repository root).
const GIT_FOLDERS: &[(&str, &str)] = &[("nvim", "editors/nvim")];

/// The date of the one commit a build without release tags serves:
/// 2026-01-01T00:00:00Z.
const GIT_EPOCH_FALLBACK: i64 = 1767225600;

/// A folder's tree at a release tag.
struct Rel {
    tag: String,
    tree: String,
    epoch: i64,
}

/// Runs git in the repository; its stdout when it succeeds.
fn git(root: &std::path::Path, args: &[&str]) -> Option<Vec<u8>> {
    let o = std::process::Command::new("git").args(args).current_dir(root).output().ok()?;
    o.status.success().then_some(o.stdout)
}

fn git_text(root: &std::path::Path, args: &[&str]) -> Option<String> {
    Some(String::from_utf8(git(root, args)?).ok()?.trim().to_owned())
}

/// `vX.Y.Z` as numbers; other tags (a pre-release, `v1`) aren't releases.
fn semver(tag: &str) -> Option<(u64, u64, u64)> {
    let mut n = tag.strip_prefix('v')?.split('.').map(|p| p.parse::<u64>().ok());
    let v = (n.next()??, n.next()??, n.next()??);
    n.next().is_none().then_some(v)
}

/// Every release tag up to `version`, oldest first, that has `dir`: its
/// tree there and its commit's date. A tag with no such folder (before it
/// existed) is left out; a tag whose commit isn't in this checkout is an
/// error (a shallow one).
fn release_trees(root: &std::path::Path, dir: &str, version: (u64, u64, u64)) -> Result<Vec<Rel>, String> {
    let tags = git_text(root, &["tag", "-l", "v*"]).ok_or("git can't list the tags")?;
    let mut tags: Vec<_> =
        tags.lines().filter_map(|t| Some((semver(t)?, t.to_owned()))).filter(|(v, _)| *v <= version).collect();
    tags.sort();
    let mut out = vec![];
    for (_, tag) in tags {
        let commit = git_text(root, &["rev-parse", "--verify", "-q", &format!("{tag}^{{commit}}")])
            .ok_or(format!("{tag}'s commit isn't in this checkout"))?;
        let Some(tree) = git_text(root, &["rev-parse", "--verify", "-q", &format!("{commit}:{dir}")]) else { continue };
        let epoch = git_text(root, &["show", "-s", "--format=%ct", &commit])
            .and_then(|t| t.parse().ok())
            .ok_or(format!("{tag}'s date can't be read"))?;
        out.push(Rel { tag, tree, epoch });
    }
    Ok(out)
}

/// Writes `$OUT_DIR/git_folders.rs` for git_serve.rs: for each allowlisted
/// folder, its chain of releases (a `Release` for each tree that differs from
/// the release before it, with the tags that have it), and `CHECKOUT_*`, the
/// folder's files as they are now, which only keeps them among the crate's
/// inputs (rustc's dep-info lists them, so `scripts/ci-hub-changed` sees an
/// edit to the folder).
///
/// A release build (a `v*` tag's push, `GITHUB_REF_TYPE` and
/// `GITHUB_REF_NAME` as release.yml has them) fails unless the tags are
/// all here, so two instances never serve different ids: git, a checkout
/// that isn't shallow (release.yml's `linux` and `linux-arm64` jobs have
/// `fetch-depth: 0`), and the workspace version's own tag. Any other build with no tags in
/// reach serves the folder as it is, as the one release `v<version>`, and
/// says so.
fn git_folders() {
    use std::{env, fmt::Write as _, fs, path::Path};
    for var in ["GITHUB_REF_TYPE", "GITHUB_REF_NAME"] {
        println!("cargo:rerun-if-env-changed={var}");
    }
    let release_build = env::var("GITHUB_REF_TYPE").as_deref() == Ok("tag")
        && env::var("GITHUB_REF_NAME").is_ok_and(|n| semver(&n).is_some());
    let manifest = env::var("CARGO_MANIFEST_DIR").unwrap();
    let root = Path::new(&manifest).join("../..").canonicalize().unwrap();
    let out_dir = Path::new(&env::var("OUT_DIR").unwrap()).to_path_buf();
    let version_text = env::var("CARGO_PKG_VERSION").unwrap();
    let version = semver(&format!("v{version_text}")).expect("the workspace version is X.Y.Z");
    // New tags and fetched ones change these.
    if let Some(common) = git_text(&root, &["rev-parse", "--git-common-dir"]) {
        let common = root.join(common);
        for p in ["packed-refs", "refs/tags"] {
            println!("cargo:rerun-if-changed={}", common.join(p).display());
        }
    }
    let blobs = out_dir.join("git_blobs");
    fs::create_dir_all(&blobs).unwrap();

    // A file of the working tree, or of a release, as `include_bytes!`.
    let file_line = |path: &str, abs: &Path, exec: bool| {
        format!("            ({path:?}, include_bytes!({:?}), {exec}),\n", abs.display().to_string())
    };

    let mut out = String::new();
    let mut checkout = String::new();
    out += "/// Each allowlisted folder's releases, oldest first (made by build.rs).\n";
    out += "pub const FOLDERS: &[(&str, &[Release<'static>])] = &[\n";
    for (name, dir) in GIT_FOLDERS {
        let folder = root.join(dir).canonicalize().expect("an allowlisted folder");
        println!("cargo:rerun-if-changed={}", folder.display());

        // The folder as it is now, without dotfiles (.DS_Store), as the daemon embeds it.
        let mut now = vec![];
        fn walk(root: &Path, dir: &Path, out: &mut Vec<(String, std::path::PathBuf, bool)>) {
            for e in fs::read_dir(dir).unwrap().flatten() {
                let path = e.path();
                if e.file_name().to_string_lossy().starts_with('.') {
                    continue;
                }
                if path.is_dir() {
                    walk(root, &path, out);
                } else {
                    let rel = path.strip_prefix(root).unwrap();
                    let rel = rel.components().map(|c| c.as_os_str().to_string_lossy()).collect::<Vec<_>>().join("/");
                    #[cfg(unix)]
                    let exec = {
                        use std::os::unix::fs::PermissionsExt;
                        fs::metadata(&path).map(|m| m.permissions().mode() & 0o111 != 0).unwrap_or(false)
                    };
                    #[cfg(not(unix))]
                    let exec = false;
                    out.push((rel, path, exec));
                }
            }
        }
        walk(&folder, &folder, &mut now);
        now.sort();
        let _ = writeln!(
            checkout,
            "/// {name}'s files as built from.\n#[allow(dead_code, clippy::type_complexity)]\npub const CHECKOUT_{}: &[(&str, &[u8], bool)] = &[",
            name.to_uppercase()
        );
        for (rel, abs, exec) in &now {
            let _ = write!(checkout, "{}", file_line(rel, abs, *exec));
        }
        checkout += "];\n";

        // The releases.
        let mut rels = match release_trees(&root, dir, version) {
            Ok(r) => r,
            Err(e) => {
                if release_build {
                    panic!("a release build serves {dir} as git, and needs every release tag: {e}");
                }
                println!("cargo:warning=/git/{name}: {e}");
                vec![]
            }
        };
        if release_build {
            let shallow = git_text(&root, &["rev-parse", "--is-shallow-repository"]);
            if shallow.as_deref() != Some("false") {
                panic!(
                    "a release build serves {dir} as git from every release tag, and this checkout is shallow or not git's: check out with fetch-depth: 0 (release.yml's linux and linux-arm64 jobs)"
                );
            }
            if rels.last().map(|r| r.tag.as_str()) != Some(&format!("v{version_text}")) {
                panic!(
                    "a release build serves {dir} as git from every release tag, and v{version_text} (the workspace version) isn't one of them or has no {dir}"
                );
            }
        }
        let _ = writeln!(out, "    ({name:?}, &[");
        if rels.is_empty() {
            println!(
                "cargo:warning=/git/{name}: no release tags to build from (a shallow or tagless checkout): serving {dir} as it is, as the only release, v{version_text}"
            );
            let _ = writeln!(
                out,
                "        Release {{ tags: &[\"v{version_text}\"], epoch: {GIT_EPOCH_FALLBACK}, files: &["
            );
            for (rel, abs, exec) in &now {
                out += &file_line(rel, abs, *exec);
            }
            out += "        ] },\n";
        } else {
            // One commit for each release whose tree differs from the one before; the others share it.
            let mut groups: Vec<(Vec<String>, i64, String)> = vec![];
            for r in rels.drain(..) {
                match groups.last_mut() {
                    Some(g) if g.2 == r.tree => g.0.push(r.tag),
                    _ => groups.push((vec![r.tag], r.epoch, r.tree)),
                }
            }
            for (tags, epoch, tree) in groups {
                let tags = tags.iter().map(|t| format!("{t:?}")).collect::<Vec<_>>().join(", ");
                let _ = writeln!(out, "        Release {{ tags: &[{tags}], epoch: {epoch}, files: &[");
                let listing = git(&root, &["ls-tree", "-r", "-z", &tree]).expect("a tree git just named");
                for entry in listing.split(|b| *b == 0).filter(|e| !e.is_empty()) {
                    let entry = String::from_utf8_lossy(entry).into_owned();
                    let (meta, path) = entry.split_once('\t').unwrap();
                    let mut m = meta.split(' ');
                    let (mode, kind, id) = (m.next().unwrap(), m.next().unwrap(), m.next().unwrap());
                    // Files only: a symlink or a submodule isn't served.
                    if kind != "blob"
                        || !(mode == "100644" || mode == "100755")
                        || path.split('/').any(|c| c.starts_with('.'))
                    {
                        continue;
                    }
                    let blob = blobs.join(id);
                    if !blob.exists() {
                        fs::write(&blob, git(&root, &["cat-file", "blob", id]).expect("a blob of that tree")).unwrap();
                    }
                    out += &file_line(path, &blob, mode == "100755");
                }
                out += "        ] },\n";
            }
        }
        out += "    ]),\n";
    }
    out += "];\n";
    out += &checkout;
    fs::write(out_dir.join("git_folders.rs"), out).unwrap();
}
