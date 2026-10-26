//! The template a hosted workspace's box is made from (M177, #1238): E2B's
//! `arugula-workspace`, tagged with the release that built it. A release's
//! `template` job (release.yml, scripts/ci-hosted-template) builds it from
//! packaging/hosted around that release's arugulad, snapshotted with
//! `arugulad --hosted` running, so the tag is the record: nothing is
//! written down per release.
//!
//! E2B fixes a box's vCPUs and memory when its template is built, so each
//! [`Size`] is a template of its own (M266, Arugula #201): `small` is
//! `arugula-workspace` (2 vCPU, 2 GiB, as every box was before sizes) and
//! `large` is `arugula-workspace-large` (8 vCPU, 8 GiB, the most E2B's
//! Hobby plan builds). A release builds both, from the same context.
//!
//! The platform makes boxes only from its own release's template, and a
//! box carries the one it was made from as its `arugula_template`
//! metadata ([`create`]). Until a release's job has finished, its template
//! doesn't exist: [`create`] says so ([`CreateError::NotBuilt`]) rather
//! than passing on E2B's 404.

use serde::{Deserialize, Serialize};

use super::{Create, Created, Error, Substrate};

/// The small template's name on E2B (local to E2B's project).
pub const NAME: &str = "arugula-workspace";

/// A workspace's size (M266): which template its box is made from, so how
/// many vCPUs and how much memory it has. scripts/ci-hosted-template builds
/// each with the same numbers.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Size {
    /// 2 vCPU, 2 GiB: every workspace's before sizes.
    #[default]
    Small,
    /// 8 vCPU, 8 GiB: enough for a Rust build.
    Large,
}

impl Size {
    pub const ALL: [Size; 2] = [Size::Small, Size::Large];

    pub fn as_str(self) -> &'static str {
        match self {
            Size::Small => "small",
            Size::Large => "large",
        }
    }

    /// For `skip_serializing_if`: a small spec is stored and hashed as one
    /// from before sizes.
    pub fn is_small(&self) -> bool {
        *self == Size::Small
    }

    /// Its template's name on E2B.
    pub fn name(self) -> &'static str {
        match self {
            Size::Small => NAME,
            Size::Large => "arugula-workspace-large",
        }
    }

    /// The vCPUs and MiB its template is built with, what E2B bills by.
    pub fn class(self) -> crate::hosted::usage::Class {
        use crate::hosted::usage::Class;
        match self {
            Size::Small => Class { cpus: 2, memory_mb: 2048 },
            Size::Large => Class { cpus: 8, memory_mb: 8192 },
        }
    }
}

impl std::fmt::Display for Size {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// A release's template of a size: `arugula-workspace:vX.Y.Z`,
/// `arugula-workspace-large:vX.Y.Z`.
pub fn for_release(version: &str, size: Size) -> String {
    format!("{}:v{version}", size.name())
}

/// This build's template of a size, the only one the platform makes boxes
/// of that size from.
pub fn current(size: Size) -> String {
    for_release(env!("CARGO_PKG_VERSION"), size)
}

/// The release and size a template was built for, from a box's
/// `arugula_template`: none for any other template.
pub fn release_of(template: &str) -> Option<(&str, Size)> {
    let (name, tag) = template.split_once(':')?;
    let size = Size::ALL.into_iter().find(|s| s.name() == name)?;
    tag.strip_prefix('v').filter(|v| !v.is_empty()).map(|v| (v, size))
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

/// Make a box from this release's template of `size`, whatever `box_`
/// names, with `arugula_template` and `arugula_release` set to match.
pub async fn create(substrate: &dyn Substrate, size: Size, mut box_: Create) -> Result<Created, CreateError> {
    box_.template = current(size);
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
        assert_eq!(create(&s, Size::Small, asked.clone()).await, Ok(made.clone()));
        let got = s.asked.lock().unwrap().clone().unwrap();
        assert_eq!(got.template, current(Size::Small));
        assert_eq!(got.metadata["arugula_template"], current(Size::Small));
        assert_eq!(got.metadata["arugula_release"], env!("CARGO_PKG_VERSION"));
        assert_eq!(create(&s, Size::Large, asked).await, Ok(made));
        let got = s.asked.lock().unwrap().clone().unwrap();
        assert_eq!(got.template, current(Size::Large));
        assert_eq!(got.metadata["arugula_template"], current(Size::Large));
    }

    #[tokio::test]
    async fn a_template_still_building_says_so() {
        let s = once(Err(Error::Refused { status: 404, code: None }));
        let err = create(&s, Size::Large, Create::default()).await.unwrap_err();
        assert_eq!(err, CreateError::NotBuilt { template: current(Size::Large) });
        assert!(err.to_string().contains("still being built"), "{err}");
        let s = once(Err(Error::Refused { status: 400, code: None }));
        assert_eq!(
            create(&s, Size::Small, Create::default()).await,
            Err(CreateError::Provider(Error::Refused { status: 400, code: None }))
        );
    }

    #[test]
    fn a_release_names_its_templates() {
        assert_eq!(for_release("0.58.0", Size::Small), "arugula-workspace:v0.58.0");
        assert_eq!(for_release("0.58.0", Size::Large), "arugula-workspace-large:v0.58.0");
        assert_eq!(current(Size::Small), format!("arugula-workspace:v{}", env!("CARGO_PKG_VERSION")));
        for size in Size::ALL {
            assert_eq!(release_of(&current(size)), Some((env!("CARGO_PKG_VERSION"), size)));
        }
        assert_eq!(release_of("arugula-workspace:v0.58.0"), Some(("0.58.0", Size::Small)));
        assert_eq!(release_of("arugula-workspace-large:v0.58.0"), Some(("0.58.0", Size::Large)));
        for other in ["base", "arugula-workspace", "arugula-workspace:v", "arugula-workspace-test:v0.58.0", "x:v1"] {
            assert_eq!(release_of(other), None, "{other}");
        }
    }

    /// Each name fits E2B's rule: `[a-z0-9-_]`, up to 128.
    #[test]
    fn the_names_are_ones_e2b_takes() {
        for size in Size::ALL {
            let name = size.name();
            assert!(
                name.len() <= 128
                    && name.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-' || b == b'_'),
                "{name}"
            );
        }
    }

    /// scripts/ci-hosted-template builds each size with the vCPUs and
    /// memory the platform meters it as, under its name. Large is at most
    /// what E2B's Hobby plan builds (8 vCPU, 8 GiB: "Max vCPUs per build:
    /// 8 on Hobby", <https://docs.e2b.dev/sandbox-template/customize-cpu-ram>).
    #[test]
    fn the_script_builds_each_size() {
        let script = include_str!("../../../../scripts/ci-hosted-template");
        for size in Size::ALL {
            let c = size.class();
            let suffix = size.name().strip_prefix(NAME).unwrap();
            let line = format!(
                "  {size}) echo '{} {}{}{suffix}' ;;",
                c.cpus,
                c.memory_mb,
                if suffix.is_empty() { "" } else { " " }
            );
            assert!(script.contains(&line), "ci-hosted-template has no line: {line}");
            assert!(c.cpus <= 8 && c.memory_mb <= 8192, "{size}: more than E2B's Hobby plan builds");
        }
        assert_eq!(serde_json::to_string(&Size::Large).unwrap(), r#""large""#);
        assert_eq!(serde_json::from_str::<Size>(r#""small""#).unwrap(), Size::Small);
        assert!(serde_json::from_str::<Size>(r#""huge""#).is_err());
    }
}
