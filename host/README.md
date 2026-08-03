# host — the own host

The architecture `spec neovim-glsl@0.6` selected is **own host speaking Neovim
protocol** (`pin architecture_choice`). Until now this repository held evidence
for that choice and no implementation of it. This directory is the
implementation.

There is no Neovim process. The editing core is here, and what used to sit at
the far end of `nvim --embed` now sits at the far end of a pipe inside this
program.

```
┌──────────────────────────────┐        ┌───────────────────────────────┐
│ editing core                 │        │ UI client                     │
│  buffer · motions · operators│msgpack │  screen model                 │
│  registers · undo · `:` line │◀──────▶│  GLSL grid renderer           │
│  Neovim protocol server      │  pipe  │  root-ui navigation surface   │
└──────────────────────────────┘        └───────────────────────────────┘
```

The two halves meet **only** over encoded msgpack. That is not decoration: the
same server also answers on stdin/stdout (`--embed`), so "speaks the Neovim
protocol" is checkable from outside the program rather than asserted from
inside it. `tests/protocol_face.rs` runs the built binary as a separate process,
attaches with `nvim_ui_attach`, types, and reads the text back out of the
`redraw` stream.

## Running it

```bash
export PATH="$HOME/.rustup/toolchains/stable-aarch64-apple-darwin/bin:$PATH"
cd host
cargo build
./target/debug/nvimglsl                    # a scratch note
./target/debug/nvimglsl path/to/note.md
```

Write a frame to a PNG instead of opening a window:

```bash
./target/debug/nvimglsl README.md --snapshot /tmp/shot.png --input 'GO# a new line<Esc>'
```

Serve the protocol to some other UI client, the way `nvim --embed` does:

```bash
./target/debug/nvimglsl --embed
```

## Keys

The baseline is the owner's existing keymap (`pin keymap_preservation`), so
nothing here is a new key language:

- motions `h j k l w W b B e E 0 ^ $ gg G { } f t F T %`, counts, `<C-d>/<C-u>/<C-f>/<C-b>`, `zz/zt/zb`
- operators `d c y > <` times any motion, doubled (`dd`, `cc`, `yy`, `>>`), and in visual mode
- `i a I A o O x X s S D C r J ~ p P u <C-r> v V n N`
- registers `"a` … `"z`, `:w :wq :q :q! :e :s/// :%s///g :noh`, `/` and `?`

Two additions, both because the primary object is a markdown note:

| key | what |
| --- | --- |
| `<Space>o`, `<Space>n`, `:Notes` | navigate the note vault |
| `<Space>f`, `:Files` | navigate the working tree instead |
| `gf` | follow the `[[wiki link]]` under the cursor |
| `:Note <title>` | create a note in the vault and open it |

## The navigation surface is root-ui

`pin navigation_locus_choice` puts the picker outside the terminal grid, in the
same window and the same process, drawn in GLSL by the host. `src/root_ui/` is a
port of root-ui's design-language phases — semantic input → layout → non-colour
decoration → user-owned colour — and `src/root_ui/adapter.rs` is a shader
adapter for them, beside the WebGL2 and WebGPU adapters root-ui already ships.

What the port buys over a hand-drawn panel is the separation the phases enforce:
layout is frozen before decoration exists, decoration before any colour is
chosen. `--scheme light` rebinds colour onto the *same* resolved layout;
`flat_scene_layout_identity` is equal across both schemes, and a test says so.

Corners are materialized in physical pixels from the shorter side, so they stay
circular when the window is resized, and the panel origin is deliberately off
the cell raster — a surface that could only be placed on cell boundaries would
be a floating window in the grid, which `pin navigation_not_in_grid` rejects.

## Notes are the primary object

`pin primary_object` makes a markdown note the thing being edited,
`pin note_substrate` makes the substrate the existing yui notes, and
`pin note_substrate_not_new` forbids inventing a second store. yui already
mirrors `yui_notes` into a local markdown vault and syncs it back, so that vault
*is* the local half of `pin storage_model`'s local-repository-and-DB pair.

`src/notes.rs` opens it — `$OBSIDIAN_VAULT_PATH`, defaulting to
`~/repos/obsidian`, both of which are yui's own. It defines no note format, no
database and no sync protocol; any of those would be the second store the pin
forbids under a different name.

## What is not decided here

- **`open_question navigation_state_owner`** — the human gate answered
  「わからない」. The picker's state is held host-side because something must
  hold it for the surface to exist; the seam that keeps the other arrangement
  reachable is `picker::Source`, which the surface asks for rows and which a
  Neovim-side supplier could implement.
- **`open_question navigation_input_routing`** — while the surface is open the
  host takes the keys directly. Also an implementation standing inside an open
  axis.
- **`open_question protocol_surface_scope`** — the UI face is served in full;
  the API face is served only where the host needs it (`nvim_buf_get_lines`,
  `nvim_buf_set_lines`, `nvim_command`, `nvim_get_mode`). `nvim_exec_lua`
  answers with an error, because `free lua_runtime_presence` leaves a Lua
  runtime optional and this host has none — a `nil` would let a client believe
  its code ran.
- **`open_question embed_candidate_disposition`** — the grid renderer, glyph
  atlas, screen model and external-UI surfaces are the measured candidate's,
  reached by `#[path]` rather than copied. `evaluation/candidate-embed-opengl`
  is byte-for-byte what it was when it was measured.

## Tests

```bash
cargo test
```

258 tests: the editing core against vim's own behaviour (including the one
documented irregularity, `cw` acting like `ce`), the redraw stream, the root-ui
phases, the note vault, and eight that drive the binary from outside as a
separate process.
