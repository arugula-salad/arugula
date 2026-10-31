//! A tracker that keeps its items in memory, for tests: one workspace, one
//! project, and the one key it accepts. [`FakeTrackers`] hands it out as the
//! platform's [`Trackers`].

use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};

use super::{
    BoxFuture, Edit, Fields, Item, Named, NewItem, Page, ProjectInfo, Relations, Result, StateInfo, TrackerError,
    TrackerProvider, Trackers,
};

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
    /// (blocker, blocked) as the tracker's ids.
    blocks: Vec<(String, String)>,
    fields: Fields,
    /// How many times the fields were read.
    fields_read: usize,
    /// Each item's labels, as ids.
    labels: HashMap<String, Vec<String>>,
    /// (module, item) as ids.
    in_modules: Vec<(String, String)>,
    /// Refuse to put an item in a module.
    module_down: bool,
}

/// A write the fake was sent, as it arrived: a body or comment is the HTML
/// the platform sent, a title the plain text.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Sent {
    /// `create`, `comment`, `state`, `edit` or `module`.
    pub kind: String,
    /// The tracker's id of the item written to; none for a create.
    pub item: Option<String>,
    pub title: Option<String>,
    pub html: Option<String>,
    /// The state's id, for a create, a state or an edit.
    pub state: Option<String>,
    /// The item type's id.
    pub item_type: Option<String>,
    /// The parent's id.
    pub parent: Option<String>,
    /// The labels' ids a create sets or an edit adds, and those it takes off.
    pub labels: Vec<String>,
    pub labels_remove: Vec<String>,
    /// The module's id, for a `module`.
    pub module: Option<String>,
}

