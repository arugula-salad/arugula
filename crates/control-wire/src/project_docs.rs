//! Project Docs (aru-118): a project's own Docs, kept by the platform
//! (`crate::db::project_docs` there) and read by the project's members, as
//! its native issues are. A Doc (`pdc_…`) is a title and an ordered list of
//! sections, each a heading and its Markdown (the machine Doc's shape:
//! pending, with an intent, until it's written). Plaintext on the
//! platform, like issues and chat; nothing here leaves the project.
//!
//! A project in a path is its id (`prj_…`), or `personal` for the reader's
//! own. Native Docs come with chat, as native issues do.
//!
//! **Who may do what:** its members read; a team's owners and editors make,
//! write and delete (a personal project's account does all of it); a viewer
//! reads. An agent session (aru-115) makes Docs with its asker's rights and
//! writes only the ones it made; it never deletes.
//!
//! **Writes** are last-writer-wins per section, as the machine Doc's
//! fills are: a section written again replaces its body.

use serde::{Deserialize, Serialize};

use crate::chat::ChatAgent;

/// `GET`: a project's Docs, [`DocList`]; `POST` [`NewDoc`]: make one,
/// answered [`DocView`].
pub const PROJECT_DOCS: &str = "/api/projects/{project}/docs";
/// `GET`: a Doc with its sections in order, [`DocView`]; `PUT` [`DocWrite`]:
/// rename it, or write or add a section, answered [`DocView`]; `DELETE`:
/// it goes (owners and editors).
pub const PROJECT_DOC: &str = "/api/projects/{project}/docs/{id}";

/// The longest title, heading and intent, in characters: one line each.
pub const TITLE_MAX: usize = 200;
pub const HEADING_MAX: usize = 200;
pub const INTENT_MAX: usize = 500;
/// A section's body, in bytes, and the most sections a Doc has: as a
/// machine Doc's.
pub const SECTION_MAX: usize = 64 * 1024;
pub const SECTIONS_MAX: usize = 40;
/// A whole Doc (its title, headings, intents and bodies), in bytes.
pub const DOC_MAX: usize = 1024 * 1024;
/// The most Docs in a project.
pub const DOCS_MAX: u64 = 1_000;

/// A Doc, without its sections: a list's row, and a view's head.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(rename = "ProjectDoc"))]
pub struct Doc {
    /// `pdc_` and 32 hex characters.
    pub id: String,
    pub project: String,
    pub title: String,
    /// The account that made it, or "" once it's deleted, or for one an
    /// agent session made.
    pub author: String,
    /// The agent session that made it as itself (aru-115): its `machine`
    /// the session's id (`ses_…`), its name `Agent session <id>`.
    pub agent: Option<ChatAgent>,
    /// How many sections it has, and how many are written.
    pub sections: u32,
    pub written: u32,
    /// Counts its changes.
    pub rev: u64,
    pub created_at: u64,
    pub updated_at: u64,
}

/// One section: pending (`body` null) until it's written.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(rename = "ProjectDocSection"))]
pub struct Section {
    /// The Doc's own: `s1`, `s2`, … in the order they were added.
    pub id: String,
    pub heading: String,
    /// One line on what it will hold, shown while it's pending.
    pub intent: String,
    /// Its Markdown (the machine Doc's: `chart` and `svg` fences draw).
    pub body: Option<String>,
    /// Counts the writes to its body.
    pub rev: u64,
}

/// `GET` [`PROJECT_DOC`]: a Doc with its sections, in order.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(rename = "ProjectDocView"))]
pub struct DocView {
    pub doc: Doc,
    pub sections: Vec<Section>,
    /// Whether the reader may write it.
    pub writer: bool,
}

/// `GET` [`PROJECT_DOCS`]: a project's Docs, the latest changed first.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(rename = "ProjectDocList"))]
pub struct DocList {
    pub project: String,
    pub docs: Vec<Doc>,
    /// Whether the reader may make one.
    pub writer: bool,
}

/// A new Doc's section: a heading, and its Markdown now or an intent for
/// later.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(rename = "NewProjectDocSection"))]
pub struct NewSection {
    pub heading: String,
    #[serde(default)]
    pub intent: String,
    #[serde(default)]
    pub markdown: Option<String>,
}

/// `POST` [`PROJECT_DOCS`]: make a Doc.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(rename = "NewProjectDoc"))]
pub struct NewDoc {
    pub title: String,
    #[serde(default)]
    pub sections: Vec<NewSection>,
}

/// `PUT` [`PROJECT_DOC`]: what changes. `title` renames it. With `section`,
/// that section's body becomes `markdown` (and its heading `heading`, when
/// given); with no `section` but a `heading`, a section is added at the
/// end, written when `markdown` is given and pending otherwise.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(rename = "ProjectDocWrite"))]
pub struct DocWrite {
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub section: Option<String>,
    #[serde(default)]
    pub heading: Option<String>,
    #[serde(default)]
    pub intent: Option<String>,
    #[serde(default)]
    pub markdown: Option<String>,
}

/// [`PROJECT_DOCS`] for `project`.
pub fn project_docs(project: &str) -> String {
    format!("/api/projects/{project}/docs")
}

/// [`PROJECT_DOC`] for `id` in `project`.
pub fn project_doc(project: &str, id: &str) -> String {
    format!("/api/projects/{project}/docs/{id}")
}
