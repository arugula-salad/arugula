//! Plane's REST API (v1), `https://developers.plane.so/api-reference`, with
//! the person's API key in `X-API-Key`. What's used is
//!
//! - `GET /api/v1/workspaces/{slug}/projects/` (paged, `results`, `id`,
//!   `identifier`, `name`), read through to find the project by id or
//!   identifier;
//! - `GET /api/v1/workspaces/{slug}/projects/{project}/work-items/`
//!   (`per_page` 100, `expand=state,type`, `order_by=-updated_at`, `cursor`;
//!   `results`, `next_cursor`, `next_page_results`), each item's `id`,
//!   `sequence_id`, `name`, `state` (`name`, `group`), `type` (`name`),
//!   `parent`, `created_at`, `updated_at`;
//! - `GET …/work-items/{item}/` with the same `expand`.
//!
//! The writes (#359), each followed by a read of the item, since Plane's
//! answer to a write doesn't expand the state:
//!
//! - `POST …/work-items/` (`name`, `description_html`, `state`, `parent`,
//!   `type_id`, `labels`);
//! - `POST …/work-items/{item}/comments/` (`comment_html`);
//! - `PATCH …/work-items/{item}/` (`name`, `description_html`, `state`,
//!   `parent`, `type_id`, `labels`), after a read of the item's own labels
//!   when some are added or taken off;
//! - `POST …/modules/{module}/module-issues/` (`issues`: item ids), a call
//!   of its own;
//! - `GET /api/v1/workspaces/{slug}/work-items/{IDENT}-{N}/`, an item by
//!   its number;
//! - `GET …/projects/{project}/` `states/`, `modules/`, `labels/` and
//!   `work-item-types/` (each paged `results`, or a bare array): a
//!   project's [`Fields`]. A project without types answers the last with
//!   404 or an empty list, which is no types.
//!
//! Bodies and comments are HTML, already escaped by the caller; a title is
//! plain text. An answer's body is never put in an error: Plane may echo
//! what it was sent, and what it was sent is the key.

use std::time::Duration;

use reqwest::header::{HeaderMap, HeaderValue};
use serde_json::{Value, json};

use super::{
    BoxFuture, Edit, Fields, Item, Named, NewItem, Page, ProjectInfo, Relations, Result, StateInfo, TrackerError,
    TrackerProvider,
};

/// Items a page asks for: Plane's most.
const PER_PAGE: u32 = 100;
/// Most pages read looking for a project.
const PROJECT_PAGES: usize = 20;
/// Most pages read of one list of a project's fields.
const FIELD_PAGES: usize = 5;
/// How long Plane has to answer one request.
const TIMEOUT: Duration = Duration::from_secs(30);

/// Plane at `base`, for one key. `Debug` hides the key.
pub struct Plane {
    http: reqwest::Client,
    base: String,
    key: HeaderValue,
}

impl std::fmt::Debug for Plane {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Plane").field("base", &self.base).field("key", &"<redacted>").finish()
    }
}

/// A slug, id or identifier as a path segment can carry it.
fn segment(s: &str) -> Result<&str> {
    let ok = !s.is_empty() && s.len() <= 128 && s.bytes().all(|b| b.is_ascii_alphanumeric() || b"-_.".contains(&b));
    if ok && s != "." && s != ".." { Ok(s) } else { Err(TrackerError::NotFound) }
}

fn millis(v: &Value) -> u64 {
    v.as_str().and_then(|s| s.parse::<jiff::Timestamp>().ok()).map_or(0, |t| t.as_millisecond().max(0) as u64)
}

/// A work item as Plane has it, with its state expanded.
fn item(v: &Value) -> Result<Item> {
    let odd = || TrackerError::Unavailable("Plane's work item wasn't what was expected".into());
    let state = v["state"]
        .as_object()
        .ok_or_else(|| TrackerError::Unavailable("Plane didn't expand an item's state".into()))?;
    Ok(Item {
        id: v["id"].as_str().ok_or_else(odd)?.to_owned(),
        sequence_id: v["sequence_id"].as_u64().ok_or_else(odd)?,
        title: v["name"].as_str().unwrap_or_default().to_owned(),
        item_type: v["type"]["name"].as_str().map(str::to_owned),
        state_name: state.get("name").and_then(Value::as_str).unwrap_or_default().to_owned(),
        state_group: state.get("group").and_then(Value::as_str).ok_or_else(odd)?.to_owned(),
        parent_id: v["parent"].as_str().map(str::to_owned),
        created_at: millis(&v["created_at"]),
        updated_at: millis(&v["updated_at"]),
    })
}

