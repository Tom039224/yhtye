# 設計の記録

このディレクトリは Claude Design で作られた Yhtye (Orchestrator Desktop) のデザインを
リポジトリ側の一次資料として固定しておくためのもの。

| ファイル | 内容 |
|---|---|
| [`orchestrator-desktop.md`](orchestrator-desktop.md) | 画面仕様。レイアウト・状態・インタラクション・ドメインモデル |
| [`tokens.md`](tokens.md) | デザイントークン。色・タイポグラフィ・アニメーション |
| [`source/Orchestrator Desktop.dc.html`](source/) | Claude Design の原本のスナップショット (下記の注意を読むこと) |

## 出典

- Claude Design プロジェクト: `0e49485a-0f81-42f7-8207-0e30f582bbc3`
- https://claude.ai/design/p/0e49485a-0f81-42f7-8207-0e30f582bbc3?file=Orchestrator+Desktop.dc.html
- 取り込み日: 2026-09-21

### スナップショットの扱い

`source/` の `.dc.html` は DesignSync 経由で読んだ内容を**書き起こしたもの**であり、
バイト単位で原本と一致することを機械検証していない。差異が疑わしいときは原本を正とし、
DesignSync の `get_file` で取り直すこと。

```
projectId = 0e49485a-0f81-42f7-8207-0e30f582bbc3
path      = "Orchestrator Desktop.dc.html"
```

`support.js` (dc-runtime, 約 70KB の生成物) はリポジトリに入れていない。
プレビュー時に Claude Design が供給するもので、実装には使わない。

## `.dc.html` 形式について

原本は Claude Design の `.dc.html` 形式で、`support.js` (dc-runtime) がブラウザ上で
React に変換してプレビューする。この形式を実装にそのまま持ち込むことはしない。
読み解きに必要な範囲だけ書いておく。

| 記法 | 意味 |
|---|---|
| `{{ expr }}` | `renderVals()` が返したスコープへのバインディング |
| `<sc-if value="{{ x }}">` | 条件レンダリング。`hint-placeholder-val` はプレビュー時の既定値 |
| `<sc-for list="{{ xs }}" as="x">` | 繰り返し。`hint-placeholder-count` はプレビュー時の件数 |
| `style-hover="..."` | ホバー時に上書きする CSS 宣言 |
| `class Component extends DCLogic` | `state` / `props` / `setState` / `renderVals()` を持つ React 相当のロジック |

`data-props` に宣言された `showGit` / `showTaskLog` / `showGroupBanner` は
**プレビュー用のトグルであって、アプリの設定項目ではない**。実装では常に表示側に倒す。
