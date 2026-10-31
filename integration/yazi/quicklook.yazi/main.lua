-- Optional Yazi adapter. All decoding and window logic stays in Rust.
local snapshot = ya.sync(function(state)
  local paths = {}
  for _, file in pairs(cx.active.selected) do paths[#paths + 1] = tostring(file.url) end
  if #paths == 0 and cx.active.current.hovered then
    paths[1] = tostring(cx.active.current.hovered.url)
  end
  table.sort(paths)
  return paths, state.cli or "@QUICKLOOK_CLI@", state.active or false
end)
local active = ya.sync(function(state, value) state.active = value end)

return {
  setup = function(state, options)
    state.cli = (options or {}).cli or "@QUICKLOOK_CLI@"
    state.active = false
    ps.sub("hover", function()
      if state.active then ya.emit("plugin", { "quicklook", "sync" }) end
    end)
    ps.sub("cd", function()
      if state.active then ya.emit("plugin", { "quicklook", "sync" }) end
    end)
  end,
  entry = function(_, job)
    local mode = job.args[1] or "toggle"
    local paths, cli, enabled = snapshot()
    if mode == "close" then
      active(false)
      Command(cli):arg("--close"):status()
      return
    end
    if mode == "sync" then
      if not enabled then return end
      ya.sleep(0.025)
      paths, cli, enabled = snapshot()
      if not enabled then return end
    end
    if #paths == 0 then return end
    local args = {}
    if mode == "sync" then args[1] = "--set-selection"
    elseif mode == "show" then args[1] = "--show" end
    args[#args + 1] = "--"
    for _, path in ipairs(paths) do args[#args + 1] = path end
    local result, err = Command(cli):arg(args):status()
    if not result or not result.success then
      active(false)
      ya.notify { title = "Quick Look", content = tostring(err or "无法打开预览"), level = "error", timeout = 4 }
      return
    end
    local output = Command(cli):arg("--status"):output()
    active(output ~= nil and output.stdout:match('"visible"%s*:%s*true') ~= nil)
  end,
}

