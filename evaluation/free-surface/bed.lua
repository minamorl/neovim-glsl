-- A plain editing session for the free surfaces to sit over.
--
-- Nothing here knows about panels. That is the point: the surface is drawn by
-- the host in pixels, and Neovim's grid is unaware it is being covered.
vim.opt.termguicolors = true
vim.opt.number = true
vim.opt.laststatus = 2
vim.opt.ruler = true

local lines = {}
for i = 1, 40 do
  lines[i] = string.format(
    '%3d  the grid keeps painting underneath, cell by cell, unaware  %s',
    i,
    string.rep('.', 8)
  )
end
lines[7] = '  7  日本語の行もそのまま下に見えている（フォールバック）'
vim.api.nvim_buf_set_lines(0, 0, -1, false, lines)
vim.api.nvim_win_set_cursor(0, { 3, 0 })
