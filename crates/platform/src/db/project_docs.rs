//! Project Docs (aru-118): a project's Docs, each a title and an ordered
//! list of sections. Who may do what is decided by the routes
//! (`crate::project_docs`); this is storage, and the size caps, which are
//! counted here under the Doc's lock. The tables, with notes on their
//! columns, are made by `0064_project_docs`.
//!
//! - Every change to a Doc takes its row first (`FOR UPDATE` on Postgres,
//!   the write lock on SQLite), so a Doc's changes run one at a time and
//!   its size is counted against what is there.
//! - A section's id is the Doc's own, `s<n>`, the next after the highest it
//!   ever had: a section's id never names another one.
//! - Deleting a project (or its account) takes its Docs; elsewhere an
//!   account's Docs stay, with no maker.

use anyhow::Result;
use arugula_control_wire::project_docs::{DOC_MAX, SECTION_MAX, SECTIONS_MAX};

use super::{
    Db,
    chat::{in_json, json_list},
    sql::{Optional as _, Row, Sql, params},
};
use crate::Deleted;

/// A new Doc's id: `pdc_` and 32 hex characters.
fn new_id() -> String {
    format!("pdc_{}", hex::encode(arugula_e2e::random::<16>()))
}

/// A Doc as stored, without its sections.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Stored {
    pub id: String,
    pub project: String,
    pub title: String,
    /// The person who made it; "" for a session's, or once they're gone.
    pub made_by: String,
    /// The agent session that made it.
    pub made_by_session: Option<String>,
    pub rev: u64,
    pub created_at: u64,
    pub updated_at: u64,
    /// How many sections it has, and how many are written.
    pub sections: u32,
    pub written: u32,
}

/// One section as stored.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Section {
    pub id: String,
    pub heading: String,
    pub intent: String,
    pub body: Option<String>,
    pub rev: u64,
}

/// A new section: its heading, intent and body (none: pending).
pub struct NewSection<'a> {
    pub heading: &'a str,
    pub intent: &'a str,
    pub body: Option<&'a str>,
}

/// A new Doc.
pub struct NewDoc<'a> {
    pub project: &'a str,
    pub title: &'a str,
    pub made_by: &'a str,
    pub made_by_session: Option<&'a str>,
    pub sections: &'a [NewSection<'a>],
}

/// What a write changes; see `wire::project_docs::DocWrite`.
#[derive(Debug, Default, Clone, Copy)]
pub struct Write<'a> {
    pub title: Option<&'a str>,
    pub section: Option<&'a str>,
    pub heading: Option<&'a str>,
    pub intent: Option<&'a str>,
    pub body: Option<&'a str>,
}

/// Why a change to a Doc wasn't made.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Refused {
    /// The Doc or the section isn't there.
    Missing(&'static str),
    /// Past a cap, or not a change; why.
    Bad(String),
}

const COLS: &str = "d.id, d.project_id, d.title, d.made_by, d.made_by_session, d.rev, d.created_at, d.updated_at, \
                    (SELECT count(*) FROM project_doc_sections s WHERE s.doc_id = d.id), \
                    (SELECT count(*) FROM project_doc_sections s WHERE s.doc_id = d.id AND s.body IS NOT NULL)";

fn doc_row(r: &Row) -> Result<Stored> {
    Ok(Stored {
        id: r.get(0)?,
        project: r.get(1)?,
        title: r.get(2)?,
        made_by: r.get(3)?,
        made_by_session: r.get(4)?,
        rev: r.get(5)?,
        created_at: r.get(6)?,
        updated_at: r.get(7)?,
        sections: r.get(8)?,
        written: r.get(9)?,
    })
}

fn section_row(r: &Row) -> Result<Section> {
    Ok(Section { id: r.get(0)?, heading: r.get(1)?, intent: r.get(2)?, body: r.get(3)?, rev: r.get(4)? })
}

/// What a section counts toward its Doc's size.
fn size(heading: &str, intent: &str, body: Option<&str>) -> usize {
    heading.len() + intent.len() + body.map_or(0, str::len)
}

fn too_big(title: &str, sections: impl Iterator<Item = usize>) -> Option<String> {
    let total = title.len() + sections.sum::<usize>();
    (total > DOC_MAX).then(|| format!("a Doc is at most {DOC_MAX} bytes (1 MiB); this would be {total}"))
}

fn section_too_big(body: Option<&str>) -> Option<String> {
    let n = body.map_or(0, str::len);
    (n > SECTION_MAX).then(|| format!("a section is at most {SECTION_MAX} bytes (64 KB); this one is {n}"))
}

