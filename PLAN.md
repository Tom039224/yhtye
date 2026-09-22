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
| 1 | ACP コア | **完了** |
| 2 | MCP サーバー + マルチセッション | **完了** |
| 3a | ドメインコア: 状態機械 (メモリ上) と UI 向けイベント列 | **完了** |
| 3b | ドメインコア: SQLite 永続化 (イベントログ + 現在状態) と再起動・再開 | **完了** |
| 3c | ドメインコア: git (worktree / ブランチ / マージ) と実 Haiku でのグループ完走 | **完了** (Stage 3 完了) |
| 4 | フロントエンド (素の UI) | **完了** |
| 5 | 統合 (Tauri + WS ブリッジ + Chrome E2E) | 未着手 |
| 6 | Claude Design 適用・残機能 | 未着手 |

## 再開の仕方

- Stage は**順番に**、それぞれ別のサブエージェントが実行する。前の Stage が完了している前提で始める。
  Stage 3 は 3a → 3b → 3c の小 Stage に分かれている (表を参照)。
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

**結果メモ (2026-09-23)**

- 構成: ルートに Cargo workspace (`crates/*` + `src-tauri`、`Cargo.lock` と release プロファイルを
  ルートへ移動、`/target` を .gitignore)。`crates/yhtye-core/src/acp/` = `config` (HarnessConfig,
  `claude_code("haiku")`) / `events` (AgentEvent・AgentOutput・AgentError) / `permission` /
  `process` (プロセスグループ起動・stderr・終了) / `startup` (initialize〜set_config_option) /
  `session` (アクター・コマンドループ) / `handle` (`spawn_agent`・`AgentHandle`)。
  `crates/yhtye-fake-agent` (シナリオ JSON 駆動の偽エージェント)。
- 実行したコマンドと結果:
  - `cargo test --workspace` → 単体 6 + 偽エージェント結合 18 (turns 7 / lifecycle 11) すべて成功、実エージェント 4 は ignored。
  - `cargo test -p yhtye-core --test acp_claude_real -- --ignored --test-threads=1 --nocapture`
    → 4/4 成功 (約 30 秒、2 回実行): (1) haiku・bypassPermissions を表明 → "pong" と `end_turn`
    (2) 長文生成中に Cancel → 約 33 ms で `cancelled` (3) 一時ディレクトリに hello.txt が
    実際に作られた (4) `session/load` で同じ ID に復元し、合言葉を答えた。
  - 各実テスト後に npx→node→claude のプロセスグループが空であることを /proc で確認。
    `ps` でも claude-agent-acp / yhtye-fake-agent の残存なし。
  - `cargo clippy --workspace --all-targets` 警告なし / `cargo check` (src-tauri) / `pnpm build` 成功。
- 実エージェントでの観察は [`acp-harnesses.md`](docs/architecture/acp-harnesses.md) §5.1。
  bypassPermissions では権限要求が**来なかった**ため、kind による自動承認は偽エージェントでのみ検証
  (完了条件 3 の「来たら」の分岐は未発生)。
- 設計からの変更 (理由は [`core-design.md`](docs/architecture/core-design.md) §3.5):
  `AcpAgent` を使わず自前のプロセスグループ管理 / イベントチャネルを unbounded に /
  `AgentCmd` を内部化してハンドルのメソッドに / `Exited{code, signal}` / `choose_permission` は
  選択肢を返す + `outcome_for`。偽エージェントの動作に `update` (任意 JSON)・`spawn_child`・
  `report_state`・`fail_at/exit_at/hang_at` を追加。
- 残課題 / Stage 2 への申し送り:
  - `session/load` の履歴再生は `Ready` より前の `Output` として届く (実機ではユーザーと
    エージェント両方のチャンク)。runtime / UI はこれを「履歴」として扱う必要がある。
  - プロセス管理は Unix 専用 (`process_group`, killpg)。Windows 対応は未着手。
  - 偽エージェントの `mcp_call` は Stage 2 で追加する。`fake_agent_bin()` はテスト中に
    `cargo build -p yhtye-fake-agent` を呼ぶ (Stage 2 以降の結合テストも同じヘルパを使う)。
  - `yhtye-fake-agent` の `serve()` はビルダー連鎖で約 90 行 (関数 50 行目安を超過)。

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

**結果メモ (2026-09-23)**

- 構成 (詳細は [`core-design.md`](docs/architecture/core-design.md) §4・§4.1):
  - `domain/` — 共有の語彙 (`Role` / `StepKind` / `TaskKind` / `StepSpec` / `Verdict` /
    `HelpKind` / `ErrorCode` / `ToolError`) と `normalize_steps` (Step 列の規則)。
  - `mcp/` — `McpHost` (rmcp 3.4 + axum、`/mcp/{token}`、ステートレス)、`TokenRegistry`
    (128bit トークン、`rebind` / `revoke`)、`tools` (mcp-tools.md の全 13 ツールの引数型・
    スキーマ・役割、`ToolCall`)、`ToolPort` トレイト、`ToolCallRecord`。
  - `prompts.rs` + `crates/yhtye-core/prompts/{orchestrator,implementer,reviewer}.md` (初版)。
  - `runtime/` — `Board` (メモリ上の仮状態機械) / `MemoryToolPort` / `inbox` (§5 書式) /
    `Orchestration` (公開 API と `OrchEvent`) / `driver` (単一タスクのループ)。
  - `HarnessConfig.session_meta` と `HarnessConfig::claude_code_orchestrator`。
  - 偽エージェントに `mcp_call` / `mcp_list` / `report_meta` (rmcp クライアント)。
