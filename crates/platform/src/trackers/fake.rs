//! A tracker that keeps its items in memory, for tests: one workspace, one
//! project, and the one key it accepts. [`FakeTrackers`] hands it out as the
//! platform's [`Trackers`].

use std::sync::{Arc, Mutex};

use super::{BoxFuture, Edit, Item, NewItem, Page, ProjectInfo, Result, TrackerError, TrackerProvider, Trackers};

struct State {
    key: String,
    workspace: String,
    project: ProjectInfo,
    items: Vec<Item>,
    /// Items a page holds.
    page: usize,
    /// How many times a key was refused.
    refused: usize,
    /// What writes were sent, in order.
    sent: Vec<Sent>,
}

/// A write the fake was sent, as it arrived: a body or comment is the HTML
/// the platform sent, a title the plain text.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Sent {
    /// `create`, `comment`, `state` or `edit`.
    pub kind: String,
    /// The tracker's id of the item written to; none for a create.
    pub item: Option<String>,
    pub title: Option<String>,
    pub html: Option<String>,
    /// The state group, for a create or a state.
    pub group: Option<String>,
}

/// The fake tracker's world; clones share it.
#[derive(Clone)]
pub struct FakeTracker {
    state: Arc<Mutex<State>>,
}

/// The fake as the platform's [`Trackers`].
pub struct FakeTrackers(pub FakeTracker);

/// What a key opens: the fake, as that key.
struct Keyed {
    tracker: FakeTracker,
    key: String,
}

impl FakeTracker {
    pub fn new(key: &str, workspace: &str, project: ProjectInfo, items: Vec<Item>) -> Self {
        let state =
            State { key: key.into(), workspace: workspace.into(), project, items, page: 2, refused: 0, sent: vec![] };
        FakeTracker { state: Arc::new(Mutex::new(state)) }
    }

    pub fn set_items(&self, items: Vec<Item>) {
        self.state.lock().unwrap().items = items;
    }

    /// The writes sent so far.
    pub fn sent(&self) -> Vec<Sent> {
        self.state.lock().unwrap().sent.clone()
    }

    /// The one key the fake accepts from now on.
    pub fn set_key(&self, key: &str) {
        self.state.lock().unwrap().key = key.into();
    }

    /// Keys refused so far.
    pub fn refused(&self) -> usize {
        self.state.lock().unwrap().refused
    }
}

/// A work item for a test.
pub fn item(sequence_id: u64, title: &str, group: &str, updated_at: u64) -> Item {
    Item {
        id: format!("item-{sequence_id}"),
        sequence_id,
        title: title.into(),
        item_type: Some("Task".into()),
        state_name: group.to_owned(),
        state_group: group.into(),
        parent_id: None,
        created_at: 1_000,
        updated_at,
    }
}

impl Trackers for FakeTrackers {
    fn plane(&self, key: &str) -> Arc<dyn TrackerProvider> {
        Arc::new(Keyed { tracker: self.0.clone(), key: key.into() })
    }
}

impl Keyed {
    /// The state, if the key and the workspace are right.
    fn check(&self, workspace: &str) -> Result<std::sync::MutexGuard<'_, State>> {
        let mut s = self.tracker.state.lock().unwrap();
        if s.key != self.key {
            s.refused += 1;
            return Err(TrackerError::Unauthorized);
        }
        if s.workspace != workspace {
            return Err(TrackerError::NotFound);
        }
        Ok(s)
    }
}

