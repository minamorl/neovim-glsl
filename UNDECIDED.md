# UNDECIDED — 決まっていない事項

`pins/domains/neovim-glsl.spec@0.11` の `quarantine` と `open_question` を、spec の文言のまま
機械転記したもの。**ここにあるものは決まっていない。** 実装で埋めてはならない。


規律:

- 実装・設定・命名・ディレクトリ構造・README の言い回しのどれによっても、ここの項目を
  「決まったこと」にしない。
- 決めたくなったら repository 側で決めない。converter と人間ゲートを通して spec 側へ lift する。
- 正本は spec。この転記が spec と食い違ったら spec が勝つ。転記側を直す。
- この file は `python3 tools/sync_undecided.py <spec-or-mirror>` で再生成する。手で書き足さない。

## quarantine — 隔離 (28 件)

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
- `neovim_glsl.root_ui_hardening_definition`: 未確定 — 「root-ui をしっかりした」の完了条件が定義されていない。順序の前提となる状態が観測不能なままである。
- `neovim_glsl.aishell_naming`: 曖昧 — メモの「aishell」が v0.2 で pin 済みの aish(ai-native-shell)と同一対象を指すのか、別の shell surface を指すのかは字面だけでは一意に定まらない。同一と仮定して新規 pin を作らず、v0.2 の pin を再利用する。
- `neovim_glsl.protocol_speaking_direction_residue`: 曖昧 — 「protocol を喋る」の面が、UI protocol(ui_attach / redraw)の server 側なのか、API protocol(nvim_* RPC)の server 側なのか、その双方なのかは回答の字面だけでは一意に定まらない。三択の中で core process を持たない案が選ばれた以上、自前 host が Neovim 側の端に座ること自体は確定するが、どの面を実装するかは未定として隔離する。
- `neovim_glsl.embed_artifact_disposition`: 未確定 — own_host を採ったとき、実測済みの embed candidate(9,878 行、Neovim 0.11.5 実機検証済み)を廃棄するのか、UI client 側の資産として温存し host だけ差し替えるのかは述べられていない。pin neovim_asset_not_discarded は Neovim 資産の全面破棄を禁じるが、この repository 内の評価 artifact の去就までは縛らない。
- `neovim_glsl.glsl_advantage_criterion`: 主観 — 「glsl の利点を活かす」に observable な受け入れ基準(何が描けれ ば活かしたと言えるか、grid 制約からの解放をどう測るか)が無い。v0.3 の vscode_defeat、v0.4 の ide_level_criterion と同型。
- `neovim_glsl.navigation_object_scope`: 未確定 — navigation の対象が file なのか markdown note なのか両方なのかが未固定。v0.4 で主 object が markdown note へ移り、user_facing.file_awareness = not_required も pin された以上、「ファイルの移動」を note の移動として読む余地が生じている。v0.4 の逐語は file だが、v0.7 で機構名が外れたことで対象側の二義性が露出した。
- `neovim_glsl.navigation_mechanism_residue`: 曖昧 — 面が host 描画に確定したことで、v0.7 が候補に残した telescope は「行を供給する側」としてしか残れない。plugin が自分の floating window を picker の面として描く形は pin navigation_locus_choice と両立しないが、同じ plugin の finder / sorter を headless に使い host 描画の面へ行を供給する形は却下されない。どちらを指して「telescope を使う」と言うかが未固定。
- `neovim_glsl.plugin_compat_expectation`: 曖昧 — 「Lua で書ける」が、既存の Neovim Lua plugin が無改変で動くことを指すのか、この host 固有の API に対して書き下ろすことを指すのかは回答の字面だけでは一意に定まらない。
- `neovim_glsl.entry_point_form`: 未確定 — 「reposから動線がいい」は方向を述べているが、起動直後に何が出るか(repository の一覧 / 直前の repository / note と repository の混在した面)は述べていない。解空間が未固定。
- `neovim_glsl.ide_capability_depth`: 未確定 — 四つの能力それぞれが「どこまで出来れば含まれると言えるか」は述べられていない。LSP が診断だけで足りるのか補完まで要るのか、検索が正規表現を要求するのかは未定。

## open_question — 人間ゲート待ち (36 件)

決めるべきだが情報がない箇所。

