# Yhtye

複数のコーディングエージェントをオーケストレータ経由で動かすデスクトップクライアント。

ユーザーはエージェント個体に直接指示しない。オーケストレータに自然文で依頼すると、
オーケストレータが作業をタスクに割り、サブエージェントへ配り、結果をまとめて返す。
異常が出たタスクはグループ全体の完了を待たずに報告が上がり、
オーケストレータ自身が対処に入る。

English: [README-EN.md](README-EN.md)

![依頼からタスク分割、実装、レビュー、base ブランチへのマージまで](docs/e2e/stage6a/2-done-merged-mention-chip.jpg)

![エージェント (ハーネス × モデル × effort) の設定](docs/e2e/stage7d/1-settings-modal-effort-rows.jpg)

## 状態

個人で作っている初期段階のプロジェクト (v0.1)。プレビルドのバイナリやリリースはまだ無く、
ソースからビルドして使う。

動いているもの:

- プロジェクト (git リポジトリ) を開き、オーケストレータに依頼すると、グループとタスクに分割して
  サブエージェントが git worktree 上で実装し、レビューを経て base ブランチへマージするところまで
- 作業ツリーごとの複数のチャット: BRANCHES のツリー (作業ツリーを今のブランチ名で表示) から新しいチャットを作る (ブランチはアプリから作成でき、専用の作業ツリーで動く)。
  チャットは並行して動かせ、過去のチャットも再開できる (グループは作成時に作業ツリーがチェックアウトしていたブランチへマージされ、マージ前にずれていればオーケストレータが対処する)
- 役割 (オーケストレータ / 実装 / 調査 / レビュー) ごとのハーネス・モデル・effort の設定 (全体とプロジェクトごと)
- ハーネス: Claude Code、OpenCode、Codex (OpenRouter のモデル一覧)、Devin (基本動作は実機確認済み・一部未検証)、MiniMax Code (基本動作は実機確認済み・一部未検証)、Google Antigravity (起動と未ログインの失敗だけ実機確認、ログイン後は未検証)、Grok Build (基本動作は実機確認済み・一部未検証)。
  実行ファイル (`npx` / `opencode` / `codex` / `devin` / `mcode` / `agy_acp_server.par` / `grok`) が `PATH` や `~/.local/bin` などで見つかったものだけが選べる (`mcode` は `~/.minimax-code/bin`、`agy_acp_server.par` は `~/.local/share/agy-acp-server` なども、`grok` は `~/.grok/bin` も探す)。
  設定の「ハーネス」タブで検出状態の確認、パスの手動指定、再検出ができる
- 中断したタスクの再開、タスクのキャンセル、マージコンフリクトなどのオーケストレータによる対処
- API キーなどの秘密の環境変数の登録 (OS のキーリングに保存)

できていないもの・制約:

