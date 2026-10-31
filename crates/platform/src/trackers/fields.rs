//! What a Plane project offers a work item (Arugula #422): its states,
//! modules, types and labels, and the names a write may use for them.
//!
//! - **The cache** is `tracker_connections.fields` (JSON) and `fields_at`.
//!   The reconcile refreshes it; a write reads it while it is younger than
//!   [`FRESH_MS`], and reads Plane once more when it names something the
//!   cache lacks, since a name just added in Plane shouldn't wait for a
//!   reconcile. A name still unknown after that is a 422 listing the real
//!   ones.
//! - **Matching** is case-insensitive after trimming, by name or by id. A
//!   state may also be named by its group, which means the first state of
//!   it; an exact state name beats a group.

use arugula_control_common::{ApiError, err};
use arugula_control_wire::projects::{TrackerFieldName, TrackerFieldState, TrackerFields};
use arugula_e2e::now_ms;
use axum::http::StatusCode;

use super::{Fields, Named, StateInfo, TrackerError, TrackerProvider};
use crate::{Platform, db::trackers::Connection};

/// How long the kept fields are good for a write.
pub const FRESH_MS: u64 = 600_000;

/// The names a write uses, as it said them.
#[derive(Debug, Default)]
pub struct Wanted<'a> {
    pub state: Option<&'a str>,
    pub module: Option<&'a str>,
    pub item_type: Option<&'a str>,
    pub labels: &'a [String],
    pub labels_remove: &'a [String],
}

impl Wanted<'_> {
    fn is_empty(&self) -> bool {
        self.state.is_none()
            && self.module.is_none()
            && self.item_type.is_none()
            && self.labels.is_empty()
            && self.labels_remove.is_empty()
    }
}

/// What the names came to.
#[derive(Debug, Default)]
pub struct Picked {
    pub state: Option<StateInfo>,
    pub module: Option<Named>,
    pub item_type: Option<Named>,
    pub labels: Vec<String>,
    pub labels_remove: Vec<String>,
}

fn same(a: &str, b: &str) -> bool {
    a.trim().eq_ignore_ascii_case(b.trim())
}

/// A state named by name or id, else by group (its first state).
pub fn state_of<'a>(f: &'a Fields, wanted: &str) -> Option<&'a StateInfo> {
    f.states
        .iter()
        .find(|s| same(&s.name, wanted) || same(&s.id, wanted))
        .or_else(|| f.states.iter().find(|s| same(&s.group, wanted)))
}

/// The state a reopened item goes to: the project's default, else the
/// first `unstarted` one.
pub fn reopen_state(f: &Fields) -> Option<&StateInfo> {
    f.states.iter().find(|s| s.default).or_else(|| f.states.iter().find(|s| s.group == "unstarted"))
}

/// The first state of a group.
pub fn group_state<'a>(f: &'a Fields, group: &str) -> Option<&'a StateInfo> {
    f.states.iter().find(|s| s.group == group)
}

fn named<'a>(list: &'a [Named], wanted: &str) -> Option<&'a Named> {
    list.iter().find(|n| same(&n.name, wanted) || same(&n.id, wanted))
}

/// The refusal for a name the project doesn't have, listing those it does.
fn unknown(what: &str, plural: &str, wanted: &str, identifier: &str, options: &[&str]) -> String {
    if options.is_empty() {
        format!("no {what} {:?} in {identifier}: it has no {plural}", wanted.trim())
    } else {
        format!("no {what} {:?} in {identifier}; {plural}: {}", wanted.trim(), options.join(", "))
    }
}

fn names(list: &[Named]) -> Vec<&str> {
    list.iter().map(|n| n.name.as_str()).collect()
}

/// `w`'s names as ids in `f`, or the sentence that refuses the first one
/// `f` doesn't have.
pub fn pick(f: &Fields, identifier: &str, w: &Wanted) -> Result<Picked, String> {
    let mut p = Picked::default();
    if let Some(s) = w.state {
        let found = state_of(f, s).ok_or_else(|| {
            let options: Vec<&str> = f.states.iter().map(|s| s.name.as_str()).collect();
            unknown("state", "states", s, identifier, &options)
        })?;
        p.state = Some(found.clone());
    }
    if let Some(m) = w.module {
        let found =
            named(&f.modules, m).ok_or_else(|| unknown("module", "modules", m, identifier, &names(&f.modules)))?;
        p.module = Some(found.clone());
    }
    if let Some(t) = w.item_type {
        let found = named(&f.types, t).ok_or_else(|| unknown("type", "types", t, identifier, &names(&f.types)))?;
        p.item_type = Some(found.clone());
    }
    for (asked, into) in [(w.labels, &mut p.labels), (w.labels_remove, &mut p.labels_remove)] {
        for l in asked {
            let found =
                named(&f.labels, l).ok_or_else(|| unknown("label", "labels", l, identifier, &names(&f.labels)))?;
            if !into.contains(&found.id) {
                into.push(found.id.clone());
            }
        }
    }
    Ok(p)
}

