-- 0101: a team Doc made from a machine Doc's copy (Arugula #418, Share
-- with…). A snapshot: the team owns and edits it, and these say where it
-- came from. `made_from_doc` is the copy's `pdc_` and `made_from_project`
-- the project it was in; `made_from_rev` is the copy's rev at the snapshot
-- and `made_rev` this Doc's own rev right after it, so "the source changed"
-- is the copy's rev above the first and "the team edited it" is this
-- Doc's above the second. All NULL for any other Doc.
ALTER TABLE project_docs ADD COLUMN made_from_project TEXT;
ALTER TABLE project_docs ADD COLUMN made_from_doc TEXT;
ALTER TABLE project_docs ADD COLUMN made_from_rev INTEGER;
ALTER TABLE project_docs ADD COLUMN made_rev INTEGER;
-- One team Doc per copy per project.
CREATE UNIQUE INDEX project_docs_made_from ON project_docs (made_from_doc, project_id)
    WHERE made_from_doc IS NOT NULL;
