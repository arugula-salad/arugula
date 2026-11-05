//! A machine's Docs copied up to the platform (#418): the routes a machine
//! signs, acting for its owner's account. A copy is keyed by the machine and
//! its own Doc id, so the same machine Doc always lands on the same `pdc_`;
//! the first put makes it in the owner's personal project, which keeps it
//! private to them. Later puts write it wherever it is now (a copy moved
//! into a team's project before the move route went, native #673, stays
//! there), as long as the owner may still write there; if not, nothing
//! changes and the machine is told why.
//!
//! A team's machine copies nothing (native #673): its puts and pictures are
//! refused ([`TEAM_MACHINE`]), whatever daemon it runs; its take-downs
//! still go. Copies it made before stay where they are.
//!
//! No grant is checked: a machine copying into its own owner's personal
//! project needs none. Chat isn't needed either: a put takes chat or the
//! account's own word for copies (native #673), and taking a copy down
//! takes neither. A machine reaches only copies it made, for the
//! machine is in the key: another machine's `machine_doc` is a Doc of its own
//! here, never the first one's.
//!
//! Pictures are kept as a machine Doc keeps them (`doc asset`): PNG, JPEG,
//! GIF or WebP by what the bytes say, the name agreeing with them, at most
//! 2 MB each and 20 to a Doc.

use std::sync::Arc;

use arugula_control_common::{ApiError, err};
use arugula_control_wire::project_docs as wire;
use arugula_e2e::now_ms;
use axum::{
    Json,
    body::Bytes,
    extract::{Path, State},
    http::StatusCode,
};
use serde_json::{Value, json};

use crate::{
    Platform,
    auth::Machine,
    db::project_docs::{self as db, Stored},
    issues::PERSONAL,
    project_docs::{Gate, bad, changed, heading_ok, intent_ok, link, no_doc, place_gated, refused, title_ok},
};

/// Copies a machine puts, and pictures, per hour: a machine puts again when
/// a Doc changes, a few seconds apart at most.
const DOC_PUTS: (&str, usize) = ("doc-put", 2_400);
const ASSET_PUTS: (&str, usize) = ("doc-asset-put", 1_200);

type J<T> = Result<Json<T>, ApiError>;

/// A machine's Doc id or section id: what its Docs are named with.
fn plain(what: &str, s: &str, max: usize) -> Result<(), ApiError> {
    let ok = (1..=max).contains(&s.len()) && s.bytes().all(|c| c.is_ascii_alphanumeric() || matches!(c, b'_' | b'-'));
    ok.then_some(()).ok_or_else(|| bad(&format!("a {what} is 1 to {max} letters, digits, _ or -")))
}

/// The picture types by name, as a machine Doc's.
const EXTENSIONS: [(&str, &str); 5] =
    [("png", "image/png"), ("jpg", "image/jpeg"), ("jpeg", "image/jpeg"), ("gif", "image/gif"), ("webp", "image/webp")];

/// What the bytes say a picture is, if it's one we keep.
fn sniff(bytes: &[u8]) -> Option<&'static str> {
    match bytes {
        [0x89, b'P', b'N', b'G', ..] => Some("image/png"),
        [0xff, 0xd8, 0xff, ..] => Some("image/jpeg"),
        [b'G', b'I', b'F', b'8', ..] => Some("image/gif"),
        [b'R', b'I', b'F', b'F', _, _, _, _, b'W', b'E', b'B', b'P', ..] => Some("image/webp"),
        _ => None,
    }
}

/// A picture's name and bytes, and the type they agree on, or why not.
fn picture(name: &str, bytes: &[u8]) -> Result<&'static str, ApiError> {
    let b = name.as_bytes();
    let plain_name = (1..=100).contains(&b.len())
        && b[0].is_ascii_alphanumeric()
        && b.iter().all(|c| c.is_ascii_alphanumeric() || matches!(c, b'_' | b'.' | b'-'));
    if !plain_name {
        return Err(bad(&format!(
            "picture name {name:?}: use 1 to 100 letters, digits, _ . or -, starting with a letter or digit (no slash)"
        )));
    }
    if name.to_ascii_lowercase().ends_with(".tmp") {
        return Err(bad(&format!("picture name {name:?} can't end in .tmp")));
    }
    let named = name.rsplit_once('.').and_then(|(_, e)| EXTENSIONS.iter().find(|(x, _)| *x == e)).map(|(_, m)| *m);
    let Some(named) = named else {
        return Err(bad(&format!("picture name {name:?} must end in .png .jpg .jpeg .gif or .webp (lowercase)")));
    };
    if bytes.len() > wire::ASSET_MAX {
        return Err(err(
            StatusCode::PAYLOAD_TOO_LARGE,
            &format!("{name} is {} bytes; a picture is {} (2 MB) at most: shrink it", bytes.len(), wire::ASSET_MAX),
        ));
    }
    let Some(mime) = sniff(bytes) else {
        return Err(bad(&format!("{name} isn't a PNG, JPEG, GIF or WebP image (the bytes say what a file is)")));
    };
    if mime != named {
        return Err(bad(&format!("{name} holds a {} image: name it for that", mime.trim_start_matches("image/"))));
    }
    Ok(mime)
}

