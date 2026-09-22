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
      api/        ApiCommand / ApiResponse / ApiEvent (serde + ts-rs で TS 型を生成)
      acp/        ACP クライアント: ハーネス起動・セッション・イベント型付け・自動承認
      mcp/        Yhtye MCP サーバー (rmcp + axum, streamable HTTP)
      domain/     状態機械 (純粋関数。I/O なし)
      store/      SQLite (sqlx): イベントログ + 現在状態テーブル, migrations/
      git/        worktree / branch / merge (git CLI をサブプロセスで)
      prompts/    役割別システムプロンプトのテンプレート
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
- `ApiEvent` = `{ seq, ts, project, body }`。`body` はドメインイベント
  (`GroupCreated`, `TaskStatusChanged`, `StepStarted`, `HelpRaised`, `MergeCompleted`, …) と
  エージェントのストリーム (`AgentOutput{session, kind: MessageChunk|ThoughtChunk|ToolCall|
  ToolCallUpdate|Plan|Usage|ModeChanged|…}`, `TurnEnded{session, stop_reason}`)。
- **クライアント同期の規則**: 接続時に `snapshot` (その時点の `seq` を含む) を取り、
  以降 `seq > snapshot.seq` のイベントだけ適用する。取りこぼし (seq の飛び) を
  検知したら snapshot を取り直す。
- Rust の API 型から ts-rs で `src/api/generated/*.ts` を生成し、手書きの重複定義をしない。

## 3. `acp` モジュール

### 3.1 セッションアクター

エージェントプロセス 1 つ = ACP 接続 1 つ = tokio タスク 1 つ。
Yhtye は 1 プロセスにつき 1 セッションだけを作る (プロセス単位で止められるように)。

```rust
pub struct AgentHandle { cmd_tx: mpsc::Sender<AgentCmd>, pub info: AgentInfo }

pub enum AgentCmd {
    Prompt { blocks: Vec<ContentBlock> },    // 応答は AgentEvent::TurnEnded で返る
    Cancel,                                  // session/cancel 通知。ターン中でも即送れる
    SetMode(String),
    SetConfigOption { id: String, value: String },
    Shutdown,
}

pub async fn spawn_agent(
    harness: &HarnessConfig, cwd: &Path, mcp: Vec<McpServer>,
    resume: Option<AcpSessionId>, system_prompt: Option<String>,
    events: mpsc::Sender<AgentEvent>,
) -> Result<AgentHandle>;
```

- 内部は `Client.builder()...connect_with(AcpAgent, |cx| loop { cmd_rx ... })`
  ([`acp-harnesses.md`](acp-harnesses.md) §4.4)。`Prompt` は `cx.spawn` に切り出し、
  **コマンドループは prompt の応答を待たない**。これで `Cancel` がターン中に届く。
- 同時に実行中のターンは 1 つまで。ターン中の `Prompt` は `AgentError::Busy` を返す
  (受信箱のまとめ送りは runtime の責務)。
- `spawn_agent` の手順: プロセス起動 → `initialize` (fs / terminal は広告しない) →
  `session/load` (resume 指定かつ `load_session` 対応時) または `session/new`
  (`mcp_servers`, `_meta.systemPrompt.append`) → `mode_after_new` があれば `set_mode` →
  `model` 指定があれば `set_config_option`。各段にタイムアウト。
- **終了処理**: `Shutdown` → ターン中なら `Cancel` し最大 10 秒 `TurnEnded` を待つ →
  接続クロージャを抜ける → `AcpAgent` の drop でプロセスグループを kill。
  プロセスの exit を `AgentEvent::Exited{status}` で必ず通知する。stderr は行ごとに
  `AgentEvent::Stderr` (ログのみ、UI には出さない)。

### 3.2 型付きイベント

```rust
pub enum AgentEvent {
    Ready { acp_session_id, modes, config_options, capabilities },
    Output(AgentOutput),               // SessionUpdate を 1:1 で型付け。未知は Unknown(serde_json::Value)
    PermissionAutoAnswered { tool_call, chosen: Option<PermissionOptionKind> },
    TurnEnded(Result<StopReason, AgentError>),
    Stderr(String),
    Exited { status: Option<i32> },
}
```

### 3.3 権限の自動承認

`fn choose_permission(options: &[PermissionOption]) -> RequestPermissionOutcome` は純粋関数。
`allow_always` → `allow_once` の順に kind で選び、どちらも無ければ `Cancelled`。
**配列の先頭を選ぶことはしない。** ハンドラ内で即応答する (ブロックしない)。

### 3.4 `HarnessConfig`

```rust
pub struct HarnessConfig {
    pub command: String, pub args: Vec<String>, pub env: BTreeMap<String, String>,
    pub mode_after_new: Option<String>,        // Claude Code: "bypassPermissions"
    pub model: Option<ModelSelect>,            // { config_id: "model", value: "haiku" }
    pub system_prompt: SystemPromptStyle,      // MetaAppend | FirstPrompt
}
```

ハーネス固有の知識はここだけ。コアのコードに `"claude"` 等の分岐を書かない。

## 4. `mcp` モジュール

- rmcp **3.4** (`features = ["server", "macros", "transport-streamable-http-server"]`) を
  axum 0.8 に `route_service("/mcp/{token}", StreamableHttpService)` でマウントし、
  `route_layer` のミドルウェアで `Path(token)` を request extensions に入れる。
  ツール側は `Extension<http::request::Parts>` から token を読む
  (rmcp `transport/streamable_http_server/tower.rs:990-1046, 1254`)。
  rmcp は API 変化が速い (9 月だけで 3.2→3.4) のでマイナーまで固定する。
