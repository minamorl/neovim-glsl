# free-surface — the evidence, and now the conformance record

This directory measures what a surface can do when it stops speaking in cells.

It was written for two undecided things in the spec:

- `open_question neovim_glsl.navigation_surface_decision`
- `quarantine neovim_glsl.external_surface_boundary`

**Both closed at v0.8**, in favour of what is drawn here: the navigation surface
is a GLSL surface over the grid in the same window, rendered by the host, and not
the grid, not a separate OS window, not a separate process. So this directory
changed job without changing content. It was evidence for a choice; it is now
also the record that the choice is met, which is why `out/locus.json` exists and
why `verify.py` reads it back instead of taking the host's word for it.

What stayed open is everything about what the surface is *for*: which mechanism
supplies the rows (`navigation_mechanism_selection`), who owns the picker state
(`navigation_state_owner`), and where the keystrokes go while it is open
(`navigation_input_routing`). Nothing here is wired to a key, a matcher or a file
list, and nothing here should be read as deciding those.

## Why this is worth measuring at all

Every other thing this program draws is addressed in cells. A glyph lands at
`col * cell_w`, a row occupies exactly `cell_h`, and a thing that wants to sit
one third of a row lower has nowhere to sit. That is not a limitation of GLSL.
It is a limitation of speaking in a terminal's coordinates, and it is inherited
by anything that draws itself *into the grid* — including a picker written as a
Neovim plugin, however good it is.

So the question underneath "does navigation have to be telescope" is really:
is the grid the only place a surface can live? This measures the alternative.

## What was drawn

`panels.json` describes two surfaces in pixels. `bed.lua` fills the editor
underneath with ordinary numbered lines, including a Japanese one, and knows
nothing about panels — the grid keeps painting, unaware it is being covered.

```bash
cd ../candidate-embed-opengl
cargo run -- --cols 120 --rows 34 \
  --snapshot ../free-surface/out/free-surface-over-grid.png \
  --panels ../free-surface/panels.json \
  --panel-report ../free-surface/out/measurement.json \
  --locus-report ../free-surface/out/locus.json \
  --lua "$(cat ../free-surface/bed.lua)" -- --clean
```

![two free surfaces over a live Neovim grid](out/free-surface-over-grid.png)

The panels go through the *same* shader, the same vertex format and the same
atlas as the grid, appended to the same draw call. There is no second renderer:
`Renderer::push_panels` adds quads to the stream `build_scene` just filled.

## What the grid cannot express, observed

Measured on Apple M4 Max / macOS 26 / OpenGL 4.1 Metal - 90.5 / GLSL 4.10, at a
2280x1224 framebuffer. Raw counts in `out/measurement.json`.

| observation | panel 0 | panel 1 |
|---|---|---|
| origin | `(275.5, 192.25)` | `(1520.25, 192.25)` |
| origin sits off the cell raster | yes | yes |
| row pitch | 62.5 px | 44 px |
| backdrop alpha | 0.62 | 0.32 |
| quads emitted | 361 | 93 |
| quads cut at an edge | 59 | 0 |
| rows visible | 11 | 4 |
| first row cut by | 23.5 px | 0 px |
| last row cut by | 49.5 px | 0 px |

Cell height on this run is 36 px and cell width 19 px, so:

- **Fractional origin.** `192.25` is not a multiple of 36. A cell-addressed
  surface would have to round it to a row; this one does not.
- **Independent pitch.** 62.5 px and 44 px are both unrelated to 36 px, and the
  two panels disagree with each other as well as with the grid.
- **Fractional scroll.** The body is scrolled 23.5 px, so the first visible row
  is cut partway through its glyphs. A grid's only legal answers are 0 and one
  whole row.
- **Translucency over live text.** A cell holds one background colour and no
  opacity. These panels hold both, and the editor text stays legible through
  them.
- **Sub-cell clipping.** 59 quads met an edge and were cut, with their texture
  coordinates cut by the same proportion, so half a quad shows half a glyph
  rather than a squeezed whole one.

