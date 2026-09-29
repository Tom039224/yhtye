# 計画中の機能: GenOffice の組み込み (Office 文書の生成と編集)

- 日付: 2026-09-29
- 状態: 調査済み・未着手
- 対象: [genspark-ai/genoffice](https://github.com/genspark-ai/genoffice)

## 目的

エージェントが docx / xlsx / pptx を作り・直せるようにし、
その成果物を**Yhtye の中で開いて編集できる**ようにする。

LibreOffice を改造するより素性が良い (TS 製・モノレポ・エンジンが UI から分離済み・
MCP / CLI を既に持つ) ため、GenOffice を土台にする。

## ライセンス

| 範囲 | ライセンス | 改造・再配布 |
|---|---|---|
| リポジトリのほぼ全体 (`apps/`, `packages/` など) | Apache-2.0 (Copyright 2026 Mainfunc, Inc.) | 可 (商用も可) |
| `ee/` | GenOffice Enterprise License | 開発・テスト目的のみ。本番利用・再配布には別契約が必要 |
| 「GenOffice」「Genspark」の名前とロゴ | 商標 | Apache-2.0 の許諾外。フォークは独自ブランドを使う |

`ee/` は現時点では LICENSE と README しかない (将来の enterprise 用の予約領域)。

取り込むときに守ること:

- `LICENSE` と `NOTICE` を同梱する。取り込んだコードを変更したら変更箇所がわかるようにする。
- 「GenOffice」の名前・ロゴを UI に出さない (「Powered by」程度の由来表記は可)。
- `ee/` のコードはビルドに含めない。
- 「Genspark でサインイン」(Genspark プロキシ経由のモデル呼び出し) を使う場合は Genspark の規約に従う。
  Claude は Anthropic API キーで使う。

## GenOffice の構成 (調査時点)

- **Electron** アプリ。ビルドは electron-vite、パッケージングは electron-builder、Node 22.12 以上、npm workspaces。
- `apps/`: `docs` / `sheets` / `slides` / `pdf` / `markdown` / `html` の6エディタと、それらを束ねる `shell`。
  各アプリが main (Node) / preload / renderer (React) の3層で、IPC でつながる。
  `src/main/` のファイル数は shell 75、slides 26、pdf 18、docs 13、sheets 13、html 8、markdown 7。
- `packages/` はほぼ純粋な TS で、Electron に依存しない:
  - `docx-engine` / `pptx-engine` / `xlsx-gateway`: 依存は jszip・fast-xml-parser・numfmt・zod 程度。
  - `agent-core` / `ai-provider`: エージェントのループとプロバイダ層。
    Electron 依存は `agent-core/src/electron-transport.ts` だけに切り出されている。
  - `cli`: `genoffice` コマンド。すべてのコマンドが MCP ツールとしても出ており、`genoffice mcp` (stdio) で起動する。
- `apps/sheets/native/xlsx-engine`: Rust 製の `xlsx-sidecar` (calamine + ironcalc)。Rust なので Tauri 側と相性が良い。
- シートの UI は Univer、PDF の表示は pdf.js。

## 方針

### 段階1 — エージェントから Office 文書を扱う

`session/new` の `mcp_servers` に `genoffice mcp` (stdio) を足す。
サブエージェントが docx / xlsx / pptx を生成・編集できるようになる。
既存の ACP / MCP の設計 ([`acp-harnesses.md`](../architecture/acp-harnesses.md)、
[`core-design.md`](../architecture/core-design.md)) にそのまま収まり、コアの変更はわずか。

決めること: Node をシステムの node に頼るか、単一バイナリ化 (Node SEA / bun compile) して同梱するか。

### 段階2 — アプリ内での編集

GenOffice の **renderer をそのまま Tauri の WebView に載せ**、
**main のコードは Electron なしの Node sidecar で動かす**。Electron と Chromium は使わない。

```
[Tauri WebView (WebKitGTK)]              [Node sidecar (素の node)]
  GenOffice renderer (無改造が目標)         GenOffice main のコード
  preload の代わりの shim:                  ipcMain.handle を受ける中継
    ipcRenderer.invoke / on  ⇄  WS  ⇄       electron API はスタブ
                                            (dialog・shell.openExternal など
                                             Tauri に委ねるものは逆向きに中継)
```

- renderer 側: preload が `contextBridge` で出している `window.*` API を、同じ形の shim で用意する。
  中身は WS 越しの invoke / イベント購読。
- sidecar 側: `electron` モジュールを差し替え (`ipcMain` / `app` / `BrowserWindow` / `dialog` /
  `shell` などの最小限のスタブ) て、各アプリの main をそのまま読み込む。
- main を Rust の command に書き直さないので、**上流との差分が shim とスタブに閉じる**。
  上流の更新を取り込み続けやすい。
- Yhtye には既に WS ブリッジ (`yhtye-dev-bridge`) と Chrome E2E の仕組みがあるので、同じ型を流用する。
- 代償として Node ランタイムを同梱することになる (段階1と共通)。

却下した案:

- **main を Rust の command に書き直す**: 7アプリ分でほぼ作り直しになり、上流の追従が途切れる。
- **`packages/` のエンジンだけを使い、編集 UI を自作する**: エンジン単体では編集 UI が手に入らない。

### 進める順番

1. 段階1 (MCP を追加)。
2. **sheets で試作する**: Univer は WebKitGTK との差が最も出やすいので、最初に試して可否を判断する。
3. docs → slides。pdf / markdown / html は必要になってから。

## 見積もり

前提: Claude が自律で進め、人のレビューは挟まない。数字は実際の作業時間 (人間ペースではない)。

| 作業 | 目安 |
|---|---|
| 段階1 (MCP の追加・ハーネス設定の UI・Node の扱い) | 1〜2時間 |
| E2E で画面を確認できる環境 (`tauri-driver` か既存の Chrome E2E の流用で、スクリーンショットを取れるようにする) | 1〜2時間 |
| 段階2: IPC shim と electron スタブの共通部分 | 半日 |
| 段階2: エディタ1つあたり (sheets / docs / slides) | 1〜2日 |
| 段階2: docs・sheets・slides の3つ | 3〜5日 |

時間を左右するのはコードの量よりも、動作を確かめる作業。

## リスク

- **WebKitGTK との相性**: Univer (canvas 描画)、pdf.js、フォントの計測は Chromium との差が出やすい。
  Yhtye は既に DMABUF で苦労している ([`core-design.md`](../architecture/core-design.md) §9)。
  sheets の試作で先に確かめる。
- **Chromium 専用の API**: renderer が Chromium でしか使えない API を使っていれば、
  polyfill か renderer の変更が要る。試作の段階で洗い出す。
- **見た目と使い心地**: テストが通っても、表示崩れや操作感の悪さは残り得る。
  各エディタが一通りできた時点で、人が数分触って確かめる。
- **上流の変化**: GenOffice は v0.1 系で動きが速い。取り込むコミットを固定し、更新は意図して取り込む。

## 未決事項

- Node を同梱するかどうか (段階1・2 共通)。
- GenOffice を取り込む方法: git submodule / subtree / npm パッケージ。
- 編集画面を開く場所: タブ / 別ウィンドウ / タスク詳細の中。
- GenOffice 内蔵の AI エージェントを残すか (Yhtye のオーケストレータと二重になる)。
