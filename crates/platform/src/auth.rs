//! Who is asking, as the hub found out ("The hub stays the identity
//! provider"). The platform never sees a cookie or checks a signature: the
//! hub does both and puts what it found in the request's extensions as an
//! [`Asked`]. In process that's the hub's `hub_port::gate` layer in front
//! of [`router`](crate::router()); out of process (M140) it's the platform's
//! check of the hub's signed principal header ([`remote::vouched`],
//! `arugula_control_common::principal`).
//!
//! [`remote::vouched`]: crate::remote::vouched
//!
//! The extractors answer as control's own do (`auth::Session`,
//! `auth::DaemonAuth`), word for word, so a route moving here keeps its
//! refusals.

use arugula_control_common::{ApiError, Principal, err};
use axum::{extract::FromRequestParts, http::StatusCode, http::request::Parts};

/// What the hub found out about who's asking. Not there at all when the
/// request carried no signature and the hub found no session.
#[derive(Debug, Clone)]
pub enum Asked {
    Principal(Principal),
    /// The request's credentials were refused: why, as the hub would have
    /// answered, and which credentials they were.
    Refused {
        status: StatusCode,
        error: String,
        by: Credential,
    },
}

/// What a request proved who it was with: it crosses in the principal
/// header out of process, so it's common's.
pub use arugula_control_common::principal::Credential;

fn asked(parts: &Parts) -> Option<&Asked> {
    parts.extensions.get::<Asked>()
}

/// Anyone the hub vouched for: a person, a CLI device or a daemon.
pub struct Caller(pub Principal);

impl<S: Send + Sync> FromRequestParts<S> for Caller {
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, _: &S) -> Result<Self, ApiError> {
        match asked(parts) {
            Some(Asked::Principal(p)) => Ok(Caller(p.clone())),
            Some(Asked::Refused { status, error, .. }) => Err(err(*status, error)),
            None => Err(err(StatusCode::UNAUTHORIZED, "sign in first")),
        }
    }
}

/// A signed-in person, or a CLI device acting as its account: what
/// control's `Session` takes.
pub struct Person {
    pub account: String,
}

impl<S: Send + Sync> FromRequestParts<S> for Person {
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, _: &S) -> Result<Self, ApiError> {
        match asked(parts) {
            Some(Asked::Principal(Principal::Person { account } | Principal::Cli { account, .. })) => {
                Ok(Person { account: account.clone() })
            }
            Some(Asked::Principal(Principal::Daemon { .. })) | None => {
                Err(err(StatusCode::UNAUTHORIZED, "sign in first"))
            }
            Some(Asked::Refused { status, error, .. }) => Err(err(*status, error)),
        }
    }
}

/// An enrolled daemon, by its signature: what control's `DaemonAuth`
/// takes.
pub struct Machine {
    pub account: String,
    pub daemon: String,
}

impl<S: Send + Sync> FromRequestParts<S> for Machine {
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, _: &S) -> Result<Self, ApiError> {
        match asked(parts) {
            Some(Asked::Principal(Principal::Daemon { account, daemon })) => {
                Ok(Machine { account: account.clone(), daemon: daemon.clone() })
            }
            Some(Asked::Principal(Principal::Cli { .. })) => {
                Err(err(StatusCode::UNAUTHORIZED, "not an enrolled daemon"))
            }
            Some(Asked::Refused { status, error, by: Credential::Signature }) => Err(err(*status, error)),
            Some(Asked::Principal(Principal::Person { .. }) | Asked::Refused { by: Credential::Session, .. })
            | None => Err(err(StatusCode::UNAUTHORIZED, "bad daemon signature")),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn ask<T: FromRequestParts<(), Rejection = ApiError>>(a: Option<Asked>) -> Result<T, (StatusCode, String)> {
        let mut req = axum::http::Request::new(());
        if let Some(a) = a {
            req.extensions_mut().insert(a);
        }
        let (mut parts, ()) = req.into_parts();
        T::from_request_parts(&mut parts, &()).await.map_err(|e| (e.0, e.1))
    }

    fn p(p: Principal) -> Option<Asked> {
        Some(Asked::Principal(p))
    }

    fn person() -> Principal {
        Principal::Person { account: "a".into() }
    }

    fn cli() -> Principal {
        Principal::Cli { account: "a".into(), device: "c".into() }
    }

    fn daemon() -> Principal {
        Principal::Daemon { account: "a".into(), daemon: "d".into() }
    }

    fn refused(by: Credential) -> Option<Asked> {
        Some(Asked::Refused { status: StatusCode::FORBIDDEN, error: "cross-origin request refused".into(), by })
    }

    const NO: StatusCode = StatusCode::UNAUTHORIZED;

    /// The same answers as control's `Session` and `DaemonAuth`.
    #[tokio::test]
    async fn answers_as_control_does() {
        for who in [person(), cli()] {
            assert_eq!(ask::<Person>(p(who)).await.unwrap().account, "a");
        }
        assert_eq!(ask::<Person>(p(daemon())).await.err(), Some((NO, "sign in first".into())));
        assert_eq!(ask::<Person>(None).await.err(), Some((NO, "sign in first".into())));
        let refusal = Some((StatusCode::FORBIDDEN, "cross-origin request refused".into()));
        assert_eq!(ask::<Person>(refused(Credential::Session)).await.err(), refusal);

        let m = ask::<Machine>(p(daemon())).await.unwrap();
        assert_eq!((m.account.as_str(), m.daemon.as_str()), ("a", "d"));
        assert_eq!(ask::<Machine>(p(cli())).await.err(), Some((NO, "not an enrolled daemon".into())));
        for a in [p(person()), refused(Credential::Session), None] {
            assert_eq!(ask::<Machine>(a).await.err(), Some((NO, "bad daemon signature".into())));
        }
        assert_eq!(ask::<Machine>(refused(Credential::Signature)).await.err(), refusal);

        for who in [person(), cli(), daemon()] {
            assert_eq!(ask::<Caller>(p(who.clone())).await.unwrap().0, who);
        }
        assert_eq!(ask::<Caller>(None).await.err(), Some((NO, "sign in first".into())));
    }
}
