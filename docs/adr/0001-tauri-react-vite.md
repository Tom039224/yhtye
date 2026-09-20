# ADR-0001: Tauri 2 + React + TypeScript + Vite を採用する

- 日付: 2026-09-21
- 状態: 採択

## 背景

Claude Design で作られた Yhtye (Orchestrator Desktop) をデスクトップアプリとして実装する。
このアプリは以下を必要とする。

- ローカルの git リポジトリの読み書き (グラフ構築、ブランチ操作、差分)
- エージェントプロセスの起動・停止・標準出力のストリーミング
- リモートホストとの同期
- 常駐して長時間動く。メモリと起動時間が効く

## 決定

**シェル: Tauri 2** — Rust 側でプロセス管理と git を持ち、UI はシステム WebView で描く。

**フロントエンド: React 19 + TypeScript + Vite**

**パッケージマネージャ: pnpm**

## 理由

### Tauri を選ぶ

Electron と比べてバイナリとメモリの占有が一桁小さく、常駐アプリに向く。
それ以上に効くのは、このアプリの仕事の中心 —
プロセス制御・git・ファイルシステム — が**そのまま Rust 側の得意分野**であること。
Electron だと Node で同じことをやるか、結局ネイティブアドオンを書くことになる。

Linux 側の要件 (webkit2gtk-4.1 / javascriptcoregtk-4.1 / libsoup-3.0) は
この開発機で既に満たされている。

### React を選ぶ

デザイン原本の `.dc.html` は `DCLogic` (state / props / setState / renderVals) という
**React そのものの形**でロジックが書かれている。dc-runtime も内部で
`React.createElement` へ変換している。移植の写像が一対一に近い。

`sc-if` → 条件式、`sc-for` → `.map()`、`renderVals()` の戻り値 → props と、
機械的に対応が付く。他のフレームワークだと状態モデルの読み替えが挟まる。

### Vite を選ぶ

`create-tauri-app` の既定であり、Tauri の HMR 構成 (固定ポート 1420、`strictPort`) が
そのまま使える。独自の設定を足す必要がない。

### pnpm を選ぶ

開発機に導入済み。ディスク効率が良く、Tauri テンプレートも公式にサポートしている。
bun も入っているが、一部の Tauri プラグインのインストールスクリプトで
相性の問題が報告されているため避けた。

## 帰結

- UI 側は**描画と純粋な UI 状態だけ**を持つ。git もプロセスも触らない
- 進捗・ログ・git 更新はすべて Rust からのプッシュ型イベントで流す想定
  (タスクカードの進捗バーとログ行がこれを前提にした設計になっている)
- WebView の差異が Linux / macOS / Windows で出る。特に `oklch()` と
  `::-webkit-scrollbar` はデザインが依存しているので、対応状況を実機で確認する必要がある
- ダーク一本のデザインなので、OS のテーマ追従は実装しない

## 却下した案

| 案 | 却下理由 |
|---|---|
| Electron | 常駐アプリとしての占有が大きい。Rust の強みを活かせない |
| Svelte | バンドルは最小だがデザイン原本からの写像が React ほど素直でない |
| Vue 3 | テンプレートは `.dc.html` に近いが、`DCLogic` の状態モデルは React 寄り |
| Vanilla TS | 状態管理を自前で持つことになる。タスク一覧・会話の更新頻度に対して割に合わない |
