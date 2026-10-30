-- Starter settings for an Arugula nvim editor block whose user has no nvim
-- config. Built-in options and netrw only; see :help arugula-starter.
local M = {}

local function exists(path)
  return vim.uv.fs_stat(path) ~= nil
end

--- Whether this nvim has a config of its own: an init.lua or init.vim in
--- stdpath("config"), $VIMINIT, or -u / --clean on the command line.
function M.has_config()
  local dir = vim.fn.stdpath("config")
  if exists(dir .. "/init.lua") or exists(dir .. "/init.vim") then return true end
  if (vim.env.VIMINIT or "") ~= "" then return true end
  for _, a in ipairs(vim.v.argv or {}) do
    if a == "-u" or a == "--clean" then return true end
  end
  return false
end

--- Applies the settings unless there is a config. Whether it did.
function M.apply()
  if M.has_config() then return false end

  vim.g.mapleader = " " -- space is easy to reach: <leader>e below
  local o = vim.o
  o.mouse = "a" -- click, drag and scroll with the mouse
  if vim.fn.has("clipboard") == 1 then o.clipboard = "unnamedplus" end -- yank and paste share the system clipboard
  o.number = true -- line numbers
  o.signcolumn = "yes" -- diagnostics don't shift the text sideways
  o.ignorecase = true -- search ignores case...
  o.smartcase = true -- ...until you type a capital
  o.undofile = true -- undo survives closing the file
  o.splitright = true -- :vsplit opens on the right
  o.splitbelow = true -- :split opens below
  o.scrolloff = 5 -- keep lines of context around the cursor
  vim.g.netrw_banner = 0 -- no help banner at the top of the file tree
  vim.g.netrw_liststyle = 3 -- netrw as a tree
  vim.keymap.set("n", "<leader>e", "<cmd>Lexplore<cr>", { desc = "Toggle the file tree" }) -- open or close it at the side
  vim.keymap.set("n", "<Esc>", "<cmd>nohlsearch<cr>", { desc = "Clear search highlight" }) -- Esc clears the highlight

  vim.api.nvim_create_autocmd("VimEnter", {
    once = true,
    callback = function()
      vim.notify("Arugula's starter settings are on (no nvim config found). :help arugula-starter")
    end,
  })
  return true
end

return M
