//! Project Docs (aru-118): a project's own Docs, kept by the platform
//! (`crate::db::project_docs` there) and read by the project's members, as
//! its native issues are. A Doc (`pdc_…`) is a title and an ordered list of
//! sections, each a heading and its Markdown (the machine Doc's shape:
//! pending, with an intent, until it's written). Plaintext on the
//! platform, like issues and chat; nothing here leaves the project.
//!
//! A project in a path is its id (`prj_…`), or `personal` for the reader's
//! own. Membership and roles alone say who reads them, as for native
//! issues: a person's chat setting doesn't (Arugula #746).
//!
//! **Who may do what:** its members read; a team's owners and editors make,
//! write and delete (a personal project's account does all of it); a viewer
//! reads. An agent session (aru-115) makes Docs with its asker's rights and
//! writes only the ones it made; it never deletes.
//!
//! **Writes** are last-writer-wins per section, as the machine Doc's
//! fills are: a section written again replaces its body.
//!
//! **Pictures** (#418): a Doc keeps PNG, JPEG, GIF and WebP files, shown by
//! the section that says `![alt](assets/<name>)`, as a machine Doc's are;
//! members read one by name ([`PROJECT_DOC_ASSET`]).
//!
//! **A copy from a machine** (#418): a machine copies a Doc of its own up to
//! its owner's personal project ([`MACHINE_DOC`]), keyed by the machine and
//! its own Doc id, so the same machine Doc always lands on the same `pdc_`.
//! It stays private to the owner: the route that moved a Doc between
//! projects is gone (native #673), and a Doc of a team's is made there. A
//! machine can't reach another machine's copies.
//! The machine's Doc is the truth and the copy is read-only (Arugula #678):
//! a write to it over [`PROJECT_DOC`] is refused, by a person or an agent
//! session, naming the machine; only the machine's own puts change it, and
//! the copy goes with the Doc (native #673): the machine's `DELETE`
//! [`MACHINE_DOC`] takes it down. A person who writes in the project may
//! delete a copy too (`DELETE` [`PROJECT_DOC`]); the machine's next change
//! makes a new one.
//!
//! **Share with…** makes a team Doc from a copy: a snapshot of its
//! title, sections and pictures as a new, ordinary Doc in a project of the
//! owner's choosing ([`PROJECT_DOC_COPIES`]), which the team owns and edits.
//! It records where it came from ([`MadeFrom`]); the copy stays the machine's
//! truth, and **Update copy** ([`PROJECT_DOC_COPY`]) replaces the team Doc
//! from the copy again, when its owner asks. Nothing is live.

use serde::{Deserialize, Serialize};

use crate::chat::ChatAgent;

/// `GET`: a project's Docs, [`DocList`]; `POST` [`NewDoc`]: make one,
/// answered [`DocView`].
pub const PROJECT_DOCS: &str = "/api/projects/{project}/docs";
/// `GET`: a Doc with its sections in order, [`DocView`]; `PUT` [`DocWrite`]:
/// rename it, or write or add a section, answered [`DocView`]; `DELETE`:
/// it goes (owners and editors; a copy too).
pub const PROJECT_DOC: &str = "/api/projects/{project}/docs/{id}";

/// A Doc's team Docs. `POST` [`ShareCopy`]: Share with… makes one in the
/// project `to`, answered [`DocView`] (409 [`Shared`] when there is one
/// already); `GET`: the team Docs made from it, [`DocCopies`]. Only the
/// Doc's owner, for a Doc in their personal project, a machine's copy or
/// one written here.
pub const PROJECT_DOC_COPIES: &str = "/api/projects/{project}/docs/{id}/copies";
/// `PUT` [`UpdateCopy`]: Update copy, the team Doc in project `to` made
/// from this Doc takes its title, sections and pictures as they are now,
/// answered [`DocCopy`]; 409 [`Edited`] when the team edited it since and
/// `replace` isn't set. Only the Doc's owner, who still writes in `to`.
pub const PROJECT_DOC_COPY: &str = "/api/projects/{project}/docs/{id}/copies/{to}";

/// `GET`: one of a Doc's pictures, the bytes (any member).
pub const PROJECT_DOC_ASSET: &str = "/api/projects/{project}/docs/{id}/assets/{name}";
/// `PUT` [`MachineDoc`]: a machine copies one of its Docs up for its owner,
/// answered [`MachineDocPlaced`]; `DELETE`: it takes its copy down.
pub const MACHINE_DOC: &str = "/api/daemon/docs/{machine_doc}";
/// `PUT` the picture's bytes (the body): a machine's copy of one of its
/// Doc's pictures, answered [`Asset`].
pub const MACHINE_DOC_ASSET: &str = "/api/daemon/docs/{machine_doc}/assets/{name}";

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
/// One picture, in bytes, and the most a Doc keeps: as a machine Doc's.
pub const ASSET_MAX: usize = 2 * 1024 * 1024;
pub const ASSETS_MAX: usize = 20;

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
    /// The machine this Doc is a copy from, or null for one made here. A
    /// copy is read-only here: the machine's Doc is the truth (#678).
    #[serde(default)]
    pub machine: Option<String>,
    /// That machine's name, when control knows it: null for a Doc made
    /// here, or once the machine left. Older pages don't read it.
    #[serde(default)]
    pub machine_name: Option<String>,
    /// For a team Doc made from a Doc of its owner's (Share with…): where
    /// from. Null for any other Doc, and from an older platform.
    #[serde(default)]
    pub made_from: Option<MadeFrom>,
    /// How the Doc is laid out: `doc`, `tabs` or `prototype`. Sent only when
    /// it isn't `doc`: absent, or a name the reader doesn't know, is `doc`,
    /// and an older page ignores it and draws the column.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub layout: Option<String>,
}

