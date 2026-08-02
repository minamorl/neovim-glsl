# DESIGN-SPACE — 開いたままにする軸

`pins/domains/neovim-glsl.spec@0.8` の `free` 項目を、spec の文言のまま機械転記したもの。
**free は free のまま。** 実装・設定・依存・慣習のどれによっても、ここの軸を事実上固定してはならない。

入力は `pins/domains/neovim-glsl.spec` ではなく `spec-mirror/neovim-glsl-0.8.lines`。これは外部 spec ledger からの転記であり、正本ではない。食い違ったら spec が勝つ。

規律:

- ここの軸に触れる成果物を置くときは、その軸を選んだことになっていないか先に確かめる。
- 「とりあえずこれで始める」も固定である。仮の選択は暗黙の pin になる。
- 選ぶ必要が出たら repository 側で選ばない。converter と人間ゲートを通して spec 側へ lift する。
- 正本は spec。この転記が spec と食い違ったら spec が勝つ。転記側を直す。
- この file は `python3 tools/sync_design_space.py <spec-or-mirror>` で再生成する。手で書き足さない。

## free 軸 (44 件)

- `neovim_glsl.integration_strategy`: Neovim の embed / external UI protocol 接続 / 再実装 / fork 等の実現手段。指示文は一つも選択していない。
- `neovim_glsl.host_implementation_language`: host および周辺実装の言語・runtime・build system(Rust 等を含む)。特定言語を必須条件にしない。
- `neovim_glsl.graphics_api`: GLSL を実行する graphics API・driver stack。Vulkan / MoltenVK / OpenGL / WebGL 等は候補にすぎず、必須依存として固定しない。
- `neovim_glsl.shader_pipeline`: shader stage 構成、GLSL の version / 方言、中間表現、lowering 経路。
- `neovim_glsl.platform_host`: OS・windowing・display server・compositor・既存 terminal との関係。
- `neovim_glsl.text_rendering`: font raster・glyph atlas・shaping・色管理・カーソル表現。
- `neovim_glsl.project_form`: 「立てる」の具体形式(repository の位置・名前、package manager、初期 scaffold、license)。
- `neovim_glsl.plugin_config_compat`: 既存の Neovim 設定・plugin・keymap の互換範囲。指示文は互換要件を述べていない。
- `neovim_glsl.performance.numeric_targets`: frame rate・入力遅延・memory・起動時間等の数値目標。指示文に存在しないので閾値を発明しない。
- `neovim_glsl.post_establishment_scope`: 設立より先の工程・期日・成果物粒度・完成条件。今回の指示文は設立までしか述べていない。
- `neovim_glsl.downstream_operation_model`: この project を第一段とするより大きな操作モデル構想は今回の指示の対象外であり、この spec は規定しない。
- `neovim_glsl.aish_integration_mechanism_choice`: 統合の実現手段(Lua / Neovim plugin / 直接 process 起動 / aish 側の既存 typed protocol / その他)。「Luaでもいいけど」は許可であって要求ではないので、Lua を必須にも禁止にもしない。
- `neovim_glsl.aish_integration_transport`: 統合の transport(stdio / socket / IPC / RPC 等)、serialization、session 寿命、process の起動・終了の所有関係。指示文は一つも選択していない。
- `neovim_glsl.aish_integration_surface`: 統合で最初に露出する aish の機能範囲、command 表面、UI 表現、結果の描画形式。
- `neovim_glsl.aish_version_coupling`: 依存する aish の version、互換範囲、release 連動、build 時依存か実行時依存か。
- `neovim_glsl.aish_integration_code_ownership`: 統合 code の配置(どちらの repository が持つか)と build 構成。指示文は述べていない。
- `neovim_glsl.own_repository_visibility`: neovim-glsl 自身の repository の公開状態。「非公開リポジトリで」は aish の配布境界についての要求であり、neovim-glsl 側の公開状態を指示していないので、そこへ拡張しない。
- `neovim_glsl.post_commencement_integration_scope`: 「統合を開始」より先の工程・期日・完了条件・成果物粒度。今回の指示文は開始までしか述べていない。
- `neovim_glsl.mac_stage_definition`: Mac first-stage の completion definition、observable milestone、acceptance criterion。first-stage が pin 済みでも完了条件は未指定。
- `neovim_glsl.zeno_evaluation_method`: Zeno evaluation の具体 method、observation scope、pass/fail criterion、evaluation 後の adoption 判断基準。
- `neovim_glsl.multi_target_catalog`: multi-target portability direction の exhaustive target set、parity requirement、各 platform の observable acceptance criterion。
- `neovim_glsl.root_ui_integration_mechanism`: Root-ui integration の transport、event contract、state ownership、rendering boundary、text-editing host port connection。
- `neovim_glsl.root_ui_integration_form_choice`: Root-ui integration を embedded / standalone / layered / compositor のどれで実現するか。
- `neovim_glsl.vscode_defeat_definition`: VS Code defeat を何をもって判定するか。defeat を target にするか、compatibility/migration を優先するか。
- `neovim_glsl.strength_usability_metric`: 強さ・使い勝手の定義、測定 method、threshold、comparison baseline。subjective quality を pin しない。
- `neovim_glsl.zeno_launch_requirement`: Zeno evaluation が successful launch を要求するか、起動試行の observation で足りるか。
- `neovim_glsl.zeno_dependency`: Zeno evaluation 後に Zeno dependency を canonical target として採用するか、optional path に留めるか。
- `neovim_glsl.native_app_packaging`: Mac first-stage が native app packaging (.app bundle / DMG / installer)を要求するか、CLI / shell から起動可能で足りるか。
- `neovim_glsl.note_schema`: markdown note の schema、frontmatter、tag、link、履歴粒度、embedding の扱い。
- `neovim_glsl.db_backend`: DB の実体(Postgres / SQLite / その他)、同期プロトコル、競合解決、offline 挙動。
- `neovim_glsl.mobile_delivery`: スマホ版の配布形態(PWA / native / その他)と署名・配布境界。
- `neovim_glsl.wysiwyg_engine`: WYSIWYG モードの実装機構(既存実装の移植 / 新規 / root-ui component 構成)。
- `neovim_glsl.delegated_editing_protocol`: 編集委譲の transport、承認 UI、差分提示、取り消し、衝突時の扱い。
- `neovim_glsl.lab_integration_surface`: lab.minamorl.com と繋ぐ範囲(artifact 参照 / 双方向編集 / 公開範囲)。
- `neovim_glsl.host_implementation_language`: 別 host を建てる場合の実装言語・ランタイム。
- `neovim_glsl.host_protocol_transport`: 自前 host が protocol を運ぶ transport(stdio msgpack-rpc / socket / in-process 呼び出し)。
- `neovim_glsl.host_editing_core_design`: 自前 host 内部の編集 core 設計(buffer 表現、undo tree、text object 実装)。protocol の外側なので observable な要求が無い。
- `neovim_glsl.lua_runtime_presence`: 自前 host が Lua runtime を内蔵するか否か。v0.7 で telescope の必然性が外れたため、Lua runtime を要求する主要な圧力も消えた。ただし「要らない」も述べられていないので free のまま。
- `neovim_glsl.navigation_mechanism_choice`: navigation を実現する機構(telescope 継続 / Neovim plugin 一般 / 自前 picker / 外部 surface)。v0.4 の名指しが外れたので、どれも要求でも禁止でもない。
- `neovim_glsl.navigation_customization`: picker の sorter / action / preview / 見た目の設計。御主人様は v0.4 で「カスタムまかせる」と述べており、内容を要求していない。
- `neovim_glsl.navigation_input_model`: navigation の入力モデル(fuzzy 検索 / 階層ブラウズ / 履歴 / 意味検索)と、その結果として開く object の種類。
- `neovim_glsl.navigation_surface_addressing`: 面の座標系(pixel でそのまま置くか、cell に揃えるか)。grid を出たことは pixel 配置を可能にするが要求しない。
- `neovim_glsl.navigation_surface_geometry`: 面の位置・大きさ・段組・行送り・余白・透明度・装飾・animation。
- `neovim_glsl.navigation_surface_compositing`: 面を grid と同じ vertex 列 / draw call に載せるか、別 pass として描くか。同じ window に重なることだけが要求。

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
