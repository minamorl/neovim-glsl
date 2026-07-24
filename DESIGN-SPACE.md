# DESIGN-SPACE — 開いたままにする軸

`pins/domains/neovim-glsl.spec@0.1` の `free` 項目を、spec の文言のまま転記したもの。**free は free のまま。** 実装・設定・依存・慣習のどれによっても、ここの軸を事実上固定してはならない。

規律:

- ここの軸に触れる成果物を置くときは、その軸を選んだことになっていないか先に確かめる。
- 「とりあえずこれで始める」も固定である。仮の選択は暗黙の pin になる。
- 選ぶ必要が出たら repository 側で選ばない。converter と人間ゲートを通して spec 側へ lift する。
- 正本は spec。この転記が spec と食い違ったら spec が勝つ。転記側を直す。

## free 軸

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

## この repository 自身が free を踏んでいないことの確認

`neovim_glsl.project_form` は repository の位置・名前・package manager・初期 scaffold・license を free にしている。したがってこの設立物は次を持たない。

- package manifest と lock file を置かない (package manager 未選択のまま)。
- source tree と build 設定を置かない (host 実装言語と build system 未選択のまま)。
- shader file を置かない。置けば GLSL の version と方言、shader stage 構成、graphics API を選んだことになる (`shader_pipeline`, `graphics_api`)。
- license file を置かない (`project_form` の license が未選択のまま)。
- 現在の repository の位置と名前は supervisor の routing による便宜であり、要件ではない。移動・改名しても pin は 1 つも壊れない。

`.gitignore` だけは置いてある。これは `secret.no_commit` (`forbid secret.location = repo`) の構造的な担保であり、どの言語・toolchain も選ばない。
