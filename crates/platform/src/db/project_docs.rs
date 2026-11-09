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
//! - A Doc's pictures (`project_doc_assets`) are its own, bytes and all, and
//!   go with it. A Doc a machine copied up is keyed by the machine and its
//!   own Doc id ([`Db::put_machine_doc`]); the key stays when it moves.

use anyhow::Result;
use arugula_control_wire::project_docs::{ASSETS_MAX, DOC_MAX, SECTION_MAX, SECTIONS_MAX};

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
    /// The machine this Doc was copied up from, and its id for it there.
    pub machine: Option<String>,
    pub machine_doc: Option<String>,
    /// For a team Doc made from a machine Doc's copy (Share with…, #418):
    /// the copy and its project, the copy's rev at the snapshot and this
    /// Doc's own rev right after it.
    pub made_from: Option<MadeFrom>,
}

/// Where a team Doc was made from, and the revs it was made at.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MadeFrom {
    pub project: String,
    pub doc: String,
    pub rev: u64,
    pub made_rev: u64,
}

/// One picture as stored, without its bytes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Asset {
    pub name: String,
    pub mime: String,
    pub size: u64,
    pub rev: u64,
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

/// A machine and its own id for a Doc: what a copy is keyed by.
#[derive(Clone, Copy)]
pub struct MachineKey<'a> {
    pub machine: &'a str,
    pub doc: &'a str,
}

/// A machine's whole Doc, as it copies it up.
pub struct MachineDoc<'a> {
    pub title: &'a str,
    pub sections: &'a [MachineSection<'a>],
}

/// One of a machine Doc's sections, under the machine's own id.
pub struct MachineSection<'a> {
    pub id: &'a str,
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
    /// A snapshot of this copy is in the project already: its id.
    Made(String),
    /// The team edited the Doc since its last snapshot, and the refresh
    /// wasn't told to replace their edits.
    Edited,
}

const COLS: &str = "d.id, d.project_id, d.title, d.made_by, d.made_by_session, d.rev, d.created_at, d.updated_at, \
                    (SELECT count(*) FROM project_doc_sections s WHERE s.doc_id = d.id), \
                    (SELECT count(*) FROM project_doc_sections s WHERE s.doc_id = d.id AND s.body IS NOT NULL), \
                    d.machine, d.machine_doc, d.made_from_project, d.made_from_doc, d.made_from_rev, d.made_rev";

fn doc_row(r: &Row) -> Result<Stored> {
    let made_from = match (r.get::<Option<String>>(12)?, r.get::<Option<String>>(13)?) {
        (Some(project), Some(doc)) => Some(MadeFrom {
            project,
            doc,
            rev: r.get::<Option<u64>>(14)?.unwrap_or(0),
            made_rev: r.get::<Option<u64>>(15)?.unwrap_or(0),
        }),
        _ => None,
    };
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
        machine: r.get(10)?,
        machine_doc: r.get(11)?,
        made_from,
    })
}

