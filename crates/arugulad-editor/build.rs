//! Embeds arugula.nvim (`editors/nvim`) in the daemon (#1733), so the copy
//! it installs into an nvim is the one from the same commit as the daemon.
//!
//! It writes `$OUT_DIR/nvim_plugin.rs`: one `include_bytes!` per file under
//! `editors/nvim`, found by walking it. A hand-kept list (as the VS Code
//! extension's `EXT_FILES`) would need editing with each file the plugin
//! gains, and forgetting one would ship a plugin that fails to load;
//! `include_dir` would do the same walk as a dependency and a proc-macro.
//! Cargo reruns this when anything under the directory changes.

use std::{
    env, fs,
    path::{Path, PathBuf},
};

fn main() {
    let root = Path::new(&env::var("CARGO_MANIFEST_DIR").unwrap()).join("../../editors/nvim");
    let root = root.canonicalize().expect("editors/nvim, arugula.nvim's source");
    println!("cargo:rerun-if-changed={}", root.display());
    let mut files = vec![];
    walk(&root, &root, &mut files);
    files.sort();
    let mut out = String::from(
        "/// arugula.nvim's files, each path relative to `editors/nvim` (made by build.rs).\n\
         pub const FILES: &[(&str, &[u8])] = &[\n",
    );
    for (rel, abs) in files {
        out += &format!("    ({rel:?}, include_bytes!({abs:?})),\n");
    }
    out += "];\n";
    fs::write(PathBuf::from(env::var("OUT_DIR").unwrap()).join("nvim_plugin.rs"), out).unwrap();
}

/// Every file under `dir`, but dotfiles (`.DS_Store`): its path from
/// `root` with `/`, and its absolute path.
fn walk(root: &Path, dir: &Path, out: &mut Vec<(String, String)>) {
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
            out.push((rel, path.display().to_string()));
        }
    }
}