- 実行したコマンドと結果:
  - `cargo test --workspace` → 59 件成功 (単体 29 + ACP 偽 19 + MCP サーバー 6 +
    オーケストレーション偽 4 + 偽エージェント単体 1)、実エージェント 6 件は ignored。
    偽エージェントの結合テストは 5 回連続で成功。
    オーケストレーション偽: create_task → サブエージェント起動 → report_step_done →
    group_settled でオーケストレータ起床 → finish_group / help → answer_help(resume) →
    返答がサブエージェントの次プロンプト → 完了 / review Step が新しい reviewer セッションで
    動き verdict 無しは invalid_argument / オーケストレータ起動失敗の報告。
    MCP サーバー: 不正トークン 404・失効トークン 404・役割別 tools/list・役割外 `forbidden`・
    引数不正 `invalid_argument`・未知ツールはプロトコルエラー・2026-07-28 版の `tools/list`。
  - `cargo test -p yhtye-core -- --ignored --test-threads=1` → 6/6 成功 (2 回実行):
    Stage 1 の 4 件 + `orchestration_claude_real` 2 件:
    (1) 実 Haiku のオーケストレータに「README.md に `hello from yhtye` を足すタスクを作って」→
    `create_group` / `create_task` → 実 Haiku のサブエージェントが README を編集して
    `report_step_done` → `group_settled` で起こされたオーケストレータが `finish_group` →
    ユーザーに報告 (約 30 秒、2 回とも同じ流れ。全セッションで model=haiku・bypassPermissions を表明)。
    (2) オーケストレータ用ハーネスでは直接頼んでもファイルを書けない。
  - 実テスト後 `ps` で claude-agent-acp / yhtye-fake-agent の残存なし、各プロセスグループも空。
  - `cargo clippy --workspace --all-targets` 警告なし / `cargo check --workspace` / `pnpm build` 成功。
- 実機で分かったこと (詳細 [`acp-harnesses.md`](docs/architecture/acp-harnesses.md) §5.2):
  - **Claude Code 2.1.280 は MCP 2026-07-28 版で接続し、`tools/list` に `ttlMs`/`cacheScope`
    が無いと `INVALID_RESULT` で一覧を捨てる** (rmcp 3.4 は任意扱い)。最初はこれで
    オーケストレータに Yhtye のツールが見えなかった。明示して解決。
  - 何も指定しないとユーザー自身の MCP サーバー (Todoist 等 100 個超) が全エージェントに付く。
    `strictMcpConfig` + `ENABLE_CLAUDEAI_MCP_SERVERS=false` で隔離、`ENABLE_TOOL_SEARCH=false`
    で yhtye のツールを遅延ロードさせない。
  - Haiku はシステムプロンプトどおりにツールを呼んだ (instruction も自己完結に書いた)。
- 設計への反映・決定:
  - orchestration-model §11 のオーケストレータ書き込み問題 → §8.1 で決定 (組み込みツールを
    Read/Glob/Grep に限定。cwd 案・disallowedTools 案は不採用、理由も記載)。
  - mcp-tools §1: 引数のスキーマ不一致も `invalid_argument` のツールエラーにする (明確化)。
  - core-design §4: keep-alive 延長ではなくステートレスモード、`tools/list` のキャッシュヒント、
    `rebind`、`ToolCallRecord`。§3.4 に `session_meta`。§12 に偽エージェントの新動作。
- 残課題 / Stage 3 への申し送り:
  - `Board` は仮実装。Stage 3 の domain で置き換える: `modify_steps` / `cancel_group` 未実装、
    review `needs_changes` の自動再挿入なし、git (サブエージェントも Stage 2 はプロジェクト
    ディレクトリで作業)、永続化、ツールを呼ばずに終わったターンの催促と `protocol_violation`、
    異常終了時の `agent_crashed` help、ターン中の `cancel_task` (今はセッション停止のみ)。
  - `ToolPort` / `ToolCall` / `SessionBinding` / `OrchEvent` の形はそのまま使える想定。
    `Board::apply` の戻り値 `(reply, effects)` は core-design §5 の `Transition` に近い。
  - `settingSources` (ユーザーの CLAUDE.md・フック・プラグイン) を Yhtye のエージェントに
    効かせるかは未決 (orchestration-model §11)。ユーザーに確認する。
  - オーケストレータの MCP ツール一覧から help/report は除外済み。サブエージェントに
    Claude Code の `Agent` ツール (自前のサブエージェント) が残っている — プロンプトで禁止のみ。
  - `board.rs` は 722 行 (上限 800 に近い)。Stage 3 で domain に移すときに分割する。

## Stage 3 — ドメインコア

オーケストレータの判断で 3 つの小 Stage に分けて順に実行する (各小 Stage も「本物で動く」ことを
完了条件にする):

