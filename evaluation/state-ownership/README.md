# state-ownership — evidence for a question nobody answered

`open_question neovim_glsl.navigation_state_owner` asks who owns the picker's
query, candidate set and selection: the host, or a Neovim-side plugin with the
host merely drawing.

On 2026-08-03 the human gate answered **「わからない」**. spec **v0.9** recorded that
answer verbatim and pinned nothing — an unknown answer is not a choice — but it
did change the question's state: it stopped waiting on a person and started
waiting on an observation. This directory is that observation.

**It decides nothing.** Both `navigation_state_owner` and
`navigation_input_routing` are open at v0.9, and a measurement cannot close them:
it can say what each arrangement costs, not which cost is acceptable. There is no
pin for that, and inventing one would answer a question the owner explicitly did
not answer.

## The two halves

| | host-owned | plugin-owned |
| --- | --- | --- |
| state lives in | this process | telescope, in the Neovim process |
| code | `../candidate-embed-opengl/src/picker_state.rs` | `probe.py` against the owner's real telescope |
| run | `cargo run -- --picker-script … --picker-report …` | `python3 probe.py --tree … --script …` |
| crossings per keystroke | 0 | 2 (deliver the key; fetch what the surface must draw) |

Both filter **the same corpus**, and the corpus is not built by this directory:
it is read out of the opened picker at an empty query and written to
`out/corpus.txt`, which the Rust half then consumes. An earlier version built it
with `find . -type f`; that is a second definition of "the candidates" and it
disagrees with telescope's ignore rules the moment the tree has a `target/`. Two
halves filtering different corpora cannot be compared, and nothing in either
report would have shown the disagreement.

## Reproducing the recorded run

```sh
cd evaluation/state-ownership
python3 probe.py --tree ~/repos/nvglsl-wt/v09-state --script 's<c-n><c-n><c-n><c-p>e<bs>'
cd ../candidate-embed-opengl
cargo run --quiet -- --picker-script 's<c-n><c-n><c-n><c-p>e<bs>' \
  --picker-corpus ../state-ownership/out/corpus.txt \
  --picker-visible-rows 12 \
  --picker-report ../state-ownership/out/host-owned.json
cd ../state-ownership && python3 compare.py
```

Measured Neovim: `NVIM v0.11.5`, with the owner's own
`plenary` / `telescope` / `telescope-file-browser` / `nvim-web-devicons` from
`~/.local/share/nvim/lazy`, and the owner's `<space>o` mapping.

## What was observed

81 candidates, 7 keystrokes, `<C-n>` and `<C-p>` and `<BS>` among them.

**Both halves admitted the same candidates at every step**: `71, 71, 71, 71, 71,
53, 71`. This is the one thing `compare.py` *checks* rather than reports — two
independent matchers over one corpus should agree on what matches, and if they
disagree, one is wrong about matching. No timing figure would have surfaced that.

Each half's measurement of its own work, in ms:

| | p50 | max |
| --- | --- | --- |
| host: state update | 0.0151 | 0.7715 |
| host: rows build | 0.0038 | 0.0247 |
| plugin: state fetch | 0.5233 | 0.9701 |

**These are not a verdict, and the columns are not racing each other.** The two
halves use different matchers — telescope sorts with fzy, the Rust half with a
span-penalised subsequence scan — so their durations compare two algorithms, not
two arrangements. What belongs to the *arrangement* is the crossing count: 0
against 2. The plugin-owned fetch is also a **lower bound**, because it is one
batched `nvim_exec_lua` rather than one request per field; a host that asked
field by field would pay more.

The two matchers ranked the same sets differently at 6 of 7 steps. That is
expected and is not a failure: same candidates, different ordering.

### What the plugin-owned arrangement does not hand over

Counted, because a comparison of milliseconds alone would miss it:

- **match positions.** telescope's entries carry the matched *text*, not the
  offsets the query hit. A surface that highlights matched characters — which any
  picker does — must re-derive them host-side or ask for a different extraction.
- **scores.** Reachable per index through the entry manager, not in the batch.
- **geometry.** telescope's window layout is meaningless to a surface outside the
  grid (spec v0.8), so none of it transfers.

## Guards

`probe.py` refuses to file a report where the selection never moved. The first
run used protocol-surface's 7-file scratch tree, where three characters narrow
the set to one candidate and `<C-n>` has nowhere to go: every step reported
`selection_row: 0`, and a measurement of moving the selection that never moved it
is not evidence. It is now a hard exit, not a note.

`compare.py` exits non-zero if the scripts differ, the corpora differ, the corpus
file disagrees with its own report's count, the candidate counts diverge, either
half claims to decide something, or either half never moved the selection.

## Files

- `probe.py` — plugin-owned half. Imports the msgpack/RPC plumbing from
  `../protocol-surface/driver.py` rather than copying it, and deliberately runs
  with `record_api=False`: that module's recorder wraps every `nvim_*` function,
  so a latency figure taken with it installed would be timing the recorder.
- `compare.py` — reads both halves back, checks what is checkable, reports what
  is not, and closes nothing.
- `out/corpus.txt` — what telescope offered, and what both halves filtered.
- `out/measurement.json`, `out/host-owned.json` — the recorded runs.
