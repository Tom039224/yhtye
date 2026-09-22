# コア設計 (モジュール構成とインターフェース)

[`orchestration-model.md`](orchestration-model.md) のモデルを実装する Rust / TypeScript の構成。
方針:

- **ドメインと I/O は Tauri から独立したライブラリ crate `yhtye-core` に置く。**
  Tauri アプリはその薄い殻にする。
- **同じコマンド / イベント API を 2 つの口で出す**: Tauri IPC (アプリ) と
  WebSocket (開発用ブリッジ)。素の Chrome には Tauri の `invoke` が無いため、
  ブラウザ上の React を本物のコアにつないで自動操作・E2E するのに WS ブリッジが要る。
- **決定的なテストのために偽 ACP エージェントを持つ。** 実エージェント (Haiku) のテストは
  `#[ignore]` にして Stage の完了条件として回す。

## 1. リポジトリ構成

Cargo workspace をリポジトリ直下に置く (`target/` も直下になる)。

```
Cargo.toml                    [workspace] members = crates/*, src-tauri
crates/
  yhtye-core/                 ライブラリ。Tauri 非依存
    src/
      api/        ApiEvent / Snapshot (3a)、ApiCommand / ApiResponse (serde + ts-rs で TS 型を生成)
      acp/        ACP クライアント: ハーネス起動・セッション・イベント型付け・自動承認
      mcp/        Yhtye MCP サーバー (rmcp + axum, streamable HTTP)
      domain/     状態機械 (純粋関数。I/O なし)
      store/      SQLite (sqlx): イベントログ + 現在状態テーブル, migrations/
      git/        GitService トレイト + NoopGit (3a)、git CLI 実装 (3c)
      prompts.rs  役割別システムプロンプト (本文は crates/yhtye-core/prompts/*.md を include_str!)
      runtime/    上記を束ねる Core (Effect の実行・受信箱・再開処理)
    tests/        偽エージェントを使う結合テスト, 実エージェントテスト (#[ignore])
  yhtye-fake-agent/           偽 ACP エージェント (bin)
  yhtye-dev-bridge/           WS ブリッジ (bin)
src-tauri/                    Tauri アプリ (Core を manage し、コマンドとイベントを中継するだけ)
src/                          React フロントエンド
  api/          transport.ts (抽象) / tauri.ts / ws.ts / generated/ (ts-rs 出力)
  store/        イベント駆動の状態ストア
  ...
```

## 2. 公開 API (`yhtye_core::api` / `runtime::Core`)

```rust
pub struct Core { /* Arc 内部 */ }

impl Core {
    pub async fn start(cfg: CoreConfig) -> Result<Core>;           // DB を開き, MCP サーバーを起動し, 中断タスクを再開
    pub async fn command(&self, cmd: ApiCommand) -> Result<ApiResponse, ApiError>;
    pub fn subscribe(&self) -> broadcast::Receiver<ApiEvent>;      // seq 付き
    pub async fn snapshot(&self, project: ProjectId) -> Result<Snapshot>; // seq を含む
    pub async fn shutdown(self);                                     // 全エージェントを止めてプロセスを回収
}

pub struct CoreConfig {
    pub data_dir: PathBuf,                   // SQLite, worktrees, ログ
    pub harnesses: HashMap<String, HarnessConfig>,
    pub roles: RoleHarnessMap,               // orchestrator / implementer / reviewer → harness 名
    pub mcp_bind: SocketAddr,                // 既定 127.0.0.1:0
}
```

- `ApiCommand` (例): `OpenProject{path}` / `SendUserMessage{project, text}` /
  `CancelOrchestratorTurn{project}` / `CancelTask{task}` / `RetryGroupMerge{group}` /
  `GetSnapshot{project}` / `ListEvents{project, after_seq, limit}`。
