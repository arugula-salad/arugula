-- arugula.nvim's diffs (#1732): an edit Claude Code proposes, as a vimdiff
-- tab, in step with the diff card on its pane and on the swarm's rail.
--
-- When Claude Code runs in the same Arugula session as this nvim, the daemon
-- sends each edit it proposes (`diff`, see the daemon's editor/link.rs). It
-- opens in a tab of its own: the file as it is on the left, the proposed
-- text on the right, and a header naming the agent, the file and the keys.
--
--   accept          <leader>aa, :ArugulaAccept   (the right side as it is
--                   now: edit it first to accept your changes instead)
--   reject          <leader>ar, :ArugulaReject
--
-- The daemon owns the answer: answering here, on the card, or in Claude
-- Code's terminal closes the tab and the card (`diff_close`). Several wait
-- as a tab each, in the order they came; the first takes focus, the rest
-- wait behind it. Closing a tab without answering leaves the card.
--
-- The keys are set in setup(): `keys = { accept = "<leader>ya", reject =
-- false }` (false: no key, the command only). See :help arugula-diff.

local M = {}

local D = {
  send = function() end,
  keys = { accept = "<leader>aa", reject = "<leader>ar" },
  open = {}, -- id -> { tab, left, right, eol, tick, name }
  order = {}, -- ids, in the order they came
}

local function lines_of(text)
  local lines = vim.split(text or "", "\n", { plain = true })
  local eol = #lines > 1 and lines[#lines] == ""
  if eol then table.remove(lines) end
  return lines, eol
end

local function scratch(name, lines, ft, modifiable)
  local buf = vim.api.nvim_create_buf(false, true)
  vim.bo[buf].bufhidden = "wipe"
  vim.bo[buf].swapfile = false
  vim.api.nvim_buf_set_lines(buf, 0, -1, false, lines)
  pcall(vim.api.nvim_buf_set_name, buf, name)
  if ft then vim.bo[buf].filetype = ft end
  vim.bo[buf].modifiable = modifiable
  vim.bo[buf].modified = false
  return buf
end

local function bar(s)
  return (s:gsub("%%", "%%%%"))
end

local function key_help()
  local parts = {}
  if D.keys.accept then table.insert(parts, D.keys.accept .. " accept") end
  if D.keys.reject then table.insert(parts, D.keys.reject .. " reject") end
  table.insert(parts, "edit the right side, then accept, to change it")
  return table.concat(parts, " | ")
end

-- The diff the current tab shows, if it's one of ours.
local function here()
  local ok, id = pcall(function() return vim.t.arugula_diff end)
  if ok and id and D.open[id] then return id end
end

local function forget(id)
  D.open[id] = nil
  for i, x in ipairs(D.order) do
    if x == id then
      table.remove(D.order, i)
      break
    end
  end
end

-- Close a diff's tab (if it's still there), and go to the next one waiting
-- when it was the one in front.
local function close(id)
  local d = D.open[id]
  if not d then return end
  forget(id)
  local front = vim.api.nvim_get_current_tabpage() == d.tab
  if vim.api.nvim_tabpage_is_valid(d.tab) then
    if #vim.api.nvim_list_tabpages() > 1 then
      vim.cmd("tabclose " .. vim.api.nvim_tabpage_get_number(d.tab))
    else
      -- The last tab can't close: leave an empty buffer in it.
      vim.api.nvim_set_current_tabpage(d.tab)
      vim.cmd("silent! diffoff! | silent! only | enew")
      vim.t.arugula_diff = nil
    end
  end
  for _, b in ipairs({ d.left, d.right }) do
    if vim.api.nvim_buf_is_valid(b) then pcall(vim.api.nvim_buf_delete, b, { force = true }) end
  end
  local next = D.order[1] and D.open[D.order[1]]
  if front and next and vim.api.nvim_tabpage_is_valid(next.tab) then vim.api.nvim_set_current_tabpage(next.tab) end
end

local function answer(accept)
  local id = here()
  if not id then
    vim.notify("arugula: no proposed edit in this tab", vim.log.levels.WARN)
    return
  end
  local d = D.open[id]
  local msg = { t = accept and "diff_accept" or "diff_reject", id = id }
  if accept and vim.api.nvim_buf_is_valid(d.right) and vim.b[d.right].changedtick ~= d.tick then
    local lines = vim.api.nvim_buf_get_lines(d.right, 0, -1, false)
    msg.text = table.concat(lines, "\n") .. (d.eol and "\n" or "")
  end
  D.send(msg)
  close(id)
  if accept then
    -- Claude Code writes the file a moment later: show it.
    vim.defer_fn(function() pcall(vim.cmd, "silent! checktime") end, 1000)
  end
