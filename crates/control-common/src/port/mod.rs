//! The internal port: what the hub and the platform say to each other over
//! the private network (`HubPort` and `PlatformPort` in
//! `arugula-platform-api`, M140), and who the hub vouches a forwarded
//! request is from.
//!
//! Each method is one `POST /internal/<hub|platform>/v1/<method>` (under
//! [`HUB_PREFIX`] or [`PLATFORM_PREFIX`]), its arguments one of the
//! `*Args` objects in [`args`] (or a wire type of its own), its answer the
//! method's JSON. The traits, the HTTP client and the router stay in
//! `arugula-platform-api` (`hub`, `port`, `remote`); only the messages live
//! here. Only the hub and the platform send them, so they live with what
//! those two share, not in `arugula-control-wire`, the wire daemons and
//! clients speak too.
//!
//! The wire's rules hold here as in `arugula-control-wire`
//! (`contract/README.md`): the bytes on the wire don't change, and nothing
//! is `deny_unknown_fields`. The hub and the platform deploy separately, so
//! a field one side added is read with `#[serde(default)]` by the other.
//! Their JSON Schema is `port/schema.json` beside this crate's manifest, and
//! `port/examples/<Type>/` holds payloads that decode and encode again
//! without a byte changing; this module's tests check both, as
//! control-wire's do for `contract/` (`ARUGULA_WRITE_SCHEMA=1` rewrites the
//! schema).

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

/// An open enum's JSON Schema: the known values, or any other string (read
/// as its `Unknown` variant). `arugula-control-wire`'s transform of the same
/// name, for this module's enums.
#[cfg(test)]
fn open_enum(schema: &mut schemars::Schema) {
    let Some(o) = schema.as_object_mut() else { return };
    let description = o.remove("description");
    let known = serde_json::Value::Object(std::mem::take(o));
    o.insert("anyOf".into(), serde_json::json!([known, { "type": "string" }]));
    if let Some(d) = description {
        o.insert("description".into(), d);
    }
}

#[cfg(test)]
mod contract_tests;
