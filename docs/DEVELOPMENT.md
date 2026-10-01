# 開発ガイド

Yhtye 本体を開発する人向け。使うだけなら [`SETUP.md`](../SETUP.md) を参照。
設計は [`architecture/`](architecture/)、進捗は [`PLAN.md`](PLAN.md)。

## 構成

| 層 | 技術 |
|---|---|
| シェル | Tauri 2 (Rust 2021) |
| フロントエンド | React 19 / TypeScript / Vite |
| ハーネス接続 | [ACP](https://agentclientprotocol.com) (Agent Client Protocol) — Claude Code / OpenCode / Codex |
| 指示・報告の経路 | Yhtye がホストする MCP サーバー (streamable HTTP) |
| パッケージマネージャ | pnpm |

採用理由は [ADR-0001](adr/0001-tauri-react-vite.md) を参照。

```
docs/design/         Claude Design から起こした画面仕様・デザイントークン・原本スナップショット
docs/architecture/   オーケストレーションモデル・MCP ツール・コア設計・ACP ハーネスの機能設計
docs/PLAN.md         再構築の段階計画と進捗
docs/adr/            設計判断の記録
docs/e2e/            各 Stage の実機確認のスクリーンショット
docs/planned/        未着手の機能案
src/                 React フロントエンド
src-tauri/           Tauri シェル (Rust)
crates/              yhtye-core (ドメインコア・ACP・MCP)、yhtye-dev-bridge (開発用 WS ブリッジ)
```

## 前提

| 必要なもの | 確認コマンド |
|---|---|
| Rust 1.98+ | `rustc --version` |
| Node 22+ | `node --version` |
| pnpm 11+ | `pnpm --version` |

Linux ではさらに WebKitGTK が要る。

```sh
pkg-config --modversion webkit2gtk-4.1 javascriptcoregtk-4.1 libsoup-3.0
```

Arch / CachyOS 系で足りない場合:

```sh
sudo pacman -S --needed webkit2gtk-4.1 base-devel curl wget file openssl \
  appmenu-gtk-module libappindicator-gtk3 librsvg
```

Debian / Ubuntu 系のビルド依存は [`.github/workflows/ci.yml`](../.github/workflows/ci.yml) の
`Install system dependencies` を参照。

## 起動

```sh
pnpm install
pnpm tauri dev      # デスクトップアプリとして起動 (本物のコア + Claude Code)
pnpm dev:browser    # ブラウザで開発: WS ブリッジ + Vite を同時に起動 → http://localhost:1420
```

- 既定のエージェントは Claude Code (ACP、`npx @agentclientprotocol/claude-agent-acp`) をローカルログインで使う。
  モデルは `YHTYE_MODEL` (既定 `haiku`)。OpenCode / Codex は、それぞれ `opencode` / `codex` が `PATH` にあるときだけ選べる。
- アプリのデータ (SQLite と worktree) は `YHTYE_DATA_DIR`、無ければ `~/.local/share/io.github.tom039224.yhtye`。
  以前の識別子 `com.tom039224.yhtye` のディレクトリが残っていれば、初回起動時に新しい名前へ自動で移す
  (両方あるときは触らない。`src-tauri/src/legacy_data.rs`)。
  Yhtye はプロジェクトのリポジトリには何も書かない (マージ以外)。
- アプリを閉じる / Ctrl+C で全エージェントを止めてから終了する。未完了の作業があるプロジェクトは
  次の起動時に自動で開き、中断したタスクを再開する。

### ブラウザで動かす (開発・E2E 用)

素の Chrome には Tauri の IPC が無いので、同じ API を WebSocket で出す開発用ブリッジ
`yhtye-dev-bridge` を使う ([`core-design.md`](architecture/core-design.md) §10)。

```sh
pnpm dev:browser                              # ブリッジ (ws://127.0.0.1:1422/ws) + Vite (1420)
pnpm dev:browser --data-dir /tmp/yhtye-dev    # 以降の引数はブリッジへ (--port / --model / --token)
```

- ブリッジは 127.0.0.1 のみで待ち受け、`Origin` が `http://localhost:1420` / `http://127.0.0.1:1420`
  (ブラウザからの接続) で、`?token=` が一致する接続だけ受け付ける。`pnpm dev:browser` はランダムな
  トークンを作ってブリッジ (`YHTYE_BRIDGE_TOKEN`) と Vite (`VITE_YHTYE_BRIDGE_TOKEN`) の両方に渡す。
- ブリッジのデータは既定で `$XDG_DATA_HOME/yhtye-dev-bridge` (アプリとは別)。
- 別々に起動する場合: `pnpm bridge -- --token T` と `VITE_YHTYE_BRIDGE_TOKEN=T pnpm dev`。
- **dev ブリッジは単一ユーザーのマシン専用。** トークンは Vite のバンドルに埋め込まれるため、同じマシンの
  他のユーザーは `localhost:1420` から読めてしまう (配布物には含まれない。Stage 7a でこの前提で決定)。

### Wayland + NVIDIA で起動直後に落ちる場合

WebKitGTK の DMA-BUF レンダラが NVIDIA プロプライエタリドライバと噛み合わず、
`Gdk-Message: Error 71 (プロトコルエラー) dispatching to Wayland display` を出して
即クラッシュすることがある。

`src-tauri/.cargo/config.toml` で `WEBKIT_DISABLE_DMABUF_RENDERER=1` を設定済みなので
`pnpm tauri dev` では対処されている。古いビルドなどで必要なら自分で渡す。

```sh
WEBKIT_DISABLE_DMABUF_RENDERER=1 ./target/debug/yhtye
```

アプリ本体も起動時に Wayland + NVIDIA を検出したとき (かつ未設定のとき) だけ自分で設定するので
(`src-tauri/src/webkit_env.rs`、[`core-design.md`](architecture/core-design.md) §9)、
ビルド済みバイナリでも通常は不要。変数が既に設定されていれば (値にかかわらず) アプリは触らない。

## ビルド・検証

```sh
pnpm build                                         # tsc + vite build
pnpm test                                          # Vitest
pnpm check:layout                                  # 実ブラウザで「ページ全体がスクロールしない」ことを検査
cargo test --workspace                             # Rust (偽エージェント・一時 git リポジトリ)
cargo test -p yhtye-core -- --ignored --test-threads=1   # 実エージェント (Claude Code は Haiku)
cargo clippy --workspace --all-targets && cargo fmt --check
pnpm tauri build --debug --no-bundle               # アプリのデバッグビルド (target/debug/yhtye)
pnpm tauri build                                   # 配布用バイナリ
```

### レイアウトの検査 (`pnpm check:layout`)

Yhtye は画面全体を 1 枚の固定レイアウト (`.app`) で使い、スクロールするのは各パネルの内側だけ。
html / body / `#root` は `height: 100%` + `overflow: hidden` で固定してあるが、それは最後の砦にすぎない。
はみ出す要素がそもそも無いことを、次の 2 つで守る。

- **`src/ui/pageFrame.test.ts` (Vitest)**: jsdom はレイアウトを計算しないので、CSS の規則だけを見る
  (html / body / `#root` の高さと overflow、`.app` の位置と `min-width` が無いこと、`min-height: 0` / `min-width: 0`、
  スクロール領域がすべて `position` を持つこと)。
- **`pnpm check:layout` (`scripts/check-layout.mjs`)**: playwright-core + Chromium (headless) で、
  `layout-harness.html` (本物の `App` をメモリ上のコアで動かす開発専用ページ。`src/test/layoutHarness.tsx`) を
  画面の状態ごとに開き、`document.scrollingElement` の `scrollHeight <= innerHeight`、`scrollWidth <= innerWidth`、
  `scrollX / scrollY == 0`、枠 (`.app` `.body` `.sidebar` `.conversation` `.right` など) の中身がはみ出していないこと、
  ポップアップ (⋯ メニュー・プロジェクトのプルダウン・モデル一覧) がウィンドウ内に収まっていることを確かめる。
  状態は、プロジェクト無し・記録した実行・大量の会話/タスク/ブランチ・ホイール操作・各ポップアップ・設定・
  エラーバナー・パネルの限界までのリサイズ・Tab で全部の部品に触れる、を 1060x600 から 1920x1080 まで。
  Rust のコアは要らない (Vite だけを空いているポートで起動する)。画面を足す・直すときは実行しておく。

```sh
pnpm check:layout
YHTYE_CHROMIUM=/usr/bin/google-chrome pnpm check:layout   # ブラウザの指定 (既定: /usr/bin/chromium など。無ければ Playwright の)
YHTYE_LAYOUT_URL=http://localhost:1420 pnpm check:layout  # 起動中の Vite を使う (既定: 空きポートで新しく起動)
YHTYE_LAYOUT_SHOTS=/tmp/yhtye-shots pnpm check:layout     # 各状態のスクリーンショットも保存する
```

失敗すると、どの状態で何がはみ出したか (例: `span.sr-only (absolute, in the page) down to y=113505`) を出す。
新しい状態を足すには、`scripts/check-layout.mjs` の `STATES` に 1 行足す。CI には入れていない (Chromium が要る)。

CI は [`.github/workflows/ci.yml`](../.github/workflows/ci.yml) (フロントエンドのビルドとテスト、
Rust の fmt / clippy / テスト。実エージェントを使う `#[ignore]` のテストは含まない)。