- `ApiEvent` = `{ seq, ts_ms, project, body }` (Stage 3a で `yhtye_core::api` に定義)。
  `body: ApiEventBody` (serde `type` タグ付き) は次のいずれか:
  - `domain { event: DomainEvent }` — 状態の変化 (§5)。UI は `State::apply` と同じ規則で畳み込める。
  - `session_started { session, role, task, pid }` / `session_failed { session, error }` /
    `session_stopped { session }`
  - `agent { session, event: AgentEvent }` — エージェントの出力とライフサイクル
    (`Output(MessageChunk|ThoughtChunk|ToolCall|…)`, `TurnEnded`, `Exited` …)。`Stderr` は流さない。
  - `tool_called { record: ToolCallRecord }` / `prompted { session, text }`
  - `seq` は 1 から欠番なし。`Snapshot = { seq, state: State }` (`Orchestration::snapshot`)。
  - ts-rs による TS 型生成は Stage 4 (フロントが使い始めるとき) に回す。`AgentEvent` は
    ACP スキーマの型を含むので、その部分は `unknown` 相当で出す必要がある。
- **クライアント同期の規則**: 接続時に `snapshot` (その時点の `seq` を含む) を取り、
  以降 `seq > snapshot.seq` のイベントだけ適用する。取りこぼし (seq の飛び) を
  検知したら snapshot を取り直す。
- Rust の API 型から ts-rs で `src/api/generated/*.ts` を生成し、手書きの重複定義をしない。

## 3. `acp` モジュール

### 3.1 セッションアクター

エージェントプロセス 1 つ = ACP 接続 1 つ = tokio タスク 1 つ。
Yhtye は 1 プロセスにつき 1 セッションだけを作る (プロセス単位で止められるように)。
(Stage 1 で実装。実装に合わせて更新済み — 当初案との差分は §3.5。)

```rust
pub struct SpawnOptions {
    pub mcp_servers: Vec<McpServer>,
    pub resume: Option<SessionId>,          // session/load で復元
    pub system_prompt: Option<String>,
}

pub async fn spawn_agent(
    harness: &HarnessConfig, cwd: &Path, options: SpawnOptions,
    events: mpsc::UnboundedSender<AgentEvent>,
) -> Result<AgentHandle, AgentError>;       // Ready まで進んでから返る。失敗は段・終了コード・stderr 末尾付きの AgentError

impl AgentHandle {
    pub fn info(&self) -> &AgentInfo;                                   // session id / modes / config_options / capabilities
    pub fn pid(&self) -> Option<u32>;                                   // プロセスグループのリーダー
    pub fn prompt(&self, blocks: Vec<ContentBlock>) -> Result<(), AgentError>; // 応答は AgentEvent::TurnEnded。ターン中は Busy
    pub fn prompt_text(&self, text: impl Into<String>) -> Result<(), AgentError>;
    pub fn cancel(&self) -> Result<(), AgentError>;                    // session/cancel 通知。ターン中でも即送る
    pub fn is_turn_running(&self) -> bool;
    pub async fn set_mode(&self, mode) -> Result<(), AgentError>;
    pub async fn set_config_option(&self, id, value) -> Result<Vec<SessionConfigOption>, AgentError>;
    pub async fn shutdown(self);                                        // Exited 送出まで待つ
}
// drop しても終了処理が走る (コマンドチャネルが閉じる → Shutdown と同じ)。
```

- 内部は `Client.builder()...connect_with(ByteStreams(stdin, stdout), |cx| loop { cmd_rx ... })`
  ([`acp-harnesses.md`](acp-harnesses.md) §4.4)。`Prompt` は `cx.spawn` に切り出し、
  **コマンドループは prompt の応答を待たない**。これで `Cancel` がターン中に届く。
  `set_mode` / `set_config_option` も `cx.spawn` で送り、結果を oneshot で返す。
- 同時に実行中のターンは 1 つまで。ターン中の `prompt` は `AgentError::Busy` を返す。
  判定はハンドル側で同期的に行う (`watch` の check-and-set。アクターへの往復なし)。
  受信箱のまとめ送りは runtime の責務。