impl Plane {
    pub fn new(http: reqwest::Client, base: &str, key: &str) -> Self {
        // An invalid header value (a newline) is a key Plane would refuse.
        let mut key = HeaderValue::from_str(key).unwrap_or_else(|_| HeaderValue::from_static(""));
        key.set_sensitive(true);
        Plane { http, base: base.trim_end_matches('/').to_owned(), key }
    }

    async fn get(&self, path: &str, query: &[(&str, String)]) -> Result<Value> {
        self.send(reqwest::Method::GET, path, query, None).await
    }

    async fn send(
        &self,
        method: reqwest::Method,
        path: &str,
        query: &[(&str, String)],
        body: Option<Value>,
    ) -> Result<Value> {
        let mut headers = HeaderMap::new();
        headers.insert("x-api-key", self.key.clone());
        let mut url = url::Url::parse(&format!("{}{path}", self.base))
            .map_err(|_| TrackerError::Unavailable("the tracker's address isn't one".into()))?;
        url.query_pairs_mut().extend_pairs(query.iter().map(|(k, v)| (*k, v.as_str())));
        let mut req = self.http.request(method, url).headers(headers).timeout(TIMEOUT);
        if let Some(b) = &body {
            req = req.json(b);
        }
        let r = req.send().await.map_err(|e| {
            let why = if e.is_timeout() { "it didn't answer in time" } else { "the connection failed" };
            TrackerError::Unavailable(why.into())
        })?;
        match r.status().as_u16() {
            200 | 201 => {
                r.json().await.map_err(|_| TrackerError::Unavailable("it answered with something unexpected".into()))
            }
            401 | 403 => Err(TrackerError::Unauthorized),
            404 => Err(TrackerError::NotFound),
            s => Err(TrackerError::Unavailable(format!("it answered {s}"))),
        }
    }

    /// The work item `id`, state expanded.
    async fn read(&self, workspace: &str, project_id: &str, id: &str) -> Result<Item> {
        let path = format!(
            "/api/v1/workspaces/{}/projects/{}/work-items/{}/",
            segment(workspace)?,
            segment(project_id)?,
            segment(id)?
        );
        item(&self.get(&path, &[("expand", "state,type".into())]).await?)
    }

    /// Change a work item, then read it.
    async fn patch(&self, workspace: &str, project_id: &str, id: &str, body: Value) -> Result<Item> {
        let path = format!(
            "/api/v1/workspaces/{}/projects/{}/work-items/{}/",
            segment(workspace)?,
            segment(project_id)?,
            segment(id)?
        );
        self.send(reqwest::Method::PATCH, &path, &[], Some(body)).await?;
        self.read(workspace, project_id, id).await
    }

    /// Every entry of a project's list (`states`, `labels`…): paged
    /// `results`, or a bare array.
    async fn list_all(&self, workspace: &str, project_id: &str, what: &str) -> Result<Vec<Value>> {
        let path = format!("/api/v1/workspaces/{}/projects/{}/{what}/", segment(workspace)?, segment(project_id)?);
        let mut cursor: Option<String> = None;
        let mut all = vec![];
        for _ in 0..FIELD_PAGES {
            let mut q = vec![("per_page", PER_PAGE.to_string())];
            q.extend(cursor.take().map(|c| ("cursor", c)));
            let v = self.get(&path, &q).await?;
            match v.as_array() {
                Some(bare) => {
                    all.extend(bare.iter().cloned());
                    break;
                }
                None => all.extend(v["results"].as_array().into_iter().flatten().cloned()),
            }
            cursor = next(&v);
            if cursor.is_none() {
                break;
            }
        }
        Ok(all)
    }

