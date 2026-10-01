# セットアップ

Yhtye を動かしたい人向け。開発する人は [`docs/DEVELOPMENT.md`](docs/DEVELOPMENT.md)。
プレビルドのバイナリはまだ無いので、ソースからビルドする。

## 必要なもの

対応 OS は Linux のみ ([README](README.md#対応環境))。

| 必要なもの | 用途 | 確認 |
|---|---|---|
| WebKitGTK 4.1 | アプリのウィンドウ | `pkg-config --modversion webkit2gtk-4.1` |
| git | worktree とマージ | `git --version` |
| Node 22+ (`npx` 含む) | ACP アダプタ (`npx` で取得して起動) | `node --version` |
| Rust 1.98+ | ビルド | `rustc --version` |
| pnpm 11+ | ビルド | `pnpm --version` |
| Secret Service (gnome-keyring / KeePassXC / KWallet など) | 秘密の環境変数を使うときだけ | 下記 |

WebKitGTK など (ビルド用を含む):

```sh
# Arch / CachyOS
sudo pacman -S --needed webkit2gtk-4.1 base-devel curl wget file openssl \
  appmenu-gtk-module libappindicator-gtk3 librsvg

# Debian / Ubuntu (CI で使っているパッケージ。実行だけなら libwebkit2gtk-4.1-0 で足りるが、ここではビルドするので -dev が要る)
sudo apt-get install -y build-essential pkg-config libdbus-1-dev \
  libwebkit2gtk-4.1-dev libjavascriptcoregtk-4.1-dev libsoup-3.0-dev \
  libayatana-appindicator3-dev librsvg2-dev
```

Debian / Ubuntu のパッケージ名は CI (`.github/workflows/ci.yml`) のものを写した。Arch 以外での実行は未確認。

## エージェントの準備

Yhtye は自前でエージェントを実装せず、既存のハーネスを [ACP](https://agentclientprotocol.com) で起動する。
使うハーネスを先に入れてログインしておく。**Claude Code は必須** (常に登録される)、OpenCode と Codex は任意。

| ハーネス | 入れるもの | Yhtye の起動方法 | 認証 |
|---|---|---|---|
| Claude Code | Node 22+ (`npx`) だけ。`claude` CLI は不要 (アダプタが SDK 同梱のバイナリを使う) | `npx -y @agentclientprotocol/claude-agent-acp@0.84.0` | Claude Code のローカルログイン (`~/.claude`) を使う。先に一度ログインしておく |
| OpenCode | `opencode` を `PATH` に | `opencode acp` | `opencode auth login` 済みのログイン (`~/.local/share/opencode/auth.json`) を使う |
| Codex | `codex` を `PATH` に | `npx -y @agentclientprotocol/codex-acp@2.0.0` (`CODEX_PATH` にあなたの `codex` を渡す) | あなたの `~/.codex/config.toml` の設定に従う。下記 |

- OpenCode と Codex は、起動時に `PATH` で実行ファイルが見つかったときだけ設定画面の候補に出る。入れた後はアプリを再起動する。
- Claude Code は SDK 同梱のバイナリで動くため、使えるモデルはアダプタのバージョンで決まる (0.84.0 で Sonnet 5.5 まで)。
- ユーザー自身の `~/.claude` の CLAUDE.md・フック・スキルは Yhtye のエージェントにも効く。MCP サーバーだけは隔離している。
- 初回の `npx` はアダプタのダウンロードに時間がかかる (Codex のアダプタは約 440 MB)。起動のタイムアウトは 120 秒。

### Codex (OpenRouter)

実機で確認したのは OpenRouter 経由だけ。Codex の設定 `model_provider` が `openrouter` のとき、
Yhtye は OpenRouter の公開 API からモデル一覧を作る (キーは送らない。エージェントとして使えるツール対応モデルのみ)。

Codex は `~/.codex/config.toml` の設定で API キーを環境変数から取る。例えば設定が
`[model_providers.openrouter.auth]` の `command = "sh"`, `args = ["-c", "echo $OPENROUTER_API_KEY_CODEX"]` の形なら、
デスクトップから起動した Yhtye はシェルの設定 (`config.fish` など) を継承しないのでキーが空になり、
`provider auth command sh produced an empty token` のような応答になる。次のように登録する。

1. 設定画面 (アイコンレール下部の歯車) の「秘密の環境変数」で、名前 (例 `OPENROUTER_API_KEY_CODEX`) と値を登録する。
2. 値は OS のキーリング (service `yhtye`) に保存され、以後起動するすべてのエージェントの環境に渡る。

OpenAI / ChatGPT のログインなど OpenRouter 以外のプロバイダは、アダプタが返すモデル一覧をそのまま出すだけで、実機では確認していない。

### 秘密の環境変数とキーリング

- Linux では Secret Service (D-Bus) が動いている必要がある。gnome-keyring、KeePassXC の Secret Service 連携、KWallet など。
- キーリングが使えない・ロックされているときに、秘密が 1 つでも登録されていると**エージェントの起動が失敗する**
  (メッセージにキーリングのエラーが出る)。
- 名前は `[A-Za-z_][A-Za-z0-9_]*`。`PATH` / `HOME` / `NODE_OPTIONS` / `LD_*` などプログラムの読み込みを変えるものは登録できない。

### 既知の制約

| ハーネス | 制約 |
|---|---|
| 全ハーネス共通 | オーケストレータも書き込める (検証のいらない小さな変更は自分でコミットし、それ以外はタスクに渡す。システムプロンプトでの指示のみ) |
| Codex | オーケストレータとしては使えない ([openai/codex#13746](https://github.com/openai/codex/issues/13746) で `create_task` が呼べない)。実装・調査・レビュー役で使う |
| Codex / OpenCode 共通 | 権限確認なしで動く (Codex は `agent-full-access`)。OpenRouter の無料モデルは 429 や遅延が多く、失敗が普通の応答として返ることがある |

## ビルドとインストール

```sh
git clone https://github.com/Tom039224/yhtye.git
cd yhtye
pnpm install
pnpm tauri build
```

- 実行ファイルは `target/release/yhtye`。
- `tauri.conf.json` のバンドル対象は `all` なので、Linux では `target/release/bundle/` 以下に deb / rpm / AppImage が作られる
  (環境によっては一部のバンドルが失敗することがある)。実行ファイルだけでよければ `pnpm tauri build --no-bundle`。
- ビルドせず試すだけなら `pnpm tauri dev` (初回は Rust のコンパイルに時間がかかる)。

### Arch / CachyOS (PKGBUILD)

`packaging/arch/PKGBUILD` は GitHub の `main` をビルドする `yhtye-git` パッケージ。
実行ファイル・`.desktop`・アイコンが入る。

```sh
cd packaging/arch
makepkg -si
```

## 初回の使い方

1. 起動する。左の欄でプロジェクトのディレクトリ (git リポジトリ) を入力して「開く」。
2. 歯車 (アイコンレール下部) で、役割ごと (オーケストレータ / 実装 / 調査 / レビュー) のハーネス・モデル・effort を選ぶ。
   「全体」と「このプロジェクト」で切り替えられる。変更はすぐ保存され、新しく起動するエージェントから有効になる。
   組み込みの既定は Claude Code で、モデルは環境変数 `YHTYE_MODEL` (未設定なら `haiku`)。
   試すときは Haiku で安く済ませ、本格的に使うときにモデルを上げるとよい。
3. 会話欄のオーケストレータに自然文で依頼を送る。

依頼を送ると次のように進む。

- オーケストレータがグループを作り、タスクに割る。タスクは実装 → レビューなどの工程を持ち、サブエージェントが担当する。
- 各タスクは**リポジトリの外**(データディレクトリ下の `worktrees/`) の専用 worktree とブランチ (`yhtye/<グループ>-<タスク>`) で作業する。
  あなたの作業ツリーには触れない。
- タスクが終わるとグループのブランチ (`yhtye/<グループ>`) に統合され、グループが完了すると
  base ブランチへ `--no-ff` でマージされる。base のブランチをチェックアウトしていて作業ツリーが clean のときだけマージされる
  (dirty や別ブランチのときはマージ待ちになり、整えてから再試行する)。
- コンフリクトやレビュー回数超過などはオーケストレータに報告され、オーケストレータが対処する。
- タスクはキャンセルできる。キャンセルすると worktree は消え、ブランチは残る。
- アプリを閉じる / Ctrl+C で全エージェントを止めて終了する。中断した作業があるプロジェクトは次の起動時に自動で開き、
  中断したタスクを同じ会話から再開する。

### データの場所と環境変数

| 項目 | 内容 |
|---|---|
| データディレクトリ | `~/.local/share/io.github.tom039224.yhtye` (SQLite と `worktrees/`)。旧 `com.tom039224.yhtye` が残っていれば初回起動時に自動で移す |
| `YHTYE_DATA_DIR` | データディレクトリを変える |
| `YHTYE_MODEL` | Claude Code の組み込み既定モデル (既定 `haiku`) |

Yhtye はあなたのリポジトリにはマージ以外では何も書かない。

## トラブルシューティング

| 症状 | 対処 |
|---|---|
| Wayland + NVIDIA で起動直後に `Error 71 ... dispatching to Wayland display` を出して落ちる | 起動時に自動で `WEBKIT_DISABLE_DMABUF_RENDERER=1` を設定して回避する。効かないときは `WEBKIT_DISABLE_DMABUF_RENDERER=1 ./target/release/yhtye` で起動する (この変数を自分で設定済みなら Yhtye は触らない) |
| エージェントが起動しない / `npx` が見つからない | Node 22+ を入れ、`npx` が `PATH` にあるか確認。アプリを起動した環境 (デスクトップのランチャーはシェルの設定を読まないことがある) の `PATH` で見えるか |
| Claude Code が認証エラーになる | Claude Code で一度ログインして `~/.claude` に認証情報があるか確認する |
| 設定画面に OpenCode / Codex が出ない | `opencode` / `codex` が `PATH` にあるか確認し、アプリを再起動する |
| OpenCode で認証エラー | `opencode auth login` を済ませる |
| Codex が空のトークンで失敗する | 「秘密の環境変数」に `config.toml` が参照している変数を登録する |
| 秘密の登録やエージェント起動が「キーリングが使えない」で失敗する | Secret Service (gnome-keyring / KeePassXC など) を起動してロックを解除する |
| グループが「マージ待ち」で止まる | base ブランチをチェックアウトし、作業ツリーを clean にして再試行する |
