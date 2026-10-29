//! The internal port: what the hub and the platform say to each other over
//! the private network (`HubPort` and `PlatformPort` in `arugula-platform`,
//! M140), and who the hub vouches a forwarded request is from.
//!
//! Each method is one `POST /internal/<hub|platform>/v1/<method>` (under
//! [`HUB_PREFIX`] or [`PLATFORM_PREFIX`]), its arguments one of the
//! `*Args` objects in [`args`] (or a wire type of its own), its answer the
//! method's JSON. The traits, the HTTP client and the router stay in
//! `arugula-platform` (`hub`, `port`, `remote`); only the messages live
//! here, so they get a schema and examples like the rest of the crate.
//!
//! Both of the crate's rules hold here too: the bytes on the wire don't
//! change, and nothing is `deny_unknown_fields`. The hub and the platform
//! deploy separately, so a field one side added is read with
//! `#[serde(default)]` by the other.

pub mod args;
pub mod hub;
pub mod platform;
pub mod principal;

pub use args::*;
pub use hub::*;
pub use platform::*;
pub use principal::*;

/// Where the hub's internal API is mounted.
pub const HUB_PREFIX: &str = "/internal/hub/v1";
/// Where the platform's is.
pub const PLATFORM_PREFIX: &str = "/internal/platform/v1";
