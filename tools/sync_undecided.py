#!/usr/bin/env python3
"""Re-derive UNDECIDED.md from the spec, which is the SSOT.

The input may be either the real spec file or a local spec-mirror transcription
that contains only live ``quarantine`` / ``open_question`` lines. The mirror is
not authoritative; it exists only for worktrees that cannot read spec-system.
"""
import io
import pathlib
import re
import sys

SPEC = pathlib.Path(sys.argv[1])
OUT = "UNDECIDED.md"

spec = io.open(SPEC, encoding="utf-8").read()

version_match = re.search(r"@meta version: ([0-9.]+)", spec)
if version_match:
    version = version_match.group(1)
else:
    name_match = re.search(r"neovim-glsl-([0-9.]+)\.lines$", SPEC.name)
    if not name_match:
        raise SystemExit("cannot determine spec version; add '@meta version: X.Y'")
    version = name_match.group(1)

source_note = ""
if "spec-mirror" in SPEC.parts:
    source_note = (
        "\n入力は `{}` ではなく `{}`。これは外部 spec ledger からの転記であり、"
        "正本ではない。食い違ったら spec が勝つ。"
    ).format("pins/domains/neovim-glsl.spec", SPEC.as_posix())

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
{source_note}

規律:

- 実装・設定・命名・ディレクトリ構造・README の言い回しのどれによっても、ここの項目を
  「決まったこと」にしない。
- 決めたくなったら repository 側で決めない。converter と人間ゲートを通して spec 側へ lift する。
- 正本は spec。この転記が spec と食い違ったら spec が勝つ。転記側を直す。
- この file は `python3 tools/sync_undecided.py <spec-or-mirror>` で再生成する。手で書き足さない。

## quarantine — 隔離 ({nq} 件)

曖昧 (語義が二義的)・未確定 (解空間が未固定)・主観 (検証可能な基準が無い) を分けて隔離してある。

""".format(v=version, source_note=source_note, nq=len(quarantines))

body = "".join("- `{}`: {}\n".format(n, d) for n, d in quarantines)
body += "\n## open_question — 人間ゲート待ち ({} 件)\n\n決めるべきだが情報がない箇所。\n\n".format(len(questions))
body += "".join("- `{}`: {}\n".format(n, d) for n, d in questions)

tail = """
## 未決のまま残した結果

この repository に GLSL 化範囲・性能基準などを表す**決定**が無いのは欠落ではなく、
上の隔離をそのまま守った結果である。埋めた瞬間に設計空間が先に潰れる。

architecture については v0.6 で `own_host_speaking_neovim_protocol` が選ばれた。
ただし protocol のどの面を喋るか、transport、editing core、Lua runtime、telescope の
実現形、実測済み embed candidate の扱いは、上の未決項目または free 軸に残っている。

`evaluation/` にあるものは決定ではない:

- 性能の**測定**は隔離と矛盾しない。隔離しているのは性能**基準**（何 ms なら合格か）であって
  観測ではない。`neovim_glsl.performance_acceptance` を人間ゲートで決めるには観測が要る。
  測定結果は閾値を含まず、閾値は実行時に人間が渡した値としてのみ report に載る。
- embed + OpenGL の候補実装は、v0.6 では採用 architecture ではなく実測済み evidence である。
  `neovim_glsl.embed_candidate_disposition` が未決なので、UI client 資産として温存するか
  廃棄するかもこの repository では決めない。
- Root-ui projection も同様に、`root_ui_integration_adoption` を決めていない。

## v0.6 で決着したもの（参考）

spec v0.6 の人間ゲート回答 `own_host_protocol` により、次は**もう未決ではない**:

- `neovim_glsl.basis_selection` — 実際の host は `own_host_speaking_neovim_protocol`。
- `neovim_glsl.architecture_decision` — architecture は
  `own_host_speaking_neovim_protocol`。
- `neovim_glsl.architecture`（quarantine）— 解空間が固まり退役。
- `free neovim_glsl.editor_basis` — `pin neovim_glsl.editor_basis_own_host` へ lift。

代わりに v0.6 が新しく開いた問いと隔離は上の一覧に含まれている
（protocol surface/version、telescope realization、embed candidate disposition）。

## v0.5 で決着したもの（参考）

spec v0.5 の人間ゲート回答 `relax` により、次は**もう未決ではない**:

- `neovim_glsl.neovim_basis_decision` — editor 基盤は Neovim 固定を緩和。
  `free neovim_glsl.editor_basis` へ降格した。ただし emacs_family は依然 forbid。
- `neovim_glsl.neovim_retention_decision` — 「NeoVim は離れない」は編集体験・操作体系の
  保持で満たす（`pin neovim_glsl.neovim_retention_mode`）。実装の継続は要求しない。
- `neovim_glsl.neovim_basis_relaxation`（quarantine）— ゲート通過により解消。

代わりに v0.5 が新しく開いた問いのうち、`neovim_asset_reuse_scope` は v0.6 で
protocol 継承だけが確定し、残りの範囲を問う形へ narrowing された。`basis_selection` は
v0.6 で解決済み。
"""

io.open(OUT, "w", encoding="utf-8").write(head + body + tail)
print("wrote", OUT, "quarantine", len(quarantines), "open_question", len(questions))