- `neovim_glsl.glsl_scope_decision`: GLSL 化の対象はどこまでか。表示・UI 描画層までか、編集エンジンの意味論まで含むか。
- `neovim_glsl.graphics_stack_decision`: GLSL をどの graphics API / driver stack / platform で実行するか。単一 backend か複数か、fallback の有無も未確定。
- `neovim_glsl.performance_acceptance`: 性能をどの workload・観測量・閾値で受け入れるか。指示文に数値は無いため人間ゲートで決める。
- `neovim_glsl.plugin_config_compat_boundary`: 既存の Neovim 設定・plugin・keymap をどこまで維持するか。
- `neovim_glsl.establishment_definition`: 「project を立てる」の完了条件は何か。何が存在すれば設立済みと判定するか。
- `neovim_glsl.post_establishment_roadmap`: 設立の次に何を要求するか。今回の指示文は設立までしか述べていない。
- `neovim_glsl.downstream_operation_model_link`: この project を第一段とする、より大きな操作モデル構想との接続点をどう定義するか。今回の指示の対象外であり、この spec は後段を pin しない。
- `neovim_glsl.aish_integration_mechanism_decision`: aish 統合の実現手段はどれを採るか(Lua / Neovim plugin / 直接 process 起動 / aish の既存 typed protocol)。「Luaでもいい」は許可にすぎないので、選定は人間ゲートで行う。
- `neovim_glsl.aish_integration_depth_decision`: aish の typed object を neovim-glsl 側で第一級 object として保持するか、aish の実行結果を描画するに留めるか。統合の深さは未確定。
- `neovim_glsl.aish_effect_confirmation_surface`: aish 側が凍結している effect 分類と実行前の明示確認を、neovim-glsl の UI 側でどう表現し、迂回していないことをどう観測するか。
- `neovim_glsl.aish_integration_completion_definition`: 「統合を開始」の次に、何が存在すれば統合済みと判定するか。完了条件は未指定。
- `neovim_glsl.aish_visibility_future`: aish の repository を将来公開するのか、恒久的に非公開とするか。「まず」の含意が未確定なので、公開側への変更は人間ゲートを通す。
- `neovim_glsl.aish_integration_code_ownership_decision`: 統合 code をどの repository が所有するか(neovim-glsl 側 / aish 側 / 第三の repository)。
- `neovim_glsl.own_repository_visibility_decision`: neovim-glsl 自身の repository を公開するか非公開にするか。今回の指示文は aish についてのみ非公開を述べている。
- `neovim_glsl.mac_stage_completion_criterion`: Mac first-stage が何をもって完了と判定されるか。observable milestone、acceptance test、人間判断のいずれで区切るか。
- `neovim_glsl.zeno_evaluation_outcome`: Zeno evaluation の結果を何をもって accept / defer / reject と判定するか。evaluation 後に Zeno を canonical target として採用するか optional path にするか。
- `neovim_glsl.multi_target_parity`: multi-target portability が全 platform で同一 feature set を要求するか、platform-specific subset を許容するか。parity baseline と divergence acceptance は未定。
- `neovim_glsl.root_ui_integration_adoption`: Root-ui integration evaluation hypothesis の結果を何をもって adoption / defer / reject と判定するか。integration が canonical path になるか optional experiment に留まるか。
- `neovim_glsl.root_ui_visual_editor_ownership`: Root-ui が visual primitive/shader のみを所有し editor state/semantics は neovim-glsl が所有するか、editor state も Root-ui へ委譲するか。
- `neovim_glsl.vscode_compatibility_vs_defeat`: VS Code を defeat 対象と見做し non-compatible にするか、migration path を提供し compatibility を保つか。defeat 対象の名指し catalog も未指定。
- `neovim_glsl.note_change_extent_decision`: yui notes をどこまで変えるか。schema・API・同期・UI のどれを触るか。
- `neovim_glsl.root_ui_hardening_done`: 「root-ui をしっかりした」の完了条件は何か。何が観測できれば surface 置換に進んでよいか。
- `neovim_glsl.surface_priority`: 実装順序として desktop(neovim-glsl 本体)・web・mobile のどれを先に置くか。メモは三者を並べているが順序を述べていない。
- `neovim_glsl.neovim_asset_reuse_scope`: Neovim 資産のどこを継承するか。v0.6 で protocol の継承だけが確定した(pin asset_reuse_includes_protocol)。残る未確定は plugin 生態系・Lua runtime・keymap 意味論の実装深度・既存 UI client 実装の去就。
- `neovim_glsl.protocol_surface_scope`: 自前 host が Neovim protocol のどの面を喋るか。UI protocol(ui_attach / redraw / ext_* option)の server 面だけか、API protocol(nvim_buf_* / nvim_exec_lua 等)の server 面も含むか。既存の UI client 資産が繋がるのは前者、plugin 生態系が繋がるのは後者。
- `neovim_glsl.protocol_version_baseline`: どの Neovim version の protocol を baseline とし、上流の変更をどう追従するか(固定 pin / 追従 / 自前拡張の許容範囲)。
- `neovim_glsl.embed_candidate_disposition`: 実測済み embed candidate を UI client 資産として温存するか、廃棄するか。温存するなら host 差し替えの seam をどこに置くか。
- `neovim_glsl.navigation_mechanism_selection`: navigation の機構として実際に何を採るか。telescope を選び直すこともできる(必然性が無いだけで禁止ではない)。
- `neovim_glsl.navigation_input_routing`: picker が開いている間の入力を host が直接受けるか、Neovim へ送って戻すか。pin keymap_preservation(keymap.baseline = current_keymap)との整合をどちらで担保するかも含む。state の所在と独立ではないが同一でもない(host が state を持ちながら入力を Neovim 経由で受ける形も、その逆も構成可能)ため、v0.9 でも別の問いとして残す。
- `neovim_glsl.plugin_discovery`: plugin をどこから読むか。有効化・無効化の単位は何か。
- `neovim_glsl.plugin_api_scope`: Lua plugin に渡す API はどこまでか。buffer 編集・keymap 登録・note 参照・aish 呼び出しのどれを含むか。
- `neovim_glsl.plugin_neovim_compat`: 既存の Neovim Lua plugin を無改変で動かすことを目標にするか、この host 固有の API へ書き下ろす前提にするか。
- `neovim_glsl.plugin_effect_boundary`: plugin は任意の code を実行する。file 書き込み・process 起動・network をどこまで許すか。
- `neovim_glsl.entry_point_decision`: 起動直後に何を出すか。repository の一覧か、直前の repository か、note と repository が同じ面に並ぶか。
- `neovim_glsl.ide_capability_thresholds`: 四つの能力それぞれの合格ライン。LSP はどの言語まで、検索は正規表現を含むか、task は PTY を要求するか。
- `neovim_glsl.dap_and_refactor`: DAP と refactor を後から IDE レベルへ足すか、恒久的に範囲外とするか。