- **3a** — 状態機械 (純粋な reducer + 決定ロジック)、runtime への組み込み、UI 向けイベント列。
  永続化なし (メモリ上)。git はトレイトの後ろに置き、何もしない `NoopGit` で動かす。
- **3b** — `store` (sqlx SQLite: イベントログ + 現在状態テーブル)、再起動時の `interrupted` と再開。
- **3c** — `git` の本実装 (group ブランチ・統合 worktree・タスク worktree・自動コミット・マージ・
  コンフリクト→help)、実 Haiku で小さなグループを一時リポジトリ上で完走。

以下の「範囲」「完了条件」は Stage 3 全体のもの。3a の結果メモの後に 3b / 3c の結果メモを足す。

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

**3a 結果メモ (2026-09-23)**

- 構成 (詳細は [`core-design.md`](docs/architecture/core-design.md) §4.1・§5・§7・§8):
  - `domain/` — 純粋な状態機械。`decide(&State, DomainCommand) -> (State, Transition{events,
    effects, reply})`。状態は `DomainEvent` を `State::apply` (reducer) で適用してしか変わらない
    (`Tx` が作業用コピーにイベントを発行しながら判断する)。`state` / `event` / `command` /
    `machine` / `flow` / `tools_orch` / `tools_sub` / `agent_rules` / `git_rules` / `inbox` / `steps`。
    受信箱もドメイン状態 (3b で永続化)。
  - `git/` — `GitService` トレイト (`current_branch` / `run(GitOp) -> GitResult`) と `NoopGit`。
  - `api/` — `ApiEvent { seq, ts_ms, project, body }` (ドメインイベント・セッション・エージェント出力・
    ツール呼び出し・プロンプト) と `Snapshot { seq, state }`。
  - `runtime/` — `Board` / `MemoryToolPort` を削除し、`driver` (単一ループ: コマンド → decide →
    イベント配信 → Effect。git はインラインで await して `GitDone` を同じ連鎖で処理) /
    `sessions` (旧 driver から分離) / `port` (`LoopPort`) / `emitter` / `orchestration`
    (`snapshot()` を追加、`OrchestrationConfig` に `domain` と `git`)。
  - Stage 2 で無かったもの: `modify_steps` / `cancel_group` / `needs_changes` の自動再挿入と
    `review_rounds_exhausted` / `cancel_task` でのセッション停止 + 状態更新 + worktree 削除 Effect /
    催促 1 回 → `protocol_violation` / `agent_stopped` / 異常終了・起動失敗の `agent_crashed` /
    返答待ち中の死亡 (help にエージェント喪失を記録し、resume で Step をやり直す) /
    依存解消 + 空 instruction → `instruction_needed` / checkpoint / group_settled (開始不能タスク込み) /
    active グループ 1 つの制約 / mcp-tools.md の全エラーケース。
- 実行したコマンドと結果:
  - `cargo test --workspace` → 109 件成功 (yhtye-core 単体 73 + ACP 偽 19 + MCP 6 +
    オーケストレーション偽 10 + 偽エージェント 1)、実エージェント 7 件は ignored。偽の結合テストは
    3 回連続で成功。
    - 単体 (`domain/tests/`): 遷移表 (タスク状態 10 種 × set_instruction / modify_steps /
      resolve_checkpoint / cancel_task / report_step_done / help、answer_help)、フロー 16、
      エラー 10 (mcp-tools.md の全エラーコード)、git 6 (コンフリクト → modify_steps → resume →
      再マージ、dirty investigate、worktree 失敗、group ブランチ失敗、merge_blocked)、
      エージェント 10 (help の返答がターン終了より先に来た場合に誤って催促しない、を含む)。**全コマンドで「旧状態 + イベント = 新状態」を検査**している。
    - 偽エージェント: 通常 / help → resume / needs_changes → 再挿入 → approve (レビュアーは毎回
      新しいセッション、実装者は 1 セッション) / checkpoint + note / 催促 → protocol_violation →
      resume / crash(3) → agent_crashed → 新セッションで完走 / 実装者の起動失敗 → agent_crashed /
      依存 + 空 instruction → set_instruction / ターン中の cancel_task (ACP cancel → `cancelled` →
      セッション停止) / オーケストレータ起動失敗。
  - `cargo test -p yhtye-core -- --ignored --test-threads=1 --nocapture` → 7/7 成功 (2 回実行、
    オーケストレーション実テストは 2 回とも約 85 秒):
    Stage 1 の 4 件、Stage 2 の 2 件 (新しい状態機械の上で)、新規
    `real_implement_then_review_uses_a_separate_reviewer_session`: 実 Haiku のオーケストレータが
    `steps: [implement, review]` のタスクを作成 → 実装者 (T-1/implementer) が README を編集して報告 →
    別セッション `T-1/review-1` のレビュアーが差分を確認して `verdict: approve` → `done` →
    `group_settled` → `finish_group`。2 回とも同じ流れ。
  - 実テスト後 `pgrep claude-agent-acp` で残存なし、各プロセスグループも空。
  - `cargo clippy --workspace --all-targets` 警告なし / `cargo fmt --check` / `cargo check --workspace` /
    `pnpm build` 成功。