- `spawn_agent` の手順: プロセス起動 → `initialize` (fs / terminal は広告しない) →
  `session/load` (resume 指定かつ `load_session` 対応時。非対応なら `session/new` に
  フォールバックし `AgentInfo.resumed = false`) または `session/new`
  (`mcp_servers`, `_meta.systemPrompt.append`) → `mode_after_new` があれば `set_mode`
  (`modes` に無ければ `Unsupported`) → `model` 指定があれば `set_config_option`
  (応答の現在値が要求値と違えば起動失敗)。各段に `HarnessConfig::startup_timeout`。
- **プロセス管理**: `tokio::process` で `process_group(0)` として起動し、プロセスグループ
  単位で止める (`npx` → `node` → `claude` のような多段起動でも孤児を残さない)。
- **終了処理**: `Shutdown` (または handle の drop) → ターン中なら `Cancel` し最大 10 秒
  `TurnEnded` を待つ → 接続クロージャを抜ける (stdin が閉じる) → 1 秒待って出なければ
  グループに SIGTERM → 2 秒後 SIGKILL → 最後にグループ全体へ SIGKILL (残党掃除)。
  プロセスの exit を `AgentEvent::Exited{code, signal}` で必ず最後に通知する。
  アクタータスクごと落ちた場合もガードの drop でグループを SIGKILL する。
  stderr は行ごとに `AgentEvent::Stderr` (ログのみ、UI には出さない) + 起動エラー用に末尾 40 行を保持。

### 3.2 型付きイベント

```rust
pub enum AgentEvent {
    Ready(Box<AgentInfo>),             // { acp_session_id, resumed, modes, config_options, capabilities, agent }
    Output(AgentOutput),               // SessionUpdate を 1:1 で型付け。未知は Unknown(serde_json::Value)
    PermissionAutoAnswered { tool_call, options, chosen: Option<PermissionOptionKind> },
    TurnEnded(Result<StopReason, AgentError>),
    Stderr(String),
    Exited { code: Option<i32>, signal: Option<i32> },   // 常に最後
}
pub enum AgentOutput {
    UserMessageChunk, MessageChunk, ThoughtChunk, ToolCall, ToolCallUpdate, Plan,
    AvailableCommands, ModeChanged, ConfigOptions, SessionInfo, Usage, Unknown(serde_json::Value),
}
```

- `session/update` は生 JSON で受けてから `SessionUpdate` に変換する (変換できない種類を
  落とさず `Unknown` にするため)。全イベントは `Serialize` (UI 転送用)。
- 順序: `session/load` の履歴再生は `Ready` より**前**の `Output` として届く
  (実 Claude Code ではユーザー・エージェント両方のチャンクが再生される)。
  1 ターンの `Output` はその `TurnEnded` より前。`Exited` は常に最後。
- `TurnEnded` を出すのはターンを終わらせた 1 者だけ (prompt 応答 / プロセス終了時の
  `Err(Closed)`) — `watch` の true→false 遷移で調停する。

### 3.3 権限の自動承認

`fn choose_permission(options: &[PermissionOption]) -> Option<&PermissionOption>` と
`fn outcome_for(Option<&PermissionOption>) -> RequestPermissionOutcome` は純粋関数。
`allow_always` → `allow_once` の順に kind で選び、どちらも無ければ `Cancelled`。
**配列の先頭を選ぶことはしない。** ハンドラ内で即応答する (ブロックしない)。

### 3.4 `HarnessConfig`

```rust
pub struct HarnessConfig {
    pub command: String, pub args: Vec<String>, pub env: BTreeMap<String, String>,
    pub mode_after_new: Option<String>,        // Claude Code: "bypassPermissions"
    pub model: Option<ModelSelect>,            // { config_id: "model", value: "haiku" }
    pub system_prompt: SystemPromptStyle,      // MetaAppend | FirstPrompt
    pub session_meta: Option<Map<String, Value>>, // session/new・load の _meta に足すハーネス固有フィールド (Stage 2)
    pub startup_timeout: Duration,             // 既定 120 秒 (npx の初回ダウンロードを見込む)
}
// HarnessConfig::claude_code("haiku") が Claude Code 用の既定値、
// HarnessConfig::claude_code_orchestrator("haiku") がオーケストレータ用 (組み込みツールを読み取り系に限定)。
```

