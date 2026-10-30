-- :checkhealth arugula (#1733): the daemon's socket, whether a daemon
-- answers on it and which release, and the Arugula pane this nvim is
-- tied to.

local M = {}

local uv = vim.uv or vim.loop

-- GET /api/host on the daemon's socket, waited for (at most 3 s): its
-- release, or why not.
local function ask_daemon(sock)
  local pipe = uv.new_pipe(false)
  local buf, done, err = "", false, nil
  local ok, e = pcall(function()
    pipe:connect(sock, function(cerr)
      if cerr then
        err, done = cerr, true
        return
      end
      pipe:write("GET /api/host HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n")
      pipe:read_start(function(rerr, data)
        if rerr then
          err, done = rerr, true
        elseif data then
          buf = buf .. data
        else
          done = true
        end
      end)
    end)
  end)
  if not ok then return nil, tostring(e) end
  vim.wait(3000, function() return done end, 20)
  pcall(function() pipe:close() end)
  if err then return nil, err end
  if buf == "" then return nil, "it didn't answer within 3 s" end
  local status = buf:match("^HTTP/1%.1 (%d+)")
  if status ~= "200" then return nil, "it answered " .. (buf:match("^[^\r\n]*") or "nothing") end
  local body = buf:match("\r\n\r\n(.*)$") or ""
  local json = body:match("{.*}")
  local dok, info = pcall(vim.json.decode, json or "")
  if not dok or type(info) ~= "table" then return nil, "its answer wasn't JSON" end
  return info.version or "?", nil
end

-- This plugin's own folder, and whether Arugula installed it (its marker).
local function install_info()
  local src = debug.getinfo(1, "S").source:gsub("^@", "")
  local root = vim.fn.fnamemodify(src, ":p:h:h:h")
  local f = io.open(root .. "/ARUGULA_INSTALLED", "r")
  if not f then return root, nil end
  local version = (f:read("*a") or ""):match("version=([^\n]+)")
  f:close()
  return root, version or "?"
end

function M.check()
  local h = vim.health
  local state = require("arugula").health_state()

  h.start("arugula.nvim")
  if vim.fn.has("nvim-0.10") == 1 then
    h.ok("nvim " .. tostring(vim.version()))
  else
    h.warn("nvim " .. tostring(vim.version()) .. " is older than 0.10, which arugula.nvim needs")
  end
  local root, installed = install_info()
  if installed then
    h.info(("Installed by Arugula at %s (version %s). The daemon refreshes it when it's upgraded; Settings > Editor > Remove arugula.nvim deletes it."):format(root, installed))
  else
    h.info("Loaded from " .. root .. " (installed by you or your plugin manager; Arugula doesn't touch it)")
  end

  h.start("arugula: daemon")
  local sock = state.socket
  local st = uv.fs_stat(sock)
  if not st then
    h.error("No daemon socket at " .. sock, {
      "Is arugulad running on this machine? Over ssh, it's the daemon on the remote machine.",
      "$ARUGULA_SOCK, or vim.g.arugula = { socket = ... }, says where it is if it's elsewhere.",
    })
  elseif st.type ~= "socket" then
    h.error(sock .. " isn't a socket (it's a " .. st.type .. ")")
  else
    h.ok("Socket: " .. sock)
    local version, err = ask_daemon(sock)
    if version then
      h.ok("Daemon: arugulad " .. version)
    else
      h.error("No daemon answers on " .. sock .. ": " .. tostring(err), {
        "The socket is left from a daemon that stopped. Start arugulad again.",
      })
    end
  end

  h.start("arugula: pane")
  if state.pane then
    if state.tied == state.pane then
      h.ok(("Tied to pane %%%d"):format(state.pane))
    elseif state.id then
      h.warn(("$ARUGULA_PANE says pane %%%d, but the daemon made it an entry of its own (%%%d)"):format(state.pane, state.id), {
        "That pane is another daemon's: $ARUGULA_PANE came along over ssh, or from a second daemon.",
      })
    elseif state.on then
      h.warn(("In pane %%%d, not connected yet"):format(state.pane))
    else
      h.info(("In pane %%%d, but :ArugulaLeave took it out"):format(state.pane))
    end
  elseif state.id then
    h.ok(("Not in an Arugula pane; in the swarm as entry %%%d"):format(state.id))
  else
    h.info("Not in an Arugula pane, and not in the swarm (:ArugulaJoin puts this folder in)")
  end
end

return M