- 実装中に分かったこと・直したこと:
  - **ターン中にエージェントのプロセスが死ぬと、`session/prompt` が `incoming_transport_closed`
    で失敗し、それが先に `TurnEnded(Err(Request))` になっていた** (core-design §3.2 の
    「プロセス終了時は `Err(Closed)`」と食い違い)。`is_incoming_transport_closed` で `Closed` に
    写すよう `acp/session.rs` を修正。これが無いと crash が `agent_stopped` に化ける。
  - タスクの最終結果 (`group_settled` / `instruction_needed` の本文) は `done` Step のマージ詳細ではなく、
    最後のエージェント / checkpoint の結果にした。
- 設計への反映・明確化 (合意済みの決定は変えていない。細部の確定のみ):
  - core-design §2 (`ApiEvent` の中身)、§4.1 (runtime を Stage 3a の形に)、§5 (実際の
    `decide` / `DomainEvent` / `Effect` とイベントソーシングの不変条件。`now` 引数は無し)、
    §7 (`GitService` トレイト)、§8 (git はループ内で await、ツール応答は連鎖の後、
    プロトコル違反はドメイン状態で判定)。
  - mcp-tools: `finish_group` はマージ結果込みで同期応答 → `merge_result` は再試行時のみ /
    help kind に `git_failed` を追加 / `create_task` の循環 `conflict` は起こりえない、cancelled への
    依存は `invalid_state` / `modify_steps` の空配列は `[done]`、`merging` 中は `invalid_state` /
    `cancel_group` のエラーと「group_settled を送らない」/ `create_group` の `internal` /
    Yhtye 起因 help の resume で reply は note になる / 返答待ち中の死亡の扱い。
  - orchestration-model: `finishing` も active 数に含め `merge_blocked` は含めない / §7 の細部。
- 3b / 3c への申し送り:
  - 3b: `Transition.events` を 1 トランザクションで追記し、現在状態テーブルも同じイベントから
    更新する (reducer が唯一の変更経路なので、イベント再生で状態を作ってもよい)。`ApiEvent.seq` と
    DB の seq を揃える。セッション (ACP セッション ID・トークン) は現在ドメイン外 (runtime の
    `sessions`) — `session/load` での再開には保存が必要。`TaskStatus::Interrupted` は定義済み・未使用。
    `create_group` 時の base ブランチは runtime が `GitService::current_branch` で読んで渡す。
  - 3c: `GitService` の git CLI 実装。`PrepareWorkspace` の結果パスがサブエージェントの cwd になる。
    レビュアーのプロンプトに「どのブランチとの差分を見るか」を足す (今は earlier results のみ)。
    git はループ内で await するので、遅い操作 (大きなマージ) が入るならタスク化を検討。
  - ts-rs による TS 型生成は Stage 4 へ (`AgentEvent` は ACP の型を含むので `unknown` 扱いが要る)。
  - 複数プロジェクトを束ねる `Core` / `ApiCommand` は 3b 以降 (現状は 1 プロジェクトの `Orchestration`)。
  - サブエージェントに Claude Code の `Agent` ツールが残っている (プロンプトで禁止のみ。Stage 2 から継続)。

**3b 結果メモ (2026-09-23)**

- 構成 (詳細は [`core-design.md`](docs/architecture/core-design.md) §2・§4.1・§6・§8.1):
  - `store/` — sqlx 0.9 (SQLite bundled、WAL + NORMAL)。`migrations/0001_init.sql`
    (`sqlx::migrate!` で埋め込み)。`events` (追記のみ、`(project_id, seq)` が主キー、`kind` /
    `session` に索引) + 現在状態 `projects` / `task_groups` / `tasks` / `task_deps` / `steps` /
    `helps` / `inbox` + `agent_sessions`。`Store::commit` = ドメイン遷移のイベント + 変わった行を
    1 トランザクション。`load_state` (テーブルから) / `replay_state` (ログから、検査用) /
    `events_after` / `session_events` / `sessions`。DB の場所は `store::db_path(data_dir)`
    (`OrchestrationConfig::db_path`)。
  - `api` — `ApiEvent.live` を追加。durable イベントの `seq` = DB の seq (1 から欠番なし)。
    チャンク (`MessageChunk` / `ThoughtChunk` / `UserMessageChunk`) と `Usage` は live
    (保存せず、seq を進めない)。チャンク列はブロックの終わりに `agent_text` として 1 行で保存・配信
    (**トークン単位の行を積まない**選択)。`session_started` に `acp_session_id` / `resumed`、
    `session_stopped` に `suspended`、`session_interrupted` を追加。
  - `runtime` — `emitter` を `Emitter` (バッファ) + `Publisher` (seq 付け・保存・配信) に。
    ループは「decide → DB にコミット → 状態を置き換え → 配信 → Effect」の順 (コミットに失敗した
    遷移は捨てて `internal`)。`transcript` (チャンクの合体)、`launch` (起動・`session/load`・
    履歴再生を捨てる転送)、launch 番号で置き換えられたプロセスのイベントを捨てる。
    `Orchestration::start` が保存状態を読み、再起動処理 (core-design §8.1) をする。
  - `domain` — `DomainCommand::Restart { orchestrator }` / `ResumeTask { task }`、
    `Effect::ResumeStep { prompt, fallback }`、受信箱 `restarted`、`restart.rs`。
    `TaskStatus::Interrupted` を使い始めた。オーケストレータのプロンプトに `restarted` を追記。