/// Where the machine's copy `machine_doc` is, and that its owner may still
/// write there: `None` when it has none yet. The owner may have left the
/// team the Doc was moved to, or been made a viewer: that's a 403 saying so,
/// and the copy stays as it is.
async fn copy_for_owner(
    app: &Platform,
    d: &Machine,
    machine_doc: &str,
    gate: Gate,
) -> Result<Option<(Stored, crate::project_docs::Place)>, ApiError> {
    let Some(doc) = app.db.project_doc_of_machine(&d.daemon, machine_doc)? else { return Ok(None) };
    // The word for copies is for the owner's own project alone: a copy
    // sitting in a team's project goes by the team's chat.
    let personal = crate::projects::personal_project(app, &d.account).await?;
    let gate = match gate {
        Gate::Copies if personal.as_deref() != Some(doc.project.as_str()) => Gate::Chat,
        g => g,
    };
    match place_gated(app, &d.account, &doc.project, true, gate).await {
        Ok(p) => Ok(Some((doc, p))),
        Err(ApiError(StatusCode::FORBIDDEN | StatusCode::NOT_FOUND, ..)) => Err(err(
            StatusCode::FORBIDDEN,
            "this Doc now sits in a project where its owner may no longer write: the copy there is left as it is",
        )),
        Err(e) => Err(e),
    }
}

/// Why a team's machine's copy isn't taken (native #673).
pub const TEAM_MACHINE: &str = "a team's machine doesn't copy its Docs to the platform: they stay on the machine";

/// A team's machine copies no Docs up (native #673), whatever daemon it
/// runs: refused here, so the rule holds from the platform's deploy on.
/// An older daemon logs it once and tries again after its next refresh.
/// Its take-downs still go, so an old copy can come down.
async fn not_a_team_machine(app: &Platform, d: &Machine) -> Result<(), ApiError> {
    let machines = app.cache.daemons(&*app.hub, std::slice::from_ref(&d.daemon)).await?;
    if machines.get(&d.daemon).is_some_and(|m| m.team.is_some()) {
        return Err(err(StatusCode::FORBIDDEN, TEAM_MACHINE));
    }
    Ok(())
}

/// `PUT /api/daemon/docs/{machine_doc}`: the whole Doc, as the machine has
/// it now.
pub async fn put_route(
    State(app): State<Arc<Platform>>,
    d: Machine,
    Path(machine_doc): Path<String>,
    Json(doc): Json<wire::MachineDoc>,
) -> J<wire::MachineDocPlaced> {
    app.limits.check_daemon(DOC_PUTS, &d.daemon)?;
    not_a_team_machine(&app, &d).await?;
    plain("Doc id", &machine_doc, 64)?;
    let title = title_ok(&doc.title)?;
    let mut kept = Vec::with_capacity(doc.sections.len());
    for (i, s) in doc.sections.iter().enumerate() {
        plain("section id", &s.id, 40)?;
        if doc.sections[..i].iter().any(|o| o.id == s.id) {
            return Err(bad(&format!("section {} is there twice", s.id)));
        }
        kept.push((heading_ok(&s.heading)?, intent_ok(&s.intent)?));
    }
    let sections: Vec<db::MachineSection> = doc
        .sections
        .iter()
        .zip(&kept)
        .map(|(s, (h, i))| db::MachineSection { id: &s.id, heading: h, intent: i, body: s.body.as_deref() })
        .collect();
    let project = match copy_for_owner(&app, &d, &machine_doc, Gate::Copies).await? {
        Some((_, p)) => p.project,
        None => place_gated(&app, &d.account, PERSONAL, true, Gate::Copies).await?.project,
    };
    let whole = db::MachineDoc { title: &title, sections: &sections };
    let (stored, was_changed) = app
        .db
        .put_machine_doc(
            db::MachineKey { machine: &d.daemon, doc: &machine_doc },
            &project,
            &d.account,
            &whole,
            now_ms(),
            wire::DOCS_MAX,
        )?
        .map_err(refused)?;
    if was_changed {
        changed(&app, &stored.project, &stored.id);
    }
    Ok(Json(wire::MachineDocPlaced {
        link: link(&app, &stored.project, &stored.id),
        project: stored.project,
        id: stored.id,
        rev: stored.rev,
    }))
}

/// `PUT /api/daemon/docs/{machine_doc}/assets/{name}`: a picture's bytes.
/// The Doc is put first: a picture has to belong to one.
pub async fn asset_route(
    State(app): State<Arc<Platform>>,
    d: Machine,
    Path((machine_doc, name)): Path<(String, String)>,
    body: Bytes,
) -> J<wire::Asset> {
    app.limits.check_daemon(ASSET_PUTS, &d.daemon)?;
    not_a_team_machine(&app, &d).await?;
    plain("Doc id", &machine_doc, 64)?;
    let mime = picture(&name, &body)?;
    let Some((doc, _)) = copy_for_owner(&app, &d, &machine_doc, Gate::Copies).await? else {
        return Err(err(StatusCode::NOT_FOUND, "put the Doc before its pictures"));
    };
    let (a, was_changed) = app.db.put_project_doc_asset(&doc.id, &name, mime, &body, now_ms())?.map_err(refused)?;
    if was_changed {
        changed(&app, &doc.project, &doc.id);
    }
    Ok(Json(wire::Asset { name: a.name, mime: a.mime, size: a.size, rev: a.rev }))
}

/// `DELETE /api/daemon/docs/{machine_doc}`: the machine takes its copy down,
/// wherever it is now (the owner must still be able to write there).
pub async fn delete_route(State(app): State<Arc<Platform>>, d: Machine, Path(machine_doc): Path<String>) -> J<Value> {
    app.limits.check_daemon(DOC_PUTS, &d.daemon)?;
    plain("Doc id", &machine_doc, 64)?;
    // A machine takes its own copy down whatever chat or the word say.
    let Some((doc, _)) = copy_for_owner(&app, &d, &machine_doc, Gate::None).await? else { return Err(no_doc()) };
    app.db.delete_project_doc(&doc.id)?;
    changed(&app, &doc.project, &doc.id);
    Ok(Json(json!({ "deleted": doc.id })))
}

#[cfg(test)]
#[path = "project_docs_machine_tests.rs"]
mod tests;
