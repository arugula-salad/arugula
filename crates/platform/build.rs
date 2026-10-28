// Release builds embed web/dist-platform (rust-embed), and so do debug
// builds with debug-embed. rust-embed tracks only the files that were there
// when the crate compiled: one built before `just web` kept an empty page
// through every later build. So rebuild when the folder changes, as the
// daemon's build.rs does for web/dist. Other debug builds read it at run
// time, and needn't recompile for a new build of the page.
fn main() {
    let embeds =
        std::env::var("PROFILE").as_deref() == Ok("release") || std::env::var_os("CARGO_FEATURE_DEBUG_EMBED").is_some();
    // There before cargo looks at it: a missing path would rerun this, and
    // rebuild the crate, every time. And a debug build reading it at run
    // time finds nothing if it wasn't there when the crate compiled (the
    // daemon's build.rs says why).
    let _ = std::fs::create_dir_all("../../web/dist-platform");
    if embeds {
        println!("cargo:rerun-if-changed=../../web/dist-platform");
    } else {
        println!("cargo:rerun-if-changed=build.rs");
    }
}
