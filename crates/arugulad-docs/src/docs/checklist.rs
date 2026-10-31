//! A section's test checklist (#1480). Definitions and feedback share one
//! atomic sidecar, so a restart keeps the person's results and pending
//! reports. Failed items go to their assigned agent's durable prompt queue.
//! A crash between enqueueing and saving its receipt can deliver twice:
//! every message names the same report id so its recipient can deduplicate.

use std::{collections::HashSet, path::Path, time::Duration};

use arugula_proto::{Driver, PaneId, fs::READ_MAX};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use super::{Doc, Entry, SECTION_MAX, write};

const ITEMS_MAX: usize = 100;
const NOTE_MAX: usize = 4000;

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize, schemars::JsonSchema)]
pub struct Owner {
    /// A role label, for example frontend or backend.
    pub role: String,
    /// The agent block responsible for this item, on this machine.
    pub pane: PaneId,
}

#[derive(Debug, Clone, Deserialize, Serialize, schemars::JsonSchema)]
pub struct Planned {
    /// Stable within this section: letters, digits, underscores and hyphens.
    pub id: String,
    /// What the person should test.
    pub text: String,
    pub owner: Owner,
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    #[default]
    Todo,
    Passed,
    Failed,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Item {
    pub id: String,
    pub text: String,
    pub owner: Owner,
    #[serde(default)]
    pub status: Status,
    #[serde(default)]
    pub note: String,
    #[serde(default)]
    pub screenshot: Option<String>,
    #[serde(default)]
    pub revision: u64,
    /// The recipient selected when the person reported this verdict.
    #[serde(default)]
    pub report_owner: Option<Owner>,
    #[serde(default)]
    pub report_revision: u64,
    #[serde(default)]
    pub notify_owner: bool,
    /// The feedback revision accepted by the recipient's prompt queue.
    #[serde(default)]
    pub sent: u64,
    #[serde(default)]
    pub delivery_error: Option<String>,
}

#[derive(Deserialize)]
pub(super) struct Feedback {
    section: String,
    item: String,
    revision: u64,
    status: Status,
    #[serde(default)]
    note: String,
    #[serde(default)]
    screenshot: Option<String>,
}

pub(super) fn load(dir: &Path, section: &str) -> Result<Vec<Item>, String> {
    let path = dir.join(format!("{section}.tasks.json"));
    match std::fs::read(&path) {
        Ok(bytes) if bytes.len() <= SECTION_MAX => {
            let items: Vec<Item> = serde_json::from_slice(&bytes)
                .map_err(|e| format!("{} doesn't parse ({e}); it is left as it is", path.display()))?;
            let mut ids = HashSet::new();
            for item in &items {
                if !ids.insert(&item.id)
                    || item.sent > item.report_revision
                    || item.report_revision > item.revision
                    || (item.status == Status::Failed && (item.report_owner.is_none() || item.report_revision == 0))
                {
                    return Err(format!("{} has invalid checklist results; it is left as it is", path.display()));
                }
                if let Some(name) = &item.screenshot {
                    super::assets::check_name(name)?;
                }
            }
            Ok(items)
        }
        Ok(_) => Err(format!("{} is over the section size limit; it is left as it is", path.display())),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(vec![]),
        Err(e) => Err(format!("can't read {}: {e}", path.display())),
    }
}

/// What read_output, copy and a shared snapshot say about the results.
pub(super) fn markdown(items: &[Item]) -> String {
    let mut out = String::new();
    for item in items {
        let checked = if item.status == Status::Passed { 'x' } else { ' ' };
        out.push_str(&format!(
            "- [{checked}] {} (owner: {}, agent %{})\n",
            item.text, item.owner.role, item.owner.pane
        ));
        if item.status == Status::Failed {
            if let Some(owner) = &item.report_owner {
                out.push_str(&format!("  Report recipient: {}, agent %{}.\n", owner.role, owner.pane));
            }
            out.push_str(if item.sent == item.report_revision {
                "  **Failed — sent to owner.**\n"
            } else {
                "  **Failed — waiting to send to owner.**\n"
            });
        }
        for line in item.note.lines() {
            out.push_str(&format!("  > {line}\n"));
        }
        if let Some(name) = &item.screenshot {
            out.push_str(&format!("  ![Test screenshot](assets/{name})\n"));
        }
    }
    out
}

/// Reserve room for future delivery receipts and bounded errors, so a
/// successful enqueue can always persist its receipt within the Doc caps.
pub(super) fn stored_size(items: &[Item]) -> usize {
    if items.is_empty() {
        return 0;
    }
    let mut budget = items.to_vec();
    for item in &mut budget {
        item.revision = u64::MAX;
        item.report_revision = u64::MAX;
        item.report_owner = Some(Owner { role: "\0".repeat(80), pane: u32::MAX });
        item.sent = u64::MAX;
        item.delivery_error = Some("\0".repeat(128));
    }
    serde_json::to_vec_pretty(&budget).map_or(usize::MAX, |b| b.len())
}

impl Doc {
    /// Define or reassign a checklist. Existing results survive; an item
    /// with feedback cannot be removed or have its test text changed.
    pub(super) async fn checklist(&self, section: &str, planned: Vec<Planned>) -> Result<Value, String> {
        let _io = self.io.lock().await;
        if planned.is_empty() || planned.len() > ITEMS_MAX {
            return Err(format!("a checklist needs 1 to {ITEMS_MAX} items"));
        }
        let mut ids = HashSet::new();
        for p in &planned {
            if p.id.is_empty()
                || p.id.len() > 64
                || !p.id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
                || !ids.insert(&p.id)
            {
                return Err("checklist item ids must be unique, 1 to 64 letters, digits, underscores or hyphens".into());
            }
            if p.text.trim().is_empty() || p.text.chars().count() > 500 || p.text.contains(['\n', '\r']) {
                return Err("each test needs one line of text, at most 500 characters".into());
            }
            if p.owner.role.trim().is_empty()
                || p.owner.role.chars().count() > 80
                || p.owner.role.contains(['\n', '\r'])
            {
                return Err("each owner needs a one-line role label, at most 80 characters".into());
            }
        }
        let existing = {
            let st = self.state.lock().unwrap();
            if let Some(e) = &st.error {
                return Err(e.clone());
            }
            let entry = st.entries.iter().find(|e| e.id == section).ok_or("no such Doc section")?;
            let others: usize = st.entries.iter().filter(|e| e.id != section).map(|e| e.tasks.len()).sum();
            if others + planned.len() > ITEMS_MAX {
                return Err(format!("a Doc has at most {ITEMS_MAX} checklist items"));
            }
            entry.tasks.clone()
        };
        if let Some(ctx) = self.ctx() {
            let agents = ctx.agents().await;
            for p in &planned {
                if !agents.iter().any(|(id, _, _)| *id == p.owner.pane) {
                    return Err(format!(
                        "{} must name an agent block on this machine, not %{}",
                        p.owner.role, p.owner.pane
                    ));
                }
            }
        }
        for item in &existing {
            if item.report_revision > 0 && !planned.iter().any(|p| p.id == item.id && p.text.trim() == item.text) {
                return Err(format!("{} has test feedback: keep its id and text, or use a new section", item.id));
            }
        }
        let items = planned
            .into_iter()
            .map(|p| {
                let mut item =
                    existing.iter().find(|item| item.id == p.id && item.text == p.text.trim()).cloned().unwrap_or(
                        Item {
                            id: p.id,
                            text: p.text.trim().into(),
                            owner: p.owner.clone(),
                            status: Status::Todo,
                            note: String::new(),
                            screenshot: None,
                            revision: 0,
                            report_owner: None,
                            report_revision: 0,
                            notify_owner: false,
                            sent: 0,
                            delivery_error: None,
                        },
                    );
                if item.owner != p.owner {
                    item.owner = p.owner;
                    item.revision += 1;
                    item.delivery_error = None;
                }
                item
            })
            .collect();
        self.save_tasks(section, items).await?;
        Ok(json!({ "section": section }))
    }

