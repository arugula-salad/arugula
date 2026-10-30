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
        let state = State { key: key.into(), workspace: workspace.into(), project, items, page: 2, refused: 0 };
        FakeTracker { state: Arc::new(Mutex::new(state)) }
    }

    pub fn set_items(&self, items: Vec<Item>) {
        self.state.lock().unwrap().items = items;
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

    fn create<'a>(&'a self, _: &'a str, _: &'a str, _: &'a NewItem) -> BoxFuture<'a, Result<Item>> {
        Box::pin(async { Err(TrackerError::NotYet("creating a fake work item")) })
    }

    fn comment<'a>(&'a self, _: &'a str, _: &'a str, _: &'a str, _: &'a str) -> BoxFuture<'a, Result<()>> {
        Box::pin(async { Err(TrackerError::NotYet("commenting on a fake work item")) })
    }

    fn set_state<'a>(&'a self, _: &'a str, _: &'a str, _: &'a str, _: &'a str) -> BoxFuture<'a, Result<Item>> {
        Box::pin(async { Err(TrackerError::NotYet("changing a fake work item's state")) })
    }

    fn edit<'a>(&'a self, _: &'a str, _: &'a str, _: &'a str, _: &'a Edit) -> BoxFuture<'a, Result<Item>> {
        Box::pin(async { Err(TrackerError::NotYet("editing a fake work item")) })
    }

    fn children<'a>(&'a self, workspace: &'a str, _: &'a str, epic: &'a str) -> BoxFuture<'a, Result<Vec<Item>>> {
        Box::pin(async move {
            let s = self.check(workspace)?;
            Ok(s.items.iter().filter(|i| i.parent_id.as_deref() == Some(epic)).cloned().collect())
        })
    }
}
