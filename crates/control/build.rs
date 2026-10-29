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

/// Used when the release's tag date can't be read (no tag in this checkout,
/// no git): 2026-01-01T00:00:00Z.
const GIT_EPOCH_FALLBACK: &str = "1767225600";

/// Writes `$OUT_DIR/git_folders.rs`: each allowlisted folder's files as
/// `include_bytes!`, found by walking it (as arugulad-editor's build.rs
/// does), and the timestamp of the commit built from them: the date of
/// the workspace version's tag if git can say, else a constant.
/// `ARUGULA_GIT_EPOCH` overrides it.
fn git_folders() {
    use std::{env, fmt::Write as _, fs, path::Path};
    fn walk(root: &Path, dir: &Path, out: &mut Vec<(String, String, bool)>) {
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
                out.push((rel, path.display().to_string(), exec));
            }
        }
    }
    println!("cargo:rerun-if-env-changed=ARUGULA_GIT_EPOCH");
    let manifest = env::var("CARGO_MANIFEST_DIR").unwrap();
    let mut out = String::from("/// (name, files as (path, bytes, executable)) for each allowlisted folder.\n");
    out += "#[allow(clippy::type_complexity)]\npub const FOLDERS: &[(&str, &[(&str, &[u8], bool)])] = &[\n";
    for (name, dir) in GIT_FOLDERS {
        let root = Path::new(&manifest).join("../..").join(dir).canonicalize().expect("an allowlisted folder");
        println!("cargo:rerun-if-changed={}", root.display());
        let mut files = vec![];
        walk(&root, &root, &mut files);
        files.sort();
        let _ = writeln!(out, "    ({name:?}, &[");
        for (rel, abs, exec) in files {
            let _ = writeln!(out, "        ({rel:?}, include_bytes!({abs:?}), {exec}),");
        }
        out += "    ]),\n";
    }
    out += "];\n";
    let epoch = env::var("ARUGULA_GIT_EPOCH")
        .ok()
        .or_else(|| {
            let tag = format!("v{}", env::var("CARGO_PKG_VERSION").ok()?);
            let o = std::process::Command::new("git")
                .args(["log", "-1", "--format=%ct", &format!("refs/tags/{tag}")])
                .current_dir(&manifest)
                .output()
                .ok()?;
            let t = String::from_utf8(o.stdout).ok()?.trim().to_owned();
            (o.status.success() && t.parse::<i64>().is_ok()).then_some(t)
        })
        .unwrap_or_else(|| GIT_EPOCH_FALLBACK.into());
    let _ = writeln!(
        out,
        "/// The commit's author and committer time, seconds since 1970.\npub const EPOCH: i64 = {epoch};"
    );
    fs::write(Path::new(&env::var("OUT_DIR").unwrap()).join("git_folders.rs"), out).unwrap();
}
