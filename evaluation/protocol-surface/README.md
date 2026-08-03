# protocol-surface - evidence, not a decision

This directory measures protocol surface area for:

- `open_question neovim_glsl.protocol_surface_scope`
- `open_question neovim_glsl.telescope_realization_decision`

It decides nothing. `protocol_surface_scope`, `telescope_realization_decision`, and
`embed_candidate_disposition` remain open.

The measurement was produced by `driver.py` against real embedded Neovim and the
owner's local telescope plugins. Raw output is in `out/measurement.json`.

Measured Neovim:

```text
NVIM v0.11.5
```

The recorder was installed before sourcing telescope's plugin file and before
running telescope setup. It records call counts only, not arguments.

## Driven session

The driver creates a deterministic scratch tree under `out/scratch`, attaches a
UI with `rgb`, `ext_linegrid`, `ext_multigrid`, `ext_popupmenu`, `ext_cmdline`,
and `ext_messages`, then drives interaction through `nvim_input`.

Plugin paths used:

- `plenary.nvim`: `/Users/minamorl/.local/share/nvim/lazy/plenary.nvim`
- `telescope.nvim`: `/Users/minamorl/.local/share/nvim/lazy/telescope.nvim`
- `telescope-file-browser.nvim`: `/Users/minamorl/.local/share/nvim/lazy/telescope-file-browser.nvim`
- `nvim-web-devicons`: `/Users/minamorl/.local/share/nvim/lazy/nvim-web-devicons`

Keys used:

- `<space>o` for `Telescope find_files`
- typed `alpha`
- `<C-n>` to move the selection
- `<CR>` to accept
- `<leader>e` for `Telescope file_browser path=%:p:h select_buffer=true`
- typed `move`
- `<C-n>` to move the selection
- `<Esc>` to close file-browser

## API protocol surface observed

This telescope session touched **52 distinct `nvim_*` API functions**.

| calls | function |
|---:|---|
| 772 | `nvim_get_hl` |
| 686 | `nvim_set_hl` |
| 181 | `nvim_buf_is_valid` |
| 153 | `nvim_replace_termcodes` |
| 148 | `nvim_buf_set_keymap` |
| 136 | `nvim_buf_line_count` |
| 131 | `nvim_buf_set_extmark` |
| 102 | `nvim__get_runtime` |
| 64 | `nvim_win_get_width` |
| 56 | `nvim_buf_get_lines` |
| 52 | `nvim_win_get_height` |
| 51 | `nvim_get_current_buf` |
| 47 | `nvim_win_get_cursor` |
| 42 | `nvim_get_option_value` |
| 38 | `nvim_buf_clear_namespace` |
| 28 | `nvim_buf_set_lines` |
| 27 | `nvim_set_option_value` |
| 26 | `nvim_buf_get_extmarks` |
| 21 | `nvim_buf_set_text` |
| 17 | `nvim_win_is_valid` |
| 16 | `nvim_buf_set_option` |
| 14 | `nvim_exec2` |
| 13 | `nvim_create_autocmd` |
| 9 | `nvim_create_namespace` |
| 9 | `nvim_win_set_cursor` |
| 8 | `nvim_create_buf` |
| 8 | `nvim_open_win` |
| 8 | `nvim_win_set_option` |
| 7 | `nvim_buf_get_name` |
| 7 | `nvim_create_augroup` |
| 6 | `nvim_win_get_buf` |
| 5 | `nvim_exec_autocmds` |
| 4 | `nvim_buf_add_highlight` |
| 4 | `nvim_get_mode` |
| 4 | `nvim_set_keymap` |
| 4 | `nvim_win_get_position` |
| 3 | `nvim_clear_autocmds` |
| 3 | `nvim_get_current_win` |
| 3 | `nvim_set_current_win` |
| 3 | `nvim_win_close` |
| 2 | `nvim_buf_attach` |
| 2 | `nvim_buf_delete` |
| 2 | `nvim_buf_is_loaded` |
| 2 | `nvim_call_function` |
| 2 | `nvim_create_user_command` |
| 2 | `nvim_feedkeys` |
| 2 | `nvim_get_current_line` |
| 2 | `nvim_get_current_tabpage` |
| 2 | `nvim_get_option_info2` |
| 2 | `nvim_list_tabpages` |
| 1 | `nvim_cmd` |
| 1 | `nvim_list_bufs` |

Observation for the API-protocol candidate answer: running telescope as the Lua
plugin itself requires an own host to provide at least the 52 observed API
functions for this session, with the behavior those calls rely on. This number
does not include unvisited telescope actions, other pickers, other extensions,
or other owner plugins.

## UI protocol surface observed

The same run observed **25 distinct `redraw` event names**. Comparing those
names with the match arms in
`evaluation/candidate-embed-opengl/src/screen.rs` and
`evaluation/candidate-embed-opengl/src/ext_ui.rs`, the existing client handles
**15** observed event names and ignores **10**.

| calls | event | existing candidate |
|---:|---|---|
| 2 | `chdir` | ignored |
| 1 | `default_colors_set` | implemented |
| 17 | `flush` | implemented |
| 1 | `grid_clear` | implemented |
| 9 | `grid_cursor_goto` | implemented |
| 4 | `grid_destroy` | implemented |
| 234 | `grid_line` | implemented |
| 10 | `grid_resize` | implemented |
| 353 | `hl_attr_define` | implemented |
| 136 | `hl_group_set` | implemented |
| 5 | `mode_change` | ignored |
| 1 | `mode_info_set` | ignored |
| 1 | `mouse_on` | ignored |
| 16 | `msg_ruler` | implemented |
| 4 | `msg_showcmd` | implemented |
| 13 | `msg_showmode` | implemented |
| 28 | `option_set` | ignored |
| 1 | `set_icon` | ignored |
| 1 | `set_title` | ignored |
| 17 | `update_menu` | ignored |
| 4 | `win_close` | implemented |
| 8 | `win_float_pos` | implemented |
| 2 | `win_pos` | implemented |
| 32 | `win_viewport` | ignored |
| 35 | `win_viewport_margins` | ignored |

Observation for the UI-protocol-only candidate answer: the UI face involved in
this session is 25 observed redraw event names, of which the existing candidate
already implements 15 by event name and ignores 10. A UI-only host does not, by
itself, run telescope as a Lua plugin; telescope-equivalent file navigation
would need to come from a separate picker implementation or from some other
non-API mechanism.

## Reproducing

Run tests without a GUI:

```sh
PATH=/opt/homebrew/bin:$PATH python3 -m unittest discover -s evaluation/protocol-surface/tests -v
```

Run the measurement:

```sh
PATH=/opt/homebrew/bin:$PATH python3 evaluation/protocol-surface/driver.py
```
