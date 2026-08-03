local root = debug.getinfo(1, "S").source:sub(2):match("(.+)/tests/[^/]+$")
local recorder = dofile(root .. "/lua/api_recorder.lua")
local case = os.getenv("RECORDER_TEST_CASE")

local function assert_eq(actual, expected, label)
  if actual ~= expected then
    error(string.format("%s: expected %s, got %s", label, vim.inspect(expected), vim.inspect(actual)))
  end
end

if case == "multiple_return_values" then
  vim.api.nvim_protocol_surface_test_multi = function()
    return "first", "second", nil, "fourth"
  end
end

recorder.install()

if case == "return_value" then
  local buf = vim.api.nvim_create_buf(false, true)
  assert_eq(type(buf), "number", "single return value")
elseif case == "multiple_return_values" then
  local a, b, c, d = vim.api.nvim_protocol_surface_test_multi()
  assert_eq(a, "first", "multiple return value 1")
  assert_eq(b, "second", "multiple return value 2")
  assert_eq(c, nil, "multiple return value 3")
  assert_eq(d, "fourth", "multiple return value 4")
elseif case == "error_propagation" then
  local ok, err = pcall(vim.api.nvim_buf_get_lines, -1, 0, 1, false)
  if ok then
    error("expected invalid buffer error to propagate")
  end
  if not tostring(err):match("Invalid buffer") then
    error("unexpected propagated error: " .. tostring(err))
  end
else
  error("unknown RECORDER_TEST_CASE: " .. tostring(case))
end

local seen = {}
for _, row_entry in ipairs(recorder.snapshot()) do
  seen[row_entry.name] = row_entry.count
end
if case == "return_value" then
  assert_eq(seen.nvim_create_buf, 1, "create_buf count")
elseif case == "multiple_return_values" then
  assert_eq(seen.nvim_protocol_surface_test_multi, 1, "multi-return count")
elseif case == "error_propagation" then
  assert_eq(seen.nvim_buf_get_lines, 1, "erroring call count")
end
