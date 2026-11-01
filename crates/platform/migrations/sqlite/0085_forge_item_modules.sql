-- 0085: the modules (names, a JSON list) a Plane work item is in (Arugula
-- #475), kept beside its labels (`forge_items.labels`, which a Plane row
-- now fills with the labels' names). Plane's work item doesn't say its
-- modules, so the sync reads each module's items and sets this. Additive:
-- the build before this one ignores the column. A row mirrored before it
-- reads `[]` until the next sync; its labels fill in when the item next
-- changes.
ALTER TABLE forge_items ADD COLUMN modules TEXT NOT NULL DEFAULT '[]';
