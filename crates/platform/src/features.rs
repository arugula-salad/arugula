//! Feature crates (`arugula-platform-<area>`, #314): what each adds to the
//! platform, registered before it starts. Both binaries register every
//! one (`arugula-platform-all`), so the platform in the hub's process and
//! the `arugula-platform` binary are the same platform. The base calls
//! these hooks where it used to name the feature; a feature that isn't
//! registered adds nothing. A feature keeps what it holds in memory in
//! [`Slots`] on the [`Platform`].

use std::{
    any::{Any, TypeId},
    collections::HashMap,
    sync::{Arc, Mutex},
};

use axum::{Router, routing::MethodRouter};

use crate::Platform;

/// What a feature crate adds.
pub trait Feature: Send + Sync + 'static {
    /// Its routes, beside the base's ([`crate::router`], [`crate::paths`]).
    fn routes(&self, r: Routes) -> Routes {
        r
    }
    /// Its background work, once the platform starts (in the hub's process
    /// or as its own binary).
    fn start(&self, _p: &Arc<Platform>) {}
}

static ALL: Mutex<Vec<&'static dyn Feature>> = Mutex::new(Vec::new());

/// Puts a feature in this platform: before it's made, by the binary (or a
/// test).
pub fn register(f: Box<dyn Feature>) {
    ALL.lock().unwrap().push(Box::leak(f));
}

pub(crate) fn all() -> Vec<&'static dyn Feature> {
    ALL.lock().unwrap().clone()
}

/// Every feature's background work ([`Feature::start`]).
pub fn start(p: &Arc<Platform>) {
    for f in all() {
        f.start(p);
    }
}

/// A router that notes its paths too: routes.toml's test reads them.
#[derive(Default)]
pub struct Routes {
    pub(crate) router: Router<Arc<Platform>>,
    pub(crate) paths: Vec<&'static str>,
}

impl Routes {
    pub fn route(mut self, path: &'static str, methods: MethodRouter<Arc<Platform>>) -> Self {
        self.paths.push(path);
        self.router = self.router.route(path, methods);
        self
    }
}

/// What feature crates keep on the platform: one value of each type, put
/// while the platform is made and read from then on.
#[derive(Default)]
pub struct Slots(HashMap<TypeId, Box<dyn Any + Send + Sync>>);

impl Slots {
    pub fn put<T: Any + Send + Sync>(&mut self, v: T) {
        self.0.insert(TypeId::of::<T>(), Box::new(v));
    }

    pub fn get<T: Any + Send + Sync>(&self) -> Option<&T> {
        self.0.get(&TypeId::of::<T>()).and_then(|v| v.downcast_ref())
    }
}
