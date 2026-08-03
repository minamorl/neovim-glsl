#!/usr/bin/env python3
"""Re-derive DESIGN-SPACE.md from the spec, which is the SSOT.

Same contract as ``sync_undecided.py``: the input may be the real spec file or a
local ``spec-mirror`` transcription carrying only the live ``free`` lines. The
mirror is not authoritative; it exists for worktrees that cannot read
spec-system.

This file used to be a hand transcription of ``@0.1``'s eleven free axes. It
went stale the moment v0.4 added seven more, which is exactly the failure
``UNDECIDED.md`` already had. Generating it removes the failure mode rather than
fixing one instance of it.
"""
import io
import pathlib
import re
import sys

SPEC = pathlib.Path(sys.argv[1])
OUT = "DESIGN-SPACE.md"

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
        "正本ではない。食い違ったら spec が勝つ。\n"
    ).format("pins/domains/neovim-glsl.spec", SPEC.as_posix())

axes = []
for line in spec.split("\n"):
    if line.startswith("free "):
        body = line[len("free "):]
        name, _, desc = body.partition(":")
        axes.append((name.strip(), desc.strip()))

head = """# DESIGN-SPACE — 開いたままにする軸

`pins/domains/neovim-glsl.spec@{v}` の `free` 項目を、spec の文言のまま機械転記したもの。
**free は free のまま。** 実装・設定・依存・慣習のどれによっても、ここの軸を事実上固定してはならない。
{source_note}
規律:

- ここの軸に触れる成果物を置くときは、その軸を選んだことになっていないか先に確かめる。
- 「とりあえずこれで始める」も固定である。仮の選択は暗黙の pin になる。
- 選ぶ必要が出たら repository 側で選ばない。converter と人間ゲートを通して spec 側へ lift する。
- 正本は spec。この転記が spec と食い違ったら spec が勝つ。転記側を直す。
- この file は `python3 tools/sync_design_space.py <spec-or-mirror>` で再生成する。手で書き足さない。

## free 軸 ({n} 件)

""".format(v=version, source_note=source_note, n=len(axes))

body = "".join("- `{}`: {}\n".format(n, d) for n, d in axes)

tail = """
## この repository 自身が free を踏んでいないことの確認

`neovim_glsl.project_form` は repository の位置・名前・package manager・初期 scaffold・license を
free にしている。したがってこの repository の root は次を持たない。

- package manifest と lock file を置かない (package manager 未選択のまま)。
- root に source tree と build 設定を置かない (host 実装言語と build system 未選択のまま)。
- root に shader file を置かない。置けば GLSL の version と方言、shader stage 構成、graphics API を
  選んだことになる (`shader_pipeline`, `graphics_api`)。
- license file を置かない (`project_form` の license が未選択のまま)。
- 現在の repository の位置と名前は supervisor の routing による便宜であり、要件ではない。
  移動・改名しても pin は 1 つも壊れない。

`evaluation/` 配下は例外ではなく、この規律の適用結果である。あそこにある Rust・OpenGL・
GLSL 3.30 は**測るために選んだ一つの候補**であって、free 軸の選択ではない。だから root ではなく
`evaluation/` に置かれ、README がそれを採用案ではないと明記する。候補を消しても pin は壊れない。

## 未決との違い

`UNDECIDED.md` は「決めるべきだが情報が無い」もの (quarantine / open_question)。
この file は「決めなくてよい」もの (free)。前者は人間ゲートを待ち、後者は待つものが無い。
どちらも実装で埋めてはならないという点だけが共通している。
"""

io.open(OUT, "w", encoding="utf-8").write(head + body + tail)
print("wrote", OUT, "free", len(axes))