- 実行したコマンドと結果:
  - `cargo test --workspace` → 128 件成功 (yhtye-core 単体 89 + ACP 偽 19 + MCP 6 +
    オーケストレーション偽 13 + 偽エージェント 1)、実エージェント 8 件は ignored。3 回連続で成功。
    - 単体 (新規): store 5 (空 DB へのマイグレーションと再オープン / 全フィールドを持つ状態の
      往復と差分書き込み / **トリガで steps の INSERT を失敗させ、遷移の途中で落ちてもイベントも
      状態も残らない**ことと、その後の再コミット / イベント再生 / agent_sessions の導出)、
      domain restart 8 (中断 → 再開で同じセッションへ / 報告済みでターン終了を失った Step は進む /
      マージ中 → FinishTask やり直し / 作業場所の準備やり直し / オーケストレータ待ちのタスクは
      そのまま / help 待ちエージェントの喪失 / マージ中のグループ → merge_blocked /
      オーケストレータへの `restarted`)、transcript 2、prompts 1。
    - **replay-equivalence**: 偽エージェントの全オーケストレーションシナリオ (既存 10 + 新規 3) の
      終わりに「ライブの状態 == テーブルから読んだ状態 == ログの再生」と「ログの最後の seq ==
      配信した seq」を検査 (`tests/common/orch.rs::assert_persisted`)。
    - 偽エージェントの再起動 3 (`tests/orchestration_fake_restart.rs`): ターン中に止めて同じ DB で
      起動 → `session_interrupted` → `interrupted` → `running` → `session/load` で同じ ACP ID
      (`resumed=true`) に `[yhtye:resume]` → 完走、seq は前回の続きから欠番なし、
      履歴再生は配信されない、前回の出力は `agent_text` 1 行でログにある /
      `session/load` 失敗 → 新しいセッションに Step の全プロンプト + 注記、オーケストレータには
      状態の要約つき `restarted` / `load_session` 非対応 → 同様にフォールバック。
  - `cargo test -p yhtye-core -- --ignored --test-threads=1` → 8/8 成功 (Stage 1 の 4、
    3a までの 3、新規 1)。新規 `orchestration_claude_restart` は 2 回実行して 2 回とも成功 (約 36 秒):
    実 Haiku のオーケストレータが `create_group` / `create_task` → 実 Haiku の実装者がターンを
    始めた直後に停止 (アプリ終了相当) → 同じ DB で起動 → 実装者もオーケストレータも同じ
    Claude Code セッション UUID に `session/load` で復元 → `[yhtye:resume]` だけで実装者が
    README に 1 行足して**新しいトークンの URL で** `report_step_done` → `done` →
    `group_settled` → `finish_group`。README の行は 1 つ (二重作業なし)。
    観察は [`acp-harnesses.md`](docs/architecture/acp-harnesses.md) §5.3。
  - 実テスト後 `pgrep` で claude-agent-acp / yhtye-fake-agent の残存なし、各プロセスグループも空。
  - `cargo clippy --workspace --all-targets` 警告なし / `cargo fmt --check` / `cargo check --workspace` /
    `pnpm build` 成功。
- 設計への反映・決定 (合意済みの決定は変えていない):
  - `ApiEvent` の durable / live の区別と `agent_text` (core-design §2)。「seq は 1 から欠番なし」は
    durable イベントについての規則になった。
  - MCP トークンは保存しない (復元時に新しいトークン。実機で確認)。ACP セッション ID は
    `agent_sessions` に保存。
  - 受信箱に `restarted` を追加 (mcp-tools §5、orchestration-model §5・§10)。
  - 再起動の細部 (help 待ちエージェントは喪失扱い、マージ中のグループは `merge_blocked`) は
    orchestration-model §10 に明記。
  - テーブル名 `groups` → `task_groups` (SQL の予約語)。
  - 3b で見つけて直したもの: 失敗した起動 (例: `session/load` の失敗) のプロセスの `Exited` が、
    同じキーで起動し直したセッションの終了と誤認されていた → launch 番号で区別。
  - コードレビュー (rust-reviewer) で直したもの: 遅れて終わった停止の `session_stopped` が同じキーの
    新しいセッションを `stopped` にしうる → 停止も launch 番号付きにし、置き換え済みなら出さない /
    durable イベントの保存に失敗したとき seq を進めたまま配信していた → seq を戻して live として配信 /
    連鎖の途中 (git 結果など) の保存失敗も `internal` を返す / オーケストレータ起動時のトークン漏れ /
    起動・停止タスクの panic をログに / ドメインイベントの取得を索引が効く範囲条件に。
