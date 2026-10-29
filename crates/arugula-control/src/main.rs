//! `arugula-control`: Arugula control's hub. The hub is
//! `arugula-control-base` (crates/control), which builds against
//! `arugula-platform-api` alone; the platform it runs in process is
//! `arugula-platform-base` (crates/platform) with its feature crates (#314):
//! this binary puts them together and starts the hub.

fn main() -> anyhow::Result<()> {
    arugula_platform_all::register();
    arugula_control::main(Box::new(arugula_platform::in_process::HubPlatforms))
}

#[cfg(test)]
mod tests {
    /// The hub's flags and the platform's make one command line: no flag of
    /// one names another's (clap's checks).
    #[test]
    fn the_command_line_is_one() {
        arugula_control::command(&arugula_platform::in_process::HubPlatforms).debug_assert();
    }
}