/// The Doc a team Doc was made from: a machine's copy, or one written here.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(rename = "ProjectDocMadeFrom"))]
pub struct MadeFrom {
    /// The source Doc's project (its owner's personal one) and its id.
    pub project: String,
    pub id: String,
    /// The name of the machine the source is a copy from, when control knows
    /// it; null for a source written here. Only what the platform could
    /// tell; the source itself is its owner's to read.
    #[serde(default)]
    pub machine_name: Option<String>,
}

/// One picture of a Doc, without its bytes: the shape a machine Doc's
/// state gives (`DocState.assets`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(rename = "ProjectDocAsset"))]
pub struct Asset {
    pub name: String,
    /// `image/png`, `image/jpeg`, `image/gif` or `image/webp`: what the
    /// bytes say.
    pub mime: String,
    pub size: u64,
    /// Changes whenever the name is written with other bytes: clients cache
    /// a picture by `name/rev`.
    pub rev: u64,
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
    /// Its pictures, by name.
    #[serde(default)]
    pub assets: Vec<Asset>,
    /// Whether the reader may write it.
    pub writer: bool,
    /// Whether the reader may delete it: a person who writes in the
    /// project, a copy included (native #673). An older platform sends
    /// none; the page then takes `writer`.
    #[serde(default)]
    pub deleter: bool,
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
    /// `doc`, `tabs` or `prototype`; missing is `doc`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub layout: Option<String>,
}

/// `PUT` [`PROJECT_DOC`]: what changes. `title` renames it, `layout` lays it out. With `section`,
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
    /// Sets the layout (`doc`, `tabs` or `prototype`); omitted leaves it as
    /// it is.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub layout: Option<String>,
}

/// `POST` [`PROJECT_DOC_COPIES`]: Share with… this project.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(rename = "ShareProjectDoc"))]
pub struct ShareCopy {
    /// The project (`prj_…`) the team Doc is made in: one where the person
    /// writes Docs, and not their personal one.
    pub to: String,
}

/// The 409 to a [`ShareCopy`] when a team Doc was made from the Doc in that
/// project already.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(rename = "ProjectDocShared"))]
pub struct Shared {
    pub error: String,
    /// The team Doc that is there.
    pub id: String,
}

/// A team Doc made from a Doc of its owner's, as its owner sees it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(rename = "ProjectDocCopy"))]
pub struct DocCopy {
    pub project: String,
    pub project_name: String,
    /// The team Doc's id, and its page on the platform's page.
    pub id: String,
    pub link: String,
    /// When it was made, and when it last changed (ms since the epoch).
    pub made: u64,
    pub updated: u64,
    /// The source changed since: its rev is above the one recorded.
    pub stale: bool,
    /// The team changed it since: its rev is above the one recorded.
    pub edited: bool,
}

/// `GET` [`PROJECT_DOC_COPIES`]: the team Docs made from a Doc, earliest
/// first, in the projects its owner still reads.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(rename = "ProjectDocCopies"))]
pub struct DocCopies {
    pub copies: Vec<DocCopy>,
}

/// `PUT` [`PROJECT_DOC_COPY`]: Update copy.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(rename = "UpdateProjectDocCopy"))]
pub struct UpdateCopy {
    /// Replace the team's edits too. Without it, a team Doc edited since is
    /// left alone and answered [`Edited`].
    #[serde(default)]
    pub replace: bool,
}

/// The 409 to an [`UpdateCopy`] that would replace the team's edits.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(rename = "ProjectDocEdited"))]
pub struct Edited {
    pub error: String,
    pub edited: bool,
}

/// A section of a machine's Doc, as it copies it up: the machine's own
/// ids, and its body or null while it's pending.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(rename = "MachineDocSection"))]
pub struct MachineSection {
    pub id: String,
    pub heading: String,
    #[serde(default)]
    pub intent: String,
    #[serde(default)]
    pub body: Option<String>,
}

/// `PUT` [`MACHINE_DOC`]: the whole Doc as the machine has it now. The
/// sections replace the copy's; the same content again changes nothing.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(rename = "MachineDoc"))]
pub struct MachineDoc {
    pub title: String,
    pub sections: Vec<MachineSection>,
    /// How the Doc is laid out: `doc`, `tabs` or `prototype`. Sent only when
    /// it isn't `doc`, so a control from before layouts still takes the Doc;
    /// absent, or a name the reader doesn't know, is `doc`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub layout: Option<String>,
}

