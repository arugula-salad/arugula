//! Chat's link previews' rows (#108, migration `0039`): what control read
//! of a web page, the messages whose author took their previews off, and
//! a team's switch for web previews. Storage only: who sees what, and the
//! fetching, are in `crate::chat::previews`.

use anyhow::Result;

use super::{
    Db,
    sql::{Optional as _, Sql, params},
};

/// What was read of a page (or that nothing could be).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct WebPreview {
    pub url: String,
    pub fetched_at: u64,
    /// Whether there's anything to show.
    pub ok: bool,
    pub site: String,
    pub title: String,
    pub description: String,
    /// The page's own address for its image; readers are sent to ours.
    pub image_url: Option<String>,
}

/// A mirrored item named by its repo and number, with what a card shows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MirrorCard {
    pub id: String,
    pub project_id: String,
    pub repo_id: u64,
    pub private: bool,
    pub full_name: String,
    pub number: u64,
    /// `pr` or `issue`.
    pub kind: String,
    /// `open`, `closed` or `merged`.
    pub state: String,
    pub title: String,
    pub draft: bool,
    pub author: Option<String>,
    pub updated_at: u64,
}

/// A cached page's image address, and the image if it was fetched.
pub type PageImage = (Option<String>, Option<(String, Vec<u8>)>);

impl Db {
    /// The cached read of `url`, if there's one.
    pub fn web_preview(&self, url: &str) -> Result<Option<WebPreview>> {
        self.c()
            .query_row(
                "SELECT url, fetched_at, ok, site, title, description, image_url FROM chat_link_previews WHERE url = ?1",
                params![url],
                |r| {
                    Ok(WebPreview {
                        url: r.get(0)?,
                        fetched_at: r.get(1)?,
                        ok: r.get::<u64>(2)? != 0,
                        site: r.get(3)?,
                        title: r.get(4)?,
                        description: r.get(5)?,
                        image_url: r.get(6)?,
                    })
                },
            )
            .optional()
    }

    /// Keep a read of a page, replacing an older one (and dropping its
    /// image, which was another read's).
    pub fn put_web_preview(&self, p: &WebPreview) -> Result<()> {
        self.c().execute(
            "INSERT INTO chat_link_previews (url, fetched_at, ok, site, title, description, image_url)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
             ON CONFLICT (url) DO UPDATE SET fetched_at = excluded.fetched_at, ok = excluded.ok,
                 site = excluded.site, title = excluded.title, description = excluded.description,
                 image_url = excluded.image_url, image_type = NULL, image = NULL",
            params![p.url, p.fetched_at, u64::from(p.ok), p.site, p.title, p.description, p.image_url],
        )?;
        Ok(())
    }

    /// The cached read whose image is at `image_url`, with the image if it
    /// was fetched. The proxy only serves addresses a page named, never
    /// one a reader picked.
    pub fn preview_image(&self, page: &str) -> Result<Option<PageImage>> {
        self.c()
            .query_row(
                "SELECT image_url, image_type, image FROM chat_link_previews WHERE url = ?1 AND ok = 1",
                params![page],
                |r| {
                    let img = match (r.get::<Option<String>>(1)?, r.get::<Option<Vec<u8>>>(2)?) {
                        (Some(t), Some(b)) => Some((t, b)),
                        _ => None,
                    };
                    Ok((r.get::<Option<String>>(0)?, img))
                },
            )
            .optional()
    }

    pub fn put_preview_image(&self, page: &str, kind: &str, bytes: &[u8]) -> Result<()> {
        self.c().execute(
            "UPDATE chat_link_previews SET image_type = ?2, image = ?3 WHERE url = ?1",
            params![page, kind, bytes.to_vec()],
        )?;
        Ok(())
    }

    /// Forget reads older than `before`.
    pub fn prune_web_previews(&self, before: u64) -> Result<usize> {
        self.c().execute("DELETE FROM chat_link_previews WHERE fetched_at < ?1", params![before])
    }

    /// Which of these messages had their previews taken off.
    pub fn hidden_previews(&self, room: &str, seqs: &[i64]) -> Result<Vec<i64>> {
        if seqs.is_empty() {
            return Ok(vec![]);
        }
        let c = self.c();
        let all = c.query_map("SELECT seq FROM chat_preview_hidden WHERE room_id = ?1", params![room], |r| r.get(0))?;
        Ok(all.into_iter().filter(|s| seqs.contains(s)).collect())
    }

    /// Take a message's previews off (or put them back): whether that
    /// changed anything.
    pub fn set_previews_hidden(&self, room: &str, seq: i64, hidden: bool, at: u64) -> Result<bool> {
        let c = self.c();
        Ok(if hidden {
            c.execute(
                "INSERT INTO chat_preview_hidden (room_id, seq, at) VALUES (?1, ?2, ?3) ON CONFLICT DO NOTHING",
                params![room, seq, at],
            )?
        } else {
            c.execute("DELETE FROM chat_preview_hidden WHERE room_id = ?1 AND seq = ?2", params![room, seq])?
        } == 1)
    }

    /// Whether a team's chat shows web previews (the default: yes).
    pub fn web_previews_on(&self, team: &str) -> Result<bool> {
        Ok(self
            .c()
            .query_row("SELECT web_previews FROM chat_team_prefs WHERE team_id = ?1", params![team], |r| {
                r.get::<u64>(0)
            })
            .optional()?
            .is_none_or(|v| v != 0))
    }

    pub fn set_web_previews(&self, team: &str, on: bool) -> Result<()> {
        self.c().execute(
            "INSERT INTO chat_team_prefs (team_id, web_previews) VALUES (?1, ?2)
             ON CONFLICT (team_id) DO UPDATE SET web_previews = excluded.web_previews",
            params![team, u64::from(on)],
        )?;
        Ok(())
    }

    /// The linked repos' mirrored items named `full_name#number` (GitHub's
    /// names are case-insensitive), one per project that links the repo.
    pub fn mirror_cards(&self, full_name: &str, number: u64) -> Result<Vec<MirrorCard>> {
        let c = self.c();
        c.query_map(
            "SELECT f.id, r.project_id, r.repo_id, r.private, r.full_name, f.number, f.kind, f.state, f.title,
                    f.draft, f.author_login, f.updated_at
             FROM forge_items f JOIN linked_repos r ON r.repo_id = f.repo_id
             WHERE lower(r.full_name) = lower(?1) AND f.number = ?2 AND r.unlinked_at IS NULL",
            params![full_name, number],
            |r| {
                Ok(MirrorCard {
                    id: r.get(0)?,
                    project_id: r.get(1)?,
                    repo_id: r.get(2)?,
                    private: r.get(3)?,
                    full_name: r.get(4)?,
                    number: r.get(5)?,
                    kind: r.get(6)?,
                    state: r.get(7)?,
                    title: r.get(8)?,
                    draft: r.get(9)?,
                    author: r.get(10)?,
                    updated_at: r.get(11)?,
                })
            },
        )
    }
}
