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
- 日本語の表示。フォントに無い字は字形単位で別フォントへ落ちる
- IME。かな入力の未確定文字列がその場に出て、確定した文字列だけが Neovim に渡る
- 画像とテキストが同じ画面に共存する。Neovim 側の Lua から位置を指定して置ける
- キーボード入力
- `.app` として組む

## 動かないもの

実験段階なので足りていないものは多い。

- ウィンドウ分割（`ext_multigrid` 未対応）
- 補完メニュー、コマンドライン、メッセージの外部描画
- 下線、斜体、太字
- 動画。表示のしくみは画像と同じだが、デコードは実装していない
- 性能は測っていない

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

画面を PNG に書き出すモードもある。ウィンドウを見なくても結果を確認できる。

```bash
./target/debug/nvimgl --snapshot /tmp/shot.png --input 'ihello<Esc>' -- --clean
```

Neovim 内の Lua から画像を置く:

```lua
vim.rpcnotify(1, 'nvimgl_image', '/path/to/image.png', 3, 6, 34, 11)
-- 引数は パス, 行, 列, 幅（文字数）, 高さ（行数）
```

### macOS での注意

Homebrew の `rustc` は環境によって起動時に落ちる（libLLVM の ABI 不整合）。その場合は
rustup のツールチェインを PATH の先頭に置く。cargo は `rustc` を PATH から探すので、
cargo をフルパスで指定するだけでは足りない。

```bash
export PATH="$HOME/.rustup/toolchains/stable-aarch64-apple-darwin/bin:$PATH"
```

ターミナルから直接起動するとウィンドウが前面に来ないことがあり、その状態では IME が
起動しない。`open nvimgl.app` を使うか、ウィンドウをクリックする。

## 状態

実験プロトタイプであり、完成品ではない。構成の選択（Neovim をどう繋ぐか、どの
グラフィックス API を使うか、どこまでを GLSL で描くか）はまだ確定しておらず、
`evaluation/` 以下はそれを判断するために実際に動かして測ったもの。

未決の設計判断は [UNDECIDED.md](UNDECIDED.md) に、開いている選択肢は
[DESIGN-SPACE.md](DESIGN-SPACE.md) に置いてある。
