-- 0110: the machine whose agent made a project Doc for its owner (Arugula
-- #862), so the Doc says "<owner>'s agent on <machine>". NULL for any other
-- Doc, as every row from before.
ALTER TABLE project_docs ADD COLUMN made_by_agent TEXT;
