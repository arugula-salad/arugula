-- 0033: messages a product posted for their author (M237, platform step 8):
-- a hud box bound to a project posts a comment-mode review into an object's
-- thread for the person who wrote it, a member who signed in to the box
-- through the hub. `via` is the product's name as the hub registered it
-- (shown as "<name> via <via>"); NULL for everything else.
ALTER TABLE chat_messages ADD COLUMN via TEXT;
