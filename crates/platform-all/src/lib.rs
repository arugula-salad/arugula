//! Every platform feature crate (`arugula-platform-<area>`, #314), put on
//! the platform's base: `arugula-control` (whose platform runs in its
//! process), `arugula-platform` and the hub's tests call [`register`]
//! before the platform is made. A new feature crate is one more line here.

/// Puts every feature crate in the platform. Once.
pub fn register() {
    static ONCE: std::sync::Once = std::sync::Once::new();
    ONCE.call_once(|| {
        // Agents with no machine, redone's import and the Infisical
        // projection, studio as a client (crates/platform-agents).
        arugula_platform_agents::register();
        // Environments, deploys and their approval, and applying them
        // (crates/platform-deploys).
        arugula_platform_deploys::register();
    });
}
