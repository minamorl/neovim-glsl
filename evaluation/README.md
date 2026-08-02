# evaluation — 未決事項に対する証拠

ここにあるものは **決定ではなく証拠** である。

`UNDECIDED.md` の `open_question neovim_glsl.architecture_decision` は、spec 自身が
「候補ごとの適合性を**評価してから** lift する」と書いている。この directory は、その評価を
実測で行った supervisor 側の作業結果を置く場所であって、pin でも設計の凍結でもない。

- 動くコードがここにあることは、その候補が採用されたことを意味しない。
- ここで使った Rust / OpenGL / winit / fontdue は、いずれも `DESIGN-SPACE.md` の `free` 軸に
  属する。**この directory の存在によって free が pin へ格上げされることはない。**
- 候補の採否は人間ゲートで決まり、決まったときだけ converter を通して spec 側へ lift される。
  lift されるまで、`pins/domains/neovim-glsl.spec` は architecture について何も言っていない。

## 評価した候補: `candidate-embed-opengl`

`open_question neovim_glsl.architecture_decision` が並べた4候補
（embed + external UI protocol / 新規 host / 再実装 / fork）のうち、**1つ目だけ**を実装して測った。
残り3候補は未評価である。

構成は次のとおり。Neovim は改造せず、編集エンジンとして子プロセスのまま動かし、
この process は画面と入力だけを持つ。

```
nvim --embed  ──msgpack-RPC──▶  grid 状態  ──▶  GLSL (OpenGL 4.1 core)
     ▲                                                    │
     └──────────── nvim_input ◀─── keyboard ──────────────┘
```

### 実測できたこと（このMacで実際に走らせて確認済み）

- GPU は実機。`GL_RENDERER = Apple M4 Max`、`GL_VERSION = 4.1 Metal - 90.5`、`GLSL 4.10`。
  software fallback ではない。証拠: `evidence/glsl-gpu-probe.png`
- 実際の Neovim 0.11.5 の画面が GLSL 経由で出る。行番号、`~` filler、status line、
  command line、highlight 色が正しく反映される。証拠: `evidence/nvim-grid-japanese.png`
- **`ext_multigrid` が繋がっている。** Neovim は window ごとに別 grid を渡し、置き場所を
  `win_pos` / `win_float_pos` / `msg_set_pos` で言ってくる。UI 側はそれを z 順に並べ、
  各 window が申告した範囲と画面の外へはみ出す分でクリップして、一枚の cell 面へ合成
  してから描く。`:vsplit` → `:split` の 3 分割と、重なった float 2 枚（`zindex` 50 と 120）を
  同一画面で実測した。証拠: `evidence/multigrid-splits-and-floats.png`
  合成の結果は描画の前段で決まるので、renderer は今も一枚の grid しか知らない。
  `--no-multigrid` を付ければ、Neovim が全部を一枚に描いていた従来の経路へ戻る。
- 日本語が出る。SF Mono に無い字は Hiragino へ字形単位で fallback する。
- cursor が反転表示される（block の下の字が背景色で描かれる）。
  証拠: `evidence/nvim-cursor-inversion.png`
- **macOS の IME が接続されている。** かな入力の未確定文字列 (preedit) が届き、確定
  (commit) された文字列が Neovim へ渡る。実測ログ:
  `IME: enabled` → `IME: preedit "あ"` → `IME: preedit "ああ"` → `IME: commit "ああ"`。
  未確定文字列は buffer に入っていないので、cursor 位置に反転表示で描き分けている。
  候補ウィンドウの位置は `set_ime_cursor_area` で cursor 行の直下へ追従させる。
- 入力が Neovim に届く。上記スクリーンショットの文字列は、すべて `nvim_input` 経由で
  打ち込んだ結果であって、こちらが描いた文字列ではない。

- **テキスト以外のオブジェクトが同じシーンに共存する。** Neovim 内の Lua が
  `vim.rpcnotify(1, 'nvimgl_image', path, row, col, cols, rows)` を投げると、UI 側が PNG を
  texture にして grid の上へ quad として描く。grid は一切変更していない — Neovim は
  そこに画像があることを知らないまま、テキストと画像が同一画面に並ぶ。
  証拠: `evidence/image-and-text-coexist.png`
  これは端末の graphics protocol 経由ではなく、renderer が全ピクセルを所有しているから
  成立している。cell 格子という制約が無い以上、置けるものは画像に限らない。

