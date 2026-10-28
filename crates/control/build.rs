// Release builds embed web/dist (rust-embed), and so do debug builds with
// debug-embed (the platform's build.rs does web/dist-platform). rust-embed
// tracks only the files that were there when the crate compiled: one built
// before `just web` kept serving no page through every later build. So
// rebuild when the folder changes, as the daemon's build.rs does. Other
// debug builds read it at run time, and needn't recompile for a new build.
fn main() {
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