    /// The ids of the labels an item has.
    async fn labels_of(&self, workspace: &str, project_id: &str, id: &str) -> Result<Vec<String>> {
        let path = format!(
            "/api/v1/workspaces/{}/projects/{}/work-items/{}/",
            segment(workspace)?,
            segment(project_id)?,
            segment(id)?
        );
        let v = self.get(&path, &[]).await?;
        let label = |l: &Value| l.as_str().or_else(|| l["id"].as_str()).map(str::to_owned);
        Ok(v["labels"].as_array().into_iter().flatten().filter_map(label).collect())
    }
}

/// An item's labels with some added and some taken off.
fn merged_labels(mut current: Vec<String>, add: &[String], remove: &[String]) -> Vec<String> {
    for l in add {
        if !current.contains(l) {
            current.push(l.clone());
        }
    }
    current.retain(|l| !remove.contains(l));
    current
}

/// The ids and names in a list of a project's fields.
fn named(all: &[Value]) -> Vec<Named> {
    let one = |v: &Value| Some(Named { id: v["id"].as_str()?.to_owned(), name: v["name"].as_str()?.to_owned() });
    all.iter().filter_map(one).collect()
}

/// `next_cursor`, when the answer says another page follows.
fn next(v: &Value) -> Option<String> {
    if v["next_page_results"].as_bool() == Some(true) { v["next_cursor"].as_str().map(str::to_owned) } else { None }
}

