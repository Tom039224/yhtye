# Yhtye 再構築計画

前回の試みは UI を静的モックの上に組んだため、実際にはほぼ何も動かなかった
(メッセージ送信が "session not started" で失敗する)。コードを捨て、下層から積み直す。
**各 Stage の完了条件は「本物で動くこと」** — 偽エージェントの決定的テストに加え、
実エージェント (Claude Code、**必ず Haiku**) での確認を含む。

設計: [`docs/architecture/`](docs/architecture/)
(モデル: `orchestration-model.md` / ツール: `mcp-tools.md` / 構成: `core-design.md` /
ACP・ハーネス: `acp-harnesses.md`)。画面: [`docs/design/`](docs/design/)。

## 状態

| Stage | 内容 | 状態 |
|---|---|---|
| 0 | 旧コード破棄・調査・設計 | **完了** |
| 1 | ACP コア | 未着手 |
| 2 | MCP サーバー + マルチセッション | 未着手 |
| 3 | ドメインコア (状態機械・SQLite・git) | 未着手 |
| 4 | フロントエンド (素の UI) | 未着手 |
| 5 | 統合 (Tauri + WS ブリッジ + Chrome E2E) | 未着手 |
| 6 | Claude Design 適用・残機能 | 未着手 |

## 再開の仕方

- Stage は**順番に**、それぞれ別のサブエージェントが実行する。前の Stage が完了している前提で始める。
- 始める前に: この PLAN.md、`docs/architecture/` 全体、`git log --oneline -20` を読む。
- 設計と食い違う実装判断が必要になったら、**先に設計ドキュメントを更新**してから実装する。
  合意済みの決定 (`orchestration-model.md`) を変える必要がある場合は、変えずに報告する。
- 各 Stage の終わりに: 完了条件のテストをすべて実行 → この表の状態を更新し、
  Stage の節に「結果メモ」(実行したコマンド・実エージェントでの観察・残課題) を追記 →
  `rebuild` ブランチにコミット (`<type>: <description>`)。push はしない。
- 実エージェントテストは Claude Code のローカルログインを使う。モデルは必ず Haiku
  (`ANTHROPIC_MODEL=haiku`、[`acp-harnesses.md`](docs/architecture/acp-harnesses.md) §5)。

---

## Stage 0 — 破棄と設計 (完了)

- 旧 `src/components|data|state|styles|types`、`src-tauri/src/acp`、旧 PLAN.md を削除し、
  `App.tsx` / `App.css` / `main.tsx` / `lib.rs` / `Cargo.toml` をスキャフォールド時点に戻した。
- 調査: `agent-client-protocol` 2.2、`@agentclientprotocol/claude-agent-acp` 0.81、`rmcp` 3.4。
- 設計ドキュメント: `orchestration-model.md` / `mcp-tools.md` / `core-design.md` /
  `acp-harnesses.md` を作成・更新。
- 検証: `pnpm build`、`cargo check` (src-tauri) が通る。

## Stage 1 — ACP コア

**範囲**
- Cargo workspace をリポジトリ直下に作り、`crates/yhtye-core` (まず `acp` モジュールのみ) と
  `crates/yhtye-fake-agent` を追加。`src-tauri` も member にする。
- `acp::spawn_agent` / `AgentHandle` / `AgentCmd` / `AgentEvent` ([`core-design.md`](docs/architecture/core-design.md) §3)。
  - プロセス起動、`initialize`、`session/new` (mcp_servers と `_meta.systemPrompt.append` を渡せる)、
    `session/load` (capability 確認付き)、`set_mode`、`set_config_option`。
  - prompt のストリーミング。**ターン中キャンセル** (コマンドループが prompt を待たない構造)。
  - 権限の自動承認 `choose_permission` (kind で選ぶ。先頭を選ばない)。
  - 全 `SessionUpdate` を型付き `AgentOutput` に。未知は `Unknown`。
  - stop reason の伝達。`Shutdown` とプロセス終了の回収 (ゾンビ・孤児を残さない)。
- `HarnessConfig` と Claude Code 用の既定値 (`bypassPermissions`、Haiku)。
- 偽エージェント: シナリオ JSON で message / thought / tool_call / plan / request_permission /
  sleep / wait_cancel / end / crash (mcp_call は Stage 2)。

**完了条件**
- `cargo test -p yhtye-core` (偽エージェント) が通る。最低限:
  チャンクが順に届く / ターン中 `Cancel` で `stop_reason = cancelled` が返る /
  `allow_once`・`allow_always`・`reject_*` が混在する選択肢で allow が選ばれ、
  allow が無ければ Cancelled / 各 SessionUpdate の型付け / `Shutdown` 後に子プロセスが残らない /
  `crash` で `Exited` が来る。
- `cargo test -p yhtye-core -- --ignored` (実 Claude Code + Haiku):
  1. 現在モデルが haiku であることを表明 → 短いプロンプトで `agent_message_chunk` と `end_turn`。
  2. 時間のかかるプロンプト (例: 長文を書かせる) を送り、チャンク受信後に Cancel → `cancelled`。
  3. 一時ディレクトリでファイル作成を頼み、`bypassPermissions` で実際に作られる
     (権限要求が来たら kind で承認されたことをイベントで確認)。
  4. `session/load` で同じセッションを復元でき、前の会話内容を覚えている。
