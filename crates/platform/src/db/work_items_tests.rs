//! Work items, the merge of two, and the one-time move of board rows
//! (Arugula #405), on SQLite and, with `ARUGULA_TEST_POSTGRES_URL`, on
//! Postgres.

use arugula_control_wire::boards::Pointer;

use super::*;
use crate::db::{
    boards::{Put, Stored},
    boards_work::WorkMoved,
    issues::{Actor, NewIssue},
};

fn project(db: &Db, id: &str) {
    db.c()
        .execute(
            "INSERT INTO projects (id, team_id, name, is_default, created_at) VALUES (?1, ?1, ?1, FALSE, 1)",
            params![id],
        )
        .unwrap();
}

/// `repo` linked to `project` as `acme/site`.
fn repo(db: &Db, project: &str, repo: u64) {
    db.c()
        .execute(
            "INSERT INTO linked_repos (repo_id, full_name, private, project_id, installation_id, linked_by, linked_at)
             VALUES (?1, 'acme/site', FALSE, ?2, 1, 'a', 1)",
            params![repo, project],
        )
        .unwrap();
}

fn mirror(db: &Db, id: &str, repo: u64, number: u64, kind: &str) {
    db.c()
        .execute(
            "INSERT INTO forge_items (id, repo_id, number, kind, state, title, created_at, updated_at, synced_at)
             VALUES (?1, ?2, ?3, ?4, 'open', 't', 1, 1, 1)",
            params![id, repo, number, kind],
        )
        .unwrap();
}

fn native(db: &Db, project: &str) -> String {
    let none: [String; 0] = [];
    db.create_issue(
        &NewIssue { project, title: "An issue", body: "", labels: &none, assignees: &none },
        Actor::person("a"),
        10,
    )
    .unwrap()
    .unwrap()
    .id
}

/// The issue exported to `acme/site` #`number`.
fn export(db: &Db, issue: &str, repo: u64, number: u64) {
    db.c()
        .execute(
            "UPDATE issues SET github_repo = 'acme/site', github_repo_id = ?2, github_number = ?3 WHERE id = ?1",
            params![issue, repo, number],
        )
        .unwrap();
}

fn board(db: &Db, id: &str, project: &str) {
    db.c()
        .execute(
            "INSERT INTO boards (id, project_id, title, created_by, created_at, updated_at)
             VALUES (?1, ?2, 'Board', 'a', 1, 1)",
            params![id, project],
        )
        .unwrap();
}

/// A row as an older build left it: a pointer and no work item.
fn old_row(db: &Db, board: &str, id: &str, position: u64, note: &str, target: &str) {
    let kind = if target.starts_with("iss_") { "issue" } else { "forge_item" };
    db.c()
        .execute(
            "INSERT INTO board_entries (id, board_id, kind, position, note, target_kind, target, created_at, updated_at)
             VALUES (?1, ?2, 'row', ?3, ?4, ?5, ?6, 1, 1)",
            params![id, board, position, note, kind, target],
        )
        .unwrap();
}

fn attach_pr(db: &Db, board: &str, row: &str, pr: &str) {
    db.c()
        .execute(
            "INSERT INTO board_attachments (id, board_id, row_id, role, target_kind, target, created_at)
             VALUES (?1, ?2, ?3, 'work', 'forge_item', ?4, 1)",
            params![format!("att_{row}_{pr}"), board, row, pr],
        )
        .unwrap();
}

fn add(db: &Db, board: &str, pointer: &Pointer) -> Result<String, crate::db::boards::Refused> {
    db.put_entry(board, &Put { pointer: Some(pointer), ..Default::default() }, &[], 5, 500, 50).unwrap()
}

fn item(id: &str) -> Pointer {
    if id.starts_with("iss_") { Pointer::Issue { id: id.into() } } else { Pointer::ForgeItem { id: id.into() } }
}

fn count(db: &Db, sql: &str) -> u64 {
    db.c().query_row(sql, params![], |r| r.get(0)).unwrap()
}

fn rows(db: &Db, board: &str) -> Vec<(String, Option<String>)> {
    let s: Stored = db.board_stored(board).unwrap().unwrap();
    s.rows.into_iter().map(|r| (r.id, r.work_item)).collect()
}

