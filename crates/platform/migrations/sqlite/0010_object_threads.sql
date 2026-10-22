-- 0010: object threads (M196, platform step 6, #1263). A chat room of
-- kind `object` is the thread on one global id: a native issue (`iss_`),
-- a mirrored GitHub PR or issue (`fi_`), later a workspace (`cw_`). Its
-- `scope` is `project:<id>` and `project_id` that project, with no
-- `machine`, so `chat_rooms_target` doesn't cover it: this index keeps it
-- to one room per target in a project. A repo relinked to another project
-- gets a new room there; the old one stays the old project's. Made the
-- first time someone opens it.
CREATE UNIQUE INDEX chat_rooms_object ON chat_rooms (kind, scope, target) WHERE kind = 'object';
