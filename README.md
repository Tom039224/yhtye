# Yhtye

複数のコーディングエージェントをオーケストレータ経由で動かすデスクトップクライアント。

ユーザーはエージェント個体に直接指示しない。オーケストレータに自然文で依頼すると、
オーケストレータが作業をタスクに割り、サブエージェントへ配り、結果をまとめて返す。
異常が出たタスクはグループ全体の完了を待たずに報告が上がり、
オーケストレータ自身が対処に入る。

現在の状態: **再構築中 (`rebuild` ブランチ)。** 静的モックの上に組んだ前回の実装は
実際には動かなかったため破棄し、下層 (ACP コア → MCP サーバー → ドメインコア →
フロントエンド → 統合) から「本物で動くこと」を各段の完了条件として積み直している。
進捗と各段の範囲は [`PLAN.md`](PLAN.md)、機能設計は [`docs/architecture/`](docs/architecture/)、
画面設計は [`docs/design/`](docs/design/)。現時点では素の UI で依頼 → タスク分割 → 実装 → base への
マージまでが動く (デザインの適用は Stage 6)。

## 構成

| 層 | 技術 |
|---|---|
| シェル | Tauri 2 (Rust 2021) |
| フロントエンド | React 19 / TypeScript / Vite |
| ハーネス接続 | [ACP](https://agentclientprotocol.com) (Agent Client Protocol) — 当面 Claude Code |
| 指示・報告の経路 | Yhtye がホストする MCP サーバー (streamable HTTP) |
| パッケージマネージャ | pnpm |

採用理由は [ADR-0001](docs/adr/0001-tauri-react-vite.md) を参照。

```
docs/design/         Claude Design から起こした画面仕様・デザイントークン・原本スナップショット
docs/architecture/   オーケストレーションモデル・MCP ツール・コア設計・ACP ハーネスの機能設計
PLAN.md              再構築の段階計画と進捗
docs/adr/            設計判断の記録
src/                 React フロントエンド
src-tauri/           Rust バックエンド
```

## 開発

### 前提

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

### 起動

```sh
pnpm install
pnpm tauri dev      # デスクトップアプリとして起動 (本物のコア + Claude Code)
pnpm dev:browser    # ブラウザで開発: WS ブリッジ + Vite を同時に起動 → http://localhost:1420
```

- エージェントは Claude Code (ACP、`npx @agentclientprotocol/claude-agent-acp`) をローカルログインで使う。
  モデルは `YHTYE_MODEL` (既定 `haiku`)。
- アプリのデータ (SQLite と worktree) は `YHTYE_DATA_DIR`、無ければ `~/.local/share/com.tom039224.yhtye`。
  Yhtye はプロジェクトのリポジトリには何も書かない (マージ以外)。
- アプリを閉じる / Ctrl+C で全エージェントを止めてから終了する。未完了の作業があるプロジェクトは
  次の起動時に自動で開き、中断したタスクを再開する。

#### ブラウザで動かす (開発・E2E 用)

素の Chrome には Tauri の IPC が無いので、同じ API を WebSocket で出す開発用ブリッジ
`yhtye-dev-bridge` を使う ([`core-design.md`](docs/architecture/core-design.md) §10)。

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

#### Wayland + NVIDIA で起動直後に落ちる場合

WebKitGTK の DMA-BUF レンダラが NVIDIA プロプライエタリドライバと噛み合わず、
`Gdk-Message: Error 71 (プロトコルエラー) dispatching to Wayland display` を出して
即クラッシュすることがある。

`src-tauri/.cargo/config.toml` で `WEBKIT_DISABLE_DMABUF_RENDERER=1` を設定済みなので
`pnpm tauri dev` では対処されている。古いビルドなどで必要なら自分で渡す。

```sh
WEBKIT_DISABLE_DMABUF_RENDERER=1 ./target/debug/yhtye
```

アプリ本体も起動時に Wayland + NVIDIA を検出したとき (かつ未設定のとき) だけ自分で設定するので
(`src-tauri/src/webkit_env.rs`、[`core-design.md`](docs/architecture/core-design.md) §9)、
ビルド済みバイナリでも通常は不要。変数が既に設定されていれば (値にかかわらず) アプリは触らない。

### ビルド・検証

```sh
pnpm build                                         # tsc + vite build
pnpm test                                          # Vitest
cargo test --workspace                             # Rust (偽エージェント・一時 git リポジトリ)
cargo test -p yhtye-core -- --ignored --test-threads=1   # 実 Claude Code (Haiku)
cargo clippy --workspace --all-targets && cargo fmt --check
pnpm tauri build --debug --no-bundle               # アプリのデバッグビルド (target/debug/yhtye)
pnpm tauri build                                   # 配布用バイナリ
```

## 設計の参照元

Claude Design プロジェクト `0e49485a-0f81-42f7-8207-0e30f582bbc3` の
`Orchestrator Desktop.dc.html`。取り込み日 2026-09-21。
詳細は [`docs/design/README.md`](docs/design/README.md)。