end

function M.accept() answer(true) end

function M.reject() answer(false) end

-- The daemon's `diff`: open it in a tab of its own.
function M.open(m)
  if type(m.id) ~= "string" then return end
  if D.open[m.id] then return end
  local front = #D.order == 0
  local was = vim.api.nvim_get_current_tabpage()
  local name = m.name or m.file or "?"
  local ft = m.file and vim.filetype.match({ filename = m.file }) or nil
  local old = lines_of(m.old)
  local new, eol = lines_of(m.new)
  local left = scratch(("arugula://%s/now/%s"):format(m.id, name), old, ft, false)
  local right = scratch(("arugula://%s/proposed/%s"):format(m.id, name), new, ft, true)
  local after = D.order[#D.order] and D.open[D.order[#D.order]]
  if after and vim.api.nvim_tabpage_is_valid(after.tab) then
    vim.cmd(vim.api.nvim_tabpage_get_number(after.tab) .. "tabnew")
  else
    vim.cmd("$tabnew")
  end
  local tab = vim.api.nvim_get_current_tabpage()
  local blank = vim.api.nvim_get_current_buf()
  local lw = vim.api.nvim_get_current_win()
  vim.api.nvim_win_set_buf(lw, left)
  if vim.api.nvim_buf_is_valid(blank) and blank ~= left then pcall(vim.api.nvim_buf_delete, blank, { force = true }) end
  vim.cmd("diffthis")
  vim.cmd("rightbelow vsplit")
  local rw = vim.api.nvim_get_current_win()
  vim.api.nvim_win_set_buf(rw, right)
  vim.cmd("diffthis")
  local agent = m.agent or "An agent"
  local what = m.new_file and "create" or "edit"
  vim.wo[lw].winbar = bar(("%s: %s as it is"):format(name, m.new_file and "new file" or "the file"))
  vim.wo[rw].winbar = bar(("%s wants to %s %s | %s"):format(agent, what, name, key_help()))
  vim.t[tab].arugula_diff = m.id
  for _, b in ipairs({ left, right }) do
    vim.b[b].arugula_diff = m.id
    local opts = { buffer = b, silent = true }
    if D.keys.accept then vim.keymap.set("n", D.keys.accept, M.accept, vim.tbl_extend("keep", { desc = "Arugula: accept the proposed edit" }, opts)) end
    if D.keys.reject then vim.keymap.set("n", D.keys.reject, M.reject, vim.tbl_extend("keep", { desc = "Arugula: reject the proposed edit" }, opts)) end
  end
  D.open[m.id] = { tab = tab, left = left, right = right, eol = eol, tick = vim.b[right].changedtick, name = name }
  table.insert(D.order, m.id)
  -- The tab closed without an answer: the card still waits.
  vim.api.nvim_create_autocmd("BufWipeout", {
    buffer = right,
    once = true,
    callback = function()
      if D.open[m.id] and D.open[m.id].right == right then
        local d = D.open[m.id]
        forget(m.id)
        vim.schedule(function()
          if vim.api.nvim_buf_is_valid(d.left) then pcall(vim.api.nvim_buf_delete, d.left, { force = true }) end
        end)
      end
    end,
  })
  if not front and vim.api.nvim_tabpage_is_valid(was) then vim.api.nvim_set_current_tabpage(was) end
end

-- The daemon's `diff_close`: answered somewhere (here, the card, Claude
-- Code's terminal) or gone.
function M.closed(m)
  if type(m.id) == "string" then close(m.id) end
end

-- The connection went: every tab goes (the daemon sends what still waits
-- again when it's back).
function M.reset()
  for _, id in ipairs(vim.deepcopy(D.order)) do close(id) end
end

-- The ids of the diffs open, in order (for tests and statuslines).
function M.list()
  return vim.deepcopy(D.order)
end

function M.setup(keys, send)
  D.send = send or D.send
  keys = keys or {}
  for _, k in ipairs({ "accept", "reject" }) do
    if keys[k] ~= nil then D.keys[k] = keys[k] end
  end
  vim.api.nvim_create_user_command("ArugulaAccept", M.accept, { desc = "Accept the edit this tab proposes (as edited)" })
  vim.api.nvim_create_user_command("ArugulaReject", M.reject, { desc = "Reject the edit this tab proposes" })
end

return M