## open_question — 観測待ち (1 件)

人間ゲートに問うたが回答が「不明」だったもの。**閉じていない。** 訊き直すのではなく、何を観測すれば決まるかが spec 側に書かれている。

- `neovim_glsl.navigation_state_owner` (v0.9: gate answered 「わからない」): picker の state(query・候補列・選択位置)を host が持つか、Neovim 側の plugin が持って host は描くだけか。面の locus は決まったが state の所在は決まっていない。2026-08-03 の人間ゲートは「わからない」を返した(@meta derived_from_v0_9)ので、この問いは人間の入力待ちではなく観測待ちである。決め方: 両案を実際に動かし、(a) 一打鍵あたりのstate 更新から面の再構築までの経路の実測、(b) pin keymap_preservation を満たすためにどちらが何を再実装する必要があるかの列挙、(c) plugin 側 state を採った場合に host が行を取得するために必要な protocol 面の実測、の三点を突き合わせる。観測はevaluation/state-ownership に置く。

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

## 決着したもの（参考・spec の closure marker から機械生成）

各項目は spec 側で `# RESOLVED at vX.Y` / `# RETIRED at vX.Y` と記録された行の主題だけを
写している。理由は spec にあり、ここには複製しない。

### v0.11 — 御主人様が機能集合を名指しした

- Retired: quarantine neovim_glsl.ide_level_criterion
- Resolved: open_question neovim_glsl.ide_level_acceptance

### v0.8 — 人間ゲート回答 glsl_overlay

- Retired: quarantine neovim_glsl.external_surface_boundary — 「外部に出す」の「外部」が
- Retired: free neovim_glsl.navigation_ui_locus
- Resolved: open_question neovim_glsl.navigation_surface_decision

### v0.7 — 2026-08-03「telescopeである必然性は特にない」

- Retired: pin neovim_glsl.file_navigation_mechanism: require neovim_glsl.file_navigation.mechanism = telescope
- Retired: example neovim_glsl.navigation: file_move_operation => telescope
- Retired: free neovim_glsl.telescope_customization(telescope 名指しに依存していたため)。
- Retired: quarantine neovim_glsl.telescope_under_own_host — 「telescope が plugin そのものを指すのか
- Resolved: open_question neovim_glsl.telescope_realization_decision

### v0.6

- Retired: quarantine neovim_glsl.architecture — 人間ゲート own_host_protocol により解消。
- Resolved: open_question neovim_glsl.architecture_decision
- Resolved: open_question neovim_glsl.basis_selection

### v0.5 — 2026-08-02 人間ゲート回答 relax

- Retired: pin neovim_glsl.editor_basis: require neovim_glsl.editor.basis = neovim
- Retired: property neovim_glsl.editor_basis_witness / no_substitution_witness
- Retired: example neovim_glsl.editor_retained (editor_basis 退役に伴う)