ハーネス固有の知識はここだけ。コアのコードに `"claude"` 等の分岐を書かない。

### 3.5 Stage 1 で当初案から変えた点 (理由)

| 当初案 | 実装 | 理由 |
|---|---|---|
| `AcpAgent` にプロセス起動と drop 時の kill を任せる | `tokio::process` + 自前のプロセスグループ管理 (`ByteStreams` で接続) | `AcpAgent` は終了コードを返さず (`Exited{status}` を出せない)、stderr も行イベントにしにくい。起動失敗メッセージに終了コードと stderr 末尾を含めるため |
| `events: mpsc::Sender<AgentEvent>` | `mpsc::UnboundedSender` | 有界だとイベント送信で dispatch ループが止まり、利用側が `set_mode` の応答を待っていると相互待ちになる |
| `AgentCmd` を公開し `handle.send(cmd)` | `AgentHandle` のメソッド (`AgentCmd` は内部) | 応答が要るコマンド (`set_mode` 等) に oneshot を持たせるため。Busy はハンドルで即判定 |
| `choose_permission -> RequestPermissionOutcome` | `-> Option<&PermissionOption>` + `outcome_for` | 選んだ kind をイベントに載せるため |
| `Exited { status }` | `Exited { code, signal }` | シグナル終了を区別するため |

## 4. `mcp` モジュール

- rmcp **3.4** (`features = ["server", "macros", "transport-streamable-http-server"]`) を
  axum 0.8 に `route_service("/mcp/{token}", StreamableHttpService)` でマウントし、
  `route_layer` のミドルウェアで `Path(token)` を request extensions に入れる。
  ツール側は `Extension<http::request::Parts>` から token を読む
  (rmcp `transport/streamable_http_server/tower.rs:990-1046, 1254`)。
  rmcp は API 変化が速い (9 月だけで 3.2→3.4) のでマイナーまで固定する。
- 127.0.0.1 のみに bind (ポートは OS 任せ)。rmcp 既定の `allowed_hosts` (localhost 限定) を維持。
- **ステートレスモード** (`legacy_session_mode = false`, `json_response = true`) で動かす。
  MCP セッション状態を持たないので `keep_alive` (既定 300 秒) の失効問題自体が無い。
  **識別は毎リクエストの token で行い、MCP セッション状態に依存しない。**
  トークンの検査は axum のミドルウェアで行い、未知・失効済みトークンは rmcp に渡す前に 404。
- **Claude Code 2.1.280 は MCP 2026-07-28 版 ("modern" プロトコル) を話す。** この版では
  `tools/list` の結果に `ttlMs` / `cacheScope` が必須で、無いと Claude Code は
  `INVALID_RESULT` として一覧を捨てる (ツールが 1 つも見えない)。rmcp 3.4 はこれらを
  任意扱いにしているので、Yhtye は `ttl_ms(0)` + `CacheScope::Private` を明示する
  (一覧はトークンごとに異なるため private・キャッシュなし)。回帰テスト
  `tests/mcp_server.rs::modern_protocol_tool_list_has_cache_hints`。
- `TokenRegistry: token → SessionBinding { session, role, project, group?, task?, step? }`。
  セッション終了時に失効させる。同じ implementer セッションで次の Step に進むときは
  `rebind` で束縛先の Step を差し替える (トークンは URL に埋め込まれていて変えられないため)。
- 全ツール呼び出しは `ToolCallRecord { binding, tool, args, result }` として observer
  チャネルに流す (Stage 3 でイベントログに保存する)。
- ツールハンドラは直接状態を触らず、`ToolPort` トレイト経由で runtime に
  `DomainCommand` を送り、結果を待って返す:

```rust
#[async_trait]
pub trait ToolPort: Send + Sync {
    async fn call(&self, binding: SessionBinding, call: ToolCall) -> Result<serde_json::Value, ToolError>;
}
```

  これにより MCP 層はテストで `ToolPort` を差し替えて単体検証できる。
- MCP 層の責務は「役割チェック (`forbidden`) と引数のデコード」まで。引数のデコード失敗
  (必須欠落・型違い・未知の enum 値・未知のフィールド) は `invalid_argument` の
  **ツールエラー**で返す (LLM が読んで直せるように)。JSON-RPC エラーは未知のツール名と
  トークン失効 (404) だけ。値の検査 (空文字、Step 列の規則、verdict と役割) は ToolPort 側。

### 4.1 runtime (Stage 3a)

Stage 2 の仮実装 (`Board` / `MemoryToolPort`) は Stage 3a で本物の状態機械 (§5) に置き換えた。

- `runtime::Orchestration` — 1 プロジェクト分の公開 API: `start(cfg)` →
  `(Orchestration, UnboundedReceiver<ApiEvent>)`、`send_user_message` / `cancel_orchestrator_turn` /
  `snapshot` / `shutdown`。`OrchestrationConfig` に `domain: DomainConfig` (`max_review_rounds`) と
  `git: Arc<dyn GitService>` を持つ。複数プロジェクト・DB を束ねる `Core` は 3b 以降。
- `runtime::driver` — ループ本体 (§8)。`runtime::port::LoopPort` が `ToolPort` を実装し、
  ツール呼び出しを oneshot 付きでループへ送る (ループが処理して応答する)。
- `runtime::sessions` — セッションの起動 (裏で `JoinSet`)・プロンプトのキュー・トークンの
  `rebind` / `revoke`・停止。セッションキーは `orchestrator` / `T-n/implementer` (タスク中は同じ
  セッション) / `T-n/review-<step>` (毎回新規)。起動中に止められたセッションは起動完了時に即停止する。
- `runtime::emitter` — `ApiEvent` に `seq` / `ts_ms` を付けて送る。
- サブエージェントの cwd は `WorkspaceReady` で記録された作業場所 (`NoopGit` ではプロジェクト
  ディレクトリ)。

## 5. `domain` モジュール (状態機械)

I/O を持たない純粋な状態遷移。すべての変化はここを通る (Stage 3a で実装)。

```rust
pub fn decide(state: &State, cmd: DomainCommand) -> Result<(State, Transition), ToolError>;
// Machine { state } は decide の結果で state を置き換えるだけの薄い包み。エラー時は何も変えない。

impl State { pub fn apply(&mut self, event: &DomainEvent); }   // 純粋な reducer。失敗しない

pub struct Transition {
    pub events: Vec<DomainEvent>,                     // 永続化・UI 配信される事実
    pub effects: Vec<Effect>,                         // runtime が実行する副作用の要求
    pub reply: Option<Result<Value, ToolError>>,      // ツール呼び出しへの戻り値
}

pub enum DomainCommand {
    CreateGroup { args, base_branch: Option<String> },   // runtime が git から base を読んで渡す
    Tool { binding: SessionBinding, call: ToolCall },    // create_group 以外の全ツール
    UserMessage { text },
    TurnEnded { agent: AgentRef, outcome: TurnOutcome,  // EndTurn / Cancelled / Stopped(reason) / Closed / Error
                prompt_queued: bool },                   // 次のプロンプト (ターン中に届いた help の返答) が待機中
    AgentExited { agent: AgentRef, detail },             // Yhtye が止めていないのに終了した
    AgentStartFailed { agent: AgentRef, error },
    GitDone { op: GitOp, result: GitResult },
    InboxDelivered { up_to: u64 },
}

pub enum Effect {
    RunStep { agent: AgentRef, group, prompt, workdir },  // 無ければセッションを起動してから送る
    PromptAgent { agent: AgentRef, text },                // help の返答・催促
    StopAgent { agent: AgentRef },                        // レビュー済みのレビュアー
    StopTaskAgents { task },                              // タスク終端
    Git(GitOp),     // CreateGroupBranch / PrepareWorkspace / FinishTask / RemoveWorkspace / MergeGroup
    WakeOrchestrator,
}
// AgentRef { task, role, step } — ステップを担当するセッションの論理名
```