    /// Save first; the background delivery loop sends failures afterwards.
    pub(super) async fn feedback(&self, req: Feedback) -> Result<Value, String> {
        let _io = self.io.lock().await;
        if req.note.len() > NOTE_MAX {
            return Err(format!("a test note is at most {NOTE_MAX} bytes"));
        }
        if req.status == Status::Failed && req.note.trim().is_empty() && req.screenshot.is_none() {
            return Err("describe the failure or attach a screenshot".into());
        }
        let mut items = {
            let st = self.state.lock().unwrap();
            if let Some(e) = &st.error {
                return Err(e.clone());
            }
            if let Some(name) = &req.screenshot
                && !st.assets.iter().any(|a| &a.name == name)
            {
                return Err("put the screenshot in this Doc before attaching it".into());
            }
            st.entries.iter().find(|e| e.id == req.section).ok_or("no such Doc section")?.tasks.clone()
        };
        let item = items.iter_mut().find(|i| i.id == req.item).ok_or("no such checklist item")?;
        if item.revision != req.revision {
            return Err("this test changed on another device; review its latest result and try again".into());
        }
        let previous_failed = item.status == Status::Failed;
        let previous_recipient = item.report_owner.clone();
        item.status = req.status;
        item.note = req.note.trim().into();
        item.screenshot = req.screenshot;
        item.revision += 1;
        item.report_revision = item.revision;
        item.notify_owner = req.status == Status::Failed || previous_failed || item.notify_owner;
        item.report_owner = if req.status == Status::Failed {
            Some(item.owner.clone())
        } else {
            previous_recipient.or_else(|| Some(item.owner.clone()))
        };
        item.delivery_error = None;
        self.save_tasks(&req.section, items).await?;
        Ok(json!({ "saved": true }))
    }