fn tracker_error(e: TrackerError) -> ApiError {
    match e {
        TrackerError::Unauthorized => err(
            StatusCode::BAD_GATEWAY,
            "Plane refused this connection's API key: connect the project again with a new one",
        ),
        TrackerError::NotFound => err(StatusCode::NOT_FOUND, "Plane has no such project any more"),
        e => err(StatusCode::BAD_GATEWAY, &format!("couldn't read the project's fields from Plane: {e}")),
    }
}

/// Read the project's fields from Plane and keep them for the connection.
pub async fn refresh(
    app: &Platform,
    connection: &str,
    workspace: &str,
    project_id: &str,
    provider: &dyn TrackerProvider,
) -> Result<Fields, TrackerError> {
    let fields = provider.fields(workspace, project_id).await?;
    let json =
        serde_json::to_string(&fields).map_err(|_| TrackerError::Unavailable("its fields wouldn't keep".into()))?;
    app.db
        .set_tracker_fields(connection, &json, now_ms())
        .map_err(|_| TrackerError::Unavailable("its fields wouldn't keep".into()))?;
    Ok(fields)
}

/// The kept fields when they are fresh.
fn kept(app: &Platform, connection: &str) -> Result<Option<Fields>, ApiError> {
    let Some((json, at)) = app.db.tracker_fields(connection)? else { return Ok(None) };
    Ok(serde_json::from_str(&json).ok().filter(|_| now_ms().saturating_sub(at) < FRESH_MS))
}

/// The connection's fields: the kept ones while fresh, else read now. The
/// flag says they were read in this call.
pub async fn load(app: &Platform, c: &Connection, provider: &dyn TrackerProvider) -> Result<(Fields, bool), ApiError> {
    if let Some(f) = kept(app, &c.id)? {
        return Ok((f, false));
    }
    let f = refresh(app, &c.id, &c.workspace_slug, &c.source_project_id, provider).await.map_err(tracker_error)?;
    Ok((f, true))
}

/// What `find` makes of the connection's fields. A refusal from the kept
/// ones makes one read of Plane, and a second from the read ones is a 422.
pub async fn find<T>(
    app: &Platform,
    c: &Connection,
    provider: &dyn TrackerProvider,
    find: impl Fn(&Fields) -> Result<T, String>,
) -> Result<T, ApiError> {
    let (f, fresh) = load(app, c, provider).await?;
    let first = match find(&f) {
        Ok(found) => return Ok(found),
        Err(why) => why,
    };
    if fresh {
        return Err(err(StatusCode::UNPROCESSABLE_ENTITY, &first));
    }
    let f = refresh(app, &c.id, &c.workspace_slug, &c.source_project_id, provider).await.map_err(tracker_error)?;
    find(&f).map_err(|why| err(StatusCode::UNPROCESSABLE_ENTITY, &why))
}

/// The names `w` says, as ids; nothing is read when it says none.
pub async fn resolve(
    app: &Platform,
    c: &Connection,
    provider: &dyn TrackerProvider,
    w: &Wanted<'_>,
) -> Result<Picked, ApiError> {
    if w.is_empty() {
        return Ok(Picked::default());
    }
    find(app, c, provider, |f| pick(f, &c.identifier, w)).await
}

/// The fields as the wire says them.
pub fn wire(c: &Connection, f: &Fields, read_at: u64) -> TrackerFields {
    let names =
        |list: &[Named]| list.iter().map(|n| TrackerFieldName { id: n.id.clone(), name: n.name.clone() }).collect();
    TrackerFields {
        connection: c.id.clone(),
        identifier: c.identifier.clone(),
        states: f
            .states
            .iter()
            .map(|s| TrackerFieldState {
                id: s.id.clone(),
                name: s.name.clone(),
                group: s.group.clone(),
                default: s.default,
            })
            .collect(),
        modules: names(&f.modules),
        types: names(&f.types),
        labels: names(&f.labels),
        read_at,
    }
}
