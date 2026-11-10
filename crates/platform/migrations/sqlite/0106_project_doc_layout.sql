-- 0106: a project Doc's layout (Arugula #826): `tabs` or `prototype`. NULL is
-- the plain Doc, `doc`, so every row from before reads as it did.
ALTER TABLE project_docs ADD COLUMN layout TEXT;