    /// Caller holds io. One atomic file owns the definitions and results.
    async fn save_tasks(&self, section: &str, items: Vec<Item>) -> Result<(), String> {
        let bytes = serde_json::to_vec_pretty(&items).map_err(|e| e.to_string())?;
        {
            let st = self.state.lock().unwrap();
            let e = st.entries.iter().find(|e| e.id == section).ok_or("no such Doc section")?;
            let size = stored_size(&items);
            if e.body.as_ref().map_or(0, String::len) + size > SECTION_MAX {
                return Err("the section and its checklist exceed 64 KB; split the checklist into sections".into());
            }
            let others: usize = st
                .entries
                .iter()
                .filter(|e| e.id != section)
                .map(|e| e.body.as_ref().map_or(0, String::len) + stored_size(&e.tasks))
                .sum();
            if (others + e.body.as_ref().map_or(0, String::len) + size) as u64 > READ_MAX {
                return Err("the Doc and its checklist exceed 1 MiB".into());
            }
        }
        write(self.dir.clone(), &format!("{section}.tasks.json"), bytes).await?;
        self.touched(None).await;
        {
            let mut st = self.state.lock().unwrap();
            let e = st.entries.iter_mut().find(|e| e.id == section).ok_or("no such Doc section")?;
            e.tasks = items;
            e.rev += 1;
            st.rev += 1;
        }
        (self.changed)();
        Ok(())
    }

    pub(super) fn start_checklist_delivery(&self) {
        let Some(ctx) = self.ctx() else { return };
        let weak = self.me.clone();
        crate::task::spawn_on(&ctx.rt, async move {
            loop {
                let Some(doc) = weak.upgrade() else { break };
                if doc.closed() {
                    break;
                }
                let _ = doc.deliver_checklists().await;
                drop(doc);
                tokio::time::sleep(Duration::from_secs(2)).await;
            }
        });
    }

    pub(super) async fn deliver_checklists(&self) -> Result<(), String> {
        let _io = self.io.lock().await;
        let Some(ctx) = self.ctx() else { return Ok(()) };
        let pending: Vec<Entry> = {
            let st = self.state.lock().unwrap();
            if st.error.is_some() {
                return Ok(());
            }
            st.entries
                .iter()
                .filter(|e| e.tasks.iter().any(|i| i.notify_owner && i.sent < i.report_revision))
                .cloned()
                .collect()
        };
        if pending.is_empty() {
            return Ok(());
        }
        let agents = ctx.agents().await;
        for entry in pending {
            let mut items = entry.tasks;
            let mut changed = false;
            for item in items.iter_mut().filter(|i| i.notify_owner && i.sent < i.report_revision) {
                let Some(owner) = item.report_owner.as_ref() else { continue };
                let report = format!("{}/{}/{}/{}", self.link.doc, entry.id, item.id, item.report_revision);
                let verdict = match item.status {
                    Status::Failed => "Test failure",
                    Status::Passed => "Test result updated: passed",
                    Status::Todo => "Test result updated: not tested",
                };
                let message = format!(
                    "{verdict} for {}\nDoc: {} (block %{}, section {})\nReport: {report}\nTest: {}\nNote: {}{}\nUse the Doc's latest result when it is readable from your tab. A repeated report id is the same feedback; a later revision supersedes it.",
                    owner.role,
                    self.state.lock().unwrap().title,
                    ctx.id,
                    entry.id,
                    item.text,
                    item.note,
                    item.screenshot.as_ref().map_or(String::new(), |n| format!("\nScreenshot in this Doc: assets/{n}"))
                );
                let result = if agents.iter().any(|(id, _, _)| *id == owner.pane) {
                    let image = match &item.screenshot {
                        Some(name) => self.asset(name).await.map(|asset| asset["data"].as_str().map(str::to_owned)),
                        None => Ok(None),
                    };
                    match image {
                        Ok(image) => ctx
                            .follow_up_image(
                                owner.pane,
                                message,
                                Driver { who: "owner".into(), name: "Checklist feedback".into() },
                                image,
                            )
                            .await
                            .map(|_| ()),
                        Err(why) => Err(why),
                    }
                } else {
                    Err(format!(
                        "{}'s agent %{} is unavailable; assign a replacement and report again",
                        owner.role, owner.pane
                    ))
                };
                match result {
                    Ok(()) => {
                        item.sent = item.report_revision;
                        item.delivery_error = None;
                        changed = true;
                    }
                    Err(why) => {
                        let why: String = why.chars().take(128).collect();
                        if item.delivery_error.as_ref() != Some(&why) {
                            item.delivery_error = Some(why);
                            changed = true;
                        }
                    }
                }
            }
            if changed {
                self.save_tasks(&entry.id, items).await?;
            }
        }
        Ok(())
    }
}
