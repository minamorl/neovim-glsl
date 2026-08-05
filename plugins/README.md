# plugins

spec v0.10 の人間ゲート回答 **A+C** で決まった拡張点。plugin は **Lua で書き**、
**面を寄与できる**。寄与するのは *宣言* であって描画ではない — 描くのは host。

```lua
nvimglsl.command('Hello', function(argument) nvimglsl.notify('hi ' .. argument) end)

nvimglsl.surface('Card', function(window)
  return {
    surfaces = {
      { id = 'card', x = 0.28, y = 0.22, w = 0.44, h = 0.30, radius = 10,
        fill = 'surface', stroke = 'outline', shadow = { dy = 10, blur = 28 } },
    },
    texts = { { x = 0.30, y = 0.28, text = 'a plugin drew this', role = 'on_surface' } },
  }
end)
```

`:Hello 引数` でコマンドが走り、`:Card` で面が開く（Esc で閉じる）。
動く例は [clock.lua](clock.lua)。

## なぜ宣言なのか

`pin plugin_surface_renderer` が描画主体を host に置いている。描ける plugin は
GL context と atlas と frame loop を要求することになり、そうなると **どの plugin も
他の全部の frame を壊せる**。宣言なら、plugin が画面に対してできる最悪のことは
「悪い絵を記述する」ことだけになる。

## 座標と色

- **x / y / w / h は窓に対する分数**（0..1）。root-ui の normalized layout そのもの
- **radius・shadow は密度非依存 px**。root-ui が絶対長を持ったのはこのため — 角の
  大きさは面の大きさに引きずられない
- **色は role で頼む**。`scrim` `shadow` `surface` `surface_raised` `outline`
  `separator` `on_surface` `on_surface_muted` `accent`。scheme が持っていない role を
  頼むと**代用せずに報告する** — 黙って別の色になった plugin は動いて見えてしまう

## 置き場

`~/.config/nvimglsl/plugins/*.lua`（`NVIMGLSL_PLUGINS` で上書き）。名前順に読む。
一つが落ちても他は読む。落ちる前に登録した分は残る — 半分登録された plugin が
消えると、実在するコマンドが無いことになるので。

`free plugin_layout` / `free plugin_discovery_mechanism` なので、この場所は
実装の選択であって台帳の決定ではない。

## まだ決まっていないこと

- **`open_question plugin_effect_boundary`** — plugin がどこまでやってよいかは
  誰も決めていない。決まるまでの暫定として、`io` / `dofile` / `loadfile` /
  `require` / `os.execute` / `os.remove` / `os.rename` / `os.exit` / `os.getenv` /
  `package.loadlib` を落としてある。**これは決定ではなく制限**。後で開くのは
  移行を伴わないが、誰も決めないうちに file を書いた plugin は取り返せない
- **`open_question plugin_api_scope`** — buffer 編集・keymap 登録・note 参照・
  aish 呼び出しのどれを渡すかは未定。だから面は **キーを受け取らない**（閉じる
  Esc だけ）。入力経路を付けるのは、この問いに事故で答えることになる
- **`open_question plugin_neovim_compat`** — 既存の Neovim Lua plugin が無改変で
  動くことは、どこにも約束されていない
- **B（protocol の API 面）** は今回の機構としては選ばれていないだけで、
  禁止されてはいない（`open_question protocol_surface_scope` は開いたまま）