- 3c / 4 への申し送り:
  - 3c: 再開は git 操作をやり直しうる — `PrepareWorkspace` (作業場所の準備中に落ちた) と
    `FinishTask` (マージ中に落ちた) を**冪等に**すること (既存の worktree・ブランチ・マージ済み
    コミットを受け入れる)。マージ中に落ちたグループは `merge_blocked` にしているので、
    `RetryGroupMerge` (Stage 5) で再試行する。フォールバック時のプロンプトに worktree の `git diff`
    を添える案 (orchestration-model §10) は 3c で `GitService` に diff を足してから。
  - 3c: テストの DB はプロジェクトディレクトリの `.yhtye/` に置いている (`tests/common/orch.rs::test_db`)。
    一時 git リポジトリでは作業ツリーを汚すので、別の一時ディレクトリへ移すこと。
  - 4: UI は durable イベントだけを畳み込み、live のチャンクはストリーミング表示にだけ使い
    `agent_text` で置き換える。履歴は `Orchestration::events` / `Store::session_events` から
    (`ListEvents` はまだ無い)。`agent_text` と `tool_called` (MCP) の相対順序はチャネルが別なので
    厳密ではない (ACP の `tool_call` 出力でブロックは閉じるので実用上は揃う)。
  - 複数プロジェクトを束ねる `Core` / `ApiCommand` は未着手 (DB は 1 ファイルを共有する前提で
    `project_id` 列を持たせてある)。ts-rs も未着手。
  - レビューで残した MEDIUM: 受信箱の配達は「送ってから `InboxDelivered` を保存」なので、保存に
    失敗すると再送されうる (at-least-once) / `projects.config` は起動時の設定で上書きしても DB に
    書き戻さない (再生は設定を引数に取るので実害なし) / 履歴再生の除外は「再生は `Ready` より前に
    届く」前提 (実 Claude Code では成り立っている。他ハーネス追加時に確認)。
  - Yhtye 自体が SIGKILL された場合 (シャットダウンなし) は、`agent_sessions` が `live` のまま残り
    起動時に `interrupted` になる (偽エージェントでは未検証。子プロセスは stdin が閉じて終了する想定)。

**3c 結果メモ (2026-09-23)**

- 構成 (詳細は [`core-design.md`](docs/architecture/core-design.md) §7.1、規則は
  [`orchestration-model.md`](docs/architecture/orchestration-model.md) §6「Stage 3c で決めた細部」):
  - `git/` — `GitCli` (git CLI を `tokio::process` で実行。git2/gix は不採用: worktree・merge・
    フック・ユーザー設定の挙動を git 本体と一致させるため) = `run` (環境の掃除・プロンプト無効・
    120 秒タイムアウト) / `repo` (照会・commit・merge・stash) / `worktree` (作成・再利用・削除) /
    `cli` (`GitOp` ごとの手順)。`git::worktree_root(data_dir, project)`。`NoopGit` はテスト用に残す。
  - `GitService::workspace_changes` を追加 → 再起動のフォールバックプロンプトに `git status` +
    分岐点からの `git diff` (16 KiB まで) を添える (driver の `ResumeStep`)。
  - `GitOp::PrepareWorkspace` / `FinishTask` に `group_branch` / `base_branch` (消えた group
    ブランチ・統合 worktree の再作成用)。ブランチ名は `domain::group_branch` / `task_branch`。
  - review Step のプロンプトにタスクブランチ・group ブランチと `git diff $(git merge-base ...)`。
    implementer / reviewer / orchestrator のプロンプトを git の流れに合わせて更新。
  - 偽エージェントに `run` アクション (コマンド実行、`run:<code>` を報告)。
  - テスト用 `tests/common/repo.rs::TempRepo` (一時ディレクトリの `repo/` と `data/`。DB と
    worktree は data 側 = git テストの DB はプロジェクトの外)。
- **設計からの変更 (要確認)**: タスクブランチ名を `yhtye/<groupId>/<taskId>` → **`yhtye/<groupId>-<taskId>`**。
  git は `refs/heads/yhtye/G-1` と `refs/heads/yhtye/G-1/T-1` を同時に持てない (ref はファイル。
  git 2.55 で確認)。group ブランチ名は合意どおり。orchestration-model §3・§6 を更新済み。
- 決めたこと (orchestration-model §6): エージェントはコミット不要で Yhtye が完了時に `add -A` +
  コミット (マージ途中ならマージコミットで完了、マーカーが残れば `merge_conflict`) / 内部の
  コミット・マージはフック・署名なし、base へのマージはリポジトリの設定どおり / base の clean =
  追跡ファイルの変更なし・操作途中でない (未追跡は妨げない)、base とのコンフリクトは abort して
  `merge_blocked` / investigate の汚れは stash に退避 / 中止タスクは WIP コミットしてから worktree 削除 /
  中断された rebase 等があれば進めず `git_failed` / 冪等 (既存 worktree 再利用、消えた worktree・
  group ブランチは作り直し、マージ済みは成功扱い、統合 worktree の中断マージは abort) /
  `create_group` で base と違うコミットを指す既存 group ブランチは採用しない / `git worktree prune`
  (全体) は使わず自分の worktree だけ `remove --force` / DB・worktree はデータディレクトリ
  (プロジェクトには何も書かない)。
