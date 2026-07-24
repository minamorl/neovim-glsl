# neovim-glsl

「NeoVim を GLSL にする」project の設立物。**この repository は project が設立されたという事実そのもの**であって、実装 scaffold ではない。

- 由来 (2026-07-25): Neovim を editor 基盤として保持したまま GLSL 化する project を設立する、という要求。逐語の要求文と会話の経緯は private な台帳側にのみ置く。
- 正本: spec-system の `pins/domains/neovim-glsl.spec` (`@meta version: 0.1`) と、それが import する `pins/house_style.pin` (`@meta version: 1.8`)。
- **spec が真実。** spec と この repository が食い違ったら spec が勝つ。repository 側を直す。pin を repository の都合に合わせて緩めない。

## 凍結されている事項

この指示文が hard pin として凍結したのは以下だけ。ここに無いものは凍結されていない。

- `neovim_glsl.project_established` — この project は設立されている (`require neovim_glsl.project.establishment = required`)。この repository がその設立物。
- `neovim_glsl.project_subject` — project の主題は neovim_to_glsl (`require neovim_glsl.project.subject = neovim_to_glsl`)。
- `neovim_glsl.establishment_order` — 設立は先行段である (`require neovim_glsl.project.establishment.order = first`)。
- `neovim_glsl.editor_basis` — editor 基盤は Neovim (`require neovim_glsl.editor.basis = neovim`)。
- `neovim_glsl.no_editor_substitution` — 他 editor への置換を許可しない (`forbid neovim_glsl.editor.substitution = allowed`)。
- `neovim_glsl.emacs_alternative_rejected` — Emacs 系を editor 基盤にしない (`forbid neovim_glsl.editor.basis = emacs_family`)。
- `neovim_glsl.target_shading_language` — target shading language は GLSL (`require neovim_glsl.target.shading_language = glsl`)。GLSL は指示文の逐語であって、実現手段としてこちらが選んだものではない。

### 全段で成り立つ法則

- `neovim_glsl.editor_basis_witness`: `forall stage . editor_basis(stage) == neovim`
- `neovim_glsl.no_substitution_witness`: `forall candidate . editor_substitution(candidate) == rejected`

後段のどの stage を追加しても editor 基盤は Neovim のままであり、editor 置換候補はすべて rejected になる。この 2 つは設立時点だけでなく project の全期間に効く。

### witness (spec の example)

- `neovim_glsl.editor_retained`: `editor_basis_choice => neovim`
- `neovim_glsl.emacs_alternative`: `emacs_family_editor_substitution => rejected`
- `neovim_glsl.founding`: `this_instruction_deliverable => established_neovim_to_glsl_project`
- `neovim_glsl.shading_language`: `project_target_shading_language => glsl`

機械可読な形は `founding/record.json`。pin id と成果物の対応は `TRACEABILITY.md`。

## 凍結されていない事項

「NeoVim を GLSL にする」の **GLSL 化の範囲**、**実現手段 (architecture)**、**性能基準** はいずれも決まっていない。host の実装言語、graphics API、shader pipeline、platform、text rendering、既存 plugin と設定の互換範囲、設立より先の工程も決まっていない。

- 未決と曖昧: `UNDECIDED.md` — spec の quarantine と open_question を文言のまま置いてある。
- 開いている設計空間: `DESIGN-SPACE.md` — spec の free 軸を文言のまま置いてある。

何をもって「設立済み」と判定するかも `open_question neovim_glsl.establishment_definition` として未決である。この repository はその判定基準を先に決めるものではない。判定は人間ゲートの領域。

そのため設立段階のこの repository には、**製品としての** source code・shader・build 設定・package manager・license を置かない。いま置けば、まだ誰も選んでいない設計判断を暗黙に固定してしまう。置けるようになるのは、対応する open_question が人間ゲートで解かれ、spec 側へ pin として lift されたあと。

### 例外ではなく別カテゴリ: `evaluation/`

上の規律は **lowering（spec → 製品実装）** に効くものであって、評価の禁止ではない。
`open_question neovim_glsl.architecture_decision` は spec 自身が「候補ごとの適合性を
**評価してから** lift する」と書いており、評価は人間ゲートの前提として要求されている。

`evaluation/` にはその評価を実測で行った結果が入っている。動くコードが含まれるが、
それは候補の採用でも設計の凍結でもない。詳細と、何を実測し何を測っていないかは
`evaluation/README.md`。ここに置かれた技術選択はすべて `DESIGN-SPACE.md` の `free` 軸のままで、
`UNDECIDED.md` の項目は 1 つも解決されていない。

CI 定義も同じ理由でまだ置かない。build も test 対象も存在せず、CI provider と package manager の選択は spec が選んでいない設計判断だから。継承した `ci.status_green` と `ci.format_prettier` は拘束として有効で、CI を作る時点でそれを満たす。詳細は `TRACEABILITY.md` の注記を見よ。

## この repository での開発の入口

継承した `spec.boot.*` はこの repository にもそのまま効く。

- 開発要求の最初の実質行動は spec 台帳側の route であり、target code からは始めない。
- 対応する domain spec が無い / 読めない / 赤い場合は BLOCKED。code-first の迂回は禁止。
- spec → 実装の生成は、spec だけを入力とする隔離された context で回す。
- 運用契約の文面の正本は台帳側にただ一つ存在する。この repository は複製せず参照するだけ。

## push しない

設立 commit までで止める。push・merge・deploy・release は人間承認の領域 (`spec.hook_no_push`、house_style boot contract の 9 項)。

## この repository の位置と名前

`free neovim_glsl.project_form` により、repository の位置・名前・package manager・初期 scaffold・license は spec が選んでいない。現在の配置は supervisor の routing による便宜であって要件ではなく、pin を 1 つも壊さずに変えられる。
