-- arugula.nvim: your nvim in the arugula swarm (M28). Options go in
-- vim.g.arugula (`{ socket = "/path/to/sock" }`) before this runs.
if vim.g.loaded_arugula then return end
vim.g.loaded_arugula = true

local arugula = require("arugula")
arugula.setup(vim.g.arugula or {})

vim.api.nvim_create_user_command("ArugulaJoin", arugula.join, { desc = "Show this folder in the arugula swarm" })
vim.api.nvim_create_user_command("ArugulaLeave", arugula.leave, { desc = "Take this folder out of the arugula swarm" })
vim.api.nvim_create_user_command("ArugulaStatus", function()
  local s = arugula.status()
  vim.notify(s == "" and "arugula: not in the swarm" or ("arugula: " .. s))
end, { desc = "Whether this nvim is in the swarm, and who follows it" })