/// A native issue exported to GitHub and its mirrored copy are one work
/// item, whichever is asked first, with a ref for each; a Plane item is its
/// own; a PR has none.
#[test]
fn an_exported_issue_and_its_mirror_are_one_work_item() {
    let db = Db::test();
    project(&db, "prj_a");
    repo(&db, "prj_a", 88);
    mirror(&db, "fi_5", 88, 5, "issue");
    mirror(&db, "fi_pr", 88, 6, "pr");
    let n = native(&db, "prj_a");
    export(&db, &n, 88, 5);

    let w = db.work_item_for(&n).unwrap().unwrap();
    assert_eq!(db.work_item_for("fi_5").unwrap(), Some(w.clone()), "the mirrored copy joined it");
    assert_eq!(db.work_item_for(&n).unwrap(), Some(w.clone()), "again: the same");
    assert!(w.starts_with("wi_") && w.len() == 35, "{w}");
    assert_eq!(count(&db, "SELECT count(*) FROM work_items"), 1);
    let refs: Vec<(String, String, String, String)> = db
        .c()
        .query_map(
            "SELECT system, target, instance, key FROM work_item_refs WHERE work_item = ?1 ORDER BY system",
            params![w],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
        )
        .unwrap();
    assert_eq!(
        refs,
        [
            ("github".into(), "fi_5".into(), "88".into(), "acme/site#5".into()),
            ("native".into(), n.clone(), "prj_a".into(), "1".into()),
        ]
    );
    assert_eq!(count(&db, "SELECT count(*) FROM work_item_refs WHERE synced_rev IS NOT NULL"), 0);
    assert_eq!(db.work_item_for("fi_pr").unwrap(), None, "a PR has no work item");

    // The other way round: the mirrored issue first, then its native one.
    mirror(&db, "fi_7", 88, 7, "issue");
    let m = native(&db, "prj_a");
    export(&db, &m, 88, 7);
    let first = db.work_item_for("fi_7").unwrap().unwrap();
    assert_eq!(db.work_item_for(&m).unwrap(), Some(first));
    assert_eq!(count(&db, "SELECT count(*) FROM work_items"), 2);

    // A native issue whose copy is not mirrored is alone, and a later
    // mirror joins it.
    let lone = native(&db, "prj_a");
    export(&db, &lone, 88, 9);
    let w = db.work_item_for(&lone).unwrap().unwrap();
    assert_eq!(count(&db, &format!("SELECT count(*) FROM work_item_refs WHERE work_item = '{w}'")), 1);
    mirror(&db, "fi_9", 88, 9, "issue");
    assert_eq!(db.work_item_for("fi_9").unwrap(), Some(w));
}

/// A board has one row for a work item: adding the native issue after its
/// GitHub copy (or the other way round) is refused as the same row.
#[test]
fn a_board_takes_one_row_for_a_work_item() {
    let db = Db::test();
    project(&db, "prj_a");
    repo(&db, "prj_a", 88);
    mirror(&db, "fi_5", 88, 5, "issue");
    let n = native(&db, "prj_a");
    export(&db, &n, 88, 5);
    board(&db, "brd_1", "prj_a");

    let row = add(&db, "brd_1", &item("fi_5")).unwrap();
    assert_eq!(
        rows(&db, "brd_1"),
        [(row.clone(), Some(db.work_item_for(&n).unwrap().unwrap()))],
        "the row has its work item"
    );
    let w = db.work_item_for(&n).unwrap().unwrap();
    assert_eq!(db.row_of_work_item("brd_1", &w).unwrap(), Some(row.clone()));
    let again = add(&db, "brd_1", &item(&n));
    assert!(
        matches!(&again, Err(crate::db::boards::Refused::Bad(why)) if why.contains(&row)),
        "a second row for the work: {again:?}"
    );
    assert_eq!(rows(&db, "brd_1").len(), 1);
}