impl TrackerProvider for Plane {
    fn project<'a>(&'a self, workspace: &'a str, reference: &'a str) -> BoxFuture<'a, Result<ProjectInfo>> {
        Box::pin(async move {
            let path = format!("/api/v1/workspaces/{}/projects/", segment(workspace)?);
            let mut cursor: Option<String> = None;
            for _ in 0..PROJECT_PAGES {
                let mut q = vec![("per_page", PER_PAGE.to_string())];
                q.extend(cursor.take().map(|c| ("cursor", c)));
                let v = self.get(&path, &q).await?;
                for p in v["results"].as_array().into_iter().flatten() {
                    let (id, identifier) = (p["id"].as_str(), p["identifier"].as_str());
                    if let (Some(id), Some(identifier)) = (id, identifier)
                        && (id == reference || identifier.eq_ignore_ascii_case(reference))
                    {
                        let name = p["name"].as_str().unwrap_or(identifier);
                        return Ok(ProjectInfo { id: id.into(), identifier: identifier.into(), name: name.into() });
                    }
                }
                cursor = next(&v);
                if cursor.is_none() {
                    break;
                }
            }
            Err(TrackerError::NotFound)
        })
    }

    fn list<'a>(
        &'a self,
        workspace: &'a str,
        project_id: &'a str,
        since: Option<u64>,
        cursor: Option<&'a str>,
    ) -> BoxFuture<'a, Result<Page>> {
        Box::pin(async move {
            let path =
                format!("/api/v1/workspaces/{}/projects/{}/work-items/", segment(workspace)?, segment(project_id)?);
            let mut q = vec![
                ("per_page", PER_PAGE.to_string()),
                ("expand", "state,type".into()),
                ("order_by", "-updated_at".into()),
            ];
            q.extend(cursor.map(|c| ("cursor", c.to_owned())));
            let v = self.get(&path, &q).await?;
            let mut items = v["results"].as_array().into_iter().flatten().map(item).collect::<Result<Vec<_>>>()?;
            let mut more = next(&v);
            if let Some(since) = since {
                // Newest first: the first one that's old ends the read.
                if let Some(i) = items.iter().position(|i| i.updated_at <= since) {
                    items.truncate(i);
                    more = None;
                }
            }
            Ok(Page { items, next: more })
        })
    }

    fn get<'a>(&'a self, workspace: &'a str, project_id: &'a str, id: &'a str) -> BoxFuture<'a, Result<Item>> {
        Box::pin(self.read(workspace, project_id, id))
    }

    fn create<'a>(&'a self, workspace: &'a str, project_id: &'a str, new: &'a NewItem) -> BoxFuture<'a, Result<Item>> {
        Box::pin(async move {
            let path =
                format!("/api/v1/workspaces/{}/projects/{}/work-items/", segment(workspace)?, segment(project_id)?);
            let mut body = json!({ "name": new.title });
            if let Some(h) = &new.body {
                body["description_html"] = json!(h);
            }
            if let Some(s) = &new.state {
                body["state"] = json!(segment(s)?);
            }
            if let Some(t) = &new.item_type {
                body["type_id"] = json!(segment(t)?);
            }
            if let Some(p) = &new.parent_id {
                body["parent"] = json!(segment(p)?);
            }
            if !new.labels.is_empty() {
                body["labels"] = json!(new.labels.iter().map(|l| segment(l)).collect::<Result<Vec<_>>>()?);
            }
            let made = self.send(reqwest::Method::POST, &path, &[], Some(body)).await?;
            let id = made["id"]
                .as_str()
                .ok_or_else(|| TrackerError::Unavailable("it answered with something unexpected".into()))?;
            self.read(workspace, project_id, id).await
        })
    }

    fn comment<'a>(
        &'a self,
        workspace: &'a str,
        project_id: &'a str,
        id: &'a str,
        html: &'a str,
    ) -> BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            let path = format!(
                "/api/v1/workspaces/{}/projects/{}/work-items/{}/comments/",
                segment(workspace)?,
                segment(project_id)?,
                segment(id)?
            );
            self.send(reqwest::Method::POST, &path, &[], Some(json!({ "comment_html": html }))).await.map(|_| ())
        })
    }

    fn set_state<'a>(
        &'a self,
        workspace: &'a str,
        project_id: &'a str,
        id: &'a str,
        state: &'a str,
    ) -> BoxFuture<'a, Result<Item>> {
        Box::pin(async move { self.patch(workspace, project_id, id, json!({ "state": segment(state)? })).await })
    }

    fn edit<'a>(
        &'a self,
        workspace: &'a str,
        project_id: &'a str,
        id: &'a str,
        edit: &'a Edit,
    ) -> BoxFuture<'a, Result<Item>> {
        Box::pin(async move {
            let mut body = json!({});
            if let Some(t) = &edit.title {
                body["name"] = json!(t);
            }
            if let Some(h) = &edit.body {
                body["description_html"] = json!(h);
            }
            if let Some(s) = &edit.state {
                body["state"] = json!(segment(s)?);
            }
            if let Some(t) = &edit.item_type {
                body["type_id"] = json!(segment(t)?);
            }
            if let Some(p) = &edit.parent_id {
                body["parent"] = json!(segment(p)?);
            }
            if !edit.labels_add.is_empty() || !edit.labels_remove.is_empty() {
                // Plane sets an item's labels whole: the union, less those taken off.
                let current = self.labels_of(workspace, project_id, id).await?;
                let labels = merged_labels(current, &edit.labels_add, &edit.labels_remove);
                body["labels"] = json!(labels.iter().map(|l| segment(l)).collect::<Result<Vec<_>>>()?);
            }
            self.patch(workspace, project_id, id, body).await
        })
    }

    fn fields<'a>(&'a self, workspace: &'a str, project_id: &'a str) -> BoxFuture<'a, Result<Fields>> {
        Box::pin(async move {
            let mut states: Vec<(f64, StateInfo)> = vec![];
            for s in self.list_all(workspace, project_id, "states").await? {
                let (Some(id), Some(name), Some(group)) = (s["id"].as_str(), s["name"].as_str(), s["group"].as_str())
                else {
                    continue;
                };
                let info = StateInfo {
                    id: id.into(),
                    name: name.into(),
                    group: group.into(),
                    default: s["default"].as_bool() == Some(true),
                };
                states.push((s["sequence"].as_f64().unwrap_or(f64::MAX), info));
            }
            states.sort_by(|a, b| a.0.total_cmp(&b.0));
            let modules = named(&self.list_all(workspace, project_id, "modules").await?);
            let labels = named(&self.list_all(workspace, project_id, "labels").await?);
            let types = match self.list_all(workspace, project_id, "work-item-types").await {
                Ok(all) => {
                    named(&all.into_iter().filter(|t| t["is_active"].as_bool() != Some(false)).collect::<Vec<_>>())
                }
                // Types aren't enabled for the project.
                Err(TrackerError::NotFound) => vec![],
                Err(e) => return Err(e),
            };
            Ok(Fields { states: states.into_iter().map(|(_, s)| s).collect(), modules, types, labels })
        })
    }

    fn add_to_module<'a>(
        &'a self,
        workspace: &'a str,
        project_id: &'a str,
        module: &'a str,
        id: &'a str,
    ) -> BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            let path = format!(
                "/api/v1/workspaces/{}/projects/{}/modules/{}/module-issues/",
                segment(workspace)?,
                segment(project_id)?,
                segment(module)?
            );
            self.send(reqwest::Method::POST, &path, &[], Some(json!({ "issues": [segment(id)?] }))).await.map(|_| ())
        })
    }

    fn by_sequence<'a>(
        &'a self,
        workspace: &'a str,
        project_id: &'a str,
        identifier: &'a str,
        sequence: u64,
    ) -> BoxFuture<'a, Result<Item>> {
        Box::pin(async move {
            let path =
                format!("/api/v1/workspaces/{}/work-items/{}-{sequence}/", segment(workspace)?, segment(identifier)?);
            let v = self.get(&path, &[("expand", "state,type".into())]).await?;
            // The identifier names a project: an item of another one isn't here.
            if v["project"].as_str().is_some_and(|p| p != project_id) {
                return Err(TrackerError::NotFound);
            }
            item(&v)
        })
    }

    /// Plane's `GET …/work-items/{id}/relations/`: arrays of work item ids
    /// under `blocked_by` (the items that block this one) and `blocking`
    /// (the items this one blocks).
    fn relations<'a>(
        &'a self,
        workspace: &'a str,
        project_id: &'a str,
        id: &'a str,
    ) -> BoxFuture<'a, Result<Relations>> {
        Box::pin(async move {
            let path = format!(
                "/api/v1/workspaces/{}/projects/{}/work-items/{}/relations/",
                segment(workspace)?,
                segment(project_id)?,
                segment(id)?
            );
            let v = self.get(&path, &[]).await?;
            let ids = |k: &str| -> Vec<String> {
                v[k].as_array().into_iter().flatten().filter_map(Value::as_str).map(str::to_owned).collect()
            };
            Ok(Relations { blocked_by: ids("blocked_by"), blocking: ids("blocking") })
        })
    }

    fn children<'a>(&'a self, _: &'a str, _: &'a str, _: &'a str) -> BoxFuture<'a, Result<Vec<Item>>> {
        Box::pin(async { Err(TrackerError::NotYet("reading a Plane epic's children")) })
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn a_work_item_as_plane_sends_it() {
        let v = json!({
            "id": "u1", "sequence_id": 7, "name": "Fix it", "parent": "u0",
            "state": { "name": "Done", "group": "completed" }, "type": { "name": "Bug" },
            "created_at": "2026-10-01T10:00:00Z", "updated_at": "2026-10-02T10:00:00.5Z",
        });
        let i = item(&v).unwrap();
        assert_eq!(
            (i.sequence_id, i.state(), i.item_type.as_deref(), i.parent_id.as_deref()),
            (7, "closed", Some("Bug"), Some("u0"))
        );
        assert_eq!(i.updated_at - i.created_at, 86_400_500);
        // Not expanded: no guessing at its state.
        assert!(item(&json!({ "id": "u1", "sequence_id": 7, "state": "uuid" })).is_err());
    }

    #[test]
    fn labels_are_the_union_less_those_taken_off() {
        let v = |l: &[&str]| l.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert_eq!(merged_labels(v(&["a", "b"]), &v(&["b", "c"]), &v(&[])), v(&["a", "b", "c"]));
        assert_eq!(merged_labels(v(&["a", "b"]), &v(&["c"]), &v(&["a"])), v(&["b", "c"]));
    }

    #[test]
    fn the_key_stays_out_of_debug_and_paths_are_checked() {
        let p = Plane::new(reqwest::Client::new(), "http://x", "plane-key-for-test");
        assert!(!format!("{p:?}").contains("plane-key-for-test"));
        assert!(segment("my-team").is_ok());
        assert!(segment("a/b").is_err() && segment("..").is_err() && segment("").is_err());
    }
}
