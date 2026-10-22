//! The template a hosted workspace's box is made from (M177, #1238): E2B's
//! `arugula-workspace`, tagged with the release that built it. A release's
//! `template` job (release.yml, scripts/ci-hosted-template) builds it from
//! packaging/hosted around that release's arugulad, snapshotted with
//! `arugulad --hosted` running, so the tag is the record: nothing is
//! written down per release.
//!
//! The platform makes boxes only from its own release's template, and a
//! box carries the one it was made from as its `arugula_template`
//! metadata ([`create`]). Until a release's job has finished, its template
//! doesn't exist: [`create`] says so ([`CreateError::NotBuilt`]) rather
//! than passing on E2B's 404.

use super::{Create, Created, Error, Substrate};

/// The template's name on E2B (local to E2B's project).
pub const NAME: &str = "arugula-workspace";

/// A release's template: `arugula-workspace:vX.Y.Z`.
pub fn for_release(version: &str) -> String {
    format!("{NAME}:v{version}")
}

/// This build's template, the only one the platform makes boxes from.
pub fn current() -> String {
    for_release(env!("CARGO_PKG_VERSION"))
}

/// The release a template was built for, from a box's `arugula_template`:
/// none for any other template.
pub fn release_of(template: &str) -> Option<&str> {
    template.strip_prefix(NAME)?.strip_prefix(":v").filter(|v| !v.is_empty())
}

/// Why [`create`] made no box.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CreateError {
    /// This release's template isn't on the provider yet: its release job
    /// is still building it (or failed).
    NotBuilt {
        template: String,
    },
    Provider(Error),
}

impl std::fmt::Display for CreateError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CreateError::NotBuilt { template } => write!(
                f,
                "this release's workspace template ({template}) is still being built: try again in a few minutes"
            ),
            CreateError::Provider(e) => e.fmt(f),
        }
    }
}

impl std::error::Error for CreateError {}

/// Make a box from this release's template, whatever `box_` names, with
/// `arugula_template` and `arugula_release` set to match.
pub async fn create(substrate: &dyn Substrate, mut box_: Create) -> Result<Created, CreateError> {
    box_.template = current();
    box_.metadata.insert("arugula_template".into(), box_.template.clone());
    box_.metadata.insert("arugula_release".into(), env!("CARGO_PKG_VERSION").into());
    match substrate.create(&box_).await {
        // The E2B adapter's unknown template (a create's 404 isn't "no such
        // box").
        Err(Error::Refused { status: 404, .. }) => Err(CreateError::NotBuilt { template: box_.template }),
        r => r.map_err(CreateError::Provider),
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::sync::Mutex;
    use std::time::Duration;

    use futures_util::future::BoxFuture;

    use super::super::{Paused, Result, Resumed, Sandbox};
    use super::*;

    /// Answers every create with `answer`, keeping what it was asked.
    struct Once {
        answer: Result<Created>,
        asked: Mutex<Option<Create>>,
    }

    impl Substrate for Once {
        fn create<'a>(&'a self, box_: &'a Create) -> BoxFuture<'a, Result<Created>> {
            *self.asked.lock().unwrap() = Some(box_.clone());
            Box::pin(async move { self.answer.clone() })
        }
        fn resume<'a>(&'a self, _: &'a str, _: Duration) -> BoxFuture<'a, Result<Resumed>> {
            unimplemented!()
        }
        fn pause<'a>(&'a self, _: &'a str) -> BoxFuture<'a, Result<Paused>> {
            unimplemented!()
        }
        fn delete<'a>(&'a self, _: &'a str) -> BoxFuture<'a, Result<()>> {
            unimplemented!()
        }
        fn status<'a>(&'a self, _: &'a str) -> BoxFuture<'a, Result<Option<Sandbox>>> {
            unimplemented!()
        }
        fn list<'a>(&'a self, _: &'a BTreeMap<String, String>) -> BoxFuture<'a, Result<Vec<Sandbox>>> {
            unimplemented!()
        }
        fn hold<'a>(&'a self, _: &'a str, _: Duration) -> BoxFuture<'a, Result<()>> {
            unimplemented!()
        }
    }

    fn once(answer: Result<Created>) -> Once {
        Once { answer, asked: Mutex::new(None) }
    }

    #[tokio::test]
    async fn a_box_comes_only_from_this_release() {
        let made = Created { id: "sb1".into(), template: "resolved".into() };
        let s = once(Ok(made.clone()));
        let asked = Create {
            template: "base".into(),
            metadata: [("arugula_template".into(), "base".into())].into(),
            ..Default::default()
        };
        assert_eq!(create(&s, asked).await, Ok(made));
        let asked = s.asked.lock().unwrap().clone().unwrap();
        assert_eq!(asked.template, current());
        assert_eq!(asked.metadata["arugula_template"], current());
        assert_eq!(asked.metadata["arugula_release"], env!("CARGO_PKG_VERSION"));
    }

    #[tokio::test]
    async fn a_template_still_building_says_so() {
        let s = once(Err(Error::Refused { status: 404, code: None }));
        let err = create(&s, Create::default()).await.unwrap_err();
        assert_eq!(err, CreateError::NotBuilt { template: current() });
        assert!(err.to_string().contains("still being built"), "{err}");
        let s = once(Err(Error::Refused { status: 400, code: None }));
        assert_eq!(
            create(&s, Create::default()).await,
            Err(CreateError::Provider(Error::Refused { status: 400, code: None }))
        );
    }

    #[test]
    fn a_release_names_its_template() {
        assert_eq!(for_release("0.58.0"), "arugula-workspace:v0.58.0");
        assert_eq!(current(), format!("arugula-workspace:v{}", env!("CARGO_PKG_VERSION")));
        assert_eq!(release_of(&current()), Some(env!("CARGO_PKG_VERSION")));
        assert_eq!(release_of("arugula-workspace:v0.58.0"), Some("0.58.0"));
        for other in ["base", "arugula-workspace", "arugula-workspace:v", "arugula-workspace-test:v0.58.0", "x:v1"] {
            assert_eq!(release_of(other), None, "{other}");
        }
    }

    /// The name fits E2B's rule: `[a-z0-9-_]`, up to 128.
    #[test]
    fn the_name_is_one_e2b_takes() {
        assert!(
            NAME.len() <= 128
                && NAME.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-' || b == b'_')
        );
    }
}