- `cargo check` (src-tauri) / `pnpm build` が通る。

## Stage 2 — MCP サーバーとマルチセッション

**範囲**
- `mcp` モジュール: rmcp 3.4 + axum、`/mcp/{token}`、`TokenRegistry`、役割別 `tools/list`、
  `ToolPort` 経由の呼び出し。まずは [`mcp-tools.md`](docs/architecture/mcp-tools.md) の
  ツールのうち `create_group` / `create_task` / `report_step_done` / `help` / `get_status` を
  **最小の仮実装の ToolPort** (メモリ上) でつなぐ。本物の状態機械は Stage 3。
- 複数 ACP セッションの並行管理 (オーケストレータ 1 + サブエージェント N)。
- 偽エージェントに `mcp_call` を追加 (rmcp クライアント)。
- 役割別システムプロンプトの初版 (`prompts/`)。

**完了条件**
- 偽エージェント: オーケストレータ役が `create_task` を呼ぶ → Yhtye がサブエージェント
  セッションを起動 → サブエージェント役が `report_step_done` → オーケストレータに通知が届く。
  不正トークン 404、役割外ツール `forbidden`、引数エラーが仕様どおり。
- 実 Haiku: オーケストレータに「README に 1 行足すタスクを作って」と頼むと、
  MCP 経由で `create_group` / `create_task` が呼ばれ、サブエージェント (Haiku) セッションが
  起動して `report_step_done` まで到達する。

## Stage 3 — ドメインコア

**範囲**
- `domain` (状態機械全体: Task 状態、Step kind 4 種、review 自動再挿入、help、checkpoint、
  依存解消と instruction_needed、group_settled、受信箱)。
- `store` (sqlx SQLite、migrations、イベントログ + 現在状態、再起動時の `interrupted` と再開)。
- `git` (group ブランチ・統合 worktree・タスク worktree・自動コミット・マージ・コンフリクト→help)。
- `runtime::Core` と `ApiCommand` / `ApiEvent` (ts-rs 生成まで)。
- MCP ツール全種を本物の ToolPort に接続。プロトコル違反時の催促と help。

**完了条件**
- domain の遷移表テスト (全状態 × 全コマンド、エラーケース含む)。
- 偽エージェントのシナリオテスト (一時 git リポジトリ上): 単純タスク完走 /
  review が needs_changes → 自動再挿入 → approve / checkpoint で abort と continue /
  help → answer_help(resume) / 依存 + 空 instruction → set_instruction /
  タスクマージのコンフリクト → help → 解消 → 再マージ / finish_group で base へマージ /
  base が dirty で merge_blocked / 途中で Core を落として再起動 → interrupted → 再開して完走 /
  ツールを呼ばずにターン終了 → 催促 → help。
- 実 Haiku: 一時リポジトリで「小さな関数とそのテストを追加して」を依頼し、
  グループ作成 → `code` タスク (implement → review → done) → base ブランチへのマージまで完走。
  結果のコミットが base に入っていることを git で確認。

## Stage 4 — フロントエンド (素の UI)

**範囲**
- `src/api/` transport 抽象 (Tauri / WS)、生成型の利用。
- `src/store/` snapshot + イベント畳み込みの状態ストア、seq 飛びの再同期。
- 素の UI (デザイントークンの CSS 変数だけ使う。レイアウトは簡素でよい):
  プロジェクトを開く、オーケストレータとの会話 (ストリーミング表示・キャンセル)、
  グループ / タスク / Step の一覧と状態、help の表示、エージェント出力の閲覧。
- 未実装の領域は空状態。**モック / フィクスチャを実データ経路に使わない。**

**完了条件**
- `pnpm test` (Vitest): reducer にイベント列を流して期待状態になる / seq 飛びで再同期要求 /
  WsTransport の要求-応答の対応付けとイベント配信。
- `pnpm build` が通る。

## Stage 5 — 統合

**範囲**
- `src-tauri`: Core の起動・`yhtye_command`・イベント emit・終了時 shutdown。
- `yhtye-dev-bridge`: WS、Origin 制限とトークン。
- 開発手順を README に記載 (`pnpm dev` + ブリッジでブラウザから本物のコアに接続)。

**完了条件**
- Chrome (自動操作) + WS ブリッジ + 実 Haiku で E2E: 一時リポジトリを開く → 依頼を送る →
  会話がストリーミング表示される → タスクが現れ状態が進む → グループ完了、base に
  マージされたことが UI と git の両方で確認できる。ターン中キャンセルも UI から効く。
- `pnpm tauri dev` でアプリが起動し、同じ依頼が通る (手動確認でよい)。終了後に
  エージェントプロセスが残っていない。

## Stage 6 — Claude Design の適用と残機能

**範囲** (実運用を見て優先度を決める)
- [`docs/design/orchestrator-desktop.md`](docs/design/orchestrator-desktop.md) のレイアウト・
  トークン・アニメーションを適用。
- git グラフ、ブランチ一覧、使用量 / quota 表示 (`usage_update`、`_meta.quota`)、
  runs (履歴) ビュー、他ハーネスの追加 (設定のみで足せることの確認)、配布ビルドの課題
  (Wayland + NVIDIA 回避策の扱いなど)。

**完了条件**
- 各機能について実データで表示されることを Chrome E2E で確認。デザイン仕様との差分を
  `docs/design/` に記録。
