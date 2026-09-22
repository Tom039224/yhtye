# Yhtye MCP ツール

Yhtye は MCP サーバーをプロセス内でホストし、各 ACP セッションの `session/new` で
`mcpServers` として渡す。エージェントが Yhtye の状態を変える経路はここに定義したツールだけ。
モデル全体は [`orchestration-model.md`](orchestration-model.md)、サーバー実装は
[`core-design.md`](core-design.md) §4。

## 1. 共通

- **トランスポート**: streamable HTTP、`http://127.0.0.1:<port>/mcp/<sessionToken>`。
  1 プロセスで全セッションを捌く。`sessionToken` はセッション作成ごとに Yhtye が発行する
  推測不能なランダム値 (128bit 以上)。トークンから「役割 (orchestrator / implementer /
  reviewer)・プロジェクト・グループ・タスク・Step」を引く。未知のトークンは HTTP 404。
- **役割ごとにツール一覧が異なる。** `tools/list` はトークンの役割に応じた集合だけを返し、
  他役割のツールを呼ばれたら `forbidden` エラー。
- **MCP サーバー名は `yhtye`。** Claude Code 上のツール名は `mcp__yhtye__<tool>` になる。
- 引数は JSON Schema で定義 (Rust 側は `serde` + `schemars` の構造体から生成)。
- 成功時は `structuredContent` に JSON を返し、同じ内容を `text` にも入れる。
- 失敗は MCP のツールエラー (`isError: true`) で返し、本文は
  `{"error": {"code": "<code>", "message": "<人間向け説明>"}}` (`structuredContent` と `text` の両方)。
  LLM が読んで直せるよう message に「何が不正で、どうすれば通るか」を書く。
  **引数がスキーマに合わない場合** (必須欠落・型違い・未知の enum 値・未知のフィールド) も
  `invalid_argument` のツールエラーにする (Stage 2 で確定。JSON-RPC エラーにすると
  クライアントが中身を LLM に見せないことがあるため)。プロトコルエラー (JSON-RPC error) は
  未知のツール名など MCP 層の問題にのみ使う。
- 失効済みトークン (セッション終了後) も未知と同じく HTTP 404。
- MCP 2026-07-28 版のクライアント (Claude Code 2.1.280 以降) 向けに、`tools/list` には
  `ttlMs: 0` / `cacheScope: "private"` を付ける ([`core-design.md`](core-design.md) §4)。
- 全ツール呼び出しはイベントログに記録する (引数・結果・エラー)。
- ツールは状態を変えたら即座に返る。**長時間ブロックするツールは作らない**
  (例: `help` は返答を待たずに返り、返答は後で新しいプロンプトとして届く)。

共通エラーコード:

| code | 意味 |
|---|---|
| `invalid_argument` | 引数の値が不正 (空文字、未知の kind など) |
| `not_found` | ID が存在しない / 他グループのもの |
| `invalid_state` | 現在の状態ではその操作ができない (message に現在状態を含める) |
| `forbidden` | その役割では使えないツール、または自分の担当外のタスク |
| `conflict` | 制約違反 (active グループが既にある、依存の循環など) |
| `internal` | Yhtye 内部エラー (ログを見る) |

## 2. 型

```ts
type StepKind = "implement" | "review" | "checkpoint" | "done";
type TaskKind = "code" | "investigate";
type StepSpec = { kind: StepKind; instruction?: string };
type Verdict = "approve" | "needs_changes";
```

## 3. オーケストレータ用ツール

### `create_group`

| | |
|---|---|
| 引数 | `title: string` (必須), `summary?: string` |
| 戻り値 | `{ group_id, group_branch, base_branch }` |
| 遷移 | 新しい Group を `active` で作成。group ブランチと統合 worktree を作る |
| エラー | `conflict` (プロジェクトに active なグループが既にある), `invalid_state` (メイン作業ツリーが detached HEAD 等で base ブランチを決められない), `invalid_argument` |