impl TrackerProvider for Keyed {
    fn project<'a>(&'a self, workspace: &'a str, reference: &'a str) -> BoxFuture<'a, Result<ProjectInfo>> {
        Box::pin(async move {
            let s = self.check(workspace)?;
            if reference == s.project.id || reference.eq_ignore_ascii_case(&s.project.identifier) {
                Ok(s.project.clone())
            } else {
                Err(TrackerError::NotFound)
            }
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
            let s = self.check(workspace)?;
            if project_id != s.project.id {
                return Err(TrackerError::NotFound);
            }
            let mut all: Vec<Item> =
                s.items.iter().filter(|i| since.is_none_or(|t| i.updated_at > t)).cloned().collect();
            all.sort_by_key(|i| std::cmp::Reverse(i.updated_at));
            let from: usize = cursor.and_then(|c| c.parse().ok()).unwrap_or(0);
            let items: Vec<Item> = all.iter().skip(from).take(s.page).cloned().collect();
            let next = (from + s.page < all.len()).then(|| (from + s.page).to_string());
            Ok(Page { items, next })
        })
    }

    fn get<'a>(&'a self, workspace: &'a str, _: &'a str, item: &'a str) -> BoxFuture<'a, Result<Item>> {
        Box::pin(async move {
            let s = self.check(workspace)?;
            s.items.iter().find(|i| i.id == item).cloned().ok_or(TrackerError::NotFound)
        })
    }

    fn create<'a>(&'a self, workspace: &'a str, _: &'a str, new: &'a NewItem) -> BoxFuture<'a, Result<Item>> {
        Box::pin(async move {
            let mut s = self.check(workspace)?;
            let n = s.items.iter().map(|i| i.sequence_id).max().unwrap_or(0) + 1;
            let group = new.state_group.clone().unwrap_or_else(|| "backlog".into());
            let mut made = item(n, &new.title, &group, 5_000_000_000_000);
            made.parent_id = new.parent_id.clone();
            s.items.push(made.clone());
            s.sent.push(Sent {
                kind: "create".into(),
                title: Some(new.title.clone()),
                html: new.body.clone(),
                group: new.state_group.clone(),
                ..Default::default()
            });
            Ok(made)
        })
    }

    fn comment<'a>(&'a self, workspace: &'a str, _: &'a str, id: &'a str, html: &'a str) -> BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            let mut s = self.check(workspace)?;
            if !s.items.iter().any(|i| i.id == id) {
                return Err(TrackerError::NotFound);
            }
            s.sent.push(Sent {
                kind: "comment".into(),
                item: Some(id.into()),
                html: Some(html.into()),
                ..Default::default()
            });
            Ok(())
        })
    }

    fn set_state<'a>(
        &'a self,
        workspace: &'a str,
        _: &'a str,
        id: &'a str,
        group: &'a str,
    ) -> BoxFuture<'a, Result<Item>> {
        Box::pin(async move {
            let mut s = self.check(workspace)?;
            let at = s.items.iter().map(|i| i.updated_at).max().unwrap_or(0) + 1;
            let it = s.items.iter_mut().find(|i| i.id == id).ok_or(TrackerError::NotFound)?;
            it.state_group = group.into();
            it.state_name = group.into();
            it.updated_at = at;
            let out = it.clone();
            s.sent.push(Sent {
                kind: "state".into(),
                item: Some(id.into()),
                group: Some(group.into()),
                ..Default::default()
            });
            Ok(out)
        })
    }

    fn edit<'a>(&'a self, workspace: &'a str, _: &'a str, id: &'a str, edit: &'a Edit) -> BoxFuture<'a, Result<Item>> {
        Box::pin(async move {
            let mut s = self.check(workspace)?;
            let at = s.items.iter().map(|i| i.updated_at).max().unwrap_or(0) + 1;
            let it = s.items.iter_mut().find(|i| i.id == id).ok_or(TrackerError::NotFound)?;
            if let Some(t) = &edit.title {
                it.title = t.clone();
            }
            it.updated_at = at;
            let out = it.clone();
            s.sent.push(Sent {
                kind: "edit".into(),
                item: Some(id.into()),
                title: edit.title.clone(),
                html: edit.body.clone(),
                ..Default::default()
            });
            Ok(out)
        })
    }

    fn children<'a>(&'a self, workspace: &'a str, _: &'a str, epic: &'a str) -> BoxFuture<'a, Result<Vec<Item>>> {
        Box::pin(async move {
            let s = self.check(workspace)?;
            Ok(s.items.iter().filter(|i| i.parent_id.as_deref() == Some(epic)).cloned().collect())
        })
    }
}