/// The move gives every issue row its work item; two rows of one work item
/// (a native issue and its GitHub copy) become one, the later folded into
/// the earlier with its note and attachments; a PR row and a note are left
/// as they are; and a second run changes nothing.
#[test]
fn the_move_folds_two_rows_of_one_work_item_and_runs_once() {
    let db = Db::test();
    project(&db, "prj_a");
    repo(&db, "prj_a", 88);
    mirror(&db, "fi_5", 88, 5, "issue");
    mirror(&db, "fi_8", 88, 8, "issue");
    mirror(&db, "fi_pr", 88, 6, "pr");
    mirror(&db, "fi_pr2", 88, 9, "pr");
    let n = native(&db, "prj_a");
    export(&db, &n, 88, 5);
    board(&db, "brd_1", "prj_a");
    board(&db, "brd_2", "prj_a");
    old_row(&db, "brd_1", "row_gh", 0, "github one", "fi_5");
    old_row(&db, "brd_1", "row_other", 1, "", "fi_8");
    old_row(&db, "brd_1", "row_pr", 2, "", "fi_pr");
    old_row(&db, "brd_1", "row_nat", 3, "native one", &n);
    attach_pr(&db, "brd_1", "row_nat", "fi_pr2");
    // The same PR is on both rows: it is on the survivor once.
    attach_pr(&db, "brd_1", "row_nat", "fi_pr");
    attach_pr(&db, "brd_1", "row_gh", "fi_pr");
    old_row(&db, "brd_2", "row_solo", 0, "", &n);

    let moved = db.work_move().unwrap();
    assert_eq!(moved, WorkMoved { boards: 2, rows: 4, folded: 1 });

    let w = db.work_item_for(&n).unwrap().unwrap();
    let kept = rows(&db, "brd_1");
    assert_eq!(kept.len(), 3, "{kept:?}");
    assert_eq!((kept[0].0.as_str(), kept[0].1.as_deref()), ("row_gh", Some(w.as_str())), "the earlier row stays");
    assert_eq!(kept[1].0, "row_other");
    assert_eq!(kept[2], ("row_pr".into(), None), "a PR row has no work item");
    assert_eq!(rows(&db, "brd_2"), [("row_solo".into(), Some(w.clone()))]);
    let stored = db.board_stored("brd_1").unwrap().unwrap();
    assert_eq!(stored.rows[0].note, "github one\nnative one");
    assert_eq!(stored.rows.iter().map(|r| r.rev).collect::<Vec<_>>(), [2, 1, 1], "the note changed once");
    let mut on_gh: Vec<&str> = stored
        .attachments
        .iter()
        .filter(|a| a.row == "row_gh")
        .map(|a| match &a.pointer {
            Pointer::ForgeItem { id } => id.as_str(),
            _ => "",
        })
        .collect();
    on_gh.sort();
    assert_eq!(on_gh, ["fi_pr", "fi_pr2"], "the attachments followed, the PR once");
    assert!(stored.attachments.iter().all(|a| a.row != "row_nat"));
    assert_eq!(stored.rows.iter().map(|r| r.id.as_str()).collect::<Vec<_>>(), ["row_gh", "row_other", "row_pr"]);

    // Again: nothing changes.
    let tables = |db: &Db| {
        (
            rows(db, "brd_1"),
            rows(db, "brd_2"),
            db.board_stored("brd_1").unwrap().unwrap().attachments.len(),
            count(db, "SELECT count(*) FROM work_items"),
            count(db, "SELECT count(*) FROM work_item_refs"),
            count(db, "SELECT CAST(sum(rev) AS BIGINT) FROM board_entries"),
        )
    };
    let before = tables(&db);
    assert_eq!(db.work_move().unwrap(), WorkMoved::default());
    assert_eq!(tables(&db), before);
}