### `create_task`

| | |
|---|---|
| 引数 | `group_id: string`, `title: string`, `kind: TaskKind`, `steps: StepSpec[]` (1 個以上), `depends_on?: string[]` (既定 `[]`), `instruction?: string` (空・省略可) |
| 戻り値 | `{ task_id, status, steps: StepSpec[] }` (末尾 `done` 補完後の正規化済み工程) |
| 遷移 | Task を `pending` で作成。依存が全て `done` なら即座に評価し、`instruction` があれば `running` (最初の Step を開始)、無ければ `awaiting_instruction` として受信箱に `instruction_needed` を積む |
| エラー | `not_found` (group / depends_on の ID), `invalid_state` (グループが `active` でない), `conflict` (依存の循環), `invalid_argument` (steps 空、`done` が末尾以外、未知の kind、`investigate` タスクに `review` — 差分が無いため) |

### `set_instruction`

| | |
|---|---|
| 引数 | `task_id`, `instruction: string` (空不可) |
| 戻り値 | `{ task_id, status }` |
| 遷移 | `pending` / `awaiting_instruction`: instruction を設定。`awaiting_instruction` なら `running` へ |
| エラー | `invalid_state` (既に開始済み — 実行中の指示変更は `answer_help` か `modify_steps` を使う), `not_found`, `invalid_argument` |

### `modify_steps`

| | |
|---|---|
| 引数 | `task_id`, `steps: StepSpec[]` — **まだ開始していない Step 全体**をこれで置き換える |
| 戻り値 | `{ task_id, steps: [{ index, kind, status, instruction? }] }` (完了済み含む全体) |
| 遷移 | 状態は変えない。`checkpoint` / `handling` / `awaiting_instruction` / `pending` 中に使う想定。`running` 中でも可 (実行中の Step の後ろが置き換わる) |
| エラー | `invalid_state` (終端状態), `invalid_argument` (空で `done` も補えない等) |

### `resolve_checkpoint`

| | |
|---|---|
| 引数 | `task_id`, `decision: "continue" \| "abort"`, `note?: string` (次の Step の担当へ渡す補足) |
| 戻り値 | `{ task_id, status, current_step }` |
| 遷移 | `checkpoint` → `continue`: 次の Step へ (`running`)。`abort`: `cancelled` (cancel_task と同じ後始末) |
| エラー | `invalid_state` (checkpoint 待ちでない) |

工程を変えてから進めたい場合は `modify_steps` → `resolve_checkpoint(continue)` の順に呼ぶ。

### `answer_help`

| | |
|---|---|
| 引数 | `help_id`, `action: "resume" \| "cancel_task"`, `reply?: string` |
| 戻り値 | `{ help_id, task_id, task_status }` |
| 遷移 | help を `answered` に。`resume`: タスクを `running` に戻す。help を上げたのがエージェントなら `reply` をそのセッションへの新しいプロンプトとして送り、同じ Step を続けさせる。Yhtye 起因 (merge_conflict 等) なら次の未完了 Step から再開 (`modify_steps` で足した Step があればそれ)。`cancel_task`: `cancelled` |
| エラー | `not_found`, `invalid_state` (既に回答済み), `invalid_argument` (エージェント起因で `resume` なのに `reply` が空) |

### `cancel_task`

| | |
|---|---|
| 引数 | `task_id`, `reason: string` |
| 戻り値 | `{ task_id, status: "cancelled" }` |
| 遷移 | 任意の非終端状態 → `cancelled`。ターン中なら ACP cancel、セッション終了、worktree 削除 (ブランチは残す)。このタスクに依存するタスクは自動では消さない — `group_settled` 判定では「依存先が cancelled のため開始できないタスク」を含めてオーケストレータに通知する |
| エラー | `invalid_state` (既に終端), `not_found` |

### `finish_group`