/// Where a machine's copy is, answering `PUT` [`MACHINE_DOC`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(rename = "MachineDocPlaced"))]
pub struct MachineDocPlaced {
    pub project: String,
    pub id: String,
    /// The Doc's page on the platform's page.
    pub link: String,
    /// Counts the copy's changes; the same content again leaves it.
    pub rev: u64,
}

/// [`PROJECT_DOCS`] for `project`.
pub fn project_docs(project: &str) -> String {
    format!("/api/projects/{project}/docs")
}

/// [`PROJECT_DOC`] for `id` in `project`.
pub fn project_doc(project: &str, id: &str) -> String {
    format!("/api/projects/{project}/docs/{id}")
}

/// [`PROJECT_DOC_ASSET`] for `name` of `id` in `project`.
pub fn project_doc_asset(project: &str, id: &str, name: &str) -> String {
    format!("/api/projects/{project}/docs/{id}/assets/{name}")
}

/// [`PROJECT_DOC_COPIES`] for the copy `id` in `project`.
pub fn project_doc_copies(project: &str, id: &str) -> String {
    format!("/api/projects/{project}/docs/{id}/copies")
}

/// [`PROJECT_DOC_COPY`] for the team Doc of copy `id` in `to`.
pub fn project_doc_copy(project: &str, id: &str, to: &str) -> String {
    format!("/api/projects/{project}/docs/{id}/copies/{to}")
}

/// [`MACHINE_DOC`] for the machine's Doc `machine_doc`.
pub fn machine_doc(machine_doc: &str) -> String {
    format!("/api/daemon/docs/{machine_doc}")
}

/// [`MACHINE_DOC_ASSET`] for `name` of the machine's Doc `machine_doc`.
pub fn machine_doc_asset(machine_doc: &str, name: &str) -> String {
    format!("/api/daemon/docs/{machine_doc}/assets/{name}")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A control from before layouts: the body as `MachineDoc` was.
    #[derive(Deserialize)]
    #[allow(dead_code)]
    struct OldMachineDoc {
        title: String,
        sections: Vec<MachineSection>,
    }

    #[test]
    fn a_machine_doc_names_its_layout_only_off_doc_and_an_older_control_takes_it() {
        let doc =
            |layout: Option<&str>| MachineDoc { title: "T".into(), sections: vec![], layout: layout.map(Into::into) };
        let plain = serde_json::to_value(doc(None)).unwrap();
        assert!(plain.get("layout").is_none(), "a plain Doc's body is what it was");
        let tabs = serde_json::to_value(doc(Some("tabs"))).unwrap();
        assert_eq!(tabs["layout"], "tabs");
        // Not refused by a reader that doesn't list it, and an old body reads here.
        assert_eq!(serde_json::from_value::<OldMachineDoc>(tabs.clone()).unwrap().title, "T");
        assert_eq!(serde_json::from_value::<MachineDoc>(plain).unwrap().layout, None);
        assert_eq!(serde_json::from_value::<MachineDoc>(tabs).unwrap().layout.as_deref(), Some("tabs"));
    }

    /// A page from before layouts: a `Doc` as it read it.
    #[derive(Deserialize)]
    #[allow(dead_code)]
    struct OldDoc {
        id: String,
        title: String,
    }

    #[test]
    fn a_doc_names_its_layout_only_off_doc_and_an_older_page_and_platform_take_it() {
        let body = |layout: Option<&str>| {
            let mut v = serde_json::json!({
                "id": "pdc_1", "project": "prj_1", "title": "T", "author": "", "agent": null, "sections": 0,
                "written": 0, "rev": 1, "created_at": 1, "updated_at": 1,
            });
            if let Some(l) = layout {
                v["layout"] = l.into();
            }
            v
        };
        // From an older platform: no field, so `doc`.
        assert_eq!(serde_json::from_value::<Doc>(body(None)).unwrap().layout, None);
        let tabs: Doc = serde_json::from_value(body(Some("tabs"))).unwrap();
        assert_eq!(tabs.layout.as_deref(), Some("tabs"));
        // A plain Doc's body is what it was; an older page reads a tabs one.
        assert!(
            serde_json::to_value(serde_json::from_value::<Doc>(body(None)).unwrap()).unwrap().get("layout").is_none()
        );
        assert_eq!(serde_json::from_value::<OldDoc>(serde_json::to_value(&tabs).unwrap()).unwrap().title, "T");
        // The writes are omitted when none, so an older platform takes them.
        assert!(serde_json::to_value(DocWrite::default()).unwrap().get("layout").is_none());
        let n = NewDoc { title: "T".into(), sections: vec![], layout: None };
        assert!(serde_json::to_value(n).unwrap().get("layout").is_none());
    }
}
