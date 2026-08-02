# UNDECIDED — 決まっていない事項

`pins/domains/neovim-glsl.spec@0.6` の `quarantine` と `open_question` を、spec の文言のまま
機械転記したもの。**ここにあるものは決まっていない。** 実装で埋めてはならない。

入力は `pins/domains/neovim-glsl.spec` ではなく `spec-mirror/neovim-glsl-0.6.lines`。これは外部 spec ledger からの転記であり、正本ではない。食い違ったら spec が勝つ。

規律:

- 実装・設定・命名・ディレクトリ構造・README の言い回しのどれによっても、ここの項目を
  「決まったこと」にしない。
- 決めたくなったら repository 側で決めない。converter と人間ゲートを通して spec 側へ lift する。
- 正本は spec。この転記が spec と食い違ったら spec が勝つ。転記側を直す。
- この file は `python3 tools/sync_undecided.py <spec-or-mirror>` で再生成する。手で書き足さない。

## quarantine — 隔離 (24 件)

曖昧 (語義が二義的)・未確定 (解空間が未固定)・主観 (検証可能な基準が無い) を分けて隔離してある。

- `neovim_glsl.glsl_scope`: 曖昧 — 「NeoVim を GLSL にする」の GLSL 化対象が、表示・UI 描画層だけなのか、Neovim の編集エンジン(buffer・編集意味論)自体まで含むのかは、指示文だけでは解釈が 2 通り以上あり一意に決まらない。
- `neovim_glsl.full_replacement_wish`: 曖昧 — 先行発話「neovimをGLSLでフル置換したい」の「フル置換」が何の全体を指すかが一意でなく、かつ「したい」は願望である。今回の指示文はこれを要求として再確定していないため、置換範囲を pin 化しない。
- `neovim_glsl.performance_criteria`: 未確定 — 滑らかさ・速さ・frame rate 等の性能基準は指示文に現れておらず、定量化する情報がない。数値を捏造せず未成立のまま隔離する。
- `neovim_glsl.aish_integration_depth`: 未確定 — 解空間が固まっておらず正統な要求が未成立。aish の typed object を neovim-glsl 側で第一級 object として保持するのか、aish の実行結果を描画するだけなのかは指示文から一意に定まらず、どちらも目標達成のあらゆる妥当な手段に共通して必要な条件ではない。今 pin 化すると設計空間を先に潰す。
- `neovim_glsl.aish_integration_mechanism`: 曖昧 — 「Luaでもいいけど」は許可であって要求ではなく、「でもいい」が Lua を推奨するのか単に許容するのかも一意に決まらない。規則 C により Lua を hard pin にせず、同時に禁止もしない。
- `neovim_glsl.aish_visibility_permanence`: 曖昧 — 「まず非公開リポジトリで」の「まず」が、統合作業に先行する順序を指すのか、当面は非公開という暫定を指すのかが一意に決まらない。両解釈に共通する「統合に先立ち aish repository は非公開」だけを pin し、将来の公開可否は pin しない。
- `neovim_glsl.connection_vs_integration`: 曖昧 — 「aishと接続する」の接続と「aishとの統合」の統合が同一の作業を指すのか、接続が統合の一部分にすぎないのかが一意に決まらない。両解釈に共通する「aish と統合する・その開始」だけを pin した。
- `neovim_glsl.mac_native_app_recollection`: 曖昧 — 「Macのネイティブアプリにするところまでやったような気がする」は recollection であって新規 native-app directive ではない。既存 artifact を確認せず hard pin 化しない。
- `neovim_glsl.vscode_defeat`: 主観 — 「VSCodeとか倒せる」は subjective comparative assertion であり、observable defeat criterion・test set・comparison method が無い。defeat 対象の名指し(VS Code その他)も列挙 explicit ではない。
- `neovim_glsl.strong_usable`: 主観 — 「普通につよつよの環境」「使い勝手のいいエディタ」は subjective quality assertion であり、検証可能な strength/usability 指標・threshold・comparison baseline が無い。
- `neovim_glsl.multi_target_set`: 未確定 — 「Winでもなんでも・どのターゲットにしても動く」は exhaustive target set と parity/acceptance を定めておらず、解空間が未固定。Win・Mac 以外の platform、parity baseline、acceptance criterion は未成立。
- `neovim_glsl.root_ui_integration_depth`: 未確定 — Root-ui integration の深さ(visual rendering / editor host / text-editing host port / semantic editor state)は未定であり、解空間が未固定。
- `neovim_glsl.root_ui_integration_form`: 未確定 — Root-ui integration の form(embedded / standalone / layered / compositor)は未定であり、解空間が未固定。
- `neovim_glsl.root_ui_visual_editor_boundary`: 未確定 — Root-ui が visual primitive/shader を所有し neovim-glsl が editor state/semantics を所有するか、editor state も Root-ui へ渡すかが未定。
- `neovim_glsl.zeno_evaluation_scope`: 未確定 — Zeno evaluation の具体 scope(launch・benchmark・compatibility・performance・user acceptance)は未定。evaluation が完了しても Zeno adoption / dependency / successful launch を pin するとは限らない。
- `neovim_glsl.zeno_adoption`: 未確定 — Zeno evaluation 後に Zeno を actual platform target として採用するか、observation のみに留めるかが未定。evaluation は pin 済みだが adoption 決定は人間ゲート待ち。
- `neovim_glsl.note_change_extent`: 未確定 — 「今の yui の notes を少しかえる」の「少し」が指す変更範囲(schema / API / UI / 同期モデルのどこまで)が定量化されておらず、解空間が未固定。
- `neovim_glsl.wysiwyg_fidelity`: 未確定 — 「今の実装と似せていい」は許可であって要求ではなく、似せる度合い(視覚的等価 / 操作等価 / 部分的踏襲)の基準が無い。
- `neovim_glsl.ide_level_criterion`: 主観 — 「IDE レベル」に observable な受け入れ基準(機能集合・比較対象・閾値)が無い。v0.3 の vscode_defeat / strong_usable と同型。
- `neovim_glsl.root_ui_hardening_definition`: 未確定 — 「root-ui をしっかりした」の完了条件が定義されていない。順序の前提となる状態が観測不能なままである。
- `neovim_glsl.aishell_naming`: 曖昧 — メモの「aishell」が v0.2 で pin 済みの aish(ai-native-shell)と同一対象を指すのか、別の shell surface を指すのかは字面だけでは一意に定まらない。同一と仮定して新規 pin を作らず、v0.2 の pin を再利用する。
- `neovim_glsl.protocol_speaking_direction_residue`: 曖昧 — 「protocol を喋る」の面が、UI protocol(ui_attach / redraw)の server 側なのか、API protocol(nvim_* RPC)の server 側なのか、その双方なのかは回答の字面だけでは一意に定まらない。三択の中で core process を持たない案が選ばれた以上、自前 host が Neovim 側の端に座ること自体は確定するが、どの面を実装するかは未定として隔離する。
- `neovim_glsl.telescope_under_own_host`: 未確定 — v0.4 pin file_navigation.mechanism = telescope は Neovim の Lua runtime と API 上で動く plugin を名指ししている。自前 host が UI protocol の server 面だけを喋る場合、その plugin はそのままでは動かない。「telescope」が (a) あの plugin そのものの動作を要求するのか (b) あの操作体験を持つ picker を要求するのかは、v0.4 の指示文だけでは一意に定まらない。両解釈に共通するのは picker 機構が telescope であること(既に pin 済み)なので、新規 pin を作らず隔離する。
- `neovim_glsl.embed_artifact_disposition`: 未確定 — own_host を採ったとき、実測済みの embed candidate(9,878 行、Neovim 0.11.5 実機検証済み)を廃棄するのか、UI client 側の資産として温存し host だけ差し替えるのかは述べられていない。pin neovim_asset_not_discarded は Neovim 資産の全面破棄を禁じるが、この repository 内の評価 artifact の去就までは縛らない。