| | |
|---|---|
| 引数 | `group_id`, `summary: string` (ユーザー向けの完了要約。イベントとして保存) |
| 戻り値 | `{ group_id, status, merge: { ok: bool, detail } }` |
| 遷移 | 全タスクが終端のときのみ。`active` → `finishing` → group ブランチを base ブランチへマージ → `done`。マージできない (dirty / 別ブランチ / コンフリクト) なら `merge_blocked` |
| エラー | `invalid_state` (終端でないタスクがある — message に一覧を含める) |

### `cancel_group`

| | |
|---|---|
| 引数 | `group_id`, `reason: string` |
| 戻り値 | `{ group_id, status: "cancelled" }` |
| 遷移 | 全非終端タスクを `cancel_task` 相当で止め、グループを `cancelled`。group ブランチは残し base へはマージしない |

### `get_status`

| | |
|---|---|
| 引数 | `group_id?` (省略時は active グループ) |
| 戻り値 | グループと全タスクの要約: `{ group, tasks: [{ task_id, title, kind, status, depends_on, current_step, steps_summary }], open_helps }` |
| 遷移 | なし |

### `get_task`

| | |
|---|---|
| 引数 | `task_id` |
| 戻り値 | タスクの全 Step (kind / status / instruction / result / verdict)、ブランチ名、worktree パス、未回答 help |
| 遷移 | なし |

## 4. サブエージェント用ツール (implementer / reviewer 共通)

サブエージェントのトークンは特定のタスクと Step に束縛されている。タスク ID を引数に取らない。

### `report_step_done`

| | |
|---|---|
| 引数 | `result: string` (必須。何をしたか / 何が分かったか。オーケストレータ・後続 Step に渡る), `verdict?: Verdict` (reviewer は必須、implementer は指定不可) |
| 戻り値 | `{ ok: true, next: "end_turn" }` — 「ターンを終えてよい」とエージェントに伝える |
| 遷移 | 現在の Step を `done` にし、Step の結果を保存。**次の Step の開始はこのセッションのターン終了 (`session/prompt` の応答) を待ってから** Yhtye が行う。review で `needs_changes` なら [`orchestration-model.md`](orchestration-model.md) §2.3 の自動挿入 |
| エラー | `invalid_state` (トークンの Step が既に完了 / 現在の Step でない), `invalid_argument` (reviewer なのに verdict なし、result 空) |

### `help`

| | |
|---|---|
| 引数 | `kind: "blocked" \| "question" \| "policy"`, `message: string` |
| 戻り値 | `{ help_id, next: "end_turn_and_wait" }` |
| 遷移 | タスクを `handling` にし、受信箱に `help_raised` を積む (オーケストレータを即座に起こす)。オーケストレータの `answer_help` の `reply` が、このセッションへの次のプロンプトになる |
| エラー | `invalid_state` (同じタスクに未回答 help が既にある) |

Yhtye 起因の help の kind: `merge_conflict` / `review_rounds_exhausted` /
`protocol_violation` / `agent_stopped` / `agent_crashed` / `dirty_readonly_tree`。

## 5. 受信箱メッセージ (Yhtye → オーケストレータ)

ツールではないが対になる仕様。オーケストレータへのプロンプトは次の形式のブロックを並べる。

```
[yhtye:<type>] key=value key=value ...
<本文 (任意の複数行)>
```

| type | キー | 本文 | 期待される応答 |
|---|---|---|---|
| `user_message` | — | ユーザーの入力 | 自由 |
| `checkpoint_reached` | `task`, `step` | それまでの Step の結果 | `resolve_checkpoint` (必要なら先に `modify_steps`) |
| `help_raised` | `help_id`, `task`, `kind` | help の message | `answer_help` / `cancel_task` |
| `instruction_needed` | `task` | 依存先タスクの最終結果 | `set_instruction` |
| `group_settled` | `group` | 全タスクの結果要約・開始不能タスク | タスク追加 or `finish_group` / `cancel_group` |
| `merge_result` | `group`, `ok` | base へのマージ結果 | ユーザーへの報告 |
