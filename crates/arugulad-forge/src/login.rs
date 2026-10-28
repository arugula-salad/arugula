//! Reading a host and a repository out of a URL or a git remote, which the
//! forge blocks use to pick a login and to match a clone to a link. (The
//! `tea` logins Forgejo reads with are in `labs/tea.rs`.)

/// The host (with a port, if not the scheme's) of a URL.
pub use arugula_forge_github::util::url_host;

pub use crate::git_remote::parse_remote;