- 対応 OS は Linux のみ (下記)。配布用パッケージ、自動更新は無い
- 過去のグループを辿る履歴 (runs) ビューは保留で、空表示のまま
- Codex はオーケストレータとして使えない
  ([codex#13746](https://github.com/openai/codex/issues/13746))。実装・レビュー役では使える
- Codex は OpenRouter 経由でのみ実機確認している
- Devin (`devin acp`) は無料プラン (SWE-1.6 Slow) の実機で起動・MCP の接続・実装エージェントとしてのタスクの完了 (`report_step_done`) までを確認した。
  未ログイン時、再起動後の復元などは未確認。動かなければ、
  [`docs/architecture/acp-harnesses.md`](docs/architecture/acp-harnesses.md) §10 (実測のまとめと手動検証チェックリスト) が手がかりになる
- MiniMax Code (`mcode acp`) は mcode 0.6.2 の実機で、起動・モデル一覧・モデルと effort の設定・1 ターンの応答までを確認した (クレジットの都合でプロンプトは少数だけ)。
  既定の `auto` では、作業ディレクトリ外へのシェルの書き込みや削除でも許可要求は来なかったため、許可要求への「1 回だけ許可」の自動応答は実機では未確認 (`permissionMode` が `default` のときに要求が来る)。
  Yhtye の MCP 経由のタスク完了と再開も未確認
  ([`docs/architecture/acp-harnesses.md`](docs/architecture/acp-harnesses.md) §11)。`permissionMode` はあなたの全体設定に書き込まれてしまうので、Yhtye は変えない
- Google Antigravity (Google 公式の ACP サーバー `agy_acp_server`、v1.3.0) は、Yhtye の acp 層から起動・未ログインの失敗・(ダミーの API キーで隔離した環境での) モデル一覧 (14 個)とモデルの設定までを実機で確認した。
  **Google アカウントでのログインとプロンプトは試していない** (認証があなたの `~/.gemini` に書き込まれ、利用枠も使うため)。ログイン後の動作 (ストリーム・許可要求・MCP・再開) は未検証。
  ログインは Yhtye から行わない (ターミナルのログインコマンドも無い): [`SETUP.md`](SETUP.md) の手順で、利用者が `settings.json` に `auth.type` を書く。
  Antigravity の利用規約が第三者のクライアントからの利用をどう扱うかは確認していないので、使う前に確かめてほしい
  ([`docs/architecture/acp-harnesses.md`](docs/architecture/acp-harnesses.md) §13)
- Grok Build (`grok agent --no-leader stdio`) は grok 1.0.46・grok.com の Free プラン (`grok-4.7`) の実機で、起動・モデル一覧・モデルと effort の設定・
  実装エージェントとしてのタスクの完了 (`report_step_done`) までを確認した (プロンプトは 2 回だけ)。許可確認が来たときの自動応答、オーケストレータとしての利用、
  未ログイン時、再開は未確認 ([`docs/architecture/acp-harnesses.md`](docs/architecture/acp-harnesses.md) §12)

段階計画と各段階の結果は [`docs/PLAN.md`](docs/PLAN.md)。

## 対応環境

- **Linux のみ。** 動作確認は Arch / CachyOS (Wayland) だけ。他のディストリビューションは未確認。
- macOS は未検証。
- Windows は非対応。エージェントのプロセス管理が Unix のプロセスグループに依存している
  ([`crates/yhtye-core/src/acp/process.rs`](crates/yhtye-core/src/acp/process.rs))。

## 注意

- エージェントはあなたのリポジトリのコピー (git worktree) でコードとシェルコマンドを承認なしで実行する。
  Yhtye はタスクの結果を統合し、最後に **base ブランチへマージする**。大事なリポジトリで試す前にバックアップかリモートへの push を。
- エージェントの実行は権限確認なしで進む設定 (Claude Code は `bypassPermissions` 相当、Codex は `agent-full-access`、Devin は `bypass`)。
  MiniMax Code は mcode 自身の既定 (`auto`: 作業ディレクトリ内の書き込みと MCP は確認なし。作業ディレクトリ外へのシェルの書き込み・削除も実機では確認なしで通った。確認が来たときは自動で 1 回だけ許可) で動かす。
  Google Antigravity は既定の `default` モードのまま動かす (実機では未確認)。Grok Build はモードが無く、あなたの `~/.grok/config.toml` の権限の設定に従う。
  許可確認が来たときは Yhtye が自動で 1 回だけ許可する。サンドボックスは無い。
- Claude Code / OpenCode / Codex (OpenRouter) / Devin / MiniMax Code / Google Antigravity / Grok Build の利用料金や利用枠は、それぞれのサービスからあなたに課金・消費される。
  Yhtye は課金を管理しない。
- 秘密の環境変数の値は OS のキーリング (Secret Service など) にだけ保存し、Yhtye のデータベースには名前しか置かない。
  ただし登録した変数は**すべてのエージェント**に渡る。

## 使い方

前提のインストールから初回の使い方までは [`SETUP.md`](SETUP.md) (日本語)。

## 開発

前提、起動、ブラウザ開発用ブリッジ、Wayland + NVIDIA の回避策、ビルドと検証のコマンドは
[`docs/DEVELOPMENT.md`](docs/DEVELOPMENT.md)。

```sh
pnpm install
pnpm tauri dev
```

アプリのデータ (SQLite と worktree) は `YHTYE_DATA_DIR`、無ければ `~/.local/share/io.github.tom039224.yhtye`。
以前の識別子 `com.tom039224.yhtye` のディレクトリが残っていれば、初回起動時に自動で移す。

## ドキュメント

| パス | 内容 |
|---|---|
| [`SETUP.md`](SETUP.md) | 使う人向けのセットアップ |
| [`docs/DEVELOPMENT.md`](docs/DEVELOPMENT.md) | 開発ガイド |
| [`docs/PLAN.md`](docs/PLAN.md) | 段階計画と進捗、未決事項 |
| [`docs/architecture/`](docs/architecture/) | オーケストレーションモデル・MCP ツール・コア設計・ACP ハーネスの機能設計 |
| [`docs/design/`](docs/design/) | 画面仕様・デザイントークン・Claude Design 原本のスナップショット |
| [`docs/adr/`](docs/adr/) | 設計判断の記録 |
| [`docs/e2e/`](docs/e2e/) | 各段階の実機確認のスクリーンショット |
| [`docs/rebuild-summary.md`](docs/rebuild-summary.md) | 再構築の要約 |
| [`docs/planned/`](docs/planned/) | 未着手の機能案 |

画面デザインは Claude Design で起こした。原本と仕様は [`docs/design/`](docs/design/) にある。

## ライセンス

BSD-3-Clause。[`LICENSE`](LICENSE) を参照。

作者: Kirsikka (GitHub: [Tom039224](https://github.com/Tom039224))
