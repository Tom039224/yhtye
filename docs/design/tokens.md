# デザイントークン

`Orchestrator Desktop.dc.html` のインラインスタイルから抽出したもの。
実装では `src/styles/tokens.css` にカスタムプロパティとして落とす。

## 色

デザインは**ダーク一本**。ライトテーマは定義されていない。
土台はニュートラルではなく**わずかに暖色に寄せたグレー** (色相が茶側)。
これを機械的に `#1a1a1a` 系へ丸めると印象が変わるので、値をそのまま使うこと。

### サーフェス

| トークン | 値 | 用途 |
|---|---|---|
| `--surface-base` | `#181716` | アプリ全体の地 |
| `--surface-chrome` | `#141312` | タイトルバー・アイコンレール・ステータスバー |
| `--surface-panel` | `#1b1a18` | プロジェクト一覧、タスク列 |
| `--surface-chat` | `#1e1d1b` | オーケストレータ会話列 |
| `--surface-card` | `#1f1e1c` | タスクカード (実行中) |
| `--surface-card-muted` | `#1d1c1a` | タスクカード (完了) |
| `--surface-inset` | `#1a1917` | git パネル、入力欄、会話内に埋まったカード |
| `--surface-selected` | `#272420` | 一覧の選択行 |
| `--surface-chip` | `#2b2825` | チップ、アイコンレールのアクティブ |
| `--surface-chip-dim` | `#232120` | モードピル、コミットのバッジ |
| `--surface-hover` | `#221f1d` | 一覧のホバー |
| `--surface-hover-strong` | `#33302b` | アイコンボタンのホバー |

### 境界線

| トークン | 値 | 用途 |
|---|---|---|
| `--border` | `#2a2724` | パネル間・ヘッダ下の主境界 |
| `--border-card` | `#302d29` | カード、入力欄、ボタン |
| `--border-subtle` | `#262320` | パネル内の弱い仕切り |
| `--border-muted` | `#2c2926` | 完了カード |
| `--border-dashed` | `#332f2a` | 待機バナー (破線) |
| `--border-hover` | `#3d3830` | ボタンのホバー |

### テキスト

| トークン | 値 | 用途 |
|---|---|---|
| `--text-primary` | `#e8e5df` | 見出し、選択行、タスク名 |
| `--text-body` | `#ddd9d2` | 会話の本文 |
| `--text-secondary` | `#d5cfc5` | 数値の強調 |
| `--text-tertiary` | `#c9c3b9` | ホバー後の一覧行 |
| `--text-quaternary` | `#b5afa5` | ログ行、完了タスク名 |
| `--text-muted` | `#9a948a` | セクション見出し、メタ情報 |
| `--text-dim` | `#8a847a` | プレースホルダ、経過時間 |
| `--text-dim-alt` | `#8e887e` | 非選択のブランチ名、ボタン |
| `--text-faint` | `#7d776e` | コミット sha、待機バナー |
| `--text-faintest` | `#57524b` | グループタスクの連番 |

### アクセント (琥珀)

| トークン | 値 | 用途 |
|---|---|---|
| `--accent` | `#d9a066` | オーケストレータの発話ラベル、アクティブアイコン、リンク |
| `--accent-hover` | `#e8b884` | リンクのホバー |
| `--accent-strong` | `#e0a45c` | 本文中の強調、`@` ボタンのホバー |

### ステータス (oklch)

ステータス色だけ oklch で書かれている。**色相を固定して明度/彩度だけ振る**設計なので、
新しい状態を足すときも同じ流儀 (`oklch(L C H)` の H を選ぶ) に従うこと。

| 状態 | 色相 | 前景 | 背景 | バー / ドット |
|---|---|---|---|---|
| 実装中 | 250 (青) | `oklch(0.86 0.07 250)` | `oklch(0.35 0.06 250)` | `oklch(0.66 0.11 250)` |
| レビュー中 | 300 (紫) | `oklch(0.87 0.06 300)` | `oklch(0.36 0.05 300)` | `oklch(0.66 0.11 300)` |
| 対処中 | 30 (赤橙) | `oklch(0.93 0.05 40)` | `oklch(0.42 0.13 30)` | `oklch(0.58 0.14 30)` |
| 完了 | 150 (緑) | `oklch(0.85 0.09 150)` | `oklch(0.33 0.05 150)` | `oklch(0.72 0.13 150)` |

「対処中」のタスクだけ**カード全体が色を持つ** — 枠 `oklch(0.4 0.09 30)` / 地 `oklch(0.23 0.03 30)` /
本文 `#d8c4b4` / タスク名 `#f0e7df` / メタ `#8e7f74`, `#bfa48f`。他の状態はバッジだけが色を持つ。

その他: 使用量メーター (5h) は `oklch(0.7 0.14 55)` の橙、(week) は `oklch(0.72 0.11 150)` の緑。

