-- 0089: a person moves a row between lanes (Arugula #494). Lanes follow the
-- work; this is the escape hatch for a row whose derived lane is wrong or
-- stale. `lane_set` (`next`, `doing` or `review`, never `done`: that closes
-- the issue) is the lane a person chose, and `lane_set_basis` the lane the
-- work derived at that moment. A board shows `lane_set` only while the
-- derived lane still equals the basis; once the work changes it, the
-- override is ignored and the next read clears it. `lane_set_by` is the
-- account and `lane_set_at` the time in milliseconds since the epoch. All
-- NULL for a row nobody moved.
--
-- Additive: the build before this one ignores the columns, and shows the
-- derived lane.
ALTER TABLE board_entries ADD COLUMN lane_set TEXT;
ALTER TABLE board_entries ADD COLUMN lane_set_basis TEXT;
ALTER TABLE board_entries ADD COLUMN lane_set_by TEXT;
ALTER TABLE board_entries ADD COLUMN lane_set_at INTEGER;
