# UNDECIDED — 決まっていない事項

`pins/domains/neovim-glsl.spec@0.1` の `quarantine` と `open_question` を、spec の文言のまま転記したもの。**ここにあるものは決まっていない。** 設立段階の実装で埋めてはならない。

規律:

- 実装・設定・命名・ディレクトリ構造・README の言い回しのどれによっても、ここの項目を「決まったこと」にしない。
- 決めたくなったら repository 側で決めない。converter と人間ゲートを通して spec 側へ lift する。
- 正本は spec。この転記が spec と食い違ったら spec が勝つ。転記側を直す。

## quarantine — 隔離

曖昧 (語義が二義的) と 未確定 (解空間が未固定) を分けて隔離してある。

- `neovim_glsl.glsl_scope`: 曖昧 — 「NeoVim を GLSL にする」の GLSL 化対象が、表示・UI 描画層だけなのか、Neovim の編集エンジン(buffer・編集意味論)自体まで含むのかは、指示文だけでは解釈が 2 通り以上あり一意に決まらない。
- `neovim_glsl.neovim_retention_form`: 曖昧 — 「Neovim を離れない」が Neovim 実装(core / process)の継続利用を指すのか、Neovim の編集体験・操作体系の保持を指すのかが一意に決まらない。両解釈に共通する「editor 基盤は Neovim・他 editor へ置換しない」だけを pin した。
- `neovim_glsl.full_replacement_wish`: 曖昧 — 「GLSL でフル置換」という表現は何の全体を指すかが一意でなく、かつ願望の表明であって要求として再確定されていない。したがって置換範囲を pin 化しない。
- `neovim_glsl.architecture`: 未確定 — 解空間が固まっておらず正統な要求が未成立。Neovim を embed して external UI protocol で繋ぐ / 別 host を書く / 再実装する等は、目標達成のあらゆる妥当な手段に共通して必要な条件ではなく一解の性質(設計判断)にすぎない。今 pin 化すると設計空間を先に潰す。
- `neovim_glsl.performance_criteria`: 未確定 — 滑らかさ・速さ・frame rate 等の性能基準は指示文に現れておらず、定量化する情報がない。数値を捏造せず未成立のまま隔離する。

## open_question — 人間ゲート待ち

決めるべきだが情報がない箇所。

- `neovim_glsl.glsl_scope_decision`: GLSL 化の対象はどこまでか。表示・UI 描画層までか、編集エンジンの意味論まで含むか。
- `neovim_glsl.neovim_retention_decision`: 「Neovim を離れない」を Neovim 実装(core / process)の継続利用で満たすか、編集体験・操作体系の保持で満たすか。
- `neovim_glsl.architecture_decision`: 実現手段(embed + external UI protocol / 新規 host / 再実装 / fork)のどれを採るか。候補ごとの適合性を評価してから lift する。
- `neovim_glsl.graphics_stack_decision`: GLSL をどの graphics API / driver stack / platform で実行するか。単一 backend か複数か、fallback の有無も未確定。
- `neovim_glsl.performance_acceptance`: 性能をどの workload・観測量・閾値で受け入れるか。指示文に数値は無いため人間ゲートで決める。
- `neovim_glsl.plugin_config_compat_boundary`: 既存の Neovim 設定・plugin・keymap をどこまで維持するか。
- `neovim_glsl.establishment_definition`: 「project を立てる」の完了条件は何か。何が存在すれば設立済みと判定するか。
- `neovim_glsl.post_establishment_roadmap`: 設立の次に何を要求するか。今回の指示文は設立までしか述べていない。
- `neovim_glsl.downstream_operation_model_link`: この project を第一段とする、より大きな操作モデル構想との接続点をどう定義するか。今回の指示の対象外であり、この spec は後段を pin しない。

## 未決のまま残した結果

この repository に architecture・GLSL 化範囲・性能基準を表す成果物が **無い** のは欠落ではなく、上の隔離をそのまま守った結果である。埋めた瞬間に設計空間が先に潰れる。

`evaluation/` に性能の**測定**があることは、この隔離と矛盾しない。隔離しているのは
性能**基準**（何 ms なら合格か）であって、観測ではない。`neovim_glsl.performance_acceptance`
を人間ゲートで決めるには観測が要る。測定結果は閾値を含まず、閾値は実行時に渡されたときだけ
report に現れる。渡されなければ `slow_frames.criterion` は `unset_awaiting_human_gate` のままで、
合否は出ない。
