// Release builds embed web/dist (rust-embed), and so do debug builds with
// debug-embed. Rebuild those when it changes, or a fresh `just web` would
// ship the previous bundle. Other debug builds read it at run time, so a
// new bundle needn't recompile the daemon (every `just test` built one).
fn main() {
    let embeds =
        std::env::var("PROFILE").as_deref() == Ok("release") || std::env::var_os("CARGO_FEATURE_DEBUG_EMBED").is_some();
    if embeds {
        println!("cargo:rerun-if-changed=../../web/dist");
    } else {
        println!("cargo:rerun-if-changed=build.rs");
    }
    // Debug builds read web/dist at run time, from the folder's canonical
    // path, which rust-embed works out when the daemon compiles. If the
    // folder doesn't exist then, it keeps the path with `..` in it, finds
    // no file under it later, and the daemon answers every page with "web
    // client not built" even after `just web`. So make sure it's there.
    let _ = std::fs::create_dir_all("../../web/dist");
    // A release build without the client would serve no page at all, so
    // stop and say why.
    if std::env::var("PROFILE").as_deref() == Ok("release")
        && !std::path::Path::new("../../web/dist/index.html").exists()
    {
        panic!("web/dist is missing: build the web client first (`just web`, or build with `just build`)");
    }
}
