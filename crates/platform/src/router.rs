//! The platform's routes. Each is listed in `crates/control/routes.toml`
//! with `process = "platform"`, and its test reads the paths from
//! [`paths`]. The paths stay what they are on control's site: in process
//! the hub mounts this router behind its `hub_port::gate`, and out of
//! process (M140) it forwards each platform path here.

use std::sync::Arc;

use axum::{Router, routing::MethodRouter};

use crate::Platform;

/// Every route the platform answers. None yet: Cloud's, chat's and
/// billing's move here from `routes()` in `crates/control/src/main.rs`
/// (M137–M139), with the same paths and methods.
fn routes() -> Routes {
    Routes::default()
}

/// The platform's router, for any state the hub's router has (the
/// platform's own is applied here).
pub fn router<S: Clone + Send + Sync + 'static>(platform: Arc<Platform>) -> Router<S> {
    routes().router.with_state(platform)
}

/// The paths [`router`] registers, for routes.toml's test.
pub fn paths() -> Vec<&'static str> {
    routes().paths
}

/// A router that notes its paths too.
#[derive(Default)]
struct Routes {
    router: Router<Arc<Platform>>,
    paths: Vec<&'static str>,
}

impl Routes {
    #[expect(dead_code, reason = "M137-M139 add the first routes")]
    fn route(mut self, path: &'static str, methods: MethodRouter<Arc<Platform>>) -> Self {
        self.paths.push(path);
        self.router = self.router.route(path, methods);
        self
    }
}