fn asset_row(r: &Row) -> Result<Asset> {
    Ok(Asset { name: r.get(0)?, mime: r.get(1)?, size: r.get(2)?, rev: r.get(3)? })
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

fn insert_section(c: &impl Sql, doc: &str, at_position: usize, s: &MachineSection, at: u64) -> Result<()> {
    c.execute(
        "INSERT INTO project_doc_sections (doc_id, id, position, heading, intent, body, rev, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        params![doc, s.id, at_position, s.heading, s.intent, s.body, u64::from(s.body.is_some()), at],
    )?;
    Ok(())
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

/// A machine Doc's copy as read to make or refresh a snapshot.
struct CopyRead {
    project: String,
    title: String,
    rev: u64,
    sections: Vec<Section>,
    /// Name, type and bytes.
    assets: Vec<(String, String, Vec<u8>)>,
}

/// The copy `src` with its sections and pictures, its row held; none for
/// a Doc that isn't there or isn't a copy.
fn read_copy(c: &impl Sql, src: &str) -> Result<Option<CopyRead>> {
    let row: Option<(String, String, u64, Option<String>)> = c
        .query_row(
            &format!("SELECT project_id, title, rev, machine FROM project_docs WHERE id = ?1{}", c.for_update()),
            params![src],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
        )
        .optional()?;
    let Some((project, title, rev, Some(_))) = row else {
        return Ok(None);
    };
    let sections = sections_of(c, src)?;
    let assets = c.query_map(
        "SELECT name, mime, bytes FROM project_doc_assets WHERE doc_id = ?1 ORDER BY name",
        params![src],
        |r| Ok((r.get::<String>(0)?, r.get::<String>(1)?, r.get::<Vec<u8>>(2)?)),
    )?;
    Ok(Some(CopyRead { project, title, rev, sections, assets }))
}

/// Put a copy's sections (numbered from `after` + 1) and pictures into the
/// Doc `id`. A picture's rev only goes up from `old`'s, so a name's rev is
/// never reused.
fn put_copy(
    c: &impl Sql,
    id: &str,
    after: u64,
    from: &CopyRead,
    old: &std::collections::HashMap<String, u64>,
    at: u64,
) -> Result<()> {
    for (i, s) in from.sections.iter().enumerate() {
        c.execute(
            "INSERT INTO project_doc_sections (doc_id, id, position, heading, intent, body, rev, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![
                id,
                format!("s{}", after + 1 + i as u64),
                i,
                s.heading,
                s.intent,
                s.body,
                u64::from(s.body.is_some()),
                at
            ],
        )?;
    }
    for (name, mime, bytes) in &from.assets {
        let rev = at.max(old.get(name).map_or(0, |r| r + 1));
        c.execute(
            "INSERT INTO project_doc_assets (doc_id, name, mime, size, rev, bytes, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![id, name, mime, bytes.len() as i64, rev, bytes.clone(), at],
        )?;
    }
    Ok(())
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
        self.c()
            .query_row(&format!("SELECT {COLS} FROM project_docs d WHERE d.id = ?1"), params![id], doc_row)
            .optional()
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
            .query_row(&format!("SELECT title FROM project_docs WHERE id = ?1{}", tx.for_update()), params![id], |r| {
                r.get::<String>(0)
            })
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

    /// Delete a Doc, its sections and its pictures; whether it was there.
    pub fn delete_project_doc(&self, id: &str) -> Result<bool> {
        let mut c = self.c();
        let tx = c.transaction()?;
        tx.execute("DELETE FROM project_doc_sections WHERE doc_id = ?1", params![id])?;
        tx.execute("DELETE FROM project_doc_assets WHERE doc_id = ?1", params![id])?;
        let n = tx.execute("DELETE FROM project_docs WHERE id = ?1", params![id])?;
        tx.commit()?;
        Ok(n > 0)
    }

    /// The Doc `machine` copied up as `machine_doc`, wherever it is now.
    pub fn project_doc_of_machine(&self, machine: &str, machine_doc: &str) -> Result<Option<Stored>> {
        self.c()
            .query_row(
                &format!("SELECT {COLS} FROM project_docs d WHERE d.machine = ?1 AND d.machine_doc = ?2"),
                params![machine, machine_doc],
                doc_row,
            )
            .optional()
    }

    /// The team Docs made from the copy `id`, the earliest first, in every
    /// project: the caller shows only those its reader may still read.
    pub fn project_doc_snapshots(&self, id: &str) -> Result<Vec<Stored>> {
        self.c().query_map(
            &format!("SELECT {COLS} FROM project_docs d WHERE d.made_from_doc = ?1 ORDER BY d.created_at, d.id"),
            params![id],
            doc_row,
        )
    }

    /// The team Doc made from the copy `id` in `project`, if there is one.
    pub fn project_doc_snapshot(&self, id: &str, project: &str) -> Result<Option<Stored>> {
        self.c()
            .query_row(
                &format!("SELECT {COLS} FROM project_docs d WHERE d.made_from_doc = ?1 AND d.project_id = ?2"),
                params![id, project],
                doc_row,
            )
            .optional()
    }

    /// Share with…: make an ordinary Doc in `to` from the copy `src`'s
    /// title, sections (under new ids) and pictures as they are now, and
    /// record where it came from and at which revs. One per copy per
    /// project ([`Refused::Made`]). Who may is the caller's to have
    /// checked.
    pub fn snapshot_project_doc(
        &self,
        src: &str,
        to: &str,
        made_by: &str,
        at: u64,
        max: u64,
    ) -> Result<std::result::Result<Stored, Refused>> {
        let mut c = self.c();
        let tx = c.immediate()?;
        let held: Option<String> = tx
            .query_row(&format!("SELECT id FROM projects WHERE id = ?1{}", tx.for_update()), params![to], |r| r.get(0))
            .optional()?;
        if held.is_none() {
            return Ok(Err(Refused::Missing("no such project")));
        }
        // The copy's row is held, so a machine's put waits for the snapshot.
        let Some(from) = read_copy(&tx, src)? else {
            return Ok(Err(Refused::Missing("no such Doc")));
        };
        let made: Option<String> = tx
            .query_row(
                "SELECT id FROM project_docs WHERE made_from_doc = ?1 AND project_id = ?2",
                params![src, to],
                |r| r.get(0),
            )
            .optional()?;
        if let Some(id) = made {
            return Ok(Err(Refused::Made(id)));
        }
        let have: u64 =
            tx.query_row("SELECT count(*) FROM project_docs WHERE project_id = ?1", params![to], |r| r.get(0))?;
        if have >= max {
            return Ok(Err(Refused::Bad(format!("a project has at most {max} Docs"))));
        }
        let id = new_id();
        tx.execute(
            "INSERT INTO project_docs (id, project_id, title, made_by, created_at, updated_at, made_from_project,
                                       made_from_doc, made_from_rev, made_rev)
             VALUES (?1, ?2, ?3, ?4, ?5, ?5, ?6, ?7, ?8, 1)",
            params![id, to, from.title, made_by, at, from.project, src, from.rev],
        )?;
        put_copy(&tx, &id, 0, &from, &std::collections::HashMap::new(), at)?;
        let doc = tx.query_row(&format!("SELECT {COLS} FROM project_docs d WHERE d.id = ?1"), params![id], doc_row)?;
        tx.commit()?;
        Ok(Ok(doc))
    }

    /// Update copy: replace the team Doc `id`'s title, sections and pictures
    /// with its copy's as they are now, and record both revs again. Unless
    /// `replace`, a Doc the team edited since ([`Refused::Edited`]) is left
    /// alone: that is read with the Doc's row held, so an edit can't land
    /// between the check and the replace. New sections are numbered past
    /// the highest the Doc has: sections are only ever taken out here, all
    /// at once, and this numbers past them first, so the highest it has is
    /// the highest it ever had. Who may is the caller's to have checked.
    pub fn refresh_project_doc_snapshot(
        &self,
        id: &str,
        at: u64,
        replace: bool,
    ) -> Result<std::result::Result<Stored, Refused>> {
        let mut c = self.c();
        let tx = c.immediate()?;
        let made: Option<(Option<String>, u64, Option<u64>)> = tx
            .query_row(
                &format!("SELECT made_from_doc, rev, made_rev FROM project_docs WHERE id = ?1{}", tx.for_update()),
                params![id],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .optional()?;
        let Some((Some(src), rev, made_rev)) = made else {
            return Ok(Err(Refused::Missing("no such Doc")));
        };
        if !replace && rev > made_rev.unwrap_or(0) {
            return Ok(Err(Refused::Edited));
        }
        let Some(from) = read_copy(&tx, &src)? else {
            return Ok(Err(Refused::Missing("the copy is gone")));
        };
        let after = sections_of(&tx, id)?.iter().map(|s| number(&s.id)).max().unwrap_or(0);
        let revs: std::collections::HashMap<String, u64> = tx
            .query_map("SELECT name, rev FROM project_doc_assets WHERE doc_id = ?1", params![id], |r| {
                Ok((r.get::<String>(0)?, r.get::<u64>(1)?))
            })?
            .into_iter()
            .collect();
        tx.execute("DELETE FROM project_doc_sections WHERE doc_id = ?1", params![id])?;
        tx.execute("DELETE FROM project_doc_assets WHERE doc_id = ?1", params![id])?;
        put_copy(&tx, id, after, &from, &revs, at)?;
        // Both revs read the old one: the Doc's rev goes up once, and that
        // is what "edited since" is counted from.
        tx.execute(
            "UPDATE project_docs SET title = ?2, rev = rev + 1, made_rev = rev + 1, updated_at = ?3,
                    made_from_project = ?4, made_from_rev = ?5 WHERE id = ?1",
            params![id, from.title, at, from.project, from.rev],
        )?;
        let doc = tx.query_row(&format!("SELECT {COLS} FROM project_docs d WHERE d.id = ?1"), params![id], doc_row)?;
        tx.commit()?;
        Ok(Ok(doc))
    }

    /// A Doc's pictures, by name.
    pub fn project_doc_assets(&self, id: &str) -> Result<Vec<Asset>> {
        self.c().query_map(
            "SELECT name, mime, size, rev FROM project_doc_assets WHERE doc_id = ?1 ORDER BY name",
            params![id],
            asset_row,
        )
    }

    /// A picture's type and bytes.
    pub fn project_doc_asset(&self, id: &str, name: &str) -> Result<Option<(String, Vec<u8>)>> {
        self.c()
            .query_row(
                "SELECT mime, bytes FROM project_doc_assets WHERE doc_id = ?1 AND name = ?2",
                params![id, name],
                |r| Ok((r.get::<String>(0)?, r.get::<Vec<u8>>(1)?)),
            )
            .optional()
    }

    /// Copy a machine's Doc up: make it in `project` the first time, and
    /// write it where it is after (which must be `project`: the caller
    /// checked that place, and a Doc that moved meanwhile is refused). The
    /// sections become `d`'s, under its ids; the same content again changes
    /// nothing, rev included. `Ok(Ok((doc, changed)))`.
    pub fn put_machine_doc(
        &self,
        key: MachineKey,
        project: &str,
        made_by: &str,
        d: &MachineDoc,
        at: u64,
        max: u64,
    ) -> Result<std::result::Result<(Stored, bool), Refused>> {
        if d.sections.len() > SECTIONS_MAX {
            return Ok(Err(Refused::Bad(format!("a Doc has at most {SECTIONS_MAX} sections"))));
        }
        if let Some(why) = d.sections.iter().find_map(|s| section_too_big(s.body)) {
            return Ok(Err(Refused::Bad(why)));
        }
        if let Some(why) = too_big(d.title, d.sections.iter().map(|s| size(s.heading, s.intent, s.body))) {
            return Ok(Err(Refused::Bad(why)));
        }
        let mut c = self.c();
        let tx = c.immediate()?;
        let held: Option<String> = tx
            .query_row(&format!("SELECT id FROM projects WHERE id = ?1{}", tx.for_update()), params![project], |r| {
                r.get(0)
            })
            .optional()?;
        if held.is_none() {
            return Ok(Err(Refused::Missing("no such project")));
        }
        let found: Option<(String, String, String)> = tx
            .query_row(
                &format!(
                    "SELECT id, project_id, title FROM project_docs WHERE machine = ?1 AND machine_doc = ?2{}",
                    tx.for_update()
                ),
                params![key.machine, key.doc],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .optional()?;
        let (id, changed) = match found {
            None => {
                let have: u64 =
                    tx.query_row("SELECT count(*) FROM project_docs WHERE project_id = ?1", params![project], |r| {
                        r.get(0)
                    })?;
                if have >= max {
                    return Ok(Err(Refused::Bad(format!("a project has at most {max} Docs"))));
                }
                let id = new_id();
                tx.execute(
                    "INSERT INTO project_docs (id, project_id, title, made_by, machine, machine_doc, created_at, updated_at)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?7)",
                    params![id, project, d.title, made_by, key.machine, key.doc, at],
                )?;
                for (i, s) in d.sections.iter().enumerate() {
                    insert_section(&tx, &id, i, s, at)?;
                }
                (id, true)
            }
            Some((id, there, title)) => {
                if there != project {
                    return Ok(Err(Refused::Bad("the Doc moved meanwhile: try again".into())));
                }
                let have = sections_of(&tx, &id)?;
                let mut changed = title != d.title || have.len() != d.sections.len();
                // Row by row: a section kept as it was keeps its rev.
                for (i, s) in d.sections.iter().enumerate() {
                    match have.iter().position(|h| h.id == s.id) {
                        Some(j) => {
                            let h = &have[j];
                            let same = h.heading == s.heading && h.intent == s.intent && h.body.as_deref() == s.body;
                            if same && j == i {
                                continue;
                            }
                            changed = true;
                            let rev = h.rev + u64::from(h.body.as_deref() != s.body && s.body.is_some());
                            tx.execute(
                                "UPDATE project_doc_sections SET position = ?3, heading = ?4, intent = ?5, body = ?6,
                                 rev = ?7, updated_at = ?8 WHERE doc_id = ?1 AND id = ?2",
                                params![id, s.id, i, s.heading, s.intent, s.body, rev, at],
                            )?;
                        }
                        None => {
                            changed = true;
                            insert_section(&tx, &id, i, s, at)?;
                        }
                    }
                }
                for h in have.iter().filter(|h| !d.sections.iter().any(|s| s.id == h.id)) {
                    tx.execute("DELETE FROM project_doc_sections WHERE doc_id = ?1 AND id = ?2", params![id, h.id])?;
                }
                if changed {
                    tx.execute(
                        "UPDATE project_docs SET title = ?2, rev = rev + 1, updated_at = ?3 WHERE id = ?1",
                        params![id, d.title, at],
                    )?;
                }
                (id, changed)
            }
        };
        let doc = tx.query_row(&format!("SELECT {COLS} FROM project_docs d WHERE d.id = ?1"), params![id], doc_row)?;
        tx.commit()?;
        Ok(Ok((doc, changed)))
    }

    /// Keep a picture of Doc `id` under `name`, replacing the one there. The
    /// same bytes again change nothing. `Ok(Ok((asset, changed)))`; past
    /// `ASSETS_MAX` pictures is refused.
    pub fn put_project_doc_asset(
        &self,
        id: &str,
        name: &str,
        mime: &str,
        bytes: &[u8],
        at: u64,
    ) -> Result<std::result::Result<(Asset, bool), Refused>> {
        let mut c = self.c();
        let tx = c.immediate()?;
        let held: Option<String> = tx
            .query_row(&format!("SELECT id FROM project_docs WHERE id = ?1{}", tx.for_update()), params![id], |r| {
                r.get(0)
            })
            .optional()?;
        if held.is_none() {
            return Ok(Err(Refused::Missing("no such Doc")));
        }
        let have = tx.query_map(
            "SELECT name, mime, size, rev FROM project_doc_assets WHERE doc_id = ?1",
            params![id],
            asset_row,
        )?;
        if let Some(a) = have.iter().find(|a| a.name != name && a.name.eq_ignore_ascii_case(name)) {
            return Ok(Err(Refused::Bad(format!("the Doc has {} already: use that name to replace it", a.name))));
        }
        let old = have.iter().find(|a| a.name == name);
        if old.is_none() && have.len() >= ASSETS_MAX {
            return Ok(Err(Refused::Bad(format!("a Doc has at most {ASSETS_MAX} pictures: replace one by its name"))));
        }
        if let Some(a) = old {
            let kept: Option<Vec<u8>> = tx
                .query_row(
                    "SELECT bytes FROM project_doc_assets WHERE doc_id = ?1 AND name = ?2",
                    params![id, name],
                    |r| r.get::<Vec<u8>>(0),
                )
                .optional()?;
            if kept.as_deref() == Some(bytes) && a.mime == mime {
                return Ok(Ok((a.clone(), false)));
            }
        }
        // Only goes up, so a name's rev is never reused.
        let rev = at.max(old.map_or(0, |a| a.rev + 1));
        tx.execute("DELETE FROM project_doc_assets WHERE doc_id = ?1 AND name = ?2", params![id, name])?;
        tx.execute(
            "INSERT INTO project_doc_assets (doc_id, name, mime, size, rev, bytes, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![id, name, mime, bytes.len() as i64, rev, bytes.to_vec(), at],
        )?;
        tx.execute("UPDATE project_docs SET rev = rev + 1, updated_at = ?2 WHERE id = ?1", params![id, at])?;
        tx.commit()?;
        Ok(Ok((Asset { name: name.to_owned(), mime: mime.to_owned(), size: bytes.len() as u64, rev }, true)))
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
    c.execute(
        "DELETE FROM project_doc_assets WHERE doc_id IN
             (SELECT id FROM project_docs WHERE project_id NOT IN (SELECT id FROM projects))",
        params![],
    )?;
    c.execute("DELETE FROM project_docs WHERE project_id NOT IN (SELECT id FROM projects)", params![])?;
    c.execute("UPDATE project_docs SET made_by = '' WHERE made_by = ?1", params![d.account])?;
    Ok(())
}
