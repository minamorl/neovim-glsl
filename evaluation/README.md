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

### 性能: 測定はする。合否は決めない

`quarantine neovim_glsl.performance_criteria` と `free neovim_glsl.performance.numeric_targets`
は「数値目標を発明するな」と言っているのであって「測るな」とは言っていない。
`open_question neovim_glsl.performance_acceptance` を人間ゲートで決めるには、
決める材料が要る。ここに置いたのはその材料であって、判定ではない。

実装は次の 2 経路。どちらも出力は同じ schema `nvimgl.perf-observation/v1` である。

- **headless benchmark** (`--perf-bench`): seed から生成した redraw event 列を
  `Grid::apply` と `gl::build_vertices` へ流す。window も GL context も Neovim も要らない。
  測っているのは実際に window が呼ぶ関数そのものであって、模造品ではない。
- **live session** (`--perf` / `--perf-report`): 実際の `nvim --embed` から来る
  `ext_linegrid` を測る。GPU 提出（draw + swap、snapshot 経路では `glFinish` まで）を含む。

計測は既定で切ってある。`--perf` 系を渡さない限り `Instant::now()` は一度も呼ばれず、
計測点はいずれも bool 判定 1 回で戻る。glyph cache の hit/miss などの counter は
時計を読まないので常時有効。

#### 実測値（このMacで実際に走らせた結果。証拠 JSON 付き）

`evidence/perf-headless-bench.json` — 120×40、500 frame、warmup 50、seed 1、release build:

| 観測量 | p50 | p99 | max |
|---|---|---|---|
| frame 全体 | 0.35 ms | 0.50 ms | 0.59 ms |
| vertex 構築 | — | — | frame のほぼ全部 |
| redraw batch 適用 | 0.003 ms 台 | — | — |

- vertex は 1 frame あたり約 57,000（背景 quad が cell 数だけ必ず出るため）
- glyph atlas: lookup 約 158 万に対して rasterize は 116 回、hit_ratio 1.0（丸め後）、
  atlas 使用高さ 38px / 1024px
- `--perf-frame-budget-ms 16.67` を渡した実行では超過 0 件。
  **この 16.67 は実行時に人間が渡した値であって、この repository が定めた基準ではない。**

`evidence/perf-live-session.json` — 実際の Neovim + 実 GPU（`GL_RENDERER = Apple M4 Max`,
`4.1 Metal - 90.5`）。snapshot 経路で計測したので frame 数は 2 と少ない:

- GPU 提出は 1 frame 目が 51 ms、2 frame 目が 0.33 ms。初回は shader/pipeline と
  atlas texture の初期化を含む。
- redraw event を種別ごとに数えている。実測では `hl_group_set` 147 件、`grid_line` 64 件、
  `option_set` 25 件など 17 種類。どの traffic が frame を重くしたかを後から辿れる。
- `mean_fps_over_wall_clock` が 0.8 と低いのは、snapshot 経路が画面を落ち着かせるために
  固定の待ち時間を持っているためで、描画能力の指標ではない。この経路では
  `instantaneous_fps`（提示間隔から出した値、実測 31.8）の方を見ること。

#### この数値で言えないこと

- **速いとも遅いとも言っていない。** 比較対象も閾値も無い。
- headless benchmark の workload は合成である。実際の編集操作の分布ではない。
  `--perf-seed` が同じなら workload は完全に再生されるが、**再現するのは workload だけで、
  時間は毎回の実測値**である。
- 他候補（新規 host / 再実装 / fork）とは比較していない。それらは未実装である。

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
- 性能の**受入基準**は無い。`open_question neovim_glsl.performance_acceptance` が未決である。
  測定そのものは行えるようになったが（下記）、合否は判定していない。

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

上の実測値を出し直す（release build で測ること。debug の数値は debug の数値であり、
その旨は report の `environment.debug_assertions` に出る）:

```bash
cargo build --release
./target/release/nvimgl --perf-bench 500 --perf-warmup 50 --perf-seed 1 \
    --cols 120 --rows 40 --perf-frame-budget-ms 16.67 \
    --perf-report ../evidence/perf-headless-bench.json
./target/release/nvimgl --snapshot /tmp/perf-shot.png --input 'ihello world<Esc>' \
    --perf-report ../evidence/perf-live-session.json -- --clean
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