- **イベントソーシング向けの構造**: 決定ロジックは状態を直接書き換えない。作業用コピーに
  `DomainEvent` を発行して `State::apply` で適用しながら判断を進める (`machine.rs` の `Tx`)。
  したがって「旧状態にその遷移のイベントを順に適用すると新状態になる」が常に成り立つ
  (単体テストの全コマンドでこれを検査している)。3b ではイベントを追記し、現在状態テーブルは
  同じイベントから更新すればよい。
- `DomainEvent` (serde `type` タグ付き): `group_created` / `group_finishing` /
  `group_merge_finished` / `group_cancelled` / `task_created` / `instruction_set` /
  `task_status_changed` / `task_cancelled` / `workspace_ready` / `step_started` /
  `step_completed` / `step_reset` / `steps_replaced` / `review_steps_inserted` / `nudge_sent` /
  `help_raised` / `help_answered` / `help_closed` / `help_agent_lost` / `inbox_queued` /
  `inbox_delivered`。
- `State` は `groups` / `tasks` (steps を含む) / `helps` / `inbox` (未配達分) / ID カウンタ /
  `DomainConfig`。すべて `Serialize + Deserialize`。受信箱もドメイン状態の一部 (3b で永続化)。
- 時刻は状態機械に入れない (`now` 引数は無し)。時刻は `ApiEvent.ts_ms` で runtime が付ける。
- ファイル構成: `state.rs` (データ) / `event.rs` (イベントと reducer) / `command.rs` /
  `machine.rs` (`decide`・`Tx`) / `flow.rs` (タスク・ステップの開始/完了/中止、help、依存解消、
  group_settled) / `tools_orch.rs` / `tools_sub.rs` / `agent_rules.rs` (ターン終了・異常終了) /
  `git_rules.rs` (git 結果)。Step kind の追加は `flow.rs::start_step` と
  `agent_rules.rs::turn_ended` に閉じる。
- 規則 (「active グループは 1 つ」「`done` は末尾」「review の自動再挿入は N 回まで」など) は
  すべてここに置き、単体テスト (`domain/tests/`: 遷移表・フロー・エラー・git・エージェント) で網羅する。

## 6. `store` モジュール

- sqlx (`sqlite`, `runtime-tokio`, `migrate`)。コンパイル時に DB を要する `query!` マクロは
  使わず `query_as` + `FromRow` にする (ビルドを単純に保つ)。WAL モード。
- テーブル: `projects` / `groups` / `tasks` / `task_deps` / `steps` / `agent_sessions`
  (role, task, harness, acp_session_id, mcp_token, status) / `helps` / `inbox` /
  `events (seq INTEGER PRIMARY KEY, ts, project_id, kind, payload JSON)`。
- `Transition` 1 回 = 1 トランザクション: イベント追記 + 現在状態テーブル更新。
  コミット後に `ApiEvent` を broadcast する (DB と UI の順序が一致する)。
- 起動時は現在状態テーブルから `State` を組み立てる (イベントの再生はしない。
  イベントログは履歴表示・デバッグ用)。

## 7. `git` モジュール

状態機械からは `GitOp` (§5) として要求され、runtime が `GitService` トレイト経由で実行する
(Stage 3a で定義。3a の実装は何もしない `NoopGit` — 作業場所はプロジェクトディレクトリ、
マージは常に成功)。

```rust
#[async_trait]
pub trait GitService: Send + Sync + 'static {
    async fn current_branch(&self) -> Result<Option<String>, String>;   // None = detached HEAD
    async fn run(&self, op: &GitOp) -> GitResult;
    // CreateGroupBranch / RemoveWorkspace → Done, PrepareWorkspace → Workspace{path},
    // FinishTask → Merged / Conflict{files} / Dirty{files}, MergeGroup → Merged / Blocked,
    // それ以外の失敗 → Failed{message}
}
```

