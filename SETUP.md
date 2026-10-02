# セットアップ

Yhtye を動かしたい人向け。開発する人は [`docs/DEVELOPMENT.md`](docs/DEVELOPMENT.md)。
プレビルドのバイナリはまだ無いので、ソースからビルドする。

## 必要なもの

対応 OS は Linux のみ ([README](README.md#対応環境))。

| 必要なもの | 用途 | 確認 |
|---|---|---|
| WebKitGTK 4.1 | アプリのウィンドウ | `pkg-config --modversion webkit2gtk-4.1` |
| git | worktree とマージ | `git --version` |
| Node 22+ (`npx` 含む) | Claude Code と Codex の ACP アダプタ (`npx` で取得して起動) | `node --version` |
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
使うハーネスを先に入れてログインしておく。**ハーネスは 1 つ以上必要**。Yhtye は実行ファイルが見つかったハーネスだけを登録するので、
どれも見つからないとエージェントを起動できない (「使えるハーネスがありません (設定 › ハーネス を確認)」)。
Claude Code は `npx` が見つかれば登録される (最初に試すならこれ)。OpenCode・Codex・Devin・Grok Build は任意。

| ハーネス | 入れるもの | Yhtye の起動方法 | 認証 |
|---|---|---|---|
| Claude Code | Node 22+ (`npx`) だけ。`claude` CLI は不要 (アダプタが SDK 同梱のバイナリを使う) | `npx -y @agentclientprotocol/claude-agent-acp@0.84.0` | Claude Code のローカルログイン (`~/.claude`) を使う。先に一度ログインしておく |
| OpenCode | `opencode` | `opencode acp` | `opencode auth login` 済みのログイン (`~/.local/share/opencode/auth.json`) を使う |
| Codex | `codex` と Node 22+ (`npx`) | `npx -y @agentclientprotocol/codex-acp@2.0.0` (`CODEX_PATH` にあなたの `codex` を渡す) | あなたの `~/.codex/config.toml` の設定に従う。下記 |
| Devin (基本動作は実機確認済み・一部未検証) | Devin CLI (`devin`)。[公式の手順](https://docs.devin.ai/cli) (`curl -fsSL https://cli.devin.ai/install.sh \| bash`) | `devin acp` | `devin auth login` で先にログインしておく。任意で環境変数 `WINDSURF_API_KEY` (下記) |
| Grok Build (基本動作は実機確認済み・一部未検証) | xAI の Grok Build CLI (`grok`)。標準のインストール先は `~/.grok/bin/grok` | `grok agent --no-leader stdio` | `grok login` で先にログインしておく (下記) |

- 入れた後は、設定 › **ハーネス** タブの「再検出」で見つけ直す (アプリの再起動は要らない)。検出の範囲と手動でパスを指定する方法は下の「ハーネスの検出」。
- Claude Code は SDK 同梱のバイナリで動くため、使えるモデルはアダプタのバージョンで決まる (0.84.0 で Sonnet 5.5 まで)。
- ユーザー自身の `~/.claude` の CLAUDE.md・フック・スキルは Yhtye のエージェントにも効く。MCP サーバーだけは隔離している。
- 初回の `npx` はアダプタのダウンロードに時間がかかる (Codex のアダプタは約 440 MB)。起動のタイムアウトは 120 秒。

### ハーネスの検出 (設定 › ハーネス)

設定画面 (アイコンレール下部の歯車) の「ハーネス」タブで、各ハーネスの状態を確認し、パスを指定し、見つけ直せる。

- **状態**: ハーネスごとに「インストール済み / 未インストール」と、必要なコマンドがどこで見つかったか (`PATH` / 既知の場所 / 手動) を出す。
  必要なコマンドは、Claude Code = `npx`、OpenCode = `opencode`、Codex = `codex` と `npx`、Devin = `devin`、Grok Build = `grok`。
  すべて見つかったハーネスだけが、設定の「エージェント」タブの候補になる。
- **自動検出の範囲**: 実行ファイルを、まず `PATH` (絶対パスの項目) から、次に既知の場所
  `~/.local/bin` → `~/.cargo/bin` → `~/.bun/bin` → `/usr/local/bin` の順に探す。デスクトップのランチャーから起動した Yhtye はシェルの設定を
  読まず `PATH` が短いことがあるので、`~/.local/bin` などを別に見ている。Grok Build (`grok`) だけは、インストーラの標準の場所
  `~/.grok/bin` (環境変数 `GROK_HOME` があれば、その `bin`) も見る (fish などでは `PATH` に入らないことがあるため)。実行ファイルは走らせず (`--version` なし)、実行可能な通常ファイルかどうかだけを見る。
- **手動パス**: 上の範囲に無い場所に入れたときは、そのハーネスの主実行ファイル (Claude Code は `npx`、他は `opencode` / `codex` / `devin` / `grok`) の
  **絶対パス**を入力して「保存」する。保存すると自動検出より優先される。実行可能な通常ファイルでなければ保存できない。
  後でそのファイルが壊れたり消えたりすると、そのハーネスは未インストール扱いになり、理由が表示される (自動検出には**戻らない**)。「自動検出に戻す」で消せる。
  手動パスで置き換えるのは主実行ファイルだけで、Codex の `npx` は常に自動検出。
- **再検出**: タブを開いたとき、「再検出」ボタン、手動パスの保存のたびに見つけ直す。「再検出」はモデル一覧のキャッシュも捨てる。
  すでに動いているエージェントには影響せず、新しく起動するエージェントから反映される。

### Devin (基本動作は実機確認済み、一部未検証)

Devin は **devin 3000.11.3・無料プラン (モデルは SWE-1.6 Slow だけ) の実機で基本動作を確認した**: 起動、権限確認なしのモード (`bypass`)、モデルの設定、
1 ターンの応答、Yhtye の MCP への接続とツールの一覧、実装エージェントとしてタスクを `report_step_done` で完了まで進めること。**まだ確かめていない**のは、
未ログイン時のエラー、再起動後の復元 (`session/load`)、ターンの中断、`WINDSURF_API_KEY` での認証。
うまく動かないときは [`docs/architecture/acp-harnesses.md`](docs/architecture/acp-harnesses.md) §10 (実測のまとめ §10.0、手動検証チェックリスト §10.9) が手がかりになる。

- 認証は `devin auth login` (保存された認証情報) が基本。環境変数 `WINDSURF_API_KEY` があれば Devin はそちらを優先する。使うときは、設定 ›
  「秘密の環境変数」に `WINDSURF_API_KEY` を登録する (下記。**すべてのエージェント**の環境に渡る)。
- Yhtye は Devin を権限確認なしのモード (`bypass`) で動かす。ほかのハーネスと同様、サンドボックスは無い。
- 未認証のときは、起動時に認証を求めるエラーが返り、エージェントの起動エラーとして出るはず (偽エージェントでの確認のみ)。
- Devin は MCP のツールを自分のツールとして並べず、MCP の一覧・呼び出し用のツールを通して使う。Yhtye は役割の指示に、その旨の短い注記を Devin にだけ付ける。
- Devin はあなたの `~/.config/devin/mcp_config.json` の MCP サーバーも読み込む (Yhtye の MCP と並んで使える)。
- 無料プランのモデル (SWE-1.6 Slow) には effort が無いので、設定 › エージェント の effort は選べない。

### Grok Build (基本動作は実機確認済み、一部未検証)

Grok Build は **grok 1.0.46・grok.com の Free プラン (モデルは `grok-4.7` だけ) の実機で基本動作を確認した**: 起動、モデル一覧、モデルと effort の設定、
Yhtye の MCP への接続、実装エージェントとしてタスクを `report_step_done` で完了まで進めること (プロンプトは 2 回だけ。利用枠の都合)。
**まだ確かめていない**のは、許可確認が来たときの自動応答、オーケストレータとしての利用、未ログイン時のエラー、再起動後の復元 (`session/load`)、ターンの中断。
実測のまとめと手動検証チェックリストは [`docs/architecture/acp-harnesses.md`](docs/architecture/acp-harnesses.md) §12。

- 認証は `grok login` で先に済ませておく (`~/.grok/auth.json`)。
- 標準のインストール先は `~/.grok/bin/grok`。fish などでは `PATH` に入らないことがあるが、Yhtye はその場所も探す (`GROK_HOME` で場所を変えたなら、その `bin` も探す)。
  `grok` という名前の**別のコマンド** (サードパーティ製の CLI など) が `PATH` の先にあると、そちらが見つかって起動に失敗する。そのときは設定 › ハーネス で `~/.grok/bin/grok` を手動パスにする。
- Yhtye は `grok agent --no-leader stdio` で起動する (エージェントごとに独立したプロセス。あなたの設定で共有の leader を有効にしていても使わない)。`--always-approve` は付けない。
- **grok にはモードが無く、Yhtye は権限の設定を変えない**: 許可の扱いはあなたの `~/.grok/config.toml` の設定 (`[ui] permission_mode`) に従う。確認が来たときは、Yhtye が
  「1 回だけ許可」(`allow_once`) で自動的に通す (「常に許可」やセッション全体の許可は選ばない。あなたの設定に残る・広く効きすぎる恐れがあるため)。ほかのハーネスと同様、サンドボックスは無い。
- effort (`reasoning_effort`) は `xhigh` / `high` / `medium` / `low` (grok の既定は `high`)。
- grok はあなたの MCP サーバー・hooks・Claude Code の規則とスキル (`~/.claude/rules` など) も読み込む。そのぶん 1 ターンの入力が大きい (実装の 1 ターンで約 340k トークン、多くはキャッシュ)。
- Yhtye が起動したセッション (モデル一覧の取得を含む) も、grok のセッションの記録 (`~/.grok/sessions/`) に残る。
- MCP のツールは grok の「ツールを探す / 使う」ツール (`search_tool` / `use_tool`) 経由で使われ、名前は `yhtye__report_step_done` の形になる。実装エージェントでは注記なしで見つけて呼べた。

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
| Devin | 基本動作は実機 (無料プラン) で確認済み、実装エージェントとしてタスクの完了 (`report_step_done`) まで確認済み。権限確認なしの `bypass` モードで動かす。Yhtye の HTTP の MCP にはつながるが、Devin はツールを MCP の一覧・呼び出し用のツール経由で使う (Yhtye が指示に注記を付ける) |
| Grok Build | 基本動作は実機 (Free プラン) で確認済み、実装エージェントとしてタスクの完了 (`report_step_done`) まで確認済み。モードが無く、許可の扱いはあなたの `~/.grok/config.toml` に従う (確認が来れば 1 回だけ許可)。MCP のツールは `search_tool` / `use_tool` 経由。オーケストレータとしては未確認 |

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
   設定していない役割の組み込みの既定は、見つかったハーネスのうち Claude Code → OpenCode → Devin → Grok Build → Codex の順で最初のもの
   (Codex はモデルの指定が要るので、ほかに無いときだけ。そのときは設定でモデルを選ぶよう案内が出る)。
   Claude Code のモデルは環境変数 `YHTYE_MODEL` (未設定なら `haiku`)。
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
| 「使えるハーネスがありません」と出てエージェントが起動しない | 設定 › ハーネス で、どのハーネスも「未インストール」になっていないか確認する。Claude Code は `npx` が必要 (Node 22+ を入れる)。入れたら「再検出」 |
| ハーネスが「未インストール」のまま / 設定の「エージェント」に出ない | 設定 › ハーネス で、必要なコマンド (Claude Code = `npx`、OpenCode = `opencode`、Codex = `codex` と `npx`、Devin = `devin`、Grok Build = `grok`) が「見つかりません」になっていないか見る。`PATH`、`~/.local/bin`、`~/.cargo/bin`、`~/.bun/bin`、`/usr/local/bin` (`grok` は `~/.grok/bin` も) のどこにも無ければ、主実行ファイルの絶対パスを手動で指定するか、入れ直して「再検出」する |
| 手動パスを保存したのに使えない | 表示された理由を確認する。絶対パスの実行可能な通常ファイルでなければならない (ディレクトリやシンボリックリンク先が無い、実行権限が無い、など)。壊れた手動パスは自動検出に戻らないので、直すか「自動検出に戻す」 |
| Claude Code が認証エラーになる | Claude Code で一度ログインして `~/.claude` に認証情報があるか確認する |
| OpenCode で認証エラー | `opencode auth login` を済ませる |
| Devin が認証エラーになる / 起動しない | `devin auth login` を済ませる (`devin auth status` で確認)。`WINDSURF_API_KEY` を使うなら「秘密の環境変数」に登録する。Devin は一部未検証なので、それでも動かないときは `docs/architecture/acp-harnesses.md` §10 の手動検証チェックリストを参照して報告してほしい |
| Grok Build が起動しない / 認証エラーになる | `grok login` を済ませる (`grok models` でログイン状態とモデルが出るか確認)。設定 › ハーネス で `grok` のパスが `~/.grok/bin/grok` (別の `grok` ではない) か確認する。それでも動かないときは `docs/architecture/acp-harnesses.md` §12 を参照して報告してほしい (一部未検証) |
| Codex が空のトークンで失敗する | 「秘密の環境変数」に `config.toml` が参照している変数を登録する |
| 秘密の登録やエージェント起動が「キーリングが使えない」で失敗する | Secret Service (gnome-keyring / KeePassXC など) を起動してロックを解除する |
| グループが「マージ待ち」で止まる | base ブランチをチェックアウトし、作業ツリーを clean にして再試行する |
