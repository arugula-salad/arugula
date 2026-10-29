-- The page's colors in an Arugula nvim editor block (#1734).
--
-- The daemon sends them (`theme` on the editor link) when the plugin joins
-- as a block's pane, and again whenever the page's palette changes. They're
-- the page's palette as highlight groups (made by the page, as it makes VS
-- Code's colors), applied here as the colorscheme `arugula`: groups set
-- with nvim_set_hl over a cleared highlight table, 'background' and the
-- :terminal colors.
--
-- Your own colorscheme wins:
--   vim.g.arugula = { theme = false }   never apply them
--   :colorscheme other                  stop applying them in this nvim

local M = {}

local applying = false
local left = false
local watching = false
local last = nil

local function kept()
  local o = vim.g.arugula
  return type(o) == "table" and o.theme == false
end

-- A colorscheme picked after ours is the user's: from then on, theirs.
local function watch()
  if watching then return end
  watching = true
  vim.api.nvim_create_autocmd("ColorScheme", {
    group = vim.api.nvim_create_augroup("arugula_theme", { clear = true }),
    callback = function()
      if not applying and vim.g.colors_name ~= "arugula" then left = true end
    end,
  })
end

--- Applies `t` ({ light, groups = { Name = spec }, terminal = { 16 colors } }).
--- Says what it did: "applied", "kept" (theme = false), "left" (another
--- colorscheme was picked) or the error.
function M.apply(t)
  if kept() then return "kept" end
  if left then return "left" end
  if type(t) ~= "table" or type(t.groups) ~= "table" then return "no colors" end
  applying = true
  local ok, err = pcall(function()
    vim.o.termguicolors = true
    vim.cmd("highlight clear")
    if vim.fn.exists("syntax_on") == 1 then vim.cmd("syntax reset") end
    vim.o.background = t.light and "light" or "dark"
    -- 'background' can load a colorscheme's other half: start clean again.
    vim.cmd("highlight clear")
    for name, spec in pairs(t.groups) do
      vim.api.nvim_set_hl(0, name, spec)
    end
    for i, c in ipairs(t.terminal or {}) do
      vim.g["terminal_color_" .. (i - 1)] = c
    end
    vim.g.colors_name = "arugula"
    vim.api.nvim_exec_autocmds("ColorScheme", { pattern = "arugula", modeline = false })
  end)
  applying = false
  if not ok then return tostring(err) end
  last = t
  watch()
  return "applied"
end

--- What it last did, for :checkhealth and tests.
function M.state()
  return { applied = last ~= nil and not left and not kept(), light = last and last.light or nil, left = left, kept = kept() }
end

return M
