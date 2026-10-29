# arugula-platform-all

Every platform feature crate (`crates/platform-*`, #314), registered on the
platform's base: `arugula_platform_all::register()` is what the
`arugula-control` binary (whose platform runs in its process), the
`arugula-platform` binary and the hub's tests call before the platform is
made. A new feature crate is a dependency and one line here.