## タイポグラフィ

3 書体を**役割で厳密に分けている**。混ぜないこと。

| 書体 | ウェイト | 役割 |
|---|---|---|
| Space Grotesk | 500/600/700 | **ワードマーク「Yhtye」のみ** (14px/600, `letter-spacing:.1em`) |
| JetBrains Mono | 400/500/600 | 機械が出す文字すべて — ラベル、タスク ID、ブランチ名、sha、ログ、数値、ボタン |
| Noto Sans JP | 400/500/600 | 人間が読む日本語 — 会話本文、タスク名、ツールチップ |

基準サイズ 13px。よく出るサイズ: 本文 13px / `line-height:1.75`、
タスク名 12.5px、一覧行 12.5px、パネル見出し 12px、ブランチ 11.5px、
メタ・ログ 10.5px、セクション見出し 10px (`letter-spacing:.14em`)。

発話ラベル (`YOU · 14:02`) は 10px / 500 / `letter-spacing:.1em` の大文字モノスペース。

`-webkit-font-smoothing:antialiased` を body に当てる。

## アニメーション

```css
@keyframes yspin  { to { transform: rotate(360deg) } }
@keyframes ysheen { 0% { transform: translateX(-120%) } 100% { transform: translateX(320%) } }
@keyframes ypulse { 0%,100% { opacity:.25 } 50% { opacity:1 } }
```

| 名前 | 指定 | 当たる場所 |
|---|---|---|
| `yspin` | `.9s linear infinite` | 実行中プロジェクトのリング (上辺だけ濃い円) |
| `ysheen` | `1.9s ease-in-out infinite` | 進捗バーを走る光。幅 34% のグラデーションを重ねる |
| `ypulse` | `1.4s ease-in-out infinite` | 稼働中ステータスバッジのドット。**完了・対処中には当てない** |

`prefers-reduced-motion: reduce` の指定はデザインに無い。Stage 6a で `App.css` に足した:
reduce のときはすべてのアニメーション (spin / sheen / pulse) とトランジションを止める。状態は色と形で読める。

## スクロールバー

```css
::-webkit-scrollbar       { width:9px; height:9px }
::-webkit-scrollbar-thumb { background:#3a3733; border-radius:6px }
::-webkit-scrollbar-track { background:transparent }
```

## 角丸

チップ・バッジ 3–4px / ボタン・一覧行 5px / カード・入力欄 6–7px / ドット `50%`。

## その他の値 (Stage 6a で `tokens.css` に追加)

原本のインラインスタイルにあるが上の表に無かった値。名前は実装で付けた。

| トークン | 値 | 用途 |
|---|---|---|
| `--border-pill` | `#33302b` | タイトルバーのブランチピル |
| `--scrollbar-thumb` | `#3a3733` | スクロールバー |
| `--ring-running-track` / `--ring-running-head` | `oklch(0.72 0.13 150 / .28)` / `oklch(0.75 0.15 150)` | 実行中プロジェクトのリング |
| `--ring-stopped` | `#4a453f` | 停止プロジェクトのリング |
| `--branch-dot` | `#403c37` | 非選択ブランチのドット |
| `--status-{implementing,reviewing}-sheen` | `oklch(0.9 0.06 H / .55)` | 進捗バーの光 |
| `--status-handling-card-{title,meta,meta-strong,icon}` | `#f0e7df` / `#8e7f74` / `#bfa48f` / `#c7ab97` | 対処中カードの文字 |
| `--git-lane-base` / `--git-node-base(-head)` | `#3a3733` / `#4a453f` (`#6f6a61`) | git の base 線と円 |
| `--git-{lane,node,badge-bg,badge-fg}-implementing` | `oklch(0.55 0.09 250)` / `oklch(0.62 0.11 250)` / `oklch(0.32 0.05 250)` / `oklch(0.85 0.06 250)` | git の実装中の枝 |
| `--git-{lane,node,badge-bg,badge-fg}-handling` | `oklch(0.5 0.1 30)` / `oklch(0.58 0.14 30)` / `oklch(0.38 0.11 30)` / `oklch(0.92 0.05 40)` | git の対処中の枝 |
| `--git-badge-reviewing-{bg,fg}` | `oklch(0.34 0.05 300)` / `oklch(0.86 0.06 300)` | git の review バッジ |
| `--usage-5h` / `--usage-week` | `oklch(0.7 0.14 55)` / `oklch(0.72 0.11 150)` | 使用量メーター (Stage 6b で使う) |

**発明した値** (原本に無い。ステータス色の流儀 = 色相固定で明度/彩度を振る、に従った):
`--git-lane-reviewing` `oklch(0.55 0.09 300)` / `--git-node-reviewing` `oklch(0.62 0.11 300)` /
`--git-{lane,node,badge-bg,badge-fg}-done` (色相 150、実装中の明度・彩度と同じ)。
