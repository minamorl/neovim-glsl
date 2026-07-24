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
- 入力が Neovim に届く。上記スクリーンショットの文字列は、すべて `nvim_input` 経由で
  打ち込んだ結果であって、こちらが描いた文字列ではない。

### まだ出来ていないこと（この候補の限界であって、project の限界ではない）

- `ext_multigrid` 未対応。分割 window は単一 grid として来るものだけ扱う。
- popup menu / cmdline / message の ext 化（`ext_popupmenu` 等）は未接続。
- 下線・斜体・太字・undercurl は highlight を読んでいるが描いていない。
- IME（macOS の日本語入力）未接続。日本語は `nvim_input` 経由でのみ確認した。
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
