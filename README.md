# neovim-glsl

Neovim の画面を GLSL で描く実験。

Neovim 本体には手を入れていない。`nvim --embed` として動かし、編集エンジンはそのまま
Neovim が担当する。このプログラムが持つのは画面と入力だけで、Neovim から送られてくる
画面情報を OpenGL / GLSL で描画する。

![text と画像が同じ画面に共存している様子](evaluation/evidence/image-and-text-coexist.png)

## 動くもの

このスクリーンショットの文字はすべて実際に打ち込んだもので、画像は Neovim 内の Lua が
配置したもの。以下は実機（Apple M4 Max / macOS 26 / Neovim 0.11.5）で確認済み。

- 実際の Neovim の画面が GLSL 経由で出る。行番号、ステータスライン、コマンドライン、
  ハイライト色、カーソルの反転表示
- 文字属性。太字・斜体（フォントに専用の face が無いので合成）、下線 5 種
  （`underline` / `undercurl` / `underdouble` / `underdotted` / `underdashed`）、
  取り消し線。色は nvim の `sp` に従い、無ければ文字色を継ぐ
- ウィンドウ分割と浮動ウィンドウ。`ext_multigrid` で window ごとに来る grid を、位置・
  クリップ・重なり順のとおりに一枚の画面へ合成する
- 日本語の表示。フォントに無い字は字形単位で別フォントへ落ちる
- IME。かな入力の未確定文字列がその場に出て、確定した文字列だけが Neovim に渡る
- 画像とテキストが同じ画面に共存する。Neovim 側の Lua から位置を指定して置ける
- キーボード入力
- `.app` として組む
- aish の read-only structured surface（discovery / status / typed object inspection）
- Mac → Zeno と multi-target 方向を実測する platform report
- Root-ui 統合仮説を検証する machine-readable grid projection
- frame・redraw・glyph atlas・vertex 数の実測と、決定的な headless benchmark

文字属性は実機の snapshot で確認した。下線 5 種はいずれも同じ quad で描いており、
undercurl は正弦を刻んだ波、点線・破線は絶対 x に位相を合わせてあるので、同じ
highlight が続く間は cell 境界で模様が途切れない。

![文字属性の描画](evaluation/evidence/text-attributes.png)

再現するコマンド（`evaluation/candidate-embed-opengl` から）:

```bash
./target/debug/nvimgl --snapshot ../evidence/text-attributes.png \
  --cols 111 --rows 3 --lua "$(cat ../evidence/text-attributes.lua)" -- --clean
```

## 動かないもの

実験段階なので足りていないものは多い。

- 別 OS ウィンドウへ出す window（`win_external_pos`）。このプログラムは窓を一つしか
  持たないので、置き場所が無い分は画面に出さない
- 浮動ウィンドウの半透明（`winblend`）
- 補完メニュー、コマンドライン、メッセージの外部描画
- 動画。表示のしくみは画像と同じだが、デコードは実装していない
- 性能の**合否**。測定はできるようになったが（下記）、何を以て合格とするかは決めていない

## 使い方

必要なもの: Neovim 0.11 以降、Rust（rustup）、macOS。

```bash
cd evaluation/candidate-embed-opengl
cargo build
./target/debug/nvimgl -- --clean
```

Mac アプリとして組む場合:

```bash
./make-app.sh
open nvimgl.app
```

window ごとの grid をやめて、Neovim が全部を一枚の grid に描いた状態で動かす場合:

```bash
./target/debug/nvimgl --no-multigrid -- --clean
```

画面を PNG に書き出すモードもある。ウィンドウを見なくても結果を確認できる。

```bash
./target/debug/nvimgl --snapshot /tmp/shot.png --input 'ihello<Esc>' -- --clean
```

Neovim 内の Lua から画像を置く:

```lua
vim.rpcnotify(1, 'nvimgl_image', '/path/to/image.png', 3, 6, 34, 11)
-- 引数は パス, 行, 列, 幅（文字数）, 高さ（行数）
```

### 性能を測る

計測は既定で**切ってある**。`--perf` を渡さない限り時計は一度も読まれない。

決定的な headless benchmark。window も GL context も Neovim も要らないので、どこでも走る:

```bash
./target/release/nvimgl --perf-bench 500 --perf-warmup 50 --perf-seed 1 \
    --cols 120 --rows 40 --perf-report /tmp/perf.json
```

同じ `--perf-seed` は同じ workload を再生する（event 数・vertex 数・glyph 数まで一致する）。
**再現するのは workload であって時間ではない。** 時間はその場の実測値なので毎回違う。

