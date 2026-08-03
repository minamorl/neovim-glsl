vim.opt.termguicolors = true
vim.opt.number = false
vim.opt.laststatus = 0
vim.opt.ruler = false
-- Each label is drawn in the attribute it names, so the render is readable as
-- its own caption.
local words = {
  { 'underline',         { underline = true,     sp = '#ff5f5f' } },
  { 'undercurl',         { undercurl = true,     sp = '#5fff87' } },
  { 'underdouble',       { underdouble = true,   sp = '#5fafff' } },
  { 'underdotted',       { underdotted = true,   sp = '#ffd75f' } },
  { 'underdashed',       { underdashed = true,   sp = '#5fffff' } },
  { 'strikethrough',     { strikethrough = true, sp = '#ff5fff' } },
  { 'bold',              { bold = true } },
  { 'italic',            { italic = true } },
  { 'bold+italic+under', { bold = true, italic = true, underline = true, sp = '#ff5f5f' } },
}
local parts, spans, col = {}, {}, 0
for i, w in ipairs(words) do
  parts[i] = w[1]
  spans[i] = { col, col + #w[1] }
  col = col + #w[1] + 2
end
vim.api.nvim_buf_set_lines(0, 0, -1, false, { table.concat(parts, '  ') })
local ns = vim.api.nvim_create_namespace('evidence')
for i, w in ipairs(words) do
  vim.api.nvim_set_hl(0, 'Ev' .. i, w[2])
  vim.api.nvim_buf_set_extmark(0, ns, 0, spans[i][1], { end_col = spans[i][2], hl_group = 'Ev' .. i })
end
