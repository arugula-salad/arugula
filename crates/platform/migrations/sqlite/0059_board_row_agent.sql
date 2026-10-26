-- 0059: who added a board row (boards, step 3). `agent` is the machine
-- whose agent added the row under its `boards.write` grant; NULL for a row
-- a person added. A machine's agents may remove only the rows their own
-- machine added (`db::boards`, `boards::agent`).
ALTER TABLE board_entries ADD COLUMN agent TEXT;
