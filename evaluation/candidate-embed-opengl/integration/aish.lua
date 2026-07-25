-- Read-only aish surface for the embed+OpenGL evaluation candidate.
--
-- The host deliberately exposes discovery, status, and typed-object inspection
-- only. Execution stays absent until the effect-confirmation surface has passed
-- its human gate.

if vim.g.nvimgl_aish_commands_installed then
  return
end
vim.g.nvimgl_aish_commands_installed = true

local channel = ...
assert(type(channel) == "number", "nvimgl must provide its embedded RPC channel")

vim.api.nvim_create_user_command("AishDiscover", function()
  vim.rpcnotify(channel, "nvimgl_aish", "discover")
end, {
  desc = "Show aish's structured, model-free discovery surface",
})

vim.api.nvim_create_user_command("AishStatus", function()
  vim.rpcnotify(channel, "nvimgl_aish", "status")
end, {
  desc = "Show the resident aish AI backend's read-only status",
})

vim.api.nvim_create_user_command("AishInspect", function(command)
  local kind = command.fargs[1]
  local identity = table.concat(command.fargs, " ", 2)
  vim.rpcnotify(channel, "nvimgl_aish", "inspect", kind, identity)
end, {
  nargs = "+",
  desc = "Inspect a typed aish object without executing a command",
  complete = function(argument_lead, command_line)
    local kinds = { "file", "process", "port", "service", "log", "executable", "repository" }
    if command_line:match("^AishInspect%s+%S+%s+") then
      return {}
    end
    return vim.tbl_filter(function(kind)
      return vim.startswith(kind, argument_lead)
    end, kinds)
  end,
})