実際のセッションを測る場合は `--perf` を足す。`--perf-report` だけでも計測は入る:

```bash
./target/release/nvimgl --perf --perf-report /tmp/live.json -- --clean
```

JSON は `nvimgl.perf-observation/v2`。frame time と提示レートの分布（p50・p90・p95・p99）、
redraw batch の event 種別内訳とその適用コスト、glyph atlas の hit / miss / 再ラスタライズ、
frame ごとの vertex 数、そして実行環境と parameter が入る。

**提示レートを FPS とは呼んでいない。** on-demand renderer は要求されたときにしか描かない
ので、提示回数 ÷ 実時間は「どれだけ速く描けるか」ではなく「どれだけ描く必要があったか」に
なる。field 名（`presentations_per_wall_clock_second` / `presentation_rate_hz`）はその
とおりに読めるようにしてあり、どちらの意味で読むべきかは
`measurement.presentation_model` が言う。

**`frame.total_ms` が覆う段は経路で違い、report がそれを名指しする**
（`measurement.frame_total_stages`）。同じ field 名で別の量を出したまま比較させないため。
redraw 適用の計測に **Neovim を待っていた時間は入らない**。span は待ちが終わってから開く。

**観測しなかった値は `null` で出る。**0 では出さない。GPU を通っていない headless 実行の
`gpu_submit_ms` が `null` なのはこのためで、「速かった」ではなく「測っていない」を意味する。

閾値は渡したときだけ効く:

```bash
--perf-frame-budget-ms 16.67
```

渡さなければ `slow_frames.criterion` は `unset_awaiting_human_gate` のままで、超過数も出ない。
`open_question neovim_glsl.performance_acceptance` が未決である以上、
**この実装は合格ラインを自分で決めない。**閾値と無関係に出る数値として、Neovim が
`flush`（フレーム完成）を送った回数と実際に提示した回数の差 `flushes_not_presented` がある。

### aish を混ぜる

`aish-nu` の場所を明示して起動する:

```bash
./target/debug/nvimgl --aish /path/to/ai-native-shell/aish-nu -- --clean
```

Neovim 内で `:AishDiscover`、`:AishStatus`、
`:AishInspect repository /path/to/repository` が使える。
結果は JSON の scratch buffer に開く。現段階は read-only で、`aish run` / `aish exec`
は公開しない。destructive / external change の確認 UI が未決なまま実行経路を足すと、
aish の effect gate を迂回しかねないため。

### platform / Root-ui 評価を記録する

```bash
./target/debug/nvimgl \
  --snapshot /tmp/nvimgl.png \
  --platform-report /tmp/nvimgl-platform.json \
  --root-ui-evaluation /tmp/nvimgl-root-ui.json \
  -- --clean
```

platform report は実行中の OS・architecture・GL/GLSL・Neovim を記録し、Macを第一段、
Zenoを次の評価、全体を multi-target 方向として残す。Root-ui projection は現行 grid を
機械可読にするが、Root-ui 採用や visual/editor ownership を決定しない。

### macOS での注意

Homebrew の `rustc` は環境によって起動時に落ちる（libLLVM の ABI 不整合）。その場合は
rustup のツールチェインを PATH の先頭に置く。cargo は `rustc` を PATH から探すので、
cargo をフルパスで指定するだけでは足りない。

```bash
export PATH="$HOME/.rustup/toolchains/stable-aarch64-apple-darwin/bin:$PATH"
```

Finder から `.app` を起動した場合は shell の `PATH` を引き継がない。nvimgl は
`NVIMGL_NVIM`、現在の `PATH`、各 OS の一般的な配置の順に Neovim を探索する。
独自の場所へ入れた場合は `NVIMGL_NVIM=/absolute/path/to/nvim` を使う。

ターミナルから直接起動するとウィンドウが前面に来ないことがあり、その状態では IME が
起動しない。`open nvimgl.app` を使うか、ウィンドウをクリックする。

## 状態

実験プロトタイプであり、完成品ではない。構成の選択（Neovim をどう繋ぐか、どの
グラフィックス API を使うか、どこまでを GLSL で描くか）はまだ確定しておらず、
`evaluation/` 以下はそれを判断するために実際に動かして測ったもの。

未決の設計判断は [UNDECIDED.md](UNDECIDED.md) に、開いている選択肢は
[DESIGN-SPACE.md](DESIGN-SPACE.md) に置いてある。
