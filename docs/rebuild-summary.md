# 再構築のまとめ (2026-09-23、Stage 7c 時点)

`rebuild` ブランチ (push していません)。詳細は [`PLAN.md`](../PLAN.md) の各 Stage の結果メモ。

## いま動くもの

- **依頼 → タスク分割 → 実装 → レビュー → base ブランチへのマージ**までを、実 Claude Code (Haiku) で通しています。
  オーケストレータ 1 + サブエージェント N を ACP で起動し、指示・報告は Yhtye がホストする MCP サーバー経由。
- タスクごとの git worktree / ブランチ (`yhtye/G-n`, `yhtye/G-n-T-m`)、コンフリクト・dirty な base の検出、マージの再試行。
- SQLite にイベントログと状態を保存。アプリを止めても再起動で中断したタスクを `session/load` で同じ会話から再開。
- Claude Design のレイアウト (会話・タスクカード・git グラフ・ブランチ一覧・エージェント出力・@ メンション)。
- **6b で追加**: エージェントの返答の Markdown 表示 (安全な描画)、ステータスバーのプラン名と 5 時間 / 週の使用量メーター
  (Claude Code の `/usage` の実値)。
- **7a で修正**: 「グループが終わったのに進行中のまま」— オーケストレータが `finish_group` を呼ばずにターンを終えていた。
  Yhtye が 1 回催促し、それでも終わらなければ自分でグループを完了 (マージ) させる。UI は「完了待ち」を区別して表示。
  配布バイナリは Wayland + NVIDIA を検出したときだけ DMABUF レンダラを切る。
- **7b で追加**: コンポーザの ⚙ から役割 (オーケストレータ / 実装 / 調査 / レビュー) ごとにハーネス × モデルの候補と既定を
  設定 (全体 + プロジェクト)。モデル一覧はハーネスから取得。オーケストレータはタスクごとに候補内で上書きできる。
- **7c で追加**: **OpenCode** (`opencode acp`) を全役割で選べる (`opencode` がインストールされているときだけ表示)。
  モデルは必ず指定 (OpenCode の「最後に使ったモデル」では起動しない)。475 件のモデル一覧は検索 + プロバイダ別の見出しで選ぶ。
  OpenCode のオーケストレータは書き込みを制限できないため、設定パネルに「⚠ 書き込み制限なし」を表示。
  OpenCode の MCP 呼び出し (コードモードの `execute`) は「execute → yhtye.report_step_done」のように表示。
  消えたハーネスを指す設定・セッションは UI に警告を出して既定で起動する。

## 動かし方

- アプリ: `pnpm tauri dev` (データ `~/.local/share/com.tom039224.yhtye`、モデルは既定 `haiku`、`YHTYE_MODEL` で変更)。
- ブラウザ: `pnpm dev:browser` → Chrome で `http://localhost:1420` → 左の欄に git リポジトリの最上位パスを入れて「開く」。
- テスト: `pnpm test` / `pnpm build` / `cargo test --workspace` /
  実エージェント `cargo test -p yhtye-core -- --ignored --test-threads=1` (Haiku・トークンを使う)。

## 実 Haiku で確認したこと

- ストリーミング、ターン中キャンセル、bypass でのファイル作成、`session/load` での会話復元 (Stage 1)。
- MCP 経由のタスク作成 → サブエージェントの報告 → オーケストレータの完了報告 (2)、implement → 別セッションの review (3a)、
  再起動をまたいだ再開 (3b)、一時リポジトリで 2 タスク → main へのマージ (3c)、長いコマンドを待ってから報告 (6a)。
- Chrome + WS ブリッジで依頼 → 完了 → マージ、中止、再起動、マージ再試行 (5・6a)、Markdown 表示と使用量 (6b)。
- 6b では週の quota が 99〜100% だったため、変更した経路 (起動ディレクトリ・MCP・使用量) に絞って実行し、全件の再実行はしていません。
- OpenCode (無料モデル `opencode/muse-spark-1.3-contributor-free` のみ): 単体の ACP 検証 (7c-1)、Claude Haiku のオーケストレータ +
  OpenCode の実装 + Claude Haiku のレビューで一時リポジトリのグループを完了・マージ (2 回)、OpenCode のオーケストレータ (1 回) (7c-2)。

## 決まったこと (Stage 7a、詳細は PLAN.md「未決事項」)

- `~/.claude` (CLAUDE.md・フック・スキル) は**常に有効** (MCP サーバーだけ隔離)。
- 「対処の内容を見る」「差分を見る」は**出さない**。runs (履歴) ビューは**保留**。
- 使用量の取得頻度・dev ブリッジのトークンは現状維持 (ブリッジは単一ユーザー機専用)。
- 以前からの要確認 (ブランチ名・中止経路・ポート 1422 など) と Tauri ウィンドウでの動作は確認済み。
- 7b / 7c (ハーネス / モデル選択、OpenCode) は実施済み。Orca の端末から起動したときに注入される `OPENCODE_CONFIG_DIR` は
  エージェントに渡さない (ユーザーの `~/.config/opencode` を使う)。

## 既知の制限

- 対応ハーネスは Claude Code (`claude-agent-acp@0.81.0` を npx で起動) と OpenCode 2.0.12 (`opencode acp`)。プロセス管理は Unix 専用。
- OpenCode は MCP サーバーを隔離できない (ユーザーが OpenCode に MCP を足すと Yhtye のエージェントにも付く)。オーケストレータは書き込み可能。
- 使用量は Claude Code の `/usage` の表示書式に依存 (アダプタのバージョン固定で安定、変われば「—」表示になる)。
- CSP はブラウザで同等の設定を検証済み。Tauri ウィンドウでの基本動作はユーザーが確認済み。
- runs ビュー (保留)・git の差分表示・リモートホストは未実装。1180px 未満は横スクロール。
- レビューで残した LOW (PLAN.md 6b 結果メモ): pgid 再利用時の誤 kill の可能性、trace ログでのトークン露出、
  長文ストリーミング時の Markdown 再解析コストなど。
