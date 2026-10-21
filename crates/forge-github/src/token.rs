//! Where [`Github`](crate::Github) gets its tokens.

use futures_util::future::BoxFuture;

/// What a token is for. GETs read; everything else writes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Purpose {
    Read,
    Write,
}

impl std::fmt::Display for Purpose {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Purpose::Read => "read",
            Purpose::Write => "write",
        })
    }
}

/// A token, and whose it is.
#[derive(Clone)]
pub struct Token {
    /// Sent as `Authorization: Bearer …`. Never logged.
    pub secret: String,
    /// Set when the token isn't the person's own (an installation's): the
    /// person's GitHub login. Then "you" is that login with no teams, and
    /// `GET /user` isn't asked.
    pub login: Option<String>,
}

impl std::fmt::Debug for Token {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Token").field("secret", &"…").field("login", &self.login).finish()
    }
}

/// Hands [`Github`](crate::Github) a token for each request.
///
/// The daemon's is the person's `gh` login on the host. The platform's can
/// be an installation token scoped to one repository (read, or write), or
/// a one-use user token that it drops after the call.
pub trait TokenSource: Send + Sync {
    /// A token for `repo` (`owner/name`; empty for what's about the person,
    /// as `GET /user`) to `purpose` it. `fresh`: GitHub refused the last one
    /// (a 401), so don't hand it out again.
    ///
    /// `Ok(None)`: this source has no token for that (an installation not
    /// on the repository, or a write asked of a source that only reads).
    /// `Err`: why it couldn't get one, to show the person.
    fn token<'a>(
        &'a self,
        repo: &'a str,
        purpose: Purpose,
        fresh: bool,
    ) -> BoxFuture<'a, Result<Option<Token>, String>>;
}

impl<T: TokenSource + ?Sized> TokenSource for std::sync::Arc<T> {
    fn token<'a>(
        &'a self,
        repo: &'a str,
        purpose: Purpose,
        fresh: bool,
    ) -> BoxFuture<'a, Result<Option<Token>, String>> {
        (**self).token(repo, purpose, fresh)
    }
}

impl<T: TokenSource + ?Sized> TokenSource for Box<T> {
    fn token<'a>(
        &'a self,
        repo: &'a str,
        purpose: Purpose,
        fresh: bool,
    ) -> BoxFuture<'a, Result<Option<Token>, String>> {
        (**self).token(repo, purpose, fresh)
    }
}
