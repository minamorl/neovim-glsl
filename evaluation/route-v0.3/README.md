# v0.3 evaluation route

This directory is the observable route for `neovim-glsl.spec@0.3`. It records
evidence and replaceable candidates; it does not choose the canonical
architecture, graphics API, host language, Root-ui ownership boundary, Zeno
adoption, or exhaustive platform catalog.

## Mac first stage

`candidate-embed-opengl` is the existing Mac-stage candidate. It keeps Neovim
as the editing engine and renders the external UI through GLSL. The app bundle,
IME, Japanese glyph fallback, keyboard input, and Lua-placed image surface are
existing Mac evidence. The stage completion criterion remains at its human gate.

The candidate now accepts two optional evidence paths:

```sh
nvimgl \
  --platform-report /tmp/nvimgl-platform.json \
  --root-ui-evaluation /tmp/nvimgl-root-ui.json \
  --snapshot /tmp/nvimgl.png \
  -- --clean
```

The platform report records the actual OS, architecture, GL renderer, GLSL
version, and Neovim version. It always marks the current Rust/OpenGL/winit stack
as a non-canonical evaluation choice.

## Aish integration commencement

The Mac candidate installs three Neovim commands:

- `:AishDiscover`
- `:AishStatus`
- `:AishInspect {file|process|port|service|log|executable|repository} {identity}`

They invoke a configured `aish-nu` launcher asynchronously and show the
structured JSON result in a Neovim scratch buffer. Configure the launcher with
`--aish /path/to/ai-native-shell/aish-nu` or `NVIMGL_AISH_NU`.

This surface is intentionally read-only. It does not expose `aish run`, `aish
exec`, raw shell text, confirmation bypass, or AI execution authority. The
effect-confirmation UI remains an open question, so adding execution now would
silently resolve a human gate.

The `minamorl/ai-native-shell` repository visibility was observed as `PRIVATE`
at integration commencement.

## Root-ui combination hypothesis

`--root-ui-evaluation` writes a machine-readable projection of the settled
Neovim grid. It records the following Root-ui constraints:

- zero React footprint;
- replaceable framework-neutral text-editing host port;
- semantic → layout → non-color decoration → user color → shader phase order.

The projection is not a Root-ui primitive program and does not adopt Root-ui as
the canonical renderer. Neovim remains the buffer/editing-semantics authority.
Visual primitive ownership and text-editing-host ownership remain explicitly
unresolved.

The promising next experiment is a workbench layer: Root-ui can own editor
chrome, typed object/result panels, command discovery, and other non-buffer
surfaces while the Neovim external UI continues to own editing semantics. That
separation avoids replacing Neovim and gives aish structured objects somewhere
better than a terminal-like text dump to live. It is still an evaluation
hypothesis, not an adoption decision.

## Zeno next evaluation and multi-target direction

Zeno is the next evaluation after the Mac stage; it is not yet adopted and a
successful launch is not claimed. The current candidate already uses
cross-platform `winit`/`glutin`, and font discovery now accepts
`NVIMGL_FONT_PATHS` plus replaceable OS candidates instead of requiring only
macOS fonts.

The Zeno evaluation should run the same snapshot, platform-report, Root-ui
projection, and aish-discovery witness inside the guest. Until that execution
exists, its status is `unverified`, never green.

Windows and other targets remain inside the multi-target direction, but the
target catalog and feature-parity policy are deliberately unresolved.
