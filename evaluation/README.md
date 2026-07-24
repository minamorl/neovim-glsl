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

### まだ出来ていないこと（この候補の限界であって、project の限界ではない）

- `ext_multigrid` 未対応。分割 window は単一 grid として来るものだけ扱う。
- popup menu / cmdline / message の ext 化（`ext_popupmenu` 等）は未接続。
- 下線・斜体・太字・undercurl は highlight を読んでいるが描いていない。
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