/// A project's fields as Plane starts one.
pub fn default_fields() -> Fields {
    let state = |id: &str, name: &str, group: &str, default| StateInfo {
        id: id.into(),
        name: name.into(),
        group: group.into(),
        default,
    };
    Fields {
        states: vec![
            state("st-backlog", "Backlog", "backlog", false),
            state("st-todo", "Todo", "unstarted", true),
            state("st-progress", "In Progress", "started", false),
            state("st-review", "In Review", "started", false),
            state("st-done", "Done", "completed", false),
            state("st-cancelled", "Cancelled", "cancelled", false),
        ],
        modules: vec![Named { id: "mod-boards".into(), name: "Boards".into() }],
        types: vec![
            Named { id: "ty-epic".into(), name: "Epic".into() },
            Named { id: "ty-task".into(), name: "Task".into() },
        ],
        labels: vec![
            Named { id: "lb-agent".into(), name: "agent".into() },
            Named { id: "lb-bug".into(), name: "bug".into() },
        ],
    }
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
        let state = State {
            key: key.into(),
            workspace: workspace.into(),
            project,
            items,
            page: 2,
            refused: 0,
            sent: vec![],
            blocks: vec![],
            fields: default_fields(),
            fields_read: 0,
            labels: HashMap::new(),
            in_modules: vec![],
            module_down: false,
        };
        FakeTracker { state: Arc::new(Mutex::new(state)) }
    }

    pub fn set_items(&self, items: Vec<Item>) {
        self.state.lock().unwrap().items = items;
    }

    /// Which items block which, as (blocker, blocked) tracker ids.
    pub fn set_blocks(&self, blocks: Vec<(&str, &str)>) {
        self.state.lock().unwrap().blocks = blocks.into_iter().map(|(a, b)| (a.into(), b.into())).collect();
    }

    /// What the project offers from now on.
    pub fn set_fields(&self, fields: Fields) {
        self.state.lock().unwrap().fields = fields;
    }

    /// How many times the fields were read.
    pub fn fields_read(&self) -> usize {
        self.state.lock().unwrap().fields_read
    }

    /// The ids of an item's labels.
    pub fn labels_of(&self, item: &str) -> Vec<String> {
        self.state.lock().unwrap().labels.get(item).cloned().unwrap_or_default()
    }

    /// The modules (ids) an item is in.
    pub fn modules_of(&self, item: &str) -> Vec<String> {
        let s = self.state.lock().unwrap();
        s.in_modules.iter().filter(|(_, i)| i == item).map(|(m, _)| m.clone()).collect()
    }

    /// Whether putting an item in a module fails.
    pub fn set_module_down(&self, down: bool) {
        self.state.lock().unwrap().module_down = down;
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

    fn relations<'a>(&'a self, workspace: &'a str, _: &'a str, item: &'a str) -> BoxFuture<'a, Result<Relations>> {
        Box::pin(async move {
            let s = self.check(workspace)?;
            if !s.items.iter().any(|i| i.id == item) {
                return Err(TrackerError::NotFound);
            }
            let ids = |pick: &dyn Fn(&(String, String)) -> Option<String>| s.blocks.iter().filter_map(pick).collect();
            Ok(Relations {
                blocked_by: ids(&|(a, b)| (b == item).then(|| a.clone())),
                blocking: ids(&|(a, b)| (a == item).then(|| b.clone())),
            })
        })
    }

    fn create<'a>(&'a self, workspace: &'a str, _: &'a str, new: &'a NewItem) -> BoxFuture<'a, Result<Item>> {
        Box::pin(async move {
            let mut s = self.check(workspace)?;
            let n = s.items.iter().map(|i| i.sequence_id).max().unwrap_or(0) + 1;
            let mut made = item(n, &new.title, "backlog", 5_000_000_000_000);
            if let Some(st) = new.state.as_deref().and_then(|id| s.fields.states.iter().find(|x| x.id == id)) {
                (made.state_name, made.state_group) = (st.name.clone(), st.group.clone());
            }
            if let Some(t) = new.item_type.as_deref().and_then(|id| s.fields.types.iter().find(|x| x.id == id)) {
                made.item_type = Some(t.name.clone());
            }
            made.parent_id = new.parent_id.clone();
            s.items.push(made.clone());
            s.labels.insert(made.id.clone(), new.labels.clone());
            s.sent.push(Sent {
                kind: "create".into(),
                title: Some(new.title.clone()),
                html: new.body.clone(),
                state: new.state.clone(),
                item_type: new.item_type.clone(),
                parent: new.parent_id.clone(),
                labels: new.labels.clone(),
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
        state: &'a str,
    ) -> BoxFuture<'a, Result<Item>> {
        Box::pin(async move {
            let mut s = self.check(workspace)?;
            let at = s.items.iter().map(|i| i.updated_at).max().unwrap_or(0) + 1;
            let st = s.fields.states.iter().find(|x| x.id == state).cloned().ok_or(TrackerError::NotFound)?;
            let it = s.items.iter_mut().find(|i| i.id == id).ok_or(TrackerError::NotFound)?;
            it.state_group = st.group;
            it.state_name = st.name;
            it.updated_at = at;
            let out = it.clone();
            s.sent.push(Sent {
                kind: "state".into(),
                item: Some(id.into()),
                state: Some(state.into()),
                ..Default::default()
            });
            Ok(out)
        })
    }

    fn edit<'a>(&'a self, workspace: &'a str, _: &'a str, id: &'a str, edit: &'a Edit) -> BoxFuture<'a, Result<Item>> {
        Box::pin(async move {
            let mut s = self.check(workspace)?;
            let at = s.items.iter().map(|i| i.updated_at).max().unwrap_or(0) + 1;
            let st = edit.state.as_deref().and_then(|id| s.fields.states.iter().find(|x| x.id == id)).cloned();
            let ty = edit.item_type.as_deref().and_then(|id| s.fields.types.iter().find(|x| x.id == id)).cloned();
            let it = s.items.iter_mut().find(|i| i.id == id).ok_or(TrackerError::NotFound)?;
            if let Some(t) = &edit.title {
                it.title = t.clone();
            }
            if let Some(st) = st {
                (it.state_name, it.state_group) = (st.name, st.group);
            }
            if let Some(ty) = ty {
                it.item_type = Some(ty.name);
            }
            if edit.parent_id.is_some() {
                it.parent_id = edit.parent_id.clone();
            }
            it.updated_at = at;
            let out = it.clone();
            let have = s.labels.entry(id.to_owned()).or_default();
            for l in &edit.labels_add {
                if !have.contains(l) {
                    have.push(l.clone());
                }
            }
            have.retain(|l| !edit.labels_remove.contains(l));
            s.sent.push(Sent {
                kind: "edit".into(),
                item: Some(id.into()),
                title: edit.title.clone(),
                html: edit.body.clone(),
                state: edit.state.clone(),
                item_type: edit.item_type.clone(),
                parent: edit.parent_id.clone(),
                labels: edit.labels_add.clone(),
                labels_remove: edit.labels_remove.clone(),
                ..Default::default()
            });
            Ok(out)
        })
    }

    fn fields<'a>(&'a self, workspace: &'a str, _: &'a str) -> BoxFuture<'a, Result<Fields>> {
        Box::pin(async move {
            let mut s = self.check(workspace)?;
            s.fields_read += 1;
            Ok(s.fields.clone())
        })
    }

    fn add_to_module<'a>(
        &'a self,
        workspace: &'a str,
        _: &'a str,
        module: &'a str,
        id: &'a str,
    ) -> BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            let mut s = self.check(workspace)?;
            if s.module_down {
                return Err(TrackerError::Unavailable("it answered 500".into()));
            }
            s.in_modules.push((module.into(), id.into()));
            s.sent.push(Sent {
                kind: "module".into(),
                item: Some(id.into()),
                module: Some(module.into()),
                ..Default::default()
            });
            Ok(())
        })
    }

    fn by_sequence<'a>(
        &'a self,
        workspace: &'a str,
        _: &'a str,
        identifier: &'a str,
        sequence: u64,
    ) -> BoxFuture<'a, Result<Item>> {
        Box::pin(async move {
            let s = self.check(workspace)?;
            if !identifier.eq_ignore_ascii_case(&s.project.identifier) {
                return Err(TrackerError::NotFound);
            }
            s.items.iter().find(|i| i.sequence_id == sequence).cloned().ok_or(TrackerError::NotFound)
        })
    }

    fn children<'a>(&'a self, workspace: &'a str, _: &'a str, epic: &'a str) -> BoxFuture<'a, Result<Vec<Item>>> {
        Box::pin(async move {
            let s = self.check(workspace)?;
            Ok(s.items.iter().filter(|i| i.parent_id.as_deref() == Some(epic)).cloned().collect())
        })
    }
}
