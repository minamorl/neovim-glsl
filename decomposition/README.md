# decomposition — neovim-glsl v0.4 work-decomposition machinery

This directory is the executable realization of the `v0.4` block of
`pins/domains/neovim-glsl.spec` (spec-system-abcd-spec). The spec makes the
work decomposition itself an observable, checkable object rather than prose.
Nothing here adopts an architecture, resolves an `open_question`, or claims
progress that was not observed.

## What it enforces

| Pin | Where |
| --- | --- |
| `work_decomposition_progress_input = verified_observed_state` | `progress_baseline.py` |
| `no_recalled_progress_input`, `no_fabricated_progress` | `progress_baseline.py` |
| `progress_unverified = unresolved` (build unverifiable ⇒ unresolved) | `progress_baseline.py` |
| `work_dependency_graph = directed_acyclic`, `no_dependency_cycle` | `dependency_dag.py` |
| `work_dependency_explicit`, `work_unit_acceptance`, `work_unit_worktree_owner` | `dependency_dag.py` |
| `integration_gate*`, `no_integration_before_acceptance/_dependencies` | `integration_gate.py` |

## The decomposition it validates

`work-units.json` records the current open-scope units drawn **only from
verified evidence** in `evaluation/` and `founding/`: the Mac-stage
embed+OpenGL candidate, its end-to-end IME preedit/commit, the read-only aish
commencement surface, the Root-ui evaluation projection, the multi-target
portability direction, and the not-yet-run Zeno evaluation. Each unit owns a
single worktree, declares explicit dependencies, and carries an observable
acceptance witness. The Zeno unit has no evidence yet, so it is never green.

## Run

```sh
python3 decomposition/check.py                 # full pipeline report (JSON)
python3 decomposition/dependency_dag.py        # integration order
python3 -m unittest discover -s decomposition/tests -p 'test_*.py'
```

## Honesty note on this host

`cargo`/`rustc` are absent on the observing host, so the Rust Mac-stage
candidate's *build/run* status is reported `unresolved` — present source and
evidence files are recorded as verified, but "it builds" is never claimed.

## Parallel-worktree provenance

The machinery was built as three independent units, each in its own git
worktree/branch off `main` (`work/a1-progress-baseline`,
`work/a2-dependency-dag`, `work/a3-integration-gate`), committed green, then
merged here in dependency order (a1, a2 → a3 → wiring) on `work/a-integration`.
No two units mutated a shared worktree.
