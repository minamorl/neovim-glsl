#!/usr/bin/env python3
"""Re-derive UNDECIDED.md from the spec, which is the SSOT.

The input may be either the real spec file or a local spec-mirror transcription
that contains only live ``quarantine`` / ``open_question`` lines. The mirror is
not authoritative; it exists only for worktrees that cannot read spec-system.

The closure section ("what stopped being undecided, and when") is derived too.
It used to be hand-written, and by v0.7 it had gone stale in exactly the way the
whole document was written to avoid: it still described v0.6 as the newest gate
while the spec had closed two more questions. A closure list that is transcribed
by hand rots on the same schedule as the list it is attached to, so it is read
out of the spec's own ``# RESOLVED at vX.Y`` / ``# RETIRED at vX.Y`` markers.

Those markers only survive in the real spec file. A mirror carries what is still
open, not the history of what closed, so when the input is a mirror the closure
section is omitted rather than guessed at.
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

from_mirror = "spec-mirror" in SPEC.parts
source_note = ""
if from_mirror:
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


def closures(text):
    """Read the spec's own closure markers.

    A marker is ``# RESOLVED at vX.Y[ (gate note)]: <subject>``, where the
    subject may sit on the marker line or on the continuation line beneath it.
    Only the subject is carried across: the reason lives in the spec, and
    duplicating it here would create a second place for it to drift.
    """
    lines = text.split("\n")
    found = {}
    pattern = re.compile(
        r"^# (RESOLVED|RETIRED) at v([0-9.]+)(?: \(([^)]*)\))?:[ \t]*(.*)$"
    )
    for i, line in enumerate(lines):
        m = pattern.match(line)
        if not m:
            continue
        verb, ver, note, subject = m.groups()
        subject = subject.strip()
        if not subject and i + 1 < len(lines):
            nxt = lines[i + 1]
            if nxt.startswith("#"):
                subject = nxt.lstrip("#").strip()
        if not subject:
            continue
        entry = found.setdefault(ver, {"note": None, "items": []})
        if note and not entry["note"]:
            entry["note"] = note.strip()
        entry["items"].append((verb, subject))
    return dict(sorted(found.items(), key=lambda kv: [int(p) for p in kv[0].split(".")], reverse=True))


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

`evaluation/` にあるものは決定ではない:

- 性能の**測定**は隔離と矛盾しない。隔離しているのは性能**基準**（何 ms なら合格か）であって
  観測ではない。`neovim_glsl.performance_acceptance` を人間ゲートで決めるには観測が要る。
  測定結果は閾値を含まず、閾値は実行時に人間が渡した値としてのみ report に載る。
- 実装済みの候補・実測は、採用された architecture の実体ではなく evidence である。
  `neovim_glsl.embed_candidate_disposition` が未決なので、UI client 資産として温存するか
  廃棄するかもこの repository では決めない。
- Root-ui projection も同様に、`root_ui_integration_adoption` を決めていない。
"""

closure = closures(spec)
if closure:
    tail += "\n## 決着したもの（参考・spec の closure marker から機械生成）\n\n"
    tail += (
        "各項目は spec 側で `# RESOLVED at vX.Y` / `# RETIRED at vX.Y` と記録された行の主題だけを\n"
        "写している。理由は spec にあり、ここには複製しない。\n"
    )
    for ver, entry in closure.items():
        note = " — {}".format(entry["note"]) if entry["note"] else ""
        tail += "\n### v{}{}\n\n".format(ver, note)
        for verb, subject in entry["items"]:
            tail += "- {}: {}\n".format(verb.capitalize(), subject)
elif from_mirror:
    tail += (
        "\n## 決着したもの\n\n"
        "この生成は mirror 入力なので closure marker を持たない。決着の一覧は\n"
        "`pins/domains/neovim-glsl.spec` を入力にして再生成すると出る。\n"
    )

io.open(OUT, "w", encoding="utf-8").write(head + body + tail)
print("wrote", OUT, "quarantine", len(quarantines), "open_question", len(questions),
      "closure versions", len(closure))
