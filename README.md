# Yhtye

複数のコーディングエージェントをオーケストレータ経由で動かすデスクトップクライアント。

ユーザーはエージェント個体に直接指示しない。オーケストレータに自然文で依頼すると、
オーケストレータが作業をタスクに割り、サブエージェントへ配り、結果をまとめて返す。
異常が出たタスクはグループ全体の完了を待たずに報告が上がり、
オーケストレータ自身が対処に入る。

現在の状態: **土台のみ。UI は未実装。**
Claude Design の設計を [`docs/design/`](docs/design/) に記録した段階。

## 構成

| 層 | 技術 |
|---|---|
| シェル | Tauri 2 (Rust 2021) |
| フロントエンド | React 19 / TypeScript / Vite |
| パッケージマネージャ | pnpm |

採用理由は [ADR-0001](docs/adr/0001-tauri-react-vite.md) を参照。

```
docs/design/   Claude Design から起こした画面仕様・デザイントークン・原本スナップショット
docs/adr/      設計判断の記録
src/           React フロントエンド
src-tauri/     Rust バックエンド
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
pnpm tauri dev      # デスクトップアプリとして起動
pnpm dev            # ブラウザで frontend だけ (http://localhost:1420)
```

#### Wayland + NVIDIA で起動直後に落ちる場合

WebKitGTK の DMA-BUF レンダラが NVIDIA プロプライエタリドライバと噛み合わず、
`Gdk-Message: Error 71 (プロトコルエラー) dispatching to Wayland display` を出して
即クラッシュすることがある。

`src-tauri/.cargo/config.toml` で `WEBKIT_DISABLE_DMABUF_RENDERER=1` を設定済みなので
`pnpm tauri dev` では対処されている。ビルド済みバイナリを直接叩くときは自分で渡す。

```sh
WEBKIT_DISABLE_DMABUF_RENDERER=1 ./src-tauri/target/debug/yhtye
```

この回避策は dev 時のみ効く。配布バイナリでどう扱うかは未決
([`docs/design/orchestrator-desktop.md`](docs/design/orchestrator-desktop.md) §7)。

### ビルド・検証

```sh
pnpm build                      # tsc --noEmit 相当 + vite build
cd src-tauri && cargo check     # Rust 側の型検査
pnpm tauri build                # 配布用バイナリ
```

## 設計の参照元

Claude Design プロジェクト `0e49485a-0f81-42f7-8207-0e30f582bbc3` の
`Orchestrator Desktop.dc.html`。取り込み日 2026-09-21。
詳細は [`docs/design/README.md`](docs/design/README.md)。