- 実行したコマンドと結果:
  - `cargo test --workspace` → 151 件成功 (yhtye-core 単体 90 + ACP 偽 19 + **git_cli 18** + MCP 6 +
    オーケストレーション偽 13 + **偽 + 実 git 4** + 偽エージェント 1)、実エージェント 9 件は ignored。2 回連続成功。
    - `git_cli.rs` (一時リポジトリ): worktree の作成・再利用・再作成 / 未知の既存 group ブランチを拒否 /
      残りのコミット → マージ → worktree 削除 → 再実行はマージ済み / 別ファイルの並行タスク /
      コンフリクト → abort → 未解消は Conflict → 解消後に Yhtye がマージコミット / 統合 worktree の
      中断マージを abort して再試行 / 変更なしタスク / investigate の汚れ → stash / 中止の WIP コミット /
      base へのマージと後片付け・再実行 / dirty base・別ブランチ・base とのコンフリクト → Blocked で
      ユーザーのツリーは不変 / 中断 rebase を進めない / `workspace_changes`。
    - `orchestration_fake_git.rs`: 並行 2 タスク (片方 review) → group → main にマージ、worktree 全削除、
      main clean / 同じ行の編集 → `merge_conflict` help → orchestrator が modify_steps + resume →
      実装者が `git merge` して解消 → マージ / dirty base → `merge_blocked`、ユーザーの編集は保持 /
      再起動で session/load 非対応 → 新セッションのプロンプトに `?? partial.txt` の差分 → 完走。
  - `cargo test -p yhtye-core -- --ignored --test-threads=1` → 9/9 成功 (Stage 1 の 4、2/3a の 3、
    3b の 1、新規 `orchestration_claude_git` 1)。新規テストは計 3 回実行して 3 回とも成功 (88〜130 秒):
    一時リポジトリで実 Haiku のオーケストレータが group + code タスク 2 つ (T-1 implement→review、
    T-2 implement) を作成 → 各実装者が自分の worktree でファイル作成 (コミットせず) → Yhtye が
    コミットして group ブランチへマージ → `finish_group` → main に
    `Merge yhtye/G-1 (G-1) into main`、`add.py`/`sub.py` が main にあり、main clean、追加 worktree なし。
    **観察**: 3 回ともレビュアー (review-1) が「型注釈が無い」で `needs_changes` → 自動再挿入の
    implement → review-3 が `approve`。理由として「project's Python coding style」を引用しており、
    **ユーザーの `~/.claude` の全体ルールがサブエージェントに効いている** (下の未決事項の実例)。
  - 実テスト後 `pgrep` で claude-agent-acp / yhtye-fake-agent の残存なし、各プロセスグループも空。
  - `cargo clippy --workspace --all-targets` 警告なし / `cargo fmt --check` / `cargo check --workspace` /
    `pnpm build` 成功。
  - コードレビュー (rust-reviewer) で直したもの: 中断 rebase/cherry-pick/revert を黙って越えて
    マージしていた (HIGH) → `git_failed`。残した MEDIUM: base へのマージで署名に pinentry が要る
    設定だと 120 秒のタイムアウトまで止まる (一般的な `Blocked` として返る)。
- Stage 4 / 5 への申し送り:
  - アプリ側の組み立て (`GitCli::new(project_dir, worktree_root(data_dir, project))`、DB は
    `store::db_path(data_dir)`) は Stage 5 の `Core` / src-tauri で行う (今は `OrchestrationConfig` の
    doc にのみ記載。src-tauri はまだ Orchestration を使っていない)。
  - `RetryGroupMerge` (merge_blocked の再試行) は未実装 (Stage 5)。`MergeGroup` は冪等なので
    そのまま再実行してよい。
  - `cancel_group` では統合 worktree `_group` を消さない (ブランチと一緒に残る)。後片付け API は未設計。
  - git はループ内で await (大きなリポジトリで遅い場合はタスク化を検討)。
  - UI の git グラフ (Stage 6) はブランチ名 `yhtye/G-n` / `yhtye/G-n-T-m` を前提にできる。

**未決事項 (ユーザー判断待ち)**

- Yhtye が起動するエージェントに**ユーザー自身の `~/.claude`** (CLAUDE.md・フック・プラグイン・
  スキル) を効かせるか。現状は MCP サーバーだけ隔離し、それ以外は読み込んだまま
  (orchestration-model §11)。ユーザーの全体ルール (例: 「planner エージェントを使え」) が
  サブエージェントの動きを変えうる。`settingSources` を `["project", "local"]` に絞る案あり。
  **3a では挙動を変えていない。** 3c の実 Haiku で、レビュアーがユーザーの Python コーディング
  ルール (型注釈必須) を根拠に毎回 `needs_changes` を出すことを確認した (実害は 1 往復の増加)。

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

**結果メモ (2026-09-23)**

