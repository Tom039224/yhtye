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
- ハーネス: Claude Code、OpenCode、Codex (OpenRouter のモデル一覧)、Devin (基本動作は実機確認済み・一部未検証)、MiniMax Code (基本動作は実機確認済み・一部未検証)。
  実行ファイル (`npx` / `opencode` / `codex` / `devin` / `mcode`) が `PATH` や `~/.local/bin` などで見つかったものだけが選べる (`mcode` は `~/.minimax-code/bin` も探す)。
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
- MiniMax Code (`mcode acp`) は mcode 0.6.2 の実機で、起動・モデル一覧・モデルと effort の設定・1 ターンの応答までを確認した (プロンプトは 1 回だけ)。
  Yhtye の MCP 経由のタスク完了、許可要求が来る操作、再開は未確認
  ([`docs/architecture/acp-harnesses.md`](docs/architecture/acp-harnesses.md) §11)。`permissionMode` はあなたの全体設定に書き込まれてしまうので、Yhtye は変えない

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
  MiniMax Code は mcode 自身の既定 (`auto`: 作業ディレクトリ内の書き込みと MCP は確認なし、それ以外は確認を自動で 1 回だけ許可) で動かす。サンドボックスは無い。
- Claude Code / OpenCode / Codex (OpenRouter) / Devin / MiniMax Code の利用料金や利用枠は、それぞれのサービスからあなたに課金・消費される。
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
