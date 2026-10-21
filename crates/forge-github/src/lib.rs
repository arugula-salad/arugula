//! The GitHub reader (M160): GitHub's REST API onto Arugula's forge model,
//! shared by the daemon's forge blocks and (later) the platform.
//!
//! - [`Github`]: a client for one API base (github.com's, or a GitHub
//!   Enterprise host's `https://HOST/api/v3`). It reads a pull request and
//!   its checks, reviews and timeline, an issue, and who "you" are, and it
//!   writes. Every GET is conditional ([`EtagCache`]), and it backs off
//!   when GitHub's rate limit runs low.
//! - [`TokenSource`]: where [`Github`] gets its tokens, asked per
//!   repository and [`Purpose`]. The daemon's is the person's `gh` login.
//!   The platform's will be an installation token or a one-use user token.
//! - [`model`]: the normalized model (pull requests, issues, checks,
//!   reviews, events) and the attention rules, the same for every forge.
//! - [`parse`]: GitHub's JSON onto the model, and GitHub's links.
//! - [`util`]: what every forge adapter shares.
//!
//! It knows nothing of blocks, `layout.json`, the daemon or the platform.
//! The caller brings the HTTP client (`reqwest::Client`): its TLS roots,
//! timeout and User-Agent are the caller's.

mod client;
pub mod model;
pub mod parse;
mod token;
pub mod util;

pub use client::{EtagCache, Github};
pub use token::{Purpose, Token, TokenSource};

use model::{Check, Item};

/// What went wrong talking to a forge.
#[derive(Debug, Clone)]
pub enum Error {
    /// No login, or no token for it.
    Login(String),
    /// 401 or 403.
    Denied(String),
    NotFound(String),
    Http(String),
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::Login(e) => write!(f, "{e}"),
            Error::Denied(e) => write!(f, "the forge refused the login: {e}"),
            Error::NotFound(e) => write!(f, "not found: {e}"),
            Error::Http(e) => write!(f, "{e}"),
        }
    }
}

/// A URL as it may be logged (no query: nothing secret is ever in one,
/// but a forge's paging is noise).
pub fn redact(url: &str) -> &str {
    url.split('?').next().unwrap_or(url)
}

/// The cheap part of a read: the item and its checks.
pub struct Polled {
    pub item: Item,
    pub checks: Vec<Check>,
    /// What changes when reviews or the timeline might have.
    pub fingerprint: String,
}

/// What a write did.
pub struct Sent {
    pub url: Option<String>,
    pub said: String,
}