/// Two work items that turn out to be one, both on boards (the trap): the
/// GitHub issue was a row before its native issue was exported, and the
/// native issue was a row too, on its own and then with the GitHub one. The
/// export is recorded and the next time the native issue is placed, there
/// is one work item, one row on each board, nothing orphaned.
#[test]
fn two_work_items_that_turn_out_to_be_one_merge() {
    let db = Db::test();
    project(&db, "prj_a");
    repo(&db, "prj_a", 88);
    mirror(&db, "fi_5", 88, 5, "issue");
    mirror(&db, "fi_pr", 88, 6, "pr");
    let n = native(&db, "prj_a");
    for b in ["brd_1", "brd_2", "brd_3"] {
        board(&db, b, "prj_a");
    }
    // Not linked yet: two work items. brd_1 has a row for each, brd_2 the
    // GitHub issue's, brd_3 the native one's.
    let gh_row = add(&db, "brd_1", &item("fi_5")).unwrap();
    let nat_row = add(&db, "brd_1", &item(&n)).unwrap();
    db.put_entry(
        "brd_1",
        &Put { id: Some(&nat_row), rev: Some(1), note: Some("native note"), ..Default::default() },
        &[],
        6,
        500,
        50,
    )
    .unwrap()
    .unwrap();
    attach_pr(&db, "brd_1", &nat_row, "fi_pr");
    let on2 = add(&db, "brd_2", &item("fi_5")).unwrap();
    let on3 = add(&db, "brd_3", &item(&n)).unwrap();
    assert_eq!(count(&db, "SELECT count(*) FROM work_items"), 2);
    let (older, younger) = {
        let wi = |t: &str| db.work_item_for(t).unwrap().unwrap();
        (wi("fi_5"), wi(&n))
    };
    assert_ne!(older, younger);
    // The GitHub issue's work item is the older one.
    db.c().execute("UPDATE work_items SET created_at = 1 WHERE id = ?1", params![older]).unwrap();
    db.c().execute("UPDATE work_items SET created_at = 2 WHERE id = ?1", params![younger]).unwrap();

    export(&db, &n, 88, 5);
    assert_eq!(db.work_item_for(&n).unwrap(), Some(older.clone()), "merged into the older");

    assert_eq!(count(&db, "SELECT count(*) FROM work_items"), 1);
    assert_eq!(count(&db, "SELECT count(*) FROM work_item_refs WHERE work_item <> ''"), 2);
    assert_eq!(
        count(&db, "SELECT count(*) FROM work_item_refs WHERE work_item NOT IN (SELECT id FROM work_items)"),
        0,
        "no ref points at a work item that is gone"
    );
    assert_eq!(
        count(
            &db,
            "SELECT count(*) FROM board_entries WHERE work_item IS NOT NULL AND work_item <> ?1"
                .replace("?1", &format!("'{older}'"))
                .as_str()
        ),
        0
    );
    assert_eq!(rows(&db, "brd_1"), [(gh_row.clone(), Some(older.clone()))], "one row where there were two");
    assert_eq!(rows(&db, "brd_2"), [(on2, Some(older.clone()))]);
    assert_eq!(rows(&db, "brd_3"), [(on3, Some(older.clone()))]);
    let s = db.board_stored("brd_1").unwrap().unwrap();
    assert_eq!(s.rows[0].note, "native note", "the folded row's note came along");
    assert_eq!(s.attachments.iter().map(|a| a.row.as_str()).collect::<Vec<_>>(), [gh_row.as_str()]);
    // A second look changes nothing.
    assert_eq!(db.work_item_for("fi_5").unwrap(), Some(older));
    assert_eq!(count(&db, "SELECT count(*) FROM work_items"), 1);
    assert_eq!(rows(&db, "brd_1").len(), 1);
}

/// A Plane item is a work item of its own, named by its key.
#[test]
fn a_plane_item_is_its_own_work_item() {
    let db = Db::test();
    project(&db, "prj_a");
    db.c()
        .execute(
            "INSERT INTO tracker_connections (id, project_id, provider, workspace_slug, source_project_id, identifier,
                 name, mirror_id, created_at)
             VALUES ('trk_1', 'prj_a', 'plane', 'acme', 'p1', 'ARUGU', 'Arugula', -1, 1)",
            params![],
        )
        .unwrap();
    db.c()
        .execute(
            "INSERT INTO forge_items (id, repo_id, number, kind, state, title, provider, connection_id, created_at,
                 updated_at, synced_at)
             VALUES ('fi_pl', -1, 12, 'issue', 'open', 't', 'plane', 'trk_1', 1, 1, 1)",
            params![],
        )
        .unwrap();
    let w = db.work_item_for("fi_pl").unwrap().unwrap();
    let (system, instance, key): (String, String, String) = db
        .c()
        .query_row("SELECT system, instance, key FROM work_item_refs WHERE work_item = ?1", params![w], |r| {
            Ok((r.get(0)?, r.get(1)?, r.get(2)?))
        })
        .unwrap();
    assert_eq!((system.as_str(), instance.as_str(), key.as_str()), ("plane", "trk_1", "ARUGU-12"));
    assert_eq!(count(&db, "SELECT count(*) FROM work_item_refs"), 1);
}

/// A project that goes with its account takes its work items and their refs.
#[test]
fn a_deleted_projects_work_items_go_with_it() {
    let db = Db::test();
    project(&db, "prj_a");
    project(&db, "prj_b");
    let (a, b) = (native(&db, "prj_a"), native(&db, "prj_b"));
    db.work_item_for(&a).unwrap().unwrap();
    db.work_item_for(&b).unwrap().unwrap();
    db.c().execute("DELETE FROM projects WHERE id = 'prj_a'", params![]).unwrap();
    let gone = crate::Deleted { account: "a".into(), machines: vec![], disbanded: vec![] };
    crate::db::boards::erase_account(&db.c(), &gone).unwrap();
    assert_eq!(count(&db, "SELECT count(*) FROM work_items"), 1);
    assert_eq!(count(&db, "SELECT count(*) FROM work_item_refs"), 1);
    assert_eq!(count(&db, "SELECT count(*) FROM work_items WHERE project_id = 'prj_b'"), 1);
}