3c では `git` CLI をサブプロセスで実行する実装を足す。内部の操作は `current_branch`, `is_clean`,
`create_branch`, `worktree_add`, `worktree_remove`, `commit_all`, `merge_no_ff`
(コンフリクト時は `merge --abort` して `Conflict{files}` を返す), `diff_stat`。
テストは `tempfile` 上の実リポジトリで行う。

## 8. `runtime` モジュール

- 単一のイベントループ (tokio タスク) が `DomainCommand` を受けて
  `domain::decide` → (3b: `store` に永続化) → `ApiEvent` 配信 → `Effect` 実行 を**直列に**行う。
  状態の同時更新を排除する。
- エージェントの起動・停止は別タスク (`JoinSet`) で行い、失敗は `AgentStartFailed` として戻す。
- **git の Effect はループ内で await し**、結果を `GitDone` として同じ連鎖の中で続けて処理する
  (3a で決定)。ツール呼び出しへの応答は連鎖全体が終わってから返し、連鎖の中で最後に
  `reply` を持った遷移の値を使う。これで `finish_group` はマージ結果込みで同期的に応答でき、
  `create_group` もブランチ作成失敗をエラーとして返せる。git 操作は短い前提 (ネットワークなし)。
- ツール呼び出しは MCP ハンドラから `LoopPort` 経由でループに送られ、ループが処理してから
  応答する。応答を受けたエージェントのターン終了はその後にしか来ないので、
  「report_step_done の効果 → ターン終了」の順序が保証される。
- オーケストレータの受信箱: アイドル時にまとめて 1 プロンプトにする
  ([`orchestration-model.md`](orchestration-model.md) §5)。
- ターン終了時の検査 (`report_step_done` / `help` が呼ばれたか) は runtime の記録ではなく
  ドメイン状態で判定する (3a で変更): ステップが `done` になっていれば報告済み、タスクが
  `handling` なら help 済み。どちらでもなく `end_turn` なら催促 (`nudge_sent`、1 回まで)、
  2 回目は `protocol_violation`。`max_tokens` 等は `agent_stopped`。`cancelled` と
  `Closed` (プロセス終了。`AgentExited` が続く) は何もしない。
- ACP: ターン中にプロセスが死ぬと `session/prompt` が `incoming_transport_closed` で失敗する。
  これを `AgentError::Closed` として扱う (§3.2 の「プロセス終了時は `Err(Closed)`」に揃えた。3a で修正)。

## 9. Tauri アプリ層 (`src-tauri`)

- `Core::start` を `setup` で呼び `manage` する。コマンドは
  `#[tauri::command] async fn yhtye_command(cmd: ApiCommand) -> Result<ApiResponse, ApiError>`
  の 1 本だけ。`subscribe` した `ApiEvent` を `app.emit("yhtye://event", ev)` で流す。
- ウィンドウ終了時に `Core::shutdown` を await してエージェントプロセスを残さない。

## 10. 開発用 WS ブリッジ (`yhtye-dev-bridge`)

- `yhtye-dev-bridge --data-dir <dir> --port 1421` で Core を起動し、`ws://127.0.0.1:1421/ws` を公開。
- メッセージ: 要求 `{ "id": n, "cmd": ApiCommand }` → 応答 `{ "id": n, "ok": ApiResponse }` /
  `{ "id": n, "err": ApiError }`。イベントは `{ "event": ApiEvent }` をプッシュ。
- 127.0.0.1 のみに bind。ブラウザ上の他サイトから叩かれないよう、`Origin` を
  `http://localhost:1420` / `http://127.0.0.1:1420` に限定し、起動時に表示する
  ランダムトークンを `?token=` で要求する (Vite には env で渡す)。
- 開発・E2E 専用。配布物には含めない。

## 11. フロントエンドの transport 抽象

