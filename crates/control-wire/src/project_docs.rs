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
//! **A move** (Arugula #863) gives a Doc to a team, leaving one home and no
//! copy. A machine's Doc is moved by its machine: it makes the team Doc
//! through [`AGENT_PROJECT_DOCS`] with [`NewDoc::moved_from`], fills it and
//! puts its pictures, then keeps only a pointer to it; the team Doc says
//! where it came from ([`Doc::moved_from`]). A Doc written on the platform
//! moves in place ([`PROJECT_DOC_MOVE`]): its project changes, and it keeps
//! its id, thread and pictures.
//!
//! **No copies** (Arugula #865): a machine's Docs stay on the machine, and
//! nothing copies them up. An older platform may still send the fields
//! copies had (`machine`, `machine_name`, `made_from`); they're ignored.

use serde::{Deserialize, Serialize};

use crate::chat::ChatAgent;

/// `GET`: a project's Docs, [`DocList`]; `POST` [`NewDoc`]: make one,
/// answered [`DocView`].
pub const PROJECT_DOCS: &str = "/api/projects/{project}/docs";
/// `GET`: a Doc with its sections in order, [`DocView`]; `PUT` [`DocWrite`]:
/// rename it, or write or add a section, answered [`DocView`]; `DELETE`:
/// it goes (owners and editors).
pub const PROJECT_DOC: &str = "/api/projects/{project}/docs/{id}";
/// `GET`: a project's trashed Docs, newest first, [`TrashList`] (any member).
/// A team Doc that is deleted waits here for [`TRASH_DAYS`] days (Arugula
/// #866). Ids are `pdc_` and hex, so `trash` is never taken for one.
pub const PROJECT_DOCS_TRASH: &str = "/api/projects/{project}/docs/trash";
/// `POST`: bring a trashed Doc back whole, answered [`DocView`] (owners,
/// editors and the Doc's maker).
pub const PROJECT_DOC_RESTORE: &str = "/api/projects/{project}/docs/{id}/restore";
/// `POST` [`DocMove`]: a Doc written on the platform moves to the team
/// project `to` in place, keeping its id, thread and pictures, answered
/// [`DocView`] (Arugula #863). 409 [`Shared`] when it is in a team's project
/// already, with its id there. Refused in a project the person doesn't
/// write Docs in. The
/// Doc's owner, or an owner or editor of `from` who writes in `to`.
pub const PROJECT_DOC_MOVE: &str = "/api/projects/{project}/docs/{id}/move";
/// How long a deleted team Doc waits in the trash before it goes for good.
pub const TRASH_DAYS: u64 = 30;