fn sections_of(c: &impl Sql, doc: &str) -> Result<Vec<Section>> {
    c.query_map(
        "SELECT id, heading, intent, body, rev FROM project_doc_sections WHERE doc_id = ?1 ORDER BY position",
        params![doc],
        section_row,
    )
}

/// The number after `s` in a section's id.
fn number(id: &str) -> u64 {
    id.strip_prefix('s').and_then(|n| n.parse().ok()).unwrap_or(0)
}

impl Db {
    /// Make a Doc. `Ok(None)`: the project isn't there, or has `max` Docs.
    /// The project and who may make one are the caller's to have checked.
    pub fn create_project_doc(&self, n: &NewDoc, at: u64, max: u64) -> Result<std::result::Result<Stored, Refused>> {
        if n.sections.len() > SECTIONS_MAX {
            return Ok(Err(Refused::Bad(format!("a Doc has at most {SECTIONS_MAX} sections"))));
        }
        if let Some(why) = n.sections.iter().find_map(|s| section_too_big(s.body)) {
            return Ok(Err(Refused::Bad(why)));
        }
        if let Some(why) = too_big(n.title, n.sections.iter().map(|s| size(s.heading, s.intent, s.body))) {
            return Ok(Err(Refused::Bad(why)));
        }
        let mut c = self.c();
        let tx = c.immediate()?;
        // Counted with the project's row held, so two at once can't both
        // take the last place.
        let held: Option<String> = tx
            .query_row(&format!("SELECT id FROM projects WHERE id = ?1{}", tx.for_update()), params![n.project], |r| {
                r.get(0)
            })
            .optional()?;
        if held.is_none() {
            return Ok(Err(Refused::Missing("no such project")));
        }
        let have: u64 =
            tx.query_row("SELECT count(*) FROM project_docs WHERE project_id = ?1", params![n.project], |r| r.get(0))?;
        if have >= max {
            return Ok(Err(Refused::Bad(format!("a project has at most {max} Docs"))));
        }
        let id = new_id();
        tx.execute(
            "INSERT INTO project_docs (id, project_id, title, made_by, made_by_session, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?6)",
            params![id, n.project, n.title, n.made_by, n.made_by_session, at],
        )?;
        for (i, s) in n.sections.iter().enumerate() {
            tx.execute(
                "INSERT INTO project_doc_sections (doc_id, id, position, heading, intent, body, rev, updated_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                params![id, format!("s{}", i + 1), i, s.heading, s.intent, s.body, u64::from(s.body.is_some()), at],
            )?;
        }
        let doc = tx.query_row(&format!("SELECT {COLS} FROM project_docs d WHERE d.id = ?1"), params![id], doc_row)?;
        tx.commit()?;
        Ok(Ok(doc))
    }

    pub fn project_doc(&self, id: &str) -> Result<Option<Stored>> {
        self.c().query_row(&format!("SELECT {COLS} FROM project_docs d WHERE d.id = ?1"), params![id], doc_row).optional()
    }

    /// A Doc's sections, in order.
    pub fn project_doc_sections(&self, id: &str) -> Result<Vec<Section>> {
        sections_of(&self.c(), id)
    }

    /// A project's Docs, the latest changed first.
    pub fn project_docs(&self, project: &str) -> Result<Vec<Stored>> {
        self.c().query_map(
            &format!("SELECT {COLS} FROM project_docs d WHERE d.project_id = ?1 ORDER BY d.updated_at DESC, d.id"),
            params![project],
            doc_row,
        )
    }

