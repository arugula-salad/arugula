//! `arugula-control`: Arugula control's hub. The hub is
//! `arugula-control-base` (crates/control), and the platform it runs in
//! process is `arugula-platform-base` (crates/platform) with its feature
//! crates (#314): this binary puts them together and starts the hub.

fn main() -> anyhow::Result<()> {
    arugula_platform_all::register();
    arugula_control::main()
}