## open_question — 人間ゲート待ち (26 件)

決めるべきだが情報がない箇所。

- `neovim_glsl.glsl_scope_decision`: GLSL 化の対象はどこまでか。表示・UI 描画層までか、編集エンジンの意味論まで含むか。
- `neovim_glsl.graphics_stack_decision`: GLSL をどの graphics API / driver stack / platform で実行するか。単一 backend か複数か、fallback の有無も未確定。
- `neovim_glsl.performance_acceptance`: 性能をどの workload・観測量・閾値で受け入れるか。指示文に数値は無いため人間ゲートで決める。
- `neovim_glsl.plugin_config_compat_boundary`: 既存の Neovim 設定・plugin・keymap をどこまで維持するか。
- `neovim_glsl.post_establishment_roadmap`: 設立の次に何を要求するか。今回の指示文は設立までしか述べていない。
- `neovim_glsl.aish_integration_mechanism_decision`: aish 統合の実現手段はどれを採るか(Lua / Neovim plugin / 直接 process 起動 / aish の既存 typed protocol)。「Luaでもいい」は許可にすぎないので、選定は人間ゲートで行う。
- `neovim_glsl.aish_integration_depth_decision`: aish の typed object を neovim-glsl 側で第一級 object として保持するか、aish の実行結果を描画するに留めるか。統合の深さは未確定。
- `neovim_glsl.aish_effect_confirmation_surface`: aish 側が凍結している effect 分類と実行前の明示確認を、neovim-glsl の UI 側でどう表現し、迂回していないことをどう観測するか。
- `neovim_glsl.aish_integration_completion_definition`: 「統合を開始」の次に、何が存在すれば統合済みと判定するか。完了条件は未指定。
- `neovim_glsl.aish_visibility_future`: aish の repository を将来公開するのか、恒久的に非公開とするか。「まず」の含意が未確定なので、公開側への変更は人間ゲートを通す。
- `neovim_glsl.aish_integration_code_ownership_decision`: 統合 code をどの repository が所有するか(neovim-glsl 側 / aish 側 / 第三の repository)。
- `neovim_glsl.mac_stage_completion_criterion`: Mac first-stage が何をもって完了と判定されるか。observable milestone、acceptance test、人間判断のいずれで区切るか。
- `neovim_glsl.zeno_evaluation_outcome`: Zeno evaluation の結果を何をもって accept / defer / reject と判定するか。evaluation 後に Zeno を canonical target として採用するか optional path にするか。
- `neovim_glsl.multi_target_parity`: multi-target portability が全 platform で同一 feature set を要求するか、platform-specific subset を許容するか。parity baseline と divergence acceptance は未定。
- `neovim_glsl.root_ui_integration_adoption`: Root-ui integration evaluation hypothesis の結果を何をもって adoption / defer / reject と判定するか。integration が canonical path になるか optional experiment に留まるか。
- `neovim_glsl.root_ui_visual_editor_ownership`: Root-ui が visual primitive/shader のみを所有し editor state/semantics は neovim-glsl が所有するか、editor state も Root-ui へ委譲するか。
- `neovim_glsl.vscode_compatibility_vs_defeat`: VS Code を defeat 対象と見做し non-compatible にするか、migration path を提供し compatibility を保つか。defeat 対象の名指し catalog も未指定。
- `neovim_glsl.note_change_extent_decision`: yui notes をどこまで変えるか。schema・API・同期・UI のどれを触るか。
- `neovim_glsl.ide_level_acceptance`: 「IDE レベル」を何が出来れば達成と判定するか(LSP / DAP / 補完 / 検索 / refactor / task runner のどれを含むか)。
- `neovim_glsl.root_ui_hardening_done`: 「root-ui をしっかりした」の完了条件は何か。何が観測できれば surface 置換に進んでよいか。
- `neovim_glsl.surface_priority`: 実装順序として desktop(neovim-glsl 本体)・web・mobile のどれを先に置くか。メモは三者を並べているが順序を述べていない。
- `neovim_glsl.neovim_asset_reuse_scope`: Neovim 資産のどこを継承するか。v0.6 で protocol の継承だけが確定した(pin asset_reuse_includes_protocol)。残る未確定は plugin 生態系・Lua runtime・keymap 意味論の実装深度・既存 UI client 実装の去就。
- `neovim_glsl.protocol_surface_scope`: 自前 host が Neovim protocol のどの面を喋るか。UI protocol(ui_attach / redraw / ext_* option)の server 面だけか、API protocol(nvim_buf_* / nvim_exec_lua 等)の server 面も含むか。既存の UI client 資産が繋がるのは前者、plugin 生態系が繋がるのは後者。
- `neovim_glsl.protocol_version_baseline`: どの Neovim version の protocol を baseline とし、上流の変更をどう追従するか(固定 pin / 追従 / 自前拡張の許容範囲)。
- `neovim_glsl.telescope_realization_decision`: telescope を plugin そのものとして動かすか、同じ操作体験を持つ自前 picker として実現するか。quarantine telescope_under_own_host の人間ゲート。
- `neovim_glsl.embed_candidate_disposition`: 実測済み embed candidate を UI client 資産として温存するか、廃棄するか。温存するなら host 差し替えの seam をどこに置くか。

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
