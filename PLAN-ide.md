# IDE レベルへの計画

spec v0.11 が `ide_level_acceptance` を閉じた — LSP・プロジェクト全文検索・Git 連携・
タスク実行/ターミナル。加えて左のファイルツリーは **本物の window split**、Git は
**読む側**（gutter・blame・diff 閲覧）、入口は **repository 志向**。

5 レーンを別々に計画し、**依存で束ね直した**のがこの文書である。合計 83 タスク。
各レーンの完全なマトリックスは実装 PR に添える。ここに書くのは、**束ねたときにだけ
見える判断** — 独立に計画すると三度書かれるもの、片方を緑にすると他方が pin 違反に
なるもの、そして着手順。

## 0. 共通 P0 — 三つ以上のレーンが独立に要求したもの

これを各レーンに書かせると、同じものが三通りの形で三度実装される。**先に一度だけ通す。**

| id | 何を | 誰が独立に要求したか |
|---|---|---|
| **P0.1** | `proto::serve` を `nvim::read_message` の block から外し、reader thread + `mpsc` にする | LSP（サーバが押す診断が**画面に届かない**）、Git（`git` 呼び出しが `nvim_input` を止める）、タスク実行（出力の streaming） |
| **P0.2** | `Buffer::revision()` — 単調カウンタ。`commit_change`/`undo`/`redo`/`set_lines`/`splice_lines` で上がり、中止した変更では上がらない | LSP（`didChange` の契機）と Git（「最後の diff 以降に編集されたか」）が**同一タスクを二度**書いていた |
| **P0.3** | `proto/paint.rs` から `GridPainter` を切り出し、`gutter_width()` を単一の真実にする | ウィンドウ（multigrid で全面書き換え）・LSP（診断サイン列）・Git（git サイン列）・ツリー（tree 用 hl）の**4レーンが同じファイルを取り合う** |
| **P0.4** | `main.rs` の overlay 分岐を 1 つの enum へ | Git が `picker.is_some() \|\| plugin_surface.is_some()` を**6箇所**、検索が `build_navigation` と `build_plugin_surface` の**二重 `adapter.begin()`** を独立に発見 |
| **P0.5** | `key.rs` に F1–F12 と ctrl の大文字小文字畳み込み | `<F5>`（Jaq）と `<c-S>`（live_grep）は**今このエディタにキーとして到達していない**。`pin keymap_preservation` が二つのバインドについて偽である |
| **P0.6** | 位置の単位変換を一箇所に。UTF-16 / バイト / 文字 / セル | LSP は UTF-16 code unit、ripgrep の column は**1始まりのバイト**、`Buffer` は文字、`paint` はセル。**三レーンが独立に踏んだ** |

P0.5 と P0.6 は「新機能」ではなく**既に壊れているもの**である。P0.6 は
[この repo が一度直した日本語の桁ずれ](README.md)と同じ族で、放置すると LSP のヒットも
grep のヒットも日本語で必ず外れる。

## 1. 幹は window lane

ツリー（第二 grid）・Git の diff view・LSP の診断リストが、**すべて**「複数 window」に
乗る。window lane が滑ると、他の 3 レーンは**緑のまま使えない状態で止まる**。

だから window lane は P0 の直後、単独で通す。他レーンの window 依存タスクは
それぞれの末尾に隔離してある（Git 5.1、LSP 5.3、ツリー 3.5）。

renderer 側は既に `win_pos` / `win_float_pos` / z 順序を食える（embed candidate で実測済み）。
**危険は完全に host 側にある** — これは推測ではなく、読んで確かめた事実。

## 2. pin がレーンをまたいで縛る一点

`pin navigation_not_in_grid` は **検索結果リストにも効く**。ヒットへ飛ぶのは navigation
だからである。つまり:

- **検索結果** — multigrid が入っても grid の quickfix window にしてはいけない。host が描く面のまま
- **LSP の診断リスト** — navigation ではないので、分割 window でよい
- **タスク出力パネル** — 同上

独立に計画すると、window lane が「quickfix を window にする」と書き、検索 lane が
「面のままにする」と書いて、片方が pin 違反になる。**ここは束ねないと見えない。**

## 3. 触ってはいけないもの

- **`evaluation/candidate-embed-opengl/`** — 実測済み artefact。renderer の変更が要ると
  判明したら、黙って編集せず `open_question embed_candidate_disposition` として報告する
- **`neovim-glsl-wt/umg-multigrid` worktree** — `feat/a-multigrid` という紛らわしい名前だが
  `host/` が存在する前から枝分かれしており、`host/` を持たない。**使わない**
- **`notes.rs` の SKIP と `picker.rs` の SKIP** — 意図的に違う。「掃除」のつもりで統合すると
  note picker の一覧が黙って変わる
- **`~/.config/nvim/init.lua`** — baseline であって、こちらが書き換える対象ではない

## 4. 着手順

```
P0 (共通 6 本)
 └→ window lane (単独。幹)
     ├→ file tree
     ├→ git   … 5.1 diff view は window 着地後
     ├→ lsp   … 5.3 診断リストは window 着地後
     └→ search / task … 結果リストは面のまま（pin）
```

P0 と window lane は直列。それ以降の 4 レーンは並列で、各々の末尾だけが window に依存する。

## 5. 決めていないこと

以下は open_question のままで、実装が**答えてしまわない**ように隔離してある。

- **`entry_point_decision`** — 起動直後に何が出るか。ツリーレーンは三形すべてを構成できる
  1 関数の seam を置き、既定を `pin entry_point_orientation` から導くだけにする
- **`plugin_effect_boundary`** — plugin の effect 境界。タスク実行は「御主人様が押したキー、
  または御主人様が打った `:` 行からのみ spawn する」という**別の**線であり、この問いを
  決めない
- **`protocol_surface_scope`** — API 面をどこまで喋るか。window lane は read-only observer
  （`nvim_list_wins` 等）に足がかかるだけ、LSP lane は UI 面（`popupmenu_*`）だけを広げる
- **`ide_capability_thresholds`** — 四つの能力それぞれの合格ライン。実装は能力を在らしめる
  だけで、閾値を宣言しない
