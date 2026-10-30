//! Plane's REST API (v1), `https://developers.plane.so/api-reference`, with
//! the person's API key in `X-API-Key`. Reads only (#356): what's used is
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
//! The writes are #359's. An answer's body is never put in an error: Plane
//! may echo what it was sent, and what it was sent is the key.

use std::time::Duration;

use reqwest::header::{HeaderMap, HeaderValue};
use serde_json::Value;

use super::{BoxFuture, Edit, Item, NewItem, Page, ProjectInfo, Result, TrackerError, TrackerProvider};

/// Items a page asks for: Plane's most.
const PER_PAGE: u32 = 100;
/// Most pages read looking for a project.
const PROJECT_PAGES: usize = 20;
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
        let mut headers = HeaderMap::new();
        headers.insert("x-api-key", self.key.clone());
        let mut url = url::Url::parse(&format!("{}{path}", self.base))
            .map_err(|_| TrackerError::Unavailable("the tracker's address isn't one".into()))?;
        url.query_pairs_mut().extend_pairs(query.iter().map(|(k, v)| (*k, v.as_str())));
        let r = self.http.get(url).headers(headers).timeout(TIMEOUT).send().await.map_err(|e| {
            let why = if e.is_timeout() { "it didn't answer in time" } else { "the connection failed" };
            TrackerError::Unavailable(why.into())
        })?;
        match r.status().as_u16() {
            200 => {
                r.json().await.map_err(|_| TrackerError::Unavailable("it answered with something unexpected".into()))
            }
            401 | 403 => Err(TrackerError::Unauthorized),
            404 => Err(TrackerError::NotFound),
            s => Err(TrackerError::Unavailable(format!("it answered {s}"))),
        }
    }
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
        Box::pin(async move {
            let path = format!(
                "/api/v1/workspaces/{}/projects/{}/work-items/{}/",
                segment(workspace)?,
                segment(project_id)?,
                segment(id)?
            );
            item(&self.get(&path, &[("expand", "state,type".into())]).await?)
        })
    }

    fn create<'a>(&'a self, _: &'a str, _: &'a str, _: &'a NewItem) -> BoxFuture<'a, Result<Item>> {
        Box::pin(async { Err(TrackerError::NotYet("creating a Plane work item")) })
    }

    fn comment<'a>(&'a self, _: &'a str, _: &'a str, _: &'a str, _: &'a str) -> BoxFuture<'a, Result<()>> {
        Box::pin(async { Err(TrackerError::NotYet("commenting on a Plane work item")) })
    }

    fn set_state<'a>(&'a self, _: &'a str, _: &'a str, _: &'a str, _: &'a str) -> BoxFuture<'a, Result<Item>> {
        Box::pin(async { Err(TrackerError::NotYet("changing a Plane work item's state")) })
    }

    fn edit<'a>(&'a self, _: &'a str, _: &'a str, _: &'a str, _: &'a Edit) -> BoxFuture<'a, Result<Item>> {
        Box::pin(async { Err(TrackerError::NotYet("editing a Plane work item")) })
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
    fn the_key_stays_out_of_debug_and_paths_are_checked() {
        let p = Plane::new(reqwest::Client::new(), "http://x", "plane-key-for-test");
        assert!(!format!("{p:?}").contains("plane-key-for-test"));
        assert!(segment("my-team").is_ok());
        assert!(segment("a/b").is_err() && segment("..").is_err() && segment("").is_err());
    }
}