## Where the surface was, as observed

`out/locus.json` records the locus rather than asserting it. The host does not
have a setting called "locus" that it prints back; the value comes out of where
the quads went:

| field | value on this run | why it is the evidence |
|---|---|---|
| `locus` | `glsl_surface_over_grid` | the pinned one |
| `renderer` | `host` | outside the grid is unaddressable by Neovim |
| `grid_vertices` | 36366 | count after the grid pass |
| `total_vertices` | 39090 | count after the surface pass |
| `shared_vertex_buffer` | `true` | one buffer, so one window and one process |

The 2724-vertex gap is the load-bearing number. A surface in another window or
another process could not have appended to this buffer at all, so "same window,
same process" is a measured relation here and not a claim. The report also
carries the six things v0.8 left open, so a reader who finds this file later
cannot mistake it for a decision about the picker.

Addressing stayed free at v0.8, so `origin_on_cell_raster` is reported and never
required. A surface that happened to land on a cell boundary would not be in
violation of anything — it would only have declined a freedom it had.

## Verifying it

`verify.py` reads the PNG back and checks the four claims that matter, against
pixels rather than against this prose:

```bash
python3 verify.py
```

```text
grid background outside every panel: (20, 22, 27)

panel 0: origin (275.5, 192.25) size 1180.0x690.0 alpha 0.62
  dominant interior colour: (30, 38, 56) (144854 samples)
  blend of (36, 48, 73) at 0.62 over (20, 22, 27) predicts (30, 38, 56); opaque would be (36, 48, 73)
  top edge in the image: row 192, coordinate asked for 192.25

grid text outside the panel: (224, 226, 234)
through panel 0 it should read (107, 116, 134); matching pixels inside: 1170

recorded locus: glsl_surface_over_grid, renderer host
  vertices: 36366 after the grid pass, 39090 after the surface pass
  still open alongside this evidence: 6

ok: backdrops blend, origins sit off the cell raster, grid text shows through, and the recorded locus matches the pixels
```

The blend arithmetic is the load-bearing part. If the panel had *replaced* what
was under it, the interior would read as `(36, 48, 73)` exactly. It reads as the
mix, which is only possible because the surface is composited rather than
written into cells.

`src/surface_locus.rs` carries 7 more that hold the locus itself: that the
observed locus is the pinned one and differs from each forbidden one, that an
off-raster origin is reported in cells as well as pixels, that an on-raster
origin is *not* treated as a violation, that the two vertex counts arrive in the
order the passes ran, and that the open questions travel with the evidence.

`src/panel.rs` carries 11 unit tests covering the same laws without a GPU:
fractional origin survival, proportional UV rescaling on clip, fractional
scroll, row pitch independence, right-edge cutting, and determinism of the
emitted geometry.

## What this does not measure

- **Performance.** No timing is quoted. The live-session recorder observes two
  frames on a snapshot run, which is not a sample, and the deterministic bench
  does not drive panels. Measuring this properly needs its own harness.
- **Interaction.** No key handling, no matcher, no source, no action. This is a
  surface, not a picker.
- **What the surface is for.** The mechanism that supplies its rows, the owner
  of its state, and the route its keystrokes take are all still open. Only the
  *place* was decided at v0.8.
- **Whether GLSL's advantage was "used".** There is no threshold for that, which
  is why `quarantine neovim_glsl.glsl_advantage_criterion` exists. What is
  recorded above is a list of expressible things, not a verdict.

## Files

- `panels.json` — the two surfaces, in pixels.
- `bed.lua` — the editing session underneath. Knows nothing about panels.
- `verify.py` — reads the PNG back and checks the claims.
- `out/free-surface-over-grid.png` — the snapshot.
- `out/measurement.json` — what the panel pass emitted, counts only.
- `out/locus.json` — where it was drawn, as observed. The v0.8 conformance record.
