#!/usr/bin/env python3
"""Re-derive UNDECIDED.md from the spec, which is the SSOT.

The transcription was frozen at spec v0.1. The spec is now at v0.5 and the
repository has been asserting things (multigrid, ext-UI, perf measurement,
root-ui projection) that the stale copy still lists as v0.1-only. Rather than
hand-patching prose, regenerate the two lists from the spec file itself and keep
the hand-written discipline / consequences sections.
"""
import io, re, sys

SPEC = sys.argv[1]
OUT = "UNDECIDED.md"

spec = io.open(SPEC, encoding="utf-8").read()
version = re.search(r"@meta version: ([0-9.]+)", spec).group(1)

quarantines, questions = [], []
for line in spec.split("\n"):
    if line.startswith("quarantine "):
        body = line[len("quarantine "):]
        name, _, desc = body.partition(":")
        quarantines.append((name.strip(), desc.strip()))
    elif line.startswith("open_question "):
        body = line[len("open_question "):]
        name, _, desc = body.partition(":")
        questions.append((name.strip(), desc.strip()))

head = """# UNDECIDED — 決まっていない事項

`pins/domains/neovim-glsl.spec@{v}` の `quarantine` と `open_question` を、spec の文言のまま
機械転記したもの。**ここにあるものは決まっていない。** 実装で埋めてはならない。

規律:

- 実装・設定・命名・ディレクトリ構造・README の言い回しのどれによっても、ここの項目を
  「決まったこと」にしない。
- 決めたくなったら repository 側で決めない。converter と人間ゲートを通して spec 側へ lift する。
- 正本は spec。この転記が spec と食い違ったら spec が勝つ。転記側を直す。
- この file は `python3 tools/sync_undecided.py <spec>` で再生成する。手で書き足さない。

## quarantine — 隔離 ({nq} 件)

曖昧 (語義が二義的)・未確定 (解空間が未固定)・主観 (検証可能な基準が無い) を分けて隔離してある。

""".format(v=version, nq=len(quarantines))

body = "".join("- `{}`: {}\n".format(n, d) for n, d in quarantines)
body += "\n## open_question — 人間ゲート待ち ({} 件)\n\n決めるべきだが情報がない箇所。\n\n".format(len(questions))
body += "".join("- `{}`: {}\n".format(n, d) for n, d in questions)

tail = """
## 未決のまま残した結果

この repository に architecture・GLSL 化範囲・性能基準を表す**決定**が無いのは欠落ではなく、
上の隔離をそのまま守った結果である。埋めた瞬間に設計空間が先に潰れる。

`evaluation/` にあるものは決定ではない:

- 性能の**測定**は隔離と矛盾しない。隔離しているのは性能**基準**（何 ms なら合格か）であって
  観測ではない。`neovim_glsl.performance_acceptance` を人間ゲートで決めるには観測が要る。
  測定結果は閾値を含まず、閾値は実行時に人間が渡した値としてのみ report に載る。
- embed + OpenGL の候補実装は `neovim_glsl.architecture_decision` を決めていない。
  評価候補であり、report の `evaluation_candidate` は `true`、`adoption_decision` は
  `awaiting_human_gate` のままである。
- Root-ui projection も同様に、`root_ui_integration_adoption` を決めていない。

## v0.5 で決着したもの（参考）

spec v0.5 の人間ゲート回答 `relax` により、次は**もう未決ではない**:

- `neovim_glsl.neovim_basis_decision` — editor 基盤は Neovim 固定を緩和。
  `free neovim_glsl.editor_basis` へ降格した。ただし emacs_family は依然 forbid。
- `neovim_glsl.neovim_retention_decision` — 「NeoVim は離れない」は編集体験・操作体系の
  保持で満たす（`pin neovim_glsl.neovim_retention_mode`）。実装の継続は要求しない。
- `neovim_glsl.neovim_basis_relaxation`（quarantine）— ゲート通過により解消。

代わりに v0.5 が新しく開いた問いは上の一覧に含まれている
（`neovim_asset_reuse_scope` と `basis_selection`）。
"""

io.open(OUT, "w", encoding="utf-8").write(head + body + tail)
print("wrote", OUT, "quarantine", len(quarantines), "open_question", len(questions))