- 127.0.0.1 のみに bind (ポートは OS 任せ)。rmcp 既定の `allowed_hosts` (localhost 限定) を維持。
- `LocalSessionManager` の `keep_alive` 既定 300 秒は長時間のエージェントセッションに短いので
  無効化 (None) か十分長くする。新しいプロトコル版はステートレス扱いになるため、
  **識別は毎リクエストの token で行い、MCP セッション状態に依存しない。**
- `TokenRegistry: token → SessionBinding { role, project, group?, task?, step? }`。
  セッション終了時に失効させる。
- ツールハンドラは直接状態を触らず、`ToolPort` トレイト経由で runtime に
  `DomainCommand` を送り、結果を待って返す:

```rust
#[async_trait]
pub trait ToolPort: Send + Sync {
    async fn call(&self, binding: SessionBinding, call: ToolCall) -> Result<serde_json::Value, ToolError>;
}
```

  これにより MCP 層はテストで `ToolPort` を差し替えて単体検証できる。

## 5. `domain` モジュール (状態機械)

I/O を持たない純粋な状態遷移。すべての変化はここを通る。

```rust
pub fn apply(state: &State, cmd: DomainCommand, now: Timestamp)
    -> Result<Transition, DomainError>;

pub struct Transition {
    pub events: Vec<DomainEvent>,   // 永続化・UI 配信される事実
    pub effects: Vec<Effect>,       // runtime が実行する副作用の要求
    pub reply: serde_json::Value,   // ツール呼び出しへの戻り値
}

pub enum DomainCommand {
    Tool { binding: SessionBinding, call: ToolCall },          // MCP から
    UserMessage { project, text },
    TurnEnded { session, stop_reason, tools_called: bool },    // ACP から
    AgentExited { session },
    GitResult { op_id, result },                               // git から
    Resume,                                                     // 起動時
    ...
}

pub enum Effect {
    StartAgent { session, role, cwd, resume },
    Prompt { session, text },
    Cancel { session },
    StopAgent { session },
    Git(GitOp),     // CreateGroupBranch / CreateTaskWorktree / CommitAll / MergeTask / MergeGroup / RemoveWorktree / CheckClean
    WakeOrchestrator,
}
```

- Step kind の追加は `StepKind` の列挙と `on_step_start` / `on_step_done` の 2 関数に閉じる。
- 規則 (「active グループは 1 つ」「`done` は末尾」「review の自動再挿入は N 回まで」など) は
  すべてここに置き、テーブル駆動の単体テストで網羅する。

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

`git` CLI をサブプロセスで実行する `GitService`。
操作: `current_branch`, `is_clean`, `create_branch`, `worktree_add`, `worktree_remove`,
`commit_all`, `merge_no_ff` (コンフリクト時は `merge --abort` して `Conflict{files}` を返す),
`diff_stat`。テストは `tempfile` 上の実リポジトリで行う。

## 8. `runtime` モジュール

- 単一のイベントループ (tokio タスク) が `DomainCommand` を受けて
  `domain::apply` → `store` に永続化 → `ApiEvent` 配信 → `Effect` 実行 を**直列に**行う。
  状態の同時更新を排除する。Effect の実行 (エージェント起動・git) は別タスクで行い、
  結果を `DomainCommand` としてループに戻す。
- オーケストレータの受信箱: アイドル時にまとめて 1 プロンプトにする
  ([`orchestration-model.md`](orchestration-model.md) §5)。
- ターン終了時の検査 (`report_step_done` / `help` が呼ばれたか) はセッションごとの
  「このターン中に呼ばれたツール」の記録で判定する。

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
- 環境変数 `YHTYE_FAKE_SCRIPT=<json>` のシナリオに従って動く。シナリオは
  「プロンプト (部分一致 or 順番) → 動作列」で、動作は:
  `message(text)` / `thought(text)` / `tool_call(..)` / `plan(..)` /
  `request_permission(options)` / `mcp_call(tool, args)` (rmcp のクライアント機能で
  `session/new` に渡された HTTP MCP サーバーを実際に呼ぶ) / `write_file(path, text)` /
  `sleep(ms)` / `wait_cancel` / `end(stop_reason)` / `crash`。
- `initialize` で `load_session: true` と `mcp_capabilities.http: true` を広告する。
  `set_mode` / `set_config_option` を記録し、テストから確認できるよう応答する。
- これで ACP・MCP・状態機械・git を含む全経路を LLM なしで決定的にテストする。

## 13. テスト方針

| 種類 | 実行 | 内容 |
|---|---|---|
| 単体 | `cargo test` | domain の遷移表、choose_permission、git (一時リポジトリ)、store |
| 結合 (偽エージェント) | `cargo test` | ACP ストリーミング・ターン中キャンセル・MCP 経由のタスク生成・グループ完走 |
| 実エージェント | `cargo test -- --ignored` | Claude Code を **必ず Haiku** で。テストヘルパが `ANTHROPIC_MODEL=haiku` を設定し、`Ready` の config_options で現在モデルが haiku であることを表明してから始める |
| フロント | `pnpm test` (Vitest) | reducer にイベント列を流す、transport の契約 |
| E2E | Chrome + WS ブリッジ + 実 Haiku | 依頼 → グループ完了まで |
