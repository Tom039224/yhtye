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
| 5 | 統合 (Tauri + WS ブリッジ + Chrome E2E) | **完了** (アプリ内 UI はユーザーが確認済み、7a) |
| 6a | Claude Design の適用・UX / 堅牢性の修正 | **完了** |
| 6b | Markdown 描画・使用量 / quota・ブランチ全体のレビュー | **完了** (設計判断が要る残機能は下の未決事項へ) |
| 7a | 「完了したグループが進行中のまま」の修正・決定事項の反映 (DMABUF など) | **完了** |
| 7b | ハーネス / モデルの選択 (役割ごと、⚙ ボタン) | **完了** |
| 7c-1 | OpenCode ハーネスの ACP 検証 (`HarnessConfig::opencode`) | **完了** |
| 7c-2 | OpenCode を選べるハーネスとして組み込む (設定・検出・UI) | **完了** |
| 7d | effort + 用途メモ付きの候補の行、アプリ全体の設定モーダル | **完了** |
| 7e | Codex ハーネス (OpenRouter のモデル一覧)、OS キーリングの秘密の環境変数 | **完了** (Codex 実装者の実機は通過。Codex オーケストレータは codex#13746 で不可、当面放置) |

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

**未決事項 (ユーザー判断待ち)** — 全 Stage 分をここにまとめる (朝の要約: [`docs/rebuild-summary.md`](docs/rebuild-summary.md))

Stage 7a で以下をユーザーが決定した (経緯は各 Stage の結果メモ、決定の記録は各設計ドキュメント):

1. ~~`~/.claude` を効かせるか~~ → **常に有効** (現状維持: CLAUDE.md・フック・スキルを読み込む、MCP サーバーだけ隔離)。
   [`orchestration-model.md`](docs/architecture/orchestration-model.md) §11 / [`acp-harnesses.md`](docs/architecture/acp-harnesses.md) §5.2。
2. **runs (履歴) ビューの中身** → **保留** (「現状は何もありません」の空表示のまま)。再開するときの案は前回の推奨
   (過去のグループ一覧 → クリックで会話のその位置へ)。
3. ~~「対処の内容を見る」「差分を見る」の遷移先~~ → **ボタンを出さない** ([`orchestrator-desktop.md`](docs/design/orchestrator-desktop.md) §3.4 / §8)。
4. ~~他ハーネスの追加~~ → **Stage 7b / 7c として実施予定** (下の Stage 7 の節)。
5. ~~配布バイナリでの DMABUF~~ → **起動時に Wayland + NVIDIA を検出したときだけ設定** (7a で実装、[`core-design.md`](docs/architecture/core-design.md) §9)。
6. ~~使用量の取得頻度~~ → **現状のまま** (接続時 + 5 分ごと + クリック)。
7. ~~dev ブリッジのトークン~~ → **現状のまま**、単一ユーザー機専用と README に明記。
8. ~~以前からの要確認~~ → すべて確認済み: タスクブランチ名 `yhtye/<G>-<T>`、UI からの中止経路、ブリッジのポート 1422 と
   Origin の無い接続をトークンだけで通す点、6a で足したデザインに無い要素、**`pnpm tauri dev` のウィンドウ内の動作**
   (ユーザーが Tauri ウィンドウでオーケストレータとの会話・サブエージェントの起動を確認)。

残っている未決 / 保留: runs ビュー (保留)。7b / 7c の細部は各 Stage の中で決める。

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

**結果メモ (2026-09-23)**

- 構成 (詳細は [`core-design.md`](docs/architecture/core-design.md) §2「Stage 5 の決定」・§9・§10):
  - `src-tauri/src/lib.rs`: `Core::start(CoreConfig::claude_code(data_dir, model))` を manage、
    `yhtye_command` 1 本、`subscribe` → `emit("yhtye://event")`、起動後に `resume_unfinished`、
    `RunEvent::Exit` で `block_on(core.shutdown())`、SIGINT/SIGTERM → `app.exit(0)`。
    データは `YHTYE_DATA_DIR` / app data dir、モデルは `YHTYE_MODEL` (既定 haiku)。
    `WEBKIT_DISABLE_DMABUF_RENDERER` は `src-tauri/.cargo/config.toml` のまま (変更なし)。
  - `crates/yhtye-dev-bridge` (lib `router`/`serve` + bin): axum ws、Origin (ある場合) + トークン、
    要求は並行実行・id で対応付け、SIGINT/SIGTERM で `Core::shutdown`。
    **既定ポートを 1422 に変更** (Vite HMR の 1421 と衝突するため。`src/api/create.ts` も 1422)。
  - `pnpm dev:browser` (`scripts/dev-browser.mjs`): ブリッジをビルドして Vite と同時に起動、同じ
    ランダムトークンを両方に渡し、一緒に止める。`pnpm bridge` はブリッジ単体。
  - Stage 4 の残り: **起動時の再開** = `Core::resume_unfinished()` (active / finishing のグループか未配達の
    受信箱があるプロジェクトだけ開く。アプリとブリッジが起動直後に呼ぶ。`Core::start` は DB を開くだけのまま)。
    **`RetryGroupMerge`** (API コマンド + ドメイン `RetryGroupMerge` + `GitOp::MergeGroup{notify}` → 受信箱
    `merge_result`、UI は merge_blocked の警告に「マージを再試行」ボタン)。**`cancel_group` の統合 worktree** は
    `GitOp::RemoveGroupWorkspace` で削除 (中断マージは abort、残りは group ブランチに WIP コミット)。
    **履歴の読み込み**は先頭からのページングのまま (新しい順は同期規則と噛み合わず別経路の設計が要る → Stage 6)。
- 実行したコマンドと結果:
  - `cargo test --workspace` → 168 件成功 (新規: domain 3 = 再試行 / 再試行の拒否 / cancel_group の後片付け順、
    git_cli 1 = グループ worktree の削除、偽 + 実 git 1 = merge_blocked → ユーザーがツリーを戻す →
    `retry_group_merge` → base にマージ + `merge_result`、core_facade 1 = 再起動で未完了プロジェクトだけ再開し
    タスク完走 (+ グループ中止後に worktree が残らない検査)、ブリッジ 4 = Origin/トークン・要求の解析・
    トークン生成 + 実 WebSocket で 401/403・エラー・並行要求・ストリーミング・2 クライアント配信)。
  - `cargo clippy --workspace --all-targets` 警告なし / `cargo fmt --check` 成功。
  - `cargo test -p yhtye-core -- --ignored --test-threads=1` → 9/9 成功 (実 Haiku)。終了後 `claude-agent-acp` 0 個。
  - `pnpm test` → 41 件成功 (再試行ボタン・`retryGroupMerge` を追加)。`pnpm build` 成功。
  - `pnpm tauri build --debug --no-bundle` 成功 → `WEBKIT_DISABLE_DMABUF_RENDERER=1 YHTYE_DATA_DIR=<E2E の data>
    ./target/debug/yhtye` を 25 秒起動: クラッシュなし、中断していたタスクのプロジェクトを自動で開き
    オーケストレータと実装者 (実 Haiku) が live → SIGTERM → 「all agents stopped」(約 30 ms)、エージェント残存 0。
- **Chrome E2E (claude-in-chrome、WS ブリッジ + Vite + 実 Haiku、一時リポジトリ `scratchpad/e2e/demo`)**:
  1. プロジェクトを開く → 依頼「README に 1 行追加 + hello.txt を作成、2 タスク」→ 会話がストリーミング表示、
     G-1 に T-1/T-2 が並行で running → 途中でページを再読み込み → プロジェクトを選ぶと完了済みまで追いつき、
     続きのストリーミングも表示 → G-1 done、`main` にマージ (git log でマージコミット 3 つ、README と hello.txt、
     worktree は main のみ)。
  2. 長文を頼んでストリーミング中に「ターンを中止」→ `turn ended: cancelled`、idle に戻る。
  3. 再起動: 実装中 (T-4 implement running) にブリッジへ SIGTERM → 2 秒で終了、エージェント残存 0、
     セッションは `suspended` → ブリッジを再起動 → `resume_unfinished` で自動再開、T-4 の実装者は
     **同じ ACP セッション ID で `session/load` 復元** → review → base にマージ。開いたままのページは再接続して追いついた。
  4. 追加: base を dirty にして依頼 → G-4 merge_blocked → ツリーを戻して UI の「マージを再試行」→ done、
     オーケストレータに `merge_result ok=true` が届き報告。コンソールエラーなし。
  - 観察: 「`sleep 45` してからファイル作成」の依頼では Haiku の実装者が 2 回報告なしでターンを終え
    `protocol_violation` → オーケストレータがタスクを中止した (Claude Code が長いコマンドを背景実行した
    とみられる)。Yhtye の規則どおりの挙動。ページ再読み込み後はプロジェクトを自動では選ばない (クリックが要る)。
  - スクリーンショット: [`docs/e2e/stage5/`](docs/e2e/stage5/) (Stage 6a で一時ディレクトリから移した)
    `0-request-sent.jpg` (依頼直後) / `1-tasks-running.jpg` (タスク並行実行) /
    `2-after-reload-done.jpg` (再読み込み後・完了) / `3-long-streaming.jpg` (長文ストリーミング) /
    `4-after-cancel.jpg` (中止後) / `5-restart-completed.jpg` (再起動後に完走) / `6-merge-retried.jpg` (マージ再試行後)。
- 開発での起動コマンド:
  - アプリ: `pnpm tauri dev` (データ: `~/.local/share/com.tom039224.yhtye`、`YHTYE_DATA_DIR` / `YHTYE_MODEL` で変更)。
  - ブラウザ: `pnpm dev:browser [--data-dir DIR]` → Chrome で `http://localhost:1420`。
    別々に: `pnpm bridge -- --token T --data-dir DIR` と `VITE_YHTYE_BRIDGE_TOKEN=T pnpm dev`。
- 設計への反映: core-design §2 (Stage 5 の決定・コマンド一覧)・§9・§10 (ポート 1422、Origin が無い接続の扱い、
  並行実行)・§11・§13、orchestration-model §6 (中止したグループ・マージの再試行)、mcp-tools §5。
  **要確認**: ブリッジの既定ポート変更 (1421 → 1422) と、`Origin` ヘッダの無い接続 (ブラウザ以外) を
  トークンだけで通す点。起動時の自動再開は「未完了の作業があるプロジェクトだけ」にした。
- Stage 6 への申し送り:
  - **`pnpm tauri dev` のウィンドウ内で依頼を通す確認は未実施** (自動操作できないため。アプリの起動・
    自動再開・終了時の後始末は確認済み)。ユーザーの手動確認が要る。
  - 再読み込み後に前回のプロジェクトを自動で選ぶ (localStorage か `open` のプロジェクト)。
  - 履歴の遅延読み込み (新しい順) は未実装 (上記)。
  - 長いシェルコマンドを背景実行した実装者が報告せずターンを終える件 (プロンプトで「待ってから報告」を促すか)。
  - 未決事項 (`~/.claude` の扱い、配布バイナリでの DMABUF 回避策) は変わらず。

## Stage 6 — Claude Design の適用と残機能

オーケストレータの判断で 2 つに分けた: **6a** = デザインの適用 + Stage 5 で出た UX / 堅牢性の課題、
**6b** = データ源の追加が要る残機能。

### Stage 6a — Claude Design の適用・UX / 堅牢性 (完了)

**範囲**
- [`docs/design/orchestrator-desktop.md`](docs/design/orchestrator-desktop.md) のレイアウト・トークン・
  アニメーション (`prefers-reduced-motion` 対応を含む) を**実データだけで**適用。データの無い要素は空 / プレースホルダ。
- git パネル・ブランチ一覧用のコアコマンド、前回のプロジェクトの自動選択、履歴の遅延読み込み、
  タスクカードの `@` メンション。
- flaky テストの根本原因の修正、長いコマンドで実装者が報告せずターンを終える件の対策。
- Stage 5 のスクリーンショットをリポジトリへ移す。

**完了条件**
- Vitest・`pnpm build`・`cargo test --workspace` (繰り返し)・clippy・fmt が通る。
- Chrome + WS ブリッジ + 実 Haiku で小さな依頼を通し、画面をデザイン仕様と比べる。スクリーンショットを
  `docs/e2e/stage6a/` に保存。

**結果メモ (2026-09-23)**

- デザインの適用 (`src/ui/`、対応表と「実データ / 空表示」の区別は
  [`orchestrator-desktop.md`](docs/design/orchestrator-desktop.md) §8、追加トークンは [`tokens.md`](docs/design/tokens.md)):
  タイトルバー (現在ブランチ)、アイコンレール (work / runs)、サイドバー (PROJECTS のリング = 開いている /
  ターン中、BRANCHES = 実ブランチ、フッタ = ローカルコアへの接続と最後のイベント)、会話 (発話ラベル・
  GROUP TASK カード・グループ待機バナー・メンションチップ付きコンポーザ)、タスク列 (注目グループ、
  `subagents N`、カード = ステータスバッジ / 進捗 = Step の完了割合 / 対処中 = open help / ログ 2 行 = 最新
  セッションの活動 / Step 列 / 担当セッションと経過時間 / `@` / ■ = タスク中止、過去のグループは折りたたみ、
  グループ中止・merge_blocked の再試行)、git パネル (実コミットグラフ、レーン = `src/ui/gitGraph.ts`、
  タスクブランチのバッジはタスクのステータス色)、エージェント出力 (git パネルの位置にタブ)、ステータスバー
  (左 = 接続、右 = 使用量メーターは「—」)。接続断・エラーはタイトルバー下の帯。
  **空表示 (6b へ)**: 使用量 / quota・プラン名・runs ビュー (デザインどおり「現状は何もありません」)・
  リモートホスト・「対処の内容を見る / 差分を見る」・コミットのクリック / 差分・本文の強調 (Markdown)。
- コア: `ApiCommand::GetGitOverview` (`git/graph.rs`、`core-design.md` §2「Stage 6a の決定」)。
  TS 型を再生成。
- UX: 前回開いたプロジェクトを `localStorage` に覚え、再読み込み後に自動で開く (コアが知っているパスのみ)。
  履歴は snapshot の直近 400 イベントから読み、「さらに前の履歴を読み込む」で 400 件ずつ前へ
  (コマンドの追加なし、`core-design.md` §2)。タスクカードの `@` → コンポーザのチップ → 送信時に
  `@T-n タイトル` 行を本文の前に付ける。
- **flaky テストの原因** (20 のテストバイナリを 4 並列 × 10 周で 15 件失敗を再現、名前を採取):
  1. `orchestration_fake_recovery` の `run_until_finished` が `TempDir` を関数内で drop しており、
     DB ファイルが実行中に消えていた。SQLite のプールが新しい接続を開くと `unable to open database file`
     (13 件)。→ `TempDir` を呼び出し側へ返す。
  2. コアの順序の不具合: Yhtye が止めたセッションの `session_stopped` が、forwarder 経由の
     `turn_ended` / `exited` を追い越すことがあった (`cancel_task_stops_the_agent_mid_turn`)。
     → `driver::StopOrder` で `Exited` の処理後に出す。回帰検査を同テストに追加。
  3. 修正 2 で、背景で止まるセッションの停止を「オーケストレータの finish_group の時点で既に出ている」と
     仮定していた 2 テスト (`create_task_spawns_…`、`restart_mid_step_…`) が表面化 → その事象を待つように。
  修正後: 4 並列 × 30 周 (計 2400 バイナリ実行) で失敗 0。`cargo test --workspace` 単独 10 周も全成功。
- **実装者が報告せずにターンを終える件**: 実 Haiku の新テスト
  `orchestration_claude_git::real_implementer_waits_for_a_long_command_before_reporting` (`sleep 45` してから
  ファイル作成) で修正前に再現 (reminder の後も報告なし → `protocol_violation` → タスク中止)。原因は Claude Code の
  背景タスク (長いコマンドの背景実行) — ACP ではターン終了後にエージェントを起こす手段が無い。
  → `HarnessConfig::claude_code` に `CLAUDE_CODE_DISABLE_BACKGROUND_TASKS=1`、implementer / reviewer の
  プロンプトに「コマンドは前面で最後まで待つ、長ければ timeout を延ばす」、reminder に「待っていたコマンドは
  前面でやり直す」。修正後 2 回とも reminder なしで報告・done・main にマージ (約 75 秒)。
  `acp-harnesses.md` §5.2 に記録。
- 実行したコマンドと結果:
  - `cargo test --workspace` → 172 件成功 (新規: core_facade `the_git_overview_shows_branches_and_the_commit_graph`、
    git/graph の単体 3)。10 周連続成功 + 上記の並列ストレス。
  - `cargo clippy --workspace --all-targets` 警告なし / `cargo fmt --check` 成功。
  - `cargo test -p yhtye-core -- --ignored --test-threads=1` → 10/10 成功 (実 Haiku、新テスト含む)。終了後 `claude-agent-acp` 0 個。
  - `pnpm test` → 8 ファイル 54 件成功 (新規: `src/ui/Layout.test.tsx` 10 件 = git パネル・ブランチ・タイトルバー /
    カードの進捗・Step・フッタ・待機バナー / `@` メンションの送信 / runs ビュー / エージェント出力タブ /
    遅延読み込みが全読み込みと一致 / 前回プロジェクトの再オープン 3 件 / git の再読み込み、`gitGraph.test.ts` 3 件)。
  - `pnpm build` 成功。
- **Chrome E2E (WS ブリッジ + Vite + 実 Haiku、一時リポジトリ `scratchpad/e2e6a/demo`、マージ済みの枝あり)**:
  開く → 依頼「2 タスクのグループ (README に 1 行 = implement→review、calc.py に sub = implement)」→
  GROUP TASK カード・待機バナー・カードの進捗 / ログ / Step が進み、git パネルにタスクブランチがステータス色の
  バッジ付きで現れる → G-1 done、main にマージ (UI のグラフと `git log --graph` が一致) → T-1 の `@` →
  チップ → 質問を送信 → オーケストレータが `@T-1 …` を受けて回答 → ページ再読み込みで demo が自動で開く →
  T-2 のタイトルで出力タブ (セッションの `process exited` の後に `session stopped` の順)。コンソールエラーなし。
  終了後ブリッジ・Vite・エージェント残存 0。
  スクリーンショット: [`docs/e2e/stage6a/`](docs/e2e/stage6a/) `1-group-running.jpg` / `2-done-merged-mention-chip.jpg` /
  `3-mention-sent-answered.jpg` / `4-reloaded-auto-reopened.jpg` / `5-task-agent-output.jpg`。
  (runs ビューのスクリーンショットは Chrome のウィンドウが非表示になり撮れなかった。ページは動作していた —
  read_page で「現状は何もありません」を確認、rAF 77fps。表示は Vitest で検査。)
- デザインとの差分 (要確認): デザインに無い要素を足した — リポジトリを開く欄、グループ行 (中止・再試行)、
  Step 列、エージェント出力タブ、接続断の帯。フッタの「remote host」はローカルコアへの接続に読み替えた。
  git のレビュー中 / 完了の枝の色は原本に無く流儀に従って発明 (`tokens.md`)。
- 6b への申し送り: 使用量 / quota (`usage_update`・`_meta.quota`) とプラン名、runs (履歴) ビュー、
  「対処の内容を見る / 差分を見る」と git の差分表示、オーケストレータ本文の Markdown 描画、他ハーネスの追加、
  配布ビルドの課題 (DMABUF)。**`pnpm tauri dev` のウィンドウ内での確認は引き続きユーザーの手動確認待ち**。
  未決事項 (`~/.claude` の扱い) は変わらず。

### Stage 6b — Markdown・使用量・ブランチ全体のレビュー (完了)

**範囲** (オーケストレータの判断で絞った): オーケストレータ / エージェントの本文の Markdown 描画、使用量 / quota メーターを
実データで、`main...rebuild` 全体のセキュリティ・品質レビューと修正。設計判断が要るもの (runs ビュー、「対処の内容を見る /
差分を見る」、他ハーネス、配布の DMABUF) は上の**未決事項**へ。

**結果メモ (2026-09-23)**

- **Markdown** (`src/ui/Markdown.tsx`): react-markdown 10 + remark-gfm。`rehype-raw` なし + `skipHtml` (生 HTML は
  要素にならない)、URL は http(s)/mailto のみ (Tauri では opener で外部に開く、webview は遷移しない)、画像は読み込まず
  `[alt]`。会話とエージェント出力のエージェント発話 (ストリーミング中も) に適用。コード・表・引用はトークンの色。
  Vitest 5 件 (書式・閉じていないフェンスのストリーミング・**XSS** (`<script>` / `onerror` / `javascript:` / `data:`) ・URL)。
- **使用量 / quota** (`crates/yhtye-core/src/usage/`、設計 [`core-design.md`](docs/architecture/core-design.md) §14):
  データ源を調査し、**Claude Code の `/usage` (ローカルコマンド、モデル呼び出しなし) を短命の ACP セッションで実行して
  アダプタの Markdown を読む**方式を採用。`usage_update` はコンテキスト量とコストだけ、`_claude/rateLimit` は変化時のみで
  常時表示には使えない、OAuth の非公開 API は不採用。`ApiCommand::GetUsage{refresh?}` (キャッシュ 60 秒・単一実行)。
  UI: ステータスバー左にプラン名 (`claude max`)、右のメーターに実値と `(3h 35m)`、クリックで再取得、取れなければ「—」+ 理由。
  実機: `real_usage_command_reports_the_subscription` 成功 (約 5〜7 秒、`~/.claude/projects` に会話は残らない)。
  **観察: 作業開始時点で週 (全モデル) が 99%、終了時に 100% 表示** (5 時間枠はリセット直後)。実 Haiku テストは最小限にした。
- **レビュー** (security-reviewer / code-reviewer、opus): CRITICAL / HIGH なし。直したもの:
  - [M] エージェントの**プロセス**をプロジェクトで起動していた → 悪意あるリポジトリの `.npmrc` / `node_modules` で `npx` が
    任意コードを実行しうる。ホーム (無ければ `/`) で起動し、作業ディレクトリは ACP の `cwd` だけで渡す。アダプタを
    `@0.81.0` に完全一致で固定 (`tests/acp_launch.rs`)。偽エージェントはセッションの `cwd` に chdir。
  - [M] Tauri の CSP が `null` → 厳格な CSP を設定 (`core-design.md` §9)。本番バンドル + 同 CSP ヘッダで Chrome の違反 0 を確認。
  - [M] 終了と開く処理の競合でエージェントが残りうる / 使用量プローブが終了後も残りうる・失敗をキャッシュしない →
    `Core::shutdown` が開く処理を待ち以後を拒否、プローブを `CancellationToken` で停止、失敗も 10 秒キャッシュ (`nothing_opens_after_shutdown`)。
  - [L] エージェントの環境からブリッジのトークンを除く / MCP サーバーは `Origin` 付き要求を 403 (`browser_requests_with_an_origin_are_forbidden`) /
    opener の権限を URL を開くだけに / `repository_root` も git の環境掃除を通す・`GIT_CONFIG_PARAMETERS` / `GIT_CONFIG_COUNT` を除く /
    起動失敗のメッセージに起動ディレクトリ。
  - 確認して問題なし: WS ブリッジ (127.0.0.1・128bit トークンの定数時間比較・Origin 完全一致・Origin 無しはブラウザからは不可能)、
    MCP (127.0.0.1・トークン失効・rmcp の Host 検査・本文サイズ制限)、git の引数 (argv 渡し・ブランチ名はコア生成)、
    `open_project` のパス (canonicalize + toplevel 一致)。800 行超のファイルなし、実行時に落ちうる unwrap なし。
  - 残した LOW (理由): ブリッジのトークンが Vite バンドルに入る (開発専用、未決 7) / `GroupKillGuard` が pgid 再利用で
    無関係のグループを kill しうる (確率極小) / `RUST_LOG=trace` で ACP ライブラリが MCP の URL (トークン) を出しうる (明示的な
    opt-in) / 終了時に古い停止が新しいセッションを `stopped` にしうる (再起動時に `session/load` せず新規になるだけ) /
    終了が起動中のセッションで最大 120 秒待つ / `StopOrder` の小さなエントリが残りうる / ストリーミング中の Markdown は
    チャンクごとに全文を再解析 (長文で重くなったら間引く) / opener の失敗は黙って無視 / `investigate` タスクも書き込み可能、
    オーケストレータの `Read` は任意のファイルを読める (設計上の前提として記録)。
- 実行したコマンドと結果:
  - `cargo test --workspace` → 182 件成功 × 3 回 (新規: usage parse 5、core_facade 3 = 使用量の取得とキャッシュ /
    取れないときは unavailable / shutdown 後は開かない、mcp_server 1、acp_launch 1)。
  - `cargo clippy --workspace --all-targets` 警告なし / `cargo fmt --check` 成功。
  - `pnpm test` → 10 ファイル 63 件成功 (新規: Markdown 5、StatusBar 4)。`pnpm build` 成功。
  - 実 Haiku (週の quota が 99〜100% のため、変更した経路に絞った): `real_bypass_mode_writes_file` (プロセスをホームで
    起動してもセッションの cwd にファイルが作られる)、`real_orchestrator_creates_task_and_sub_agent_reports_back`
    (MCP の Origin 検査下で実 Claude Code がツールを呼べる)、`real_usage_command_reports_the_subscription` (2 回) — すべて成功。
    **全 11 件の再実行はしていない** (quota)。終了後 `claude-agent-acp` 残存 0。
  - Chrome (WS ブリッジ + 本番バンドル + CSP ヘッダ + 実 Haiku): プロジェクトを開き「見出し・太字・箇条書き・インラインコード・
    コードブロック・表」の返答を依頼 → 正しく描画、ステータスバーに `claude max`・5h 4% (4h 52m)・week 100% (1d 18h)、
    コンソールに CSP 違反なし。スクリーンショット [`docs/e2e/stage6b/1-markdown-and-usage.jpg`](docs/e2e/stage6b/1-markdown-and-usage.jpg)。
    ブリッジ・プレビューは停止済み。

## Stage 7 — 不具合修正とハーネス / モデルの選択

ユーザーの決定 (上の未決事項) を受けた作業。**7a** = ユーザー報告の不具合と小さな決定事項、**7b** = ハーネス / モデルの選択、
**7c** = OpenCode ハーネス、**7d** = effort と用途メモ付きの候補の行 + 設定モーダル、**7e** = Codex ハーネス + 秘密の環境変数。

### Stage 7a — 完了したグループが「進行中」のまま + 決定事項の反映 (完了)

**範囲**: ユーザー報告の不具合「グループが終わったのに UI が『進行中』のまま、タスクカードに『同グループの他タスク完了を待機 —
送信保留』が出続ける」の根本原因の修正。決定事項 (未決 1〜8) の反映、DMABUF の自動設定。

**結果メモ (2026-09-23)**

- **根本原因** (ユーザーのアプリの DB `~/.local/share/com.tom039224.yhtye/yhtye.sqlite3` の複製から特定): UI は正しかった —
  コアの状態自体が `G-1 active` / `T-1 done` のまま。実 Haiku のオーケストレータが `[yhtye:group_settled]` を受けて
  「テスト完了しました！…」とユーザーに報告しただけで**`finish_group` を呼ばずにターンを終えた** (seq 140〜146)。
  設計上グループは `finish_group` まで `active` なので、UI の「進行中」と「送信保留」は状態の忠実な表示だった。
  TS のリデューサ・スナップショット / 追いつきの経路に食い違いは無い (記録済みの 3 本の実イベント列で Rust と一致を検査)。
- **修正 (コア)**: サブエージェントの催促 (§7) と同じ安全網をオーケストレータにも
  ([`orchestration-model.md`](docs/architecture/orchestration-model.md) §2.1):
  オーケストレータのターンが `end_turn` で終わり、受信箱が空で次のプロンプトも無いのに、全タスクが落ち着いた active グループが
  あれば 1 回目は `group_settled` を `reminder=1` 付きで再送、それでも終わらなければ Yhtye が `finish_group` を代行して
  `merge_result` で知らせる (開始不能タスクが残る場合は代行しない)。新しいドメインコマンド `OrchestratorTurnEnded`、
  イベント `group_finish_reminded`、`Group.finish_nudges` (マイグレーション `0002`、古いログは `serde(default)` で 0)、
  `GitOp::MergeGroup.notify: bool` → `trigger: MergeTrigger` (FinishGroup / UserRetry / Yhtye)。
  オーケストレータのプロンプトにも「同じターンで `finish_group`、それまで作業はマージされない」を追記。
  **合意済みの決定 (「オーケストレータがタスク追加か finish_group を選ぶ」) は変えていない** — 選ばずに終えたときの安全網。
  ユーザーの DB に残っている `G-1` は、次にオーケストレータのターンが終わった時点で催促され、完了する。
- **修正 (UI)**: 全タスクが落ち着いた active グループを「進行中 / 送信保留」と区別して表示: グループのバッジ「完了待ち」、
  完了タスクの補足行「全タスク完了 — オーケストレータのグループ完了 (マージ) 待ち」、待機バナー
  「全タスクが完了 — オーケストレータがグループを完了 (マージ) するのを待っています」。finishing は「base ブランチへマージ中」。
  done / cancelled / merge_blocked では補足行もバナーも出ない ([`orchestrator-desktop.md`](docs/design/orchestrator-desktop.md) §8)。
- **テスト (先に失敗を確認)**:
  - Rust ドメイン `domain/tests/orch.rs` 7 件 (催促 → 代行 / 催促後に自分で finish / 動いているタスク・未配達の受信箱・
    キャンセルされたターン・キューあり・空グループでは何もしない / 開始不能タスクは催促のみ / 代行マージが dirty で
    `merge_blocked` + `merge_result ok=false` / 古いイベントの読み込み)。store の往復で `finish_nudges` を検査、マイグレーション数 2。
  - core_facade (偽エージェント) 2 件: `a_group_the_orchestrator_leaves_open_is_finished_by_yhtye` (Haiku の振る舞いを再現 →
    催促 1 回 → Yhtye が main にマージ) と `a_reminded_orchestrator_finishes_the_group_itself`。前者の実イベント列を
    `src/test/fixtures/fake-unfinished.json` に記録 (`pnpm record:fixtures`、既存 2 本も再記録)。
  - Vitest `src/ui/GroupFinish.test.tsx` 7 件 (記録した列で: 催促前の表示 / ライブで done になる / 再読み込み (終了時の snapshot) /
    ログ全体の追いつき / cancelled / merge_blocked → 再試行 → finishing → done)。UI 修正を戻すと 2 件失敗することを確認。
    `domain.test.ts` に新しい記録のリデューサ一致と古いグループの催促カウント。
- **決定事項の反映**: `~/.claude` = 常に有効 (orchestration-model §11・acp-harnesses §5.2)。「対処の内容を見る / 差分を見る」は
  コードに無かった (6a で未実装のまま) ので設計ドキュメントに「出さない」と記録。runs ビューは保留。使用量の頻度・ブリッジの
  トークンは現状維持 (README に単一ユーザー機専用と明記)。以前からの要確認と `pnpm tauri dev` の動作はユーザー確認済み。
- **DMABUF**: `src-tauri/src/webkit_env.rs` — `run()` の最初に、Wayland (`WAYLAND_DISPLAY` / `XDG_SESSION_TYPE=wayland`、
  `GDK_BACKEND=x11` なら除外) かつ NVIDIA (`/proc/driver/nvidia` / `/sys/module/nvidia`) で、ユーザーが未設定のときだけ
  `WEBKIT_DISABLE_DMABUF_RENDERER=1`。単体テスト 3 件 (入力を注入)。dev の `.cargo/config.toml` はそのまま (設定済みなので何もしない)。
  この機械 (Hyprland + NVIDIA) では検出が真になることを確認。
- 実行したコマンドと結果:
  - `cargo test --workspace` → 193 件成功 × 2 回 (ignored 11)。`cargo clippy --workspace --all-targets` 警告なし / `cargo fmt --check` 成功。
  - `pnpm test` → 11 ファイル 73 件成功。`pnpm build` 成功。
  - 実 Haiku (変更した経路に絞った): 新規 `orchestration_claude_git::real_group_without_a_finish_hint_still_ends_done`
    (バグ報告と同じ依頼文、`finish_group` の指示なし) 2 回と `real_group_with_two_code_tasks_lands_on_the_base_branch` 1 回、すべて成功。
    **観察**: 3 回ともオーケストレータ自身が `finish_group` を呼んだ (催促 0 回、プロンプトの追記が効いた可能性)。催促・代行の経路は
    偽エージェントで検証。終了後 `claude-agent-acp` 残存 0。
  - Chrome での目視確認はしていない (記録した実イベント列による UI テストで代替)。
- 残課題: 開始不能タスクが残るグループは催促後も `active` のまま (UI は「完了待ち」)。オーケストレータが `cancel_task` するか、
  ユーザーがグループを中止する。

### Stage 7b — ハーネス / モデルの選択 (完了、ユーザー決定済みの仕様)

- コンポーザの下の行に ⚙ ボタン。**役割ごと** (orchestrator / implementer (code) / investigator / reviewer) に、
  **候補集合 (ハーネス × モデル) と既定値**を設定する。
- 設定は**全体の既定値の上にプロジェクトごとの既定値を重ねる**。SQLite に保存。
- モデル一覧は各ハーネスから ACP の `configOptions` で取得する (**ハードコードしない**)。
- 変更は**新しく起動するセッション**から効く (動いているセッションは変えない)。
- オーケストレータは `create_task` の任意引数でタスクごとに上書きできるが、**その役割の候補集合の中だけ**。
  候補外ならツールがエラーで拒否する。
- 設計ドキュメント (mcp-tools の `create_task`、core-design の設定・API、orchestration-model の役割) を先に更新してから実装する。

**結果メモ (2026-09-23)**

- 設計: [`core-design.md`](docs/architecture/core-design.md) §15 (新設)、[`mcp-tools.md`](docs/architecture/mcp-tools.md) の
  `create_task` / `get_status`、[`orchestration-model.md`](docs/architecture/orchestration-model.md) §8.0、
  [`orchestrator-desktop.md`](docs/design/orchestrator-desktop.md) §8 を先に更新した。
- コア: `crates/yhtye-core/src/agents/` — `settings.rs` (役割 4 つ・`AgentChoice`・`RoleSettings`・層の重ね合わせ・
  検査・`pick`。純粋関数)、`catalog.rs` (`HarnessPreset` = ハーネスの登録簿、`AgentCatalog` = 設定の写しと起動時の解決)、
  `models.rs` (プロンプトを送らない短命セッションの `configOptions` からモデル一覧、キャッシュ 10 分)。
  `CoreConfig` の役割別 `HarnessConfig` 3 つを `harnesses` + `default_agent` に、`OrchestrationConfig` の 3 つを
  `agents: Arc<AgentCatalog>` に置き換えた (**`HarnessConfig` の形は変えていない**)。マイグレーション `0003`
  (`agent_settings`、`tasks.agent / review_agent`、`agent_sessions.harness / model`)。`session_started.agent` と
  `SessionRecord.agent` で実際のハーネス × モデルを記録し、`session/load` での復元は記録した組のまま。
  `create_task` の `harness / model / review_harness / review_model` はドライバが候補集合で検査
  (`runtime/agent_args.rs`、候補外は許される一覧付き `invalid_argument`)、`get_status` に `agents`、オーケストレータの
  システムプロンプトに起動時点の候補一覧。API `get_agent_settings` / `set_agent_settings` / `list_harness_models`、TS 型を再生成。
- **レビューの決定**: review Step は `review_harness / review_model` の上書きがあればそれ、無ければ**起動時点の** reviewer の既定。
- UI: コンポーザの下段に ⚙ → `src/ui/AgentSettingsPanel.tsx` (範囲「全体 / このプロジェクト」、役割ごとに「全体の設定を使う」
  (全体では「組み込みの既定を使う」)・候補のチェックリスト (最後の 1 つは外せない)・既定のセレクト、即保存)。
- 実行したコマンドと結果:
  - `cargo test --workspace` → 220 件成功 × 2 回 (新規: agents 単体 14、agent_args 5、prompts 1、store 2、
    `tests/agent_selection.rs` (偽エージェント) 5 = 候補内の上書き・片方だけの指定の補完・候補外の拒否・`get_status` の agents /
    設定の変更後も動いているセッションはそのまま・新しいセッションは新しい既定 / 再起動で復元したセッションは記録した組のまま
    (記録を無視する変異で失敗することを確認) / 検査・層・保存 / モデル一覧)。`cargo clippy --workspace --all-targets` 警告なし、`cargo fmt --check` 成功。
  - `pnpm test` → 12 ファイル 80 件成功 (新規 `src/ui/AgentSettings.test.tsx` 7 件)。`pnpm build` 成功。フィクスチャを再記録 (`session_started.agent`)。
  - 実 Haiku (変更した経路に絞った): 新規 `acp_claude_real::real_model_list_comes_from_the_adapter` (実アダプタの一覧 = default / opus[1m] /
    claude-fable-5-1[1m] / sonnet / haiku、約 2 秒)、新規 `agent_selection_claude::real_orchestrator_override_is_checked_and_runs_the_resolved_model`
    (implementer の候補 = haiku のみ。**Haiku のオーケストレータが `model=sonnet` を指定 → 許される一覧付きで拒否 → 一覧を読んで `haiku` で作り直し**、
    タスクのセッションは `claude-code/haiku` で起動し Ready のモデルも haiku、README に反映・main にマージ)、
    `orchestration_claude_real::real_orchestrator_creates_task_and_sub_agent_reports_back`、`orchestration_claude_restart` — すべて成功。
    安いモデルが haiku だけなので 2 つのモデルでの実行は偽エージェントで検証。終了後 `claude-agent-acp` 残存 0。
  - Chrome (WS ブリッジ + Vite + 実 Claude Code、一時リポジトリ): ⚙ → パネルに実モデル一覧、「このプロジェクト」でレビューの継承を外し
    sonnet を候補に追加 → DB の `agent_settings` に保存を確認。[`docs/e2e/stage7b/`](docs/e2e/stage7b/) `1-settings-panel-real-models.jpg` /
    `2-project-reviewer-override.jpg`。ブリッジ・Vite は停止済み。
- 7c への申し送り: OpenCode は `HarnessPreset` を 1 つ作り `CoreConfig::claude_code` (または新しい既定の構成関数) の `harnesses` に足すだけで
  UI・検査・一覧に載る。モデル一覧は `configOptions` の `category: model` か id `model` の select を読む (無ければ「既定のモデル」だけ)。
  モデルを env でも渡す必要があれば `model_env`。一覧用のセッションが MCP / 永続化なしで起動できるなら `probe` を設定する。
  `HarnessPreset::fixed` のように役割ごとの設定を持てるので、オーケストレータの書き込み制限 (§8.1 相当) は preset の `orchestrator` で。
  残課題: 設定から外したハーネスの記録は既定に戻して起動する (警告ログのみ、UI 表示なし)。

### Stage 7c — OpenCode ハーネス (完了、ユーザー決定済みの仕様)

- OpenCode を**全役割で選べる**ハーネスとして追加 (7b の選択に載せる)。
- 検証の順: まず implementer、次に reviewer / orchestrator。確認項目は acp-harnesses の他ハーネスと同じ (モード名・MCP・
  `session/load`・履歴再生の順序・ACP の `cwd` を守るか)。
- 実エージェントテストのモデルは `opencode/muse-spark-1.3-contributor-free`。
- 7c-2 のユーザー決定: オーケストレータにも選べる (読み取り専用にできないので設定パネルで警告、プロンプトでの禁止は維持)。
  実テストは OpenCode = `opencode/muse-spark-1.3-contributor-free`、Claude Code = Haiku のみ。`~/.claude` とユーザーの OpenCode 設定は有効のまま。

**7c-1 結果メモ (2026-09-23)** — 詳細は [`acp-harnesses.md`](docs/architecture/acp-harnesses.md) §7.1〜7.5。
`opencode acp` (2.0.12) は ACP の cwd を守り、`build` モードで自動承認、モデルは `set_config_option` (`<provider>/<model>`、475 件、
既定は最後に使ったモデル)、システムプロンプトは FirstPrompt、MCP は HTTP のみでコードモードの `execute` 経由、session/load は
作成時の cwd でのみ可、オーケストレータを読み取り専用にする手段は無い。`HarnessConfig::opencode`、実テスト
`acp_opencode_real` 7 件・`orchestration_opencode_real` 2 件 (各 2 回成功)。

**7c-2 結果メモ (2026-09-23)** — 詳細は [`acp-harnesses.md`](docs/architecture/acp-harnesses.md) §7.2・§7.6、
[`core-design.md`](docs/architecture/core-design.md) §3.4・§15.2・§15.4、[`orchestration-model.md`](docs/architecture/orchestration-model.md) §8.1。

- コア: `HarnessPreset::opencode` (全役割 `HarnessConfig::opencode`、probe は mode / model なし)、preset と `HarnessInfo` に
  `requires_model` / `orchestrator_read_only`。`AgentCatalog::validate` が OpenCode の `model: null` を拒否 (念のため preset 側にも
  無料モデルのフォールバック)。`agents/installed.rs`: `PATH` の実行可能な `opencode` を検出したときだけ登録
  (`CoreConfig::installed`、アプリと開発ブリッジ)。`HarnessConfig.env_remove` (追加のみ) で、Orca の端末が注入した
  `OPENCODE_CONFIG_DIR` (= `ORCA_OPENCODE_CONFIG_DIR`) だけをエージェントの環境から外す (ユーザー自身の値は残す)。
- 7b の残課題 (消えたハーネス): `AgentCatalog::resolve` → `Resolved { replaced }`、`session_started.replaced` (省略可能、古いログは無し)。
  UI は会話 / エージェント出力にエラー行、設定パネルの役割に「⚠ … が見つかりません … 既定 (…) で起動します」と候補の「(見つからない)」。
- UI: 役割ごとのモデル検索 (空白区切りの AND、候補は常に表示、12 件超のハーネスは検索したときだけ出す、最大 40 件 + 「他に N 件」)、
  長い一覧はプロバイダ (`<provider>/`) 別の見出し。オーケストレータの役割で書き込み制限の無いハーネスの候補・既定に「⚠ 書き込み制限なし」、
  選んでいれば役割の上に説明の帯。requires_model のハーネスには「既定のモデル」を出さない。
  `execute` ツールの `rawInput.code` から `tools.<server>.<tool>(` を読み「tool execute → yhtye.report_step_done」(会話・タスクカードのログ)。
  サーバー側の `tool_called` 記録の表示 (ハーネス非依存) は従来どおり。
- session/load のフォールバック: OpenCode は別ディレクトリの load を約 0.65 秒でエラーにする (実機) → 既存の `retry_fresh`
  (新しいセッション + Step の全プロンプト + 注記) に乗る。フォールバックは偽エージェントで検証済み (`orchestration_fake_restart`)。
- テスト: Rust 単体 (installed 3 件・catalog の replaced)、偽エージェント `agent_selection` +2 (requires_model の検査、消えたハーネスの置き換えと
  `replaced`)、`acp_launch` に `env_remove`。Vitest: `AgentSettings.test.tsx` +4 (検索とプロバイダ見出し・追加 / オーケストレータの警告 /
  見つからないハーネス / 検索の上限)、新規 `src/store/transcript.test.tsx` 3 件 (コードモードの呼び出し・表示・置き換えの行)。
  実機: `acp_opencode_real` +2 (preset のプローブで 475 件・約 1.5 秒 / 別ディレクトリの load が速く失敗)、
  新規 `orchestration_mixed_real` 2 件 — **Claude Haiku オーケストレータ + OpenCode implementer + Claude Haiku レビュアーで
  implement → review → finish_group → main にマージを 2 回成功 (約 60 秒)**、OpenCode オーケストレータ + Claude Haiku implementer 1 回成功。
  各セッションの Ready のモデルが設定どおり、終了後 `opencode acp` / `serve --stdio` / `claude-agent-acp` の残存 0。
- 実行したコマンド: `cargo test --workspace` → 226 件成功 × 2 回 (ignored 27)、`cargo clippy --workspace --all-targets` 警告なし、
  `cargo fmt --check` 成功、`pnpm test` → 13 ファイル 87 件成功、`pnpm build` 成功、`pnpm gen:types` で TS 型を再生成 (フィクスチャは変更なし)。
- Chrome (`pnpm dev:browser` + 実 OpenCode / Claude Code、一時リポジトリ): ⚙ パネルに Claude Code の 5 モデルと「OpenCode: 475 モデル — 検索して候補に追加」、
  オーケストレータで「muse free」を検索 → `OpenCode · opencode` 見出しに 2 件 (⚠ 書き込み制限なし)、1.3 を選ぶと警告の帯。
  [`docs/e2e/stage7c/`](docs/e2e/stage7c/) `1-settings-opencode-models.jpg` / `2-opencode-orchestrator-warning.jpg`。
  ブラウザでの実混成実行は行っていない (上の実テストで代替)。ブリッジ・Vite は停止済み。
- 残課題: OpenCode の MCP 隔離が無い (ユーザーが OpenCode に MCP を足すと付く。必要になれば Yhtye 専用 `OPENCODE_CONFIG_DIR`)。
  モデル一覧のプローブが OpenCode の履歴にセッションを 1 つ残す。消えたハーネスの記録を持つセッションの load は別ハーネスで試みて失敗 → 新しいセッション
  (その場合 `replaced` は付かない)。OpenCode オーケストレータの書き込みはプロンプト頼み。

### Stage 7d — effort + 用途メモ付きの候補の行、アプリ全体の設定モーダル (完了、ユーザー決定済みの仕様)

- 候補 = 行 (ハーネス × モデル × effort (任意) + 用途メモ)。同じハーネス × モデルを effort 違いの別の行として置ける。役割ごとに行の一覧と既定の行 (★)。
- オーケストレータの `create_task` に `effort` / `review_effort`。**組が行と完全一致**しなければ拒否 (全行を用途メモ付きで列挙)。effort を
  省略できるのはそのハーネス × モデルの行が 1 行だけのとき、複数行なら effort の指定を求める。システムプロンプトと `get_status` は行 + 用途メモ。
- effort の一覧は ACP の `configOptions` から (モデルごと)。セッション起動は**モデル → effort の順**で設定し、要求値 = 現在値を検証。
  effort を持たないモデルに指定したら起動を失敗させる。
- 設定 UI: コンポーザの ⚙ を廃止し、左端の縦バーの最下部の ⚙ でアプリ全体の設定モーダル (約 90%、Esc / × で閉じる)。左に項目 (いまは「エージェント」)、
  上に 全体 / このプロジェクト、役割ごとの行の表。

**結果メモ (2026-09-29)** — 詳細は [`core-design.md`](docs/architecture/core-design.md) §15、[`acp-harnesses.md`](docs/architecture/acp-harnesses.md) §8、
[`mcp-tools.md`](docs/architecture/mcp-tools.md) `create_task`、[`orchestrator-desktop.md`](docs/design/orchestrator-desktop.md) §8。

- コア: `AgentChoice` に `effort`、新しい `Candidate { harness, model, effort, note }`、`RoleSettings.candidates: Vec<Candidate>`、
  `RoleSettings::pick(harness?, model?, effort?)` → `PickError::{NoMatch, NeedsEffort}` (`agents/settings.rs`)。`HarnessConfig.effort`
  (`acp/startup.rs` がモデルの後に設定・検証)、`HarnessPreset::config(role, model, effort)`。`runtime/agent_args.rs` が完全一致を検査、
  `prompts::agent_choices_prompt` が行 + 用途メモ。
- **移行**: 設定と `tasks.agent` は JSON なので、古い保存値は serde の既定 (effort なし・メモ空) で読める (単体テストあり)。SQL は `0004_agent_effort.sql`
  (`agent_sessions.effort`) だけ。
- **effort 一覧の取り方と費用**: モデルごとに選択肢が変わる (Claude Code はモデルを変えると作り直す) ので、プローブのセッションでモデルを 1 つずつ選んで読む。
  12 モデル以下のハーネス (Claude Code = 5) はモデル一覧と一緒に全モデル分 (プローブ全体で約 2.7 秒、以前は約 2 秒)。それ以上 (OpenCode ≈ 480) は
  一覧では読まず、UI が選んだモデルについて新 API `list_model_efforts` で 1 つだけ (約 1.2 秒、10 分キャッシュ)。先頭の `default` 行は一覧から除く。
- 偽エージェントに `efforts` (モデルごとの effort option、モデルを変えるとリセット) を追加。
- UI: `SettingsModal` (シェル + 項目一覧 + 範囲切替)、`AgentSettingsSection` (取得・保存)、`CandidateTable` (行の表)、`ModelPicker` (OpenCode の検索・プロバイダ別)、
  `agentSettings.ts` (純粋な行の編集: 重複の拒否・既定の追従・行の削除・新しい行の提案)。⚠ 書き込み制限なし / 見つからないハーネスの警告は維持。
- 実行したコマンドと結果:
  - `cargo test --workspace` → 234 件成功 (ignored 30)、`cargo clippy --workspace --all-targets` 警告なし (`create_task` の引数が大きくなったので enum 3 か所に
    `allow(clippy::large_enum_variant)`)、`cargo fmt --check` 成功、`pnpm test` → 13 ファイル 89 件成功、`pnpm build` 成功、`pnpm gen:types` で型を再生成。
    新規: 設定の単体 (完全一致・effort 違いの行・重複・古い JSON)、`agent_args` (拒否と曖昧さ)、プロンプト、store (effort の往復・マイグレーション 4)、
    ACP (`effort_is_set_after_the_model_and_verified`)、偽エージェントの Core (`create_task_needs_an_exact_row_and_the_effort_is_applied`、
    `efforts_of_a_long_model_list_are_read_per_model`)、Vitest 設定モーダル 13 件 (旧 `AgentSettings.test.tsx` を全面書き換え)。
  - 実 Claude Code (Haiku): `acp_claude_real::real_model_list_comes_from_the_adapter` (全モデルの effort: `default`/`opus[1m]`/`fable`/`sonnet` = low〜max、
    **haiku = 無し**)、`real_effort_is_applied_after_the_model` (Haiku に effort が無いので、**プロンプトを送らず** sonnet のセッション起動だけ: `effort=low` を
    アダプタが報告、未知の値は `set_config_option` で失敗)、`agent_selection_claude::real_orchestrator_override_is_checked_and_runs_the_resolved_model`
    (Haiku のオーケストレータが行に無い `effort=high` を指定 → 行 + 用途メモ付きで拒否 → 完全一致の行で作り直し、Haiku で起動して main にマージ)。
  - 実 OpenCode (`opencode/muse-spark-1.3-contributor-free`): `acp_opencode_real::real_opencode_efforts_are_read_per_model_and_applied` (variant =
    minimal/low/medium/high/xhigh を約 1.2 秒で読み、`minimal` を設定 → `effort=minimal` を報告)。
  - **[解決済み: 下の 2026-09-29 の追記]** **実 OpenCode を含む混成実行は今は再現できない (環境の問題)**: 開発中に OpenCode が 2.0.12 → 2.0.18 に更新され、この環境では **git リポジトリの cwd で
    `opencode acp` に無料モデルを `set_config_option` すると `model not found` になる** (git 以外のディレクトリでは通る。最小の再現: 一時 git リポジトリ +
    `HarnessPreset::opencode` の設定で起動。`opencode models` にも muse-spark が出ない)。7c-2 の `orchestration_mixed_real` 既存 2 件も同じ理由で今は通らない
    (7d の変更とは無関係)。追加した `orchestration_mixed_real::real_effort_rows_are_matched_exactly_and_applied` は、OpenCode が使える状態だった開発序盤の
    2 回の実行で「存在しない effort の拒否 → 行の一覧と用途メモから `minimal` の行を選んで作成成功」まで確認済み (その後 OpenCode 側で失敗)。OpenCode が
    直れば通す想定 (未確認)。終了後 `claude-agent-acp` / `opencode acp` の残存 0。
  - Chrome (`pnpm dev:browser` + 実 Claude Code / OpenCode、一時リポジトリ、`docs/e2e/stage7d/`): ⚙ が左端バーの最下部、コンポーザに ⚙ が無い
    (`4-gear-at-bottom-of-rail.jpg`)。設定モーダルで実装に行を追加 → sonnet + High + 用途メモ (`1-settings-modal-effort-rows.jpg`、DB の `agent_settings` に
    effort・note が保存されたことを確認)。OpenCode の行で検索付きピッカー (`2-opencode-model-picker.jpg`)、無料モデルを選ぶと effort 一覧
    (minimal〜xhigh) を `list_model_efforts` で取得 (`3-opencode-lazy-efforts.jpg`)。Esc で閉じる。ブリッジ・Vite は停止済み。
- 2026-09-29 追記 (OpenCode の環境問題の原因と対処): 原因は OpenCode 2.0.18 の上流バグ anomalyco/opencode#50236 (修正 PR #50619 未マージ) —
  `opencode acp` の**最初の `session/new`** のモデル一覧にプロバイダ読み込み前のスナップショットが使われ `opencode/*` が無く、`model not found` になる
  (git リポジトリかどうかは無関係だった)。**Yhtye 本体では回避しない (ユーザー決定)**。実機テストだけ、`crates/yhtye-fake-agent` の
  テスト用シム `opencode-warmup-shim` (捨ての `session/new` を 1 回先に流す) 経由で `opencode acp` を起動する。詳細は `acp-harnesses.md` §7.7。
  実 OpenCode を含むテストのヘルパーは `tests/common/opencode.rs::{opencode_harness, opencode_preset}`。実 Agent のオーケストレーションテストは、想定外の
  `SessionFailed` / `AgentCrashed` の help で全体のタイムアウトを待たず即失敗する (`common/orch.rs::until_healthy`)。
  シム経由の実機: `orchestration_mixed_real` 3 件 (混成 + effort の行) と `orchestration_opencode_real` 2 件が成功、`acp_opencode_real` は 10 件中 9 件成功
  (無料モデルが合言葉を答えない回がある `real_opencode_system_prompt_reaches_the_agent` は不安定、再実行で通る)。残存プロセス 0。
- 残課題: 上の OpenCode の環境問題 (混成の実機テストが通せない)。モデルが effort を持たなくなった古い設定はセッション起動が失敗する (警告のみで既定に戻す挙動にはしていない)。
  設定モーダルは項目が 1 つだけ (足す作りは用意した)。

### Stage 7e — Codex ハーネスと秘密の環境変数 (完了、ユーザー決定済みの仕様)

調査: [`research/codex-acp.md`](docs/architecture/research/codex-acp.md)。仕様と実測: [`acp-harnesses.md`](docs/architecture/acp-harnesses.md) §9、[`core-design.md`](docs/architecture/core-design.md) §15.2・§15.6・§16。

ユーザー決定 (再検討しない):
1. 入口 `npx -y @agentclientprotocol/codex-acp@2.0.0` (完全固定)、`CODEX_PATH` = `PATH` のユーザーの `codex`。`codex` が `PATH` にあるときだけ登録 (OpenCode と同じ)。
2. 全役割 `agent-full-access`。オーケストレータも選べるが読み取り専用にできない (`orchestrator_read_only = false`、⚠ 書き込み制限なし、プロンプトで禁止)。
3. システムプロンプトは `FirstPrompt`。
4. モデル一覧はプロバイダ次第: Codex 設定の `model_provider = openrouter` なら OpenRouter の公開 `GET /api/v1/models` (認証なし、`tools` 対応のみ、effort は `reasoning.supported_efforts`)、
   それ以外はアダプタの一覧。
5. モデルと effort は環境変数 `CODEX_CONFIG` (JSON) で渡す (アダプタはカタログ外のモデルを `set_config_option` で拒否)。モデルは従来どおり検証。effort の config id は preset ごと (`reasoning_effort`)。
6. 秘密の環境変数: OS キーリング (Secret Service)、DB は名前だけ、全エージェントの環境に注入、設定モーダルの新項目。
7. `CODEX_HOME` は継承のまま。8. 認証・課金エラー (応答テキスト + `end_turn`) の検出は今回しない。9. セッション履歴は `~/.codex/sessions` に残す。10. 「Model metadata not found」の警告はそのまま。

**結果メモ (2026-09-29)**

- コア: `HarnessConfig::codex`、`HarnessPreset::codex` + `model_config_env` / `effort_config_id` / `ModelSource` (`agents/catalog.rs`)、`agents/openrouter.rs` (取得と絞り込み、リダイレクトなし・16 MiB 上限)、
  `agents/codex_config.rs` (`config.toml` のトップレベル `model_provider` だけ読む)、`ModelService` (`with_env` / `with_openrouter_url`、OpenRouter のときは `list_model_efforts` を一覧から返す)、
  `installed_presets` が `codex` を登録。`efforts_from_options(options, config_id)`。
- 秘密の環境変数 (`crates/yhtye-core/src/secrets/`): `SecretBackend` (`KeyringBackend` = `keyring` 3.6 / `MemoryBackend`)、`Secrets`、`spawn_agent_with_secrets` (サブエージェント・オーケストレータ・モデル一覧・使用量の**全 spawn**)。
  適用順は 秘密 → 削除 → ハーネスの env。名前は環境変数の文字種 + 予約 (`YHTYE_BRIDGE_TOKEN` 等) + 読み込みを変える名前 (`PATH` / `LD_*` …) を拒否。DB の `secret_env_names` (マイグレーション 0005、名前だけ)。
  API `list_secret_env` / `set_secret_env` / `delete_secret_env`。値は `Debug`・ログ・応答・エラーに出ない。読めないときは起動を失敗させる (`secret environment variables`)。
  UI: `SecretEnvSection` (登録・検証・上書き・削除確認・エラー表示、値は保存後に保持しない)。開発ブリッジは id 無しメッセージのログを長さだけにした。
- **Codex が受け付ける effort**: Codex 0.159 は `model_reasoning_effort` を**空でない任意の文字列**として受け、検証せずプロバイダに渡す (`max` / `xhigh` / `none` / `minimal` / `bogus` でも設定は読める)。
  Yhtye は OpenRouter が示すそのモデルの `supported_efforts` だけを提示する (一覧では `none` / `minimal` / `low` / `medium` / `high` / `xhigh` / `max`)。実機: `medium` / `low` のモデルで `low` を渡し、
  Codex のロールアウトの全 `turn_context.effort` が `low`。
- **実機のモデル**: 当初の `nvidia/nemotron-3.5-lightning:free` は OpenRouter の障害で 1 回の推論が 2〜6 分 (この日) のため、ユーザー決定で `nvidia/nemotron-3-super-120b-a12b:free` (effort = medium / low) に変更。
- 実行結果:
  - `cargo test --workspace` 257 件成功 (ignored 42)、`cargo clippy --workspace --all-targets` 警告なし、`cargo fmt --check` 成功、`pnpm test` 96 件 (89 → +7)、`pnpm gen:types` で型を再生成。
    新規: secrets 単体 8 (実キーリングの往復 1 件は `--ignored` で成功)、`tests/acp_secret_env.rs` 2、`tests/secret_env_api.rs` 2、`tests/codex_models.rs` 3 (OpenRouter の模擬サーバー: `Authorization` 無し・
    `tools` 絞り込み・effort・プロバイダ違い・失敗)、`agents/openrouter.rs` 2 / `codex_config.rs` 3 / `installed.rs` / `config.rs` / `store` の単体、Vitest `SecretEnv.test.tsx` 7。
  - 実 Codex (`acp_codex_real`、`fish -c` 経由で `OPENROUTER_API_KEY_CODEX` あり、`CODEX_HOME=~/.codex`): 9 件すべてが少なくとも 1 回成功。
    `nemotron-3-super`: cancel (約 8 ms で `cancelled`)、MCP で implementer が `report_step_done`、effort=low の適用、OpenRouter のモデル一覧 (392 件・約 0.3 秒、effort は medium / low)、
    アダプタの一覧 (464 件)、prompt (pong)。`nemotron-3.5-lightning` (障害前後): system prompt (ZEBRA-7)、cwd での `pwd`/`echo` (承認ゼロ)、`session/load` で合言葉 (PINEAPPLE-42) を復元 (リプレイ + 再モード設定)。
    **`nemotron-3-super` での再実行は system prompt / cwd / session_load が OpenRouter の `429 Too Many Requests` で失敗** (429 は「`exceeded retry limit … 429` というメッセージ + `end_turn`」で返る = §9.6 の見え方の実例)。
    終了後、`codex-acp` / `codex app-server` / `claude-agent-acp` の残存 0、ユーザーの `~/.codex` のデーモンは元の PID のまま。
  - Chrome (`pnpm dev:browser` + 実コア、`docs/e2e/stage7e/`): 設定モーダルに Codex の行が並び、モデルは 392 件から検索できる (`1-codex-openrouter-model-search.jpg`)。「秘密の環境変数」に
    ダミー `YHTYE_TEST_SECRET` を登録 (`2-secret-env-registered.jpg`、`secret-tool` で `service=yhtye` のキーリング項目と DB の名前を確認、値は表示しない) → 削除 (`3-secret-env-deleted.jpg`、キーリング・DB とも 0 件)。
    ブリッジ・Vite は停止済み。
- レビュー (rust-reviewer / security-reviewer) の指摘のうち、秘密が `env_remove` を上書きできる順序・DB とキーリングの不整合・DB 手書きの禁止名・ブリッジのログ・OpenRouter の応答サイズとリダイレクトは対応。
  未対応 (core-design §16 に記載): 全エージェントへの一律注入、stderr の値の伏せ字化、キーリング不可時の全起動失敗、モデル一覧キャッシュがプロバイダ変更に追従しない (最大 10 分)。
- **残課題**: (1) **Codex を含むオーケストレーションの実機実行 (`orchestration_mixed_real` の Codex 実装者 / Codex オーケストレータ) は成功していない**: `nemotron-3.5-lightning` は障害で遅く (実装者のセッションが 25 分以上)、
  `nemotron-3-super` は 429 を返し続けた (実装者が「報告なしのターン」になり `cancel_task`、Codex オーケストレータは 240 秒で無応答)。テストは追加済み・コンパイル済みで、Codex オーケストレータのセッション起動 (`agent-full-access`、モデル検証) までは
  実機で動いた。プロバイダが回復したら `fish -c '… orchestration_mixed_real -- --ignored codex'` で再実行する。(2) 認証・課金・429 の失敗検出 (§9.6)。(3) Codex 0.159 + `~/.codex` ではアダプタ自身の一覧にも OpenRouter が含まれる。
- **2026-09-29 追試 (`stealth/space-bunny-alpha`、テスト定数を一時的に差し替え、コミットはしていない)**:
  `real_mixed_claude_orchestrator_codex_implementer_claude_reviewer` は**通過** (Haiku オーケストレータ + Codex 実装者 + Haiku レビュー → main へマージ)。
  `real_codex_orchestrator_with_a_claude_implementer` は失敗: Codex の既知バグ [openai/codex#13746](https://github.com/openai/codex/issues/13746)
  (MCP ツールスキーマの `$defs` / `$ref` を解決せずモデルに崩れたスキーマを見せる) により、`create_task` の `steps` (`items: {$ref: StepSpec}`) を
  `{"item": {...}}` や文字列で送り 6 回とも拒否された。Yhtye 側でスキーマの `$ref` を展開すれば回避できるが、**メインは Claude のため当面放置 (ユーザー決定)**。