/// A team Doc's public link (Arugula #871). `PUT` [`PublicMake`]: make it
/// (or answer the one it has), answered [`PublicLink`] at version 0, which
/// serves nothing until the client that sealed the Doc has put the parts to
/// control and says so with `POST` [`PublicServed`]. `DELETE`: forget it,
/// after the client took it down at control. Owners and editors, as a person
/// or a machine with `docs.write`; a viewer is refused (403) and an agent
/// session never makes or revokes one. Any member who reads the Doc sees the
/// link in its [`DocView`].
pub const PROJECT_DOC_PUBLIC: &str = "/api/projects/{project}/docs/{id}/public";
/// `PUT` the raw bytes of one sealed part (at most 1 MiB) of version `ver`
/// of a team Doc's public link, for the page: it can't reach the hub's own
/// `/api/doc-links/…` (the platform's host answers none of the hub's), so the
/// platform checks that the person writes the Doc and passes it on. The
/// answers are the hub's: 200 `{stored}`, or its refusal with a `reason`.
pub const PROJECT_DOC_PUBLIC_PART: &str = "/api/projects/{project}/docs/{id}/public/v/{ver}/parts/{n}";
/// `POST` `{parts, size}`: commit version `ver` once its parts are put, as
/// [`PROJECT_DOC_PUBLIC_PART`]; then `POST` [`PROJECT_DOC_PUBLIC`] says it.
pub const PROJECT_DOC_PUBLIC_COMMIT: &str = "/api/projects/{project}/docs/{id}/public/v/{ver}/commit";
/// The same for a machine's agents, signed by the machine: `docs.write`
/// over the project, and the owner an owner or editor.
pub const AGENT_PROJECT_DOC_PUBLIC: &str = "/api/daemon/projects/{project}/docs/{doc}/public";
/// `GET`: one of a Doc's pictures, the bytes (any member).
pub const PROJECT_DOC_ASSET: &str = "/api/projects/{project}/docs/{id}/assets/{name}";
/// A machine's agents' (Arugula #862), signed by the machine, acting for
/// its owner in a team's project: each checked against the owner's role
/// there and the machine's `docs.write` grant over the project, asked on
/// every call (reading needs only that the owner reads the project). What an
/// agent writes lands at once, as the owner's, labelled "<owner>'s agent on
/// <machine>" ([`Doc::agent`]). `POST` [`NewDoc`]: make a Doc in the project
/// (its id, or `personal`), answered [`DocView`]; 403 `agent_ungranted` without
/// the grant, and the person's refusals otherwise (a viewer, a full project).
pub const AGENT_PROJECT_DOCS: &str = "/api/daemon/projects/{project}/docs";
/// `GET`: a Doc with its sections, [`DocView`]; `PUT` [`DocWrite`]: as
/// [`PROJECT_DOC`]'s, answered [`DocView`]. `DELETE` (Arugula #863): only a Doc this
/// machine made by a move ([`Doc::moved_from`] names it), when the move
/// failed partway and is undone, at once or after a crash (the machine's
/// next move of the Doc, or its next start); it goes for good, not to the
/// trash, and anything else is refused (403). A Doc that isn't there is a
/// 404, which the machine takes as done.
pub const AGENT_PROJECT_DOC: &str = "/api/daemon/projects/{project}/docs/{doc}";
/// `PUT` the picture's bytes (the body): a picture of the Doc, answered
/// [`Asset`], with a machine Doc's limits ([`ASSET_MAX`], [`ASSETS_MAX`]).
/// `GET`: the picture, [`AssetData`] (any Doc the owner reads; no power), for
/// a team Doc's block, whose daemon reads JSON from control.
pub const AGENT_PROJECT_DOC_ASSET: &str = "/api/daemon/projects/{project}/docs/{doc}/assets/{name}";

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
    /// The account that made it (an agent's owner's, for one a machine's
    /// agent made), or "" once it's deleted, or for one an agent session made.
    pub author: String,
    /// The agent session that made it as itself (aru-115): its `machine`
    /// the session's id (`ses_…`), its name `Agent session <id>`; or the
    /// machine whose agent made it for `author` (Arugula #862), its
    /// `machine` and `machine_name` the machine's.
    pub agent: Option<ChatAgent>,
    /// How many sections it has, and how many are written.
    pub sections: u32,
    pub written: u32,
    /// Counts its changes.
    pub rev: u64,
    pub created_at: u64,
    pub updated_at: u64,
    /// How the Doc is laid out: `doc`, `tabs` or `prototype`. Sent only when
    /// it isn't `doc`: absent, or a name the reader doesn't know, is `doc`,
    /// and an older page ignores it and draws the column.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub layout: Option<String>,
    /// For a team Doc a machine moved here (Arugula #863): the machine and
    /// its Doc's id there. Absent for any other Doc, and from an older
    /// platform.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub moved_from: Option<MovedFrom>,
}

/// Where a Doc a machine moved to a team was (Arugula #863).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(rename = "ProjectDocMovedFrom"))]
pub struct MovedFrom {
    /// The machine's id in control: the one that signs the call.
    pub machine: String,
    /// The Doc's id in that machine's library (16 hex digits).
    pub doc: String,
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

/// A picture's type and bytes, as `GET` [`AGENT_PROJECT_DOC_ASSET`] answers
/// it: `data` is the file in base64.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct AssetData {
    /// `image/png`, `image/jpeg`, `image/gif` or `image/webp`.
    pub mime: String,
    pub data: String,
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
    /// project. An older platform sends none; the page then takes `writer`.
    #[serde(default)]
    pub deleter: bool,
    /// The Doc's page on the platform's page, which the team opens. An older
    /// platform sends none.
    #[serde(default)]
    pub link: Option<String>,
    /// The Doc's public link, when it has one (Arugula #871). Left out for
    /// an agent session, which is never given the key.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub public: Option<PublicLink>,
}

/// A team Doc's public link (Arugula #871). The platform keeps its key with
/// the Doc, and gives it only to those who read the Doc; control holds the
/// ciphertext and never the key. The URL is `<viewer>/<id>#k=<key>`, the
/// viewer being `/control.json`'s `doc_viewer`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(rename = "ProjectDocPublic"))]
pub struct PublicLink {
    /// The link's id at control: 32 lower-case hex digits.
    pub id: String,
    /// The link's key, base64url without padding: 32 bytes.
    pub key: String,
    /// The version control serves; 0 while the first one isn't uploaded.
    pub ver: u64,
    /// When the served version was made (ms); 0 while there is none.
    pub updated_at: u64,
    /// The Doc changed since the served version was sealed.
    pub behind: bool,
    /// The address anyone can open, `<viewer>/<id>#k=<key>`; empty when this
    /// control keeps no shared Docs, or from an older platform. The platform
    /// page can't read `/control.json` (the platform's host answers none of
    /// the hub's), so the platform says where the viewer is.
    #[serde(default)]
    pub url: String,
}