- Rust (API ファサード、詳細は [`core-design.md`](docs/architecture/core-design.md) §2・§11):
  - `runtime::Core` / `CoreConfig` (`runtime/core.rs`): 共有 DB + 開いたプロジェクトごとの `Orchestration`
    (`GitCli`)、`command(ApiCommand) -> Result<ApiResponse, ApiError>`、`subscribe()` (broadcast、全プロジェクト)、
    `shutdown()`。Tauri コマンドと WS ブリッジはこの 2 つを中継するだけで済む形。
  - `api::{ApiCommand, ApiResponse, ApiError, ProjectInfo, LoggedEvent}` (`api/command.rs`)、WS のメッセージ型
    (`api/wire.rs`)。`Snapshot` に `sessions` を追加。`Store::projects()`。
  - ユーザーによる中止: `Orchestration::cancel_task` / `cancel_group` (driver の `Cmd::UserTool`。状態機械の
    `cancel_task` / `cancel_group` をセッションキー `user` で通し、成功したらオーケストレータに `user_message`)。
    `Orchestration::shutdown` は `&self` に。
  - **TS 型生成は ts-rs 12**: `api/typegen.rs` のテストが `src/api/generated/` と一致を検査
    (`cargo test` に含まれる)。更新は `pnpm gen:types`。ACP の型は `unknown`、`u64` は `number`。
- フロント (`src/`): `api/` (Transport 抽象・Tauri・WS・環境で選択・ACP ペイロードの実行時検査)、`store/`
  (`State::apply` の TS 移植、セッション導出、会話・出力の畳み込み、`AppStore` = 唯一のストアと同期処理)、
  `ui/` (素の画面。トークン CSS 変数のみ: `styles/tokens.css`)。モック / フィクスチャは実行経路に無い
  (テスト用は `src/test/` のみ)。未実装の領域は空状態。接続状態とコマンドのエラーは常にバナーに出す。
- 実行したコマンドと結果:
  - `cargo test --workspace` → 158 件成功 (新規: `core_facade` 4 件 = プロジェクトの検証・登録・再オープン・
    エラーコード / 偽エージェントで依頼 → implement → review → base へマージまでを Core 経由で完走し、
    `ListEvents` のページングが配信と一致 / ターン中止・タスク中止・グループ中止と通知・エラー、
    Core 単体 2、typegen 1)。実エージェント 9 件は ignored (Stage 4 では挙動を変えていないので未実行)。
  - `cargo clippy --workspace --all-targets` 警告なし / `cargo fmt --check` 成功。
  - `pnpm test` (Vitest 5 + Testing Library, jsdom) → 6 ファイル 41 件成功:
    reducer が本物のコア (偽エージェント) で記録した 2 つのイベント列 (`src/test/fixtures/fake-{run,cancel}.json`、
    `pnpm record:fixtures`) を畳み込んでコアの最終 state / sessions と一致 + 記録に無い遷移の構成テスト /
    ストリーミングの合体と置き換え / 読み込み中のバッファと重複排除 / seq 飛び → ListEvents で補完 /
    再接続で OpenProject + cursor 以降の補完・途中ストリームの破棄 / エラー表示 /
    WsTransport を実際の ws サーバー (テスト内) に対して: 要求-応答の対応付け (順不同)・エラーコード・
    イベント配信・不正メッセージの無視・切断で保留中の要求を失敗 → 再接続・タイムアウト・トークン /
    画面: 会話・思考の折りたたみ・ツール呼び出し・タスク状態・エージェント出力・Enter/Shift+Enter・
    送信失敗で本文を保持・待機中表示・ターン中止・help / interrupted / merge_blocked 表示・空状態。
  - `pnpm build` 成功 (tsconfig の target/lib を ES2022 に上げた)。
  - `pnpm dev` を Chrome で開き、コア未接続で「切断: … 再接続」バナーと空状態が出ることだけ確認
    (本物のコアとの E2E は Stage 5)。dev サーバーは停止済み。
- 設計への反映: core-design §2 (Core の形・コマンド一覧・ts-rs の選択・ユーザー中止・Snapshot.sessions)、
  §11 (Transport と同期規則の実装)。合意済みのモデルは変えていない。
  **要確認**: UI からのタスク / グループ中止はユーザーがオーケストレータを介さず状態を変える経路
  (orchestration-model §9 に「ユーザーによるタスク中止」はあったが経路は未定義だった)。オーケストレータには
  `user_message` で必ず知らせる形にした。
- Stage 5 への申し送り:
  - src-tauri: `Core::start(CoreConfig::claude_code(data_dir, "haiku"))` を manage し、
    `yhtye_command(cmd: ApiCommand) -> Result<ApiResponse, ApiError>` と `subscribe()` → `emit("yhtye://event")`、
    終了時 `shutdown()`。broadcast の `Lagged` は捨ててよい (クライアントが seq 飛びで補う)。
  - WS ブリッジ: `WsRequest` / `WsServerMessage` をそのまま使う。**ポートの衝突注意**: `vite.config.ts` の HMR が
    `TAURI_DEV_HOST` 設定時に 1421 を使う (§10 のブリッジ既定も 1421)。
  - `Core::start` は保存済みプロジェクトを自動で開かない (中断タスクの再開は OpenProject 時)。起動時に
    前回開いていたものを開くかは Stage 5 で決める。
  - 履歴は毎回ログの先頭から読む (長いプロジェクトで重くなったらページングの遅延読み込みを検討)。
  - `RetryGroupMerge` (merge_blocked の再試行) は未実装。UI は merge_blocked を表示するだけ。
  - 未決事項 (`~/.claude` の扱い) は変わらず。

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