- **補完 popup / command line / message が external UI として GLSL 側で描かれる。**
  `nvim_ui_attach` に `ext_popupmenu` / `ext_cmdline` / `ext_messages` を渡すと、Neovim は
  それらを grid へ描くのをやめ、構造化 event として送ってくる。UI 側がその state を保持し、
  overlay として配置・描画する。grid は一切変更していない。
  - popup menu: `popupmenu_show` / `popupmenu_select` / `popupmenu_hide`。anchor の下に置き、
    画面下端に入らないときは上へ反転する。選択行は `PmenuSel`、候補が箱より多いときは
    scrollbar が出る。証拠: `evidence/ext-popupmenu.png`
    （同じ画面の最下行は `msg_showmode` 由来の `-- Keyword completion (^N^P) match 1 of 2`）
  - command line: `cmdline_show` / `cmdline_pos` / `cmdline_special_char` / `cmdline_hide` と
    `cmdline_block_*`。`firstc`・prompt・indent を前置し、cursor は byte offset `pos` を
    表示 cell へ変換して置く。level は stack になっており、外側を閉じると内側も閉じる。
    cmdline が cursor を持つ間、grid 側の cursor block は描かない。
    証拠: `evidence/ext-cmdline.png`
  - message: `msg_show`（`replace_last` / `append`）・`msg_clear`・`msg_showmode`・
    `msg_showcmd`・`msg_ruler`・`msg_history_show` / `msg_history_clear`。chunk ごとの
    highlight id をそのまま使うので、`echoerr` は `ErrorMsg` の色で出る。
    証拠: `evidence/ext-messages.png`、`:messages` は `evidence/ext-messages-history.png`
  - built-in UI highlight group は `hl_group_set` から取る。colour scheme が定義していない
    group は default 背景と前景の中間色へ落とす（見えなくならないため）。
  - `--no-ext-ui` を渡すと3つとも要求しない。その場合 Neovim が従来どおり grid へ描く。
  - 壊れた／途中までの event（引数不足・型違い・item が array でない等）は、前の状態を
    保ったまま無視する。UI が1フレームで落ちると編集 session ごと道連れになるため。

### まだ出来ていないこと（この候補の限界であって、project の限界ではない）

- `win_external_pos`（window を別の OS ウィンドウへ出す要求）は置き場所が無い。窓を
  一つしか持たない構成なので、誤った位置へ描くより画面から外す方を選んでいる。
- 浮動ウィンドウの `winblend`（半透明）は cell を不透明のまま重ねている。
- `win_viewport` / `win_viewport_margins` / `win_extmark` は受け取って捨てている。
  合成に必要な情報ではないが、scrollbar や sign 表示を作るなら要る。
- 下線・斜体・太字・undercurl は highlight を読んでいるが描いていない。
- external UI の未対応部分:
  - `ext_tabline` / `ext_termcolors` は要求していない。tabline は grid のまま。
  - `ext_hlstate` を要求していないので、`hl_attr_define` の `info`（`hi_name`・`kind`）は
    読んでいない。built-in group の解決は `hl_group_set` だけに依っている。
  - popup menu の `info`（候補の詳細説明）は state に保持しているが描いていない。
    Neovim 側の preview window に相当する面をまだ持たない。
  - message の `kind` は保持しているが、描画は chunk の highlight id にのみ依っている。
    `confirm` / `return_prompt` の類も他の message と同じ扱いで、専用の modal を出さない。
  - overlay の文字幅は「`U+2500` 未満なら1 cell」という近似で、grid 本体の cell 割当と
    同じ規則。East Asian Ambiguous や結合文字は正しく測れない。
  - popup の高さは 10 行上限で、`pumheight` は見ていない。
  - message 行が画面より多いときは新しいものを残して古いものを落とす。scroll しない。
- IME の変換候補ウィンドウ自体は macOS が描くもので、GLSL 側では描いていない。
- macOS が `error messaging the mach port for IMKCFRunLoopWakeUpReliable` を出す。
  **`.app` bundle 化しても消えない**（bundle 版・素のバイナリ版の両方で 1 件ずつ出ることを
  実測済み。当初「bundle 化で解消する」と書いたが、実測で否定された）。IME の
  enabled / preedit / commit はいずれも警告と無関係に成立するので、実害は確認されていない。
- glyph atlas は 1024×1024 固定で eviction が無い。字種が増えると埋まる。
- 性能は測っていない。`open_question neovim_glsl.performance_acceptance` が未決なので、
  何を以て合格とするかが無く、数値目標を捏造しないため測定自体を保留した。

### 再現手順

Homebrew の `rustc` はこの Mac で libLLVM ABI 不整合により SIGABRT する。
rustup の toolchain を **PATH の先頭に置く**（binary をフルパス指定するだけでは、
cargo が PATH から `rustc` を拾って落ちる）。

```bash
export PATH="$HOME/.rustup/toolchains/stable-aarch64-apple-darwin/bin:$PATH"
cd evaluation/candidate-embed-opengl
cargo build
```

対話起動:

```bash
./target/debug/nvimgl -- --clean
```

Mac アプリ (`.app` bundle) として組む場合:

```bash
./make-app.sh          # PROFILE=debug で debug ビルドを包む
open nvimgl.app
```

bundle は ad-hoc 署名まで行う。bundle 版でも IME は同じく enabled → preedit → commit まで
通ることを実測済み。CLI から直接起動すると window が前面化しないことがあり、その場合は
focus が来ないので IME も起動しない（`open` 経由か、明示的な activate が要る）。

人間が窓を見なくても結果を検査できるよう、1フレームを offscreen で描いて PNG にする
モードを持たせてある。上の evidence はすべてこれで撮った。

```bash
./target/debug/nvimgl --snapshot /tmp/shot.png --input 'ihello<Esc>' -- --clean
```

### この候補について分かった判断材料

- 「Neovim を編集エンジンとして残したまま、表示だけ GLSL に置換する」は**実際に成立する**。
  仮説ではなく、動いている。
- したがって `quarantine neovim_glsl.glsl_scope`（表示層だけか編集エンジンもか）は、
  少なくとも「表示層だけ」側が技術的に可能だと確認された。
  もう一方の解釈（編集エンジンごと GLSL 化）は評価していない。
- `quarantine neovim_glsl.neovim_retention_form` の2解釈のうち、
  「Neovim 実装(core/process)の継続利用」で満たす形は成立する。

いずれも **可能性の確認であって、採用の宣言ではない。**