```ts
export interface Transport {
  command(cmd: ApiCommand): Promise<ApiResponse>;
  subscribe(onEvent: (ev: ApiEvent) => void): () => void;
}
// createTransport(): Tauri 内 (window.__TAURI_INTERNALS__ がある) なら TauriTransport,
// それ以外は WsTransport (VITE_YHTYE_BRIDGE_URL, VITE_YHTYE_BRIDGE_TOKEN)。
```

- 状態ストアは `snapshot` + イベントの畳み込み (reducer) のみで作る。
  **実データの経路にモック / フィクスチャは使わない。** 未実装の領域は空状態を出す。
- reducer は Vitest でイベント列を流し込んで検証する (テスト用の入力としてのイベント列は可)。

## 12. 偽 ACP エージェント (`yhtye-fake-agent`)

- `agent-client-protocol` の `Agent.builder()` で ACP のエージェント側を実装した bin。
- 環境変数 `YHTYE_FAKE_SCRIPT=<json>` のシナリオに従って動く (書式は
  `crates/yhtye-fake-agent/src/scenario.rs` の doc コメント)。シナリオは
  「プロンプト (`match` の部分一致 or 順番) → 動作列」で、動作は:
  `message` / `thought` / `tool_call{id,title}` / `tool_call_update{id,status}` / `plan([..])` /
  `update(<任意の update JSON>)` (未知の種類も送れる) / `request_permission([{id,kind}])`
  (結果を `permission:selected:<id>` / `permission:cancelled` というメッセージで返す) /
  `sleep(ms)` (キャンセルで中断) / `wait_cancel` / `end(stop_reason)` / `crash(code)` /
  `spawn_child` (同じプロセスグループに `sleep 600` を起こす。後始末の検証用) /
  `report_state` (mode・model・systemPrompt・受け取った prompt を返す) / `report_meta`
  (`session/new` の `_meta` 全体を返す) / `write_file{path,text}` /
  `mcp_call{tool, args}` (Stage 2: rmcp クライアントで `session/new` に渡された最初の HTTP
  MCP サーバーを呼び、`mcp:<tool>:ok|error:<json>` / `mcp:<tool>:failed:<理由>` を返す。
  `args` 中の `${name}` は、プロンプト中の `key=value` (例: 受信箱の `help_id=H-1`) と
  それまでの成功結果のトップレベル値 (例: `group_id`) で置換) / `mcp_list`
  (`mcp:tools:<名前,...>`)。
- 起動段の異常系: `fail_at` (JSON-RPC エラー) / `exit_at` (stderr に書いて exit(2)) /
  `hang_at` (応答しない) に段名 (`initialize` / `session/new` / ...) を指定する。
- `initialize` で `load_session` (シナリオで切替可) と `mcp_capabilities.http: true` を広告する。
  `session/load` では履歴として `user_message_chunk` を 1 つ再生する。
  `set_mode` / `set_config_option` を記録し、未知の値はエラーにする。
- テストは `tests/common` のヘルパが `cargo build -p yhtye-fake-agent` を 1 回実行して
  `target/debug/yhtye-fake-agent` を使う (`CARGO_TARGET_DIR` 対応)。
- これで ACP・MCP・状態機械・git を含む全経路を LLM なしで決定的にテストする。

## 13. テスト方針

| 種類 | 実行 | 内容 |
|---|---|---|
| 単体 | `cargo test` | domain の遷移表、choose_permission、git (一時リポジトリ)、store |
| 結合 (偽エージェント) | `cargo test` | ACP ストリーミング・ターン中キャンセル・MCP 経由のタスク生成・グループ完走 |
| 実エージェント | `cargo test -- --ignored` | Claude Code を **必ず Haiku** で。テストヘルパが `ANTHROPIC_MODEL=haiku` を設定し、`Ready` の config_options で現在モデルが haiku であることを表明してから始める |
| フロント | `pnpm test` (Vitest) | reducer にイベント列を流す、transport の契約 |
| E2E | Chrome + WS ブリッジ + 実 Haiku | 依頼 → グループ完了まで |