    /// Change a Doc (see [`Write`]): its title, a section's body (and
    /// heading), or a new section at the end. Its size is counted after
    /// the change, against what is there.
    pub fn write_project_doc(&self, id: &str, w: &Write, at: u64) -> Result<std::result::Result<Stored, Refused>> {
        if w.title.is_none() && w.section.is_none() && w.heading.is_none() {
            return Ok(Err(Refused::Bad("nothing to change: give a title, or a section or a heading".into())));
        }
        if w.section.is_some() && w.body.is_none() && w.heading.is_none() && w.intent.is_none() {
            return Ok(Err(Refused::Bad("give the section's markdown (or its heading)".into())));
        }
        if let Some(why) = section_too_big(w.body) {
            return Ok(Err(Refused::Bad(why)));
        }
        let mut c = self.c();
        let tx = c.immediate()?;
        let Some(doc) = tx
            .query_row(
                &format!("SELECT title FROM project_docs WHERE id = ?1{}", tx.for_update()),
                params![id],
                |r| r.get::<String>(0),
            )
            .optional()?
        else {
            return Ok(Err(Refused::Missing("no such Doc")));
        };
        let mut sections = sections_of(&tx, id)?;
        let title = w.title.unwrap_or(&doc);
        match w.section {
            Some(sid) => {
                let Some(s) = sections.iter_mut().find(|s| s.id == sid) else {
                    return Ok(Err(Refused::Missing("no such section")));
                };
                if let Some(h) = w.heading {
                    s.heading = h.to_owned();
                }
                if let Some(i) = w.intent {
                    s.intent = i.to_owned();
                }
                if let Some(b) = w.body {
                    s.body = Some(b.to_owned());
                    s.rev += 1;
                }
            }
            None if w.heading.is_some() => {
                if sections.len() >= SECTIONS_MAX {
                    return Ok(Err(Refused::Bad(format!("a Doc has at most {SECTIONS_MAX} sections"))));
                }
                // Sections are never taken out, so the highest there is is
                // the highest there was.
                let next = sections.iter().map(|s| number(&s.id)).max().unwrap_or(0) + 1;
                sections.push(Section {
                    id: format!("s{next}"),
                    heading: w.heading.unwrap_or_default().to_owned(),
                    intent: w.intent.unwrap_or_default().to_owned(),
                    body: w.body.map(str::to_owned),
                    rev: u64::from(w.body.is_some()),
                });
                let s = sections.last().expect("just pushed");
                tx.execute(
                    "INSERT INTO project_doc_sections (doc_id, id, position, heading, intent, body, rev, updated_at)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                    params![id, s.id, sections.len() - 1, s.heading, s.intent, s.body, s.rev, at],
                )?;
            }
            None => {}
        }
        if let Some(why) = too_big(title, sections.iter().map(|s| size(&s.heading, &s.intent, s.body.as_deref()))) {
            return Ok(Err(Refused::Bad(why)));
        }
        if let Some(sid) = w.section {
            let s = sections.iter().find(|s| s.id == sid).expect("found above");
            tx.execute(
                "UPDATE project_doc_sections SET heading = ?3, intent = ?4, body = ?5, rev = ?6, updated_at = ?7
                 WHERE doc_id = ?1 AND id = ?2",
                params![id, sid, s.heading, s.intent, s.body, s.rev, at],
            )?;
        }
        tx.execute(
            "UPDATE project_docs SET title = ?2, rev = rev + 1, updated_at = ?3 WHERE id = ?1",
            params![id, title, at],
        )?;
        let doc = tx.query_row(&format!("SELECT {COLS} FROM project_docs d WHERE d.id = ?1"), params![id], doc_row)?;
        tx.commit()?;
        Ok(Ok(doc))
    }

    /// Delete a Doc and its sections; whether it was there.
    pub fn delete_project_doc(&self, id: &str) -> Result<bool> {
        let mut c = self.c();
        let tx = c.transaction()?;
        tx.execute("DELETE FROM project_doc_sections WHERE doc_id = ?1", params![id])?;
        let n = tx.execute("DELETE FROM project_docs WHERE id = ?1", params![id])?;
        tx.commit()?;
        Ok(n > 0)
    }

    /// Each of these Docs' project and title, for their threads
    /// (`chat::objects`).
    pub fn project_doc_places(&self, ids: &[&str]) -> Result<Vec<(String, String, String)>> {
        if ids.is_empty() {
            return Ok(vec![]);
        }
        let c = self.c();
        c.query_map(
            &format!("SELECT id, project_id, title FROM project_docs WHERE {}", in_json(&c, "id", 1)),
            params![json_list(ids)?],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
    }
}

/// What deleting an account takes from project Docs, inside the platform's
/// `account_deleted` transaction, after [`super::projects::erase_account`]:
/// the Docs of the projects that went with it, with their sections;
/// elsewhere its Docs stay, with no maker.
pub(crate) fn erase_account(c: &impl Sql, d: &Deleted) -> Result<()> {
    c.execute(
        "DELETE FROM project_doc_sections WHERE doc_id IN
             (SELECT id FROM project_docs WHERE project_id NOT IN (SELECT id FROM projects))",
        params![],
    )?;
    c.execute("DELETE FROM project_docs WHERE project_id NOT IN (SELECT id FROM projects)", params![])?;
    c.execute("UPDATE project_docs SET made_by = '' WHERE made_by = ?1", params![d.account])?;
    Ok(())
}
