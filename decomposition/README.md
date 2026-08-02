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

The build/run claim is only as good as the toolchain the observer can see.
`build_capability` reports `unresolved` when `cargo`/`rustc` are absent and
`verifiable` when they are present, so "it builds" is never claimed on a host
that cannot build it.

On this Mac the Homebrew `rustc` aborts (libLLVM ABI mismatch), so the rustup
toolchain has to be **first on PATH** before the check reports `verifiable`:

```sh
export PATH="$HOME/.rustup/toolchains/stable-aarch64-apple-darwin/bin:$PATH"
python3 decomposition/check.py   # baseline.build_capability.build_runs == "verifiable"
```

Without that export the same command reports `unresolved`, which is the honest
answer for an observer that cannot compile the candidate.

## Parallel-worktree provenance

The machinery was built as three independent units, each in its own git
worktree/branch off `main` (`work/a1-progress-baseline`,
`work/a2-dependency-dag`, `work/a3-integration-gate`), committed green, then
merged here in dependency order (a1, a2 → a3 → wiring) on `work/a-integration`.
No two units mutated a shared worktree.
