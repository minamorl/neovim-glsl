local M = {}

local originals = {}
local counts = {}
local installed = false

local function is_api_function(name, value)
  return type(name) == "string" and name:match("^nvim_") and type(value) == "function"
end

local function wrap(name, fn)
  return function(...)
    counts[name] = (counts[name] or 0) + 1
    return fn(...)
  end
end

function M.install()
  if installed then
    return M
  end
  for name, value in pairs(vim.api) do
    if is_api_function(name, value) then
      originals[name] = value
      vim.api[name] = wrap(name, value)
    end
  end
  installed = true
  return M
end

function M.reset()
  counts = {}
end

function M.snapshot()
  local out = {}
  for name, count in pairs(counts) do
    out[#out + 1] = { name = name, count = count }
  end
  table.sort(out, function(a, b)
    if a.count == b.count then
      return a.name < b.name
    end
    return a.count > b.count
  end)
  return out
end

function M.dump(path)
  local encoded = vim.json.encode(M.snapshot())
  vim.fn.writefile({ encoded }, path)
end

function M.uninstall()
  if not installed then
    return M
  end
  for name, value in pairs(originals) do
    vim.api[name] = value
  end
  originals = {}
  installed = false
  return M
end

return M