/// `PUT` [`PROJECT_DOC_PUBLIC`]: make the link. Empty.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(rename = "ProjectDocPublicMake"))]
pub struct PublicMake {}

/// `POST` [`PROJECT_DOC_PUBLIC`]: the client put version `ver` to control and
/// it was committed. `rev` is the Doc's rev it sealed, so the page can say
/// when the Doc has moved on. Answered [`PublicLink`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(rename = "ProjectDocPublicServed"))]
pub struct PublicServed {
    pub ver: u64,
    pub rev: u64,
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

/// One trashed Doc: its head, who deleted it and when.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(rename = "ProjectDocTrashed"))]
pub struct Trashed {
    pub doc: Doc,
    /// When it was deleted (ms).
    pub trashed_at: u64,
    /// The account that deleted it, or "" once that account is deleted.
    /// The page names it from the team's members, as it does a Doc's
    /// `author`.
    pub trashed_by: String,
    /// Whether the reader may restore it.
    pub restorable: bool,
}

/// `GET` [`PROJECT_DOCS_TRASH`]: a project's trashed Docs, the latest
/// deleted first.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(rename = "ProjectDocTrash"))]
pub struct TrashList {
    pub project: String,
    pub docs: Vec<Trashed>,
    /// How many days a Doc waits ([`TRASH_DAYS`]).
    pub days: u64,
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
    /// A machine moving one of its Docs here (Arugula #863), over
    /// [`AGENT_PROJECT_DOCS`]: kept as the Doc's [`Doc::moved_from`]. Its
    /// `machine` must be the one signing; a person never sends it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "ts", ts(optional))]
    pub moved_from: Option<MovedFrom>,
}

/// `POST` [`PROJECT_DOC_MOVE`]: move a Doc written on the platform to `to`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(rename = "MoveProjectDoc"))]
pub struct DocMove {
    /// The team project (`prj_…`) it moves to.
    pub to: String,
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

/// The 409 to a [`DocMove`] when the Doc is in a team's project already.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(rename = "ProjectDocShared"))]
pub struct Shared {
    pub error: String,
    /// The team Doc that is there.
    pub id: String,
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

/// [`PROJECT_DOC_MOVE`] for `id` in `project`.
pub fn project_doc_move(project: &str, id: &str) -> String {
    format!("/api/projects/{project}/docs/{id}/move")
}

/// [`AGENT_PROJECT_DOCS`] for `project`.
pub fn agent_project_docs(project: &str) -> String {
    format!("/api/daemon/projects/{project}/docs")
}

/// [`AGENT_PROJECT_DOC`] for `doc` in `project`.
pub fn agent_project_doc(project: &str, doc: &str) -> String {
    format!("/api/daemon/projects/{project}/docs/{doc}")
}

/// [`AGENT_PROJECT_DOC_ASSET`] for `name` of `doc` in `project`.
pub fn agent_project_doc_asset(project: &str, doc: &str, name: &str) -> String {
    format!("/api/daemon/projects/{project}/docs/{doc}/assets/{name}")
}

/// [`AGENT_PROJECT_DOC_PUBLIC`] for `doc` in `project`.
pub fn agent_project_doc_public(project: &str, doc: &str) -> String {
    format!("/api/daemon/projects/{project}/docs/{doc}/public")
}

#[cfg(test)]
mod tests {
    use super::*;

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
        let n = NewDoc { title: "T".into(), sections: vec![], layout: None, moved_from: None };
        let n = serde_json::to_value(n).unwrap();
        assert!(n.get("layout").is_none() && n.get("moved_from").is_none());
    }

    #[test]
    fn a_doc_from_a_platform_that_still_had_copies_reads() {
        let v = serde_json::json!({
            "id": "pdc_1", "project": "prj_1", "title": "T", "author": "", "agent": null, "sections": 0,
            "written": 0, "rev": 1, "created_at": 1, "updated_at": 1, "machine": "mch_1",
            "machine_name": "laptop", "made_from": { "project": "prj_0", "id": "pdc_0", "machine_name": null },
        });
        let doc: Doc = serde_json::from_value(v).unwrap();
        assert_eq!(doc.title, "T");
        assert!(serde_json::to_value(&doc).unwrap().get("machine").is_none());
    }
}
