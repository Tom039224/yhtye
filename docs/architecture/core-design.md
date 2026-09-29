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

Stage 4 で実装 (`runtime/core.rs`)。当初案からの変更点は下の「Stage 4 の決定」。

```rust
#[derive(Clone)] pub struct Core { /* Arc 内部 */ }

impl Core {
    pub async fn start(cfg: CoreConfig) -> Result<Core, ApiError>;  // DB を開くだけ。プロジェクトは OpenProject で起動
    pub async fn resume_unfinished(&self) -> Vec<(String, Result<ProjectInfo, ApiError>)>; // Stage 5: 未完了の作業があるプロジェクトを開く
    pub async fn command(&self, cmd: ApiCommand) -> Result<ApiResponse, ApiError>;
    pub fn subscribe(&self) -> broadcast::Receiver<ApiEvent>;       // 全プロジェクトの durable + live
    pub async fn snapshot(&self, project: &str) -> Result<Snapshot, ApiError>;
    pub async fn shutdown(&self);                                     // 開いている全プロジェクトを止める
}

pub struct CoreConfig {
    pub data_dir: PathBuf,                   // yhtye.sqlite3 と worktrees/
    pub harnesses: Vec<HarnessPreset>,       // Stage 7b: ハーネスの登録簿 (§15)。以前は役割ごとの HarnessConfig 3 つ
    pub default_agent: AgentChoice,          // Stage 7b: 設定が無い役割の既定 (ハーネス × モデル)
    pub mcp_bind: SocketAddr,                // 既定 127.0.0.1:0
    pub domain: DomainConfig,
}
// CoreConfig::claude_code(data_dir, "haiku") が Claude Code だけを登録し、全役割の既定を claude-code × haiku にする。
```

- **Stage 4 の決定**:
  - 役割 → ハーネスは `HashMap<String, HarnessConfig>` + `RoleHarnessMap` ではなく役割ごとの 3 フィールド
    (`OrchestrationConfig` と同じ。ハーネスの選択 UI ができたら見直す)。
  - `Core` は 1 つの DB を共有し、開いたプロジェクトごとに `Orchestration` (git は `GitCli`) を持つ。
    `OpenProject{path}` はパスを正規化し、**git リポジトリの最上位ディレクトリ**であることを検査
    (`git rev-parse --show-toplevel`) してから、`projects` テーブルでパスを引いて ID を再利用する。
    新規の ID はディレクトリ名から作る (`my-app`、衝突したら `my-app-2`)。起動時に自動では開かない。
  - `ListEvents` は開いていないプロジェクトでも DB から読める (履歴の閲覧)。
  - ユーザーによる中止 `CancelTask` / `CancelGroup` は、オーケストレータの `cancel_task` /
    `cancel_group` と同じ規則で状態機械に通し (セッションキー `user` の `tool_called` として記録)、
    成功したらオーケストレータに `user_message` で「ユーザーが UI から中止した」と知らせる
    (`cancel_group` は `group_settled` を送らないため、知らせないとオーケストレータが気づかない)。
  - `Snapshot` に `sessions: Vec<SessionRecord>` (agent_sessions) を足した。UI がオーケストレータの
    ターン中かどうか・各セッションの状態を snapshot 時点から畳み込めるように。
  - `RetryGroupMerge` は Stage 5 で実装 (下の「Stage 5 の決定」)。
- **Stage 5 の決定**:
  - **起動時の再開**: `Core::start` 自体は DB を開くだけのまま (テストや他の用途で勝手にエージェントを
    起こさないため)。アプリ (src-tauri) と WS ブリッジは起動直後に `Core::resume_unfinished()` を
    バックグラウンドで呼ぶ。これは **未完了の作業があるプロジェクト** (active / finishing のグループが
    ある、または受信箱に未配達がある) だけを `OpenProject` と同じ経路で開くので、中断したタスクは UI を
    待たずに再開する。作業の無いプロジェクトは開かない (オーケストレータを無駄に起こさない)。
    「前回開いていたか」は記録していない (必要になったら `projects` に列を足す)。
  - `RetryGroupMerge{project, group}`: `DomainCommand::RetryGroupMerge` → `merge_blocked` を `finishing` に
    戻して `GitOp::MergeGroup{notify: true}`。結果は状態 (`done` / `merge_blocked`) と受信箱の
    `merge_result` (本文に「ユーザーが UI から再試行した」)。他に active / finishing のグループがあれば
    `conflict`、`merge_blocked` 以外は `invalid_state`。応答は `accepted` (マージは応答前に終わっている)。
  - `cancel_group` は各タスクの後片付けの後に `GitOp::RemoveGroupWorkspace` で統合 worktree も消す
    (Stage 4 で残っていた問題)。
  - 履歴の読み込みは先頭からのページングのまま (Stage 6a で直近からの遅延読み込みに変更 → 下の「Stage 6a の決定」)。
    新しい順のページングは UI の同期規則 (cursor から先を順に畳み込む) と噛み合わないため、
    別の「過去の会話だけを遅延で読む」経路として設計が要る。
- **Stage 6a の決定**:
  - `GetGitOverview{project, limit?}` → `ApiResponse::GitOverview{git}` (git パネル・ブランチ一覧・タイトルバー用)。
    `git::overview` (`git/graph.rs`) が `symbolic-ref` / `for-each-ref refs/heads` / `git log --topo-order
    --branches HEAD` を読み、`GitOverview{head, head_sha, branches[{name, sha}], commits[{sha, parents,
    branches, subject, ts_ms}], truncated}` を返す (既定 120 件・最大 500)。読み取りだけ、開いていない
    プロジェクトでも可 (パスは DB から)。空のリポジトリはコミット 0 件。レーンの割り当ては UI。
    UI は読み込み時・再接続時と、git に関わるドメインイベント (group_created / workspace_ready /
    task_status_changed / step_completed / group_merge_finished / group_cancelled / task_cancelled) の
    400 ms 後にまとめて読み直す (イベントに git の内容は載せない。git の真実は git にある)。
  - **履歴の遅延読み込み** (Stage 5 の課題): 新しいコマンドは足さない。seq は 1 から欠番なしなので、
    snapshot の seq を S として `ListEvents{after_seq: S-400}` から読み (会話・出力は直近 400 件分だけ)、
    それより前は UI の「さらに前の履歴を読み込む」で `after_seq` をずらして 400 件ずつ前に足す。
    状態は snapshot から作るので過去のイベントは不要。前に足したページは単独で畳み込んでから連結する
    (ページをまたぐツール呼び出しは開始位置に 1 つにまとめる)。同期規則 (cursor から先を順に畳み込む) は不変。
  - **`SessionStopped` の順序** (flaky テストの原因の 1 つ): Yhtye が止めたセッションの停止タスクは
    `Exited` を送った後に終わるが、エージェントのイベントは別タスク (forwarder) と別チャネルを通るため、
    ループが停止完了を先に見て `session_stopped` を `turn_ended` / `exited` より前に出すことがあった
    (負荷時)。`driver::StopOrder` で「停止タスクの完了」と「そのセッションの `Exited` の処理」の
    両方が揃ってから出すようにした。
- **Stage 6b の決定**:
  - `GetUsage{refresh?}` → `ApiResponse::Usage{usage: UsageReport{plan, windows[{kind: five_hour|week|other, label, percent,
    resets_at_ms}], fetched_at_ms}}` (§14)。取れなければ `unavailable` (値は作らない)。
  - `Core::shutdown` は開いている途中のプロジェクトを待ち (`opening` ロック)、以後の `OpenProject` / `GetUsage` を
    `unavailable` にし、実行中の使用量プローブを止める (終了と開く処理が競合してエージェントが残る問題、レビューの MEDIUM)。
- `ApiCommand` (serde `type` タグ): `list_projects` / `open_project{path}` / `get_snapshot{project}` /
  `list_events{project, after_seq, limit?}` (既定 500・最大 2000、応答 `events{events, more}`) /
  `send_user_message{project, text}` / `cancel_orchestrator_turn{project}` /
  `cancel_task{project, task, reason?}` / `cancel_group{project, group, reason?}` /
  `retry_group_merge{project, group}` (Stage 5) / `get_git_overview{project, limit?}` (Stage 6a) / `get_usage{refresh?}` (Stage 6b) /
  `get_agent_settings{project?}` / `set_agent_settings{project?, role, settings}` / `list_harness_models{harness, refresh?}` (Stage 7b、§15)。
  `ApiResponse` = `projects` / `project` / `snapshot` / `events` / `accepted` / `git_overview` / `usage` /
  `agent_settings` / `harness_models`。
  `ApiError { code: invalid_argument|not_found|invalid_state|conflict|forbidden|unavailable|internal, message }`。
- `ApiEvent` = `{ seq, ts_ms, project, live, body }` (Stage 3a で `yhtye_core::api` に定義、
  3b で `live` を追加)。`body: ApiEventBody` (serde `type` タグ付き) は次のいずれか:
  - `domain { event: DomainEvent }` — 状態の変化 (§5)。UI は `State::apply` と同じ規則で畳み込める。
  - `session_started { session, role, task, pid, acp_session_id, resumed }` /
    `session_failed { session, error }` / `session_stopped { session, suspended }` /
    `session_interrupted { session }` (3b: 起動時、前回動いていたセッション。`suspended` は
    Yhtye の終了で止めた = 次回の起動で復元する)
  - `agent { session, event: AgentEvent }` — エージェントの出力とライフサイクル
    (`Output(MessageChunk|ThoughtChunk|ToolCall|…)`, `TurnEnded`, `Exited` …)。`Stderr` は流さない。
  - `agent_text { session, kind: message|thought, text }` (3b) — ストリーミングされたチャンクを
    まとめた 1 ブロック (下記)。
  - `tool_called { record: ToolCallRecord }` / `prompted { session, text }`
  - **durable と live** (3b): `live == false` のイベントはイベントログ (§6) に保存され、
    `seq` はログの行番号と一致し 1 から欠番なし。`live == true` のイベント
    (`agent` のうち `MessageChunk` / `ThoughtChunk` / `UserMessageChunk` / `Usage`) は保存せず、
    直前の durable イベントの `seq` を持つ (`seq` を進めない)。チャンク列はブロックの終わり
    (同じセッションの別種の出力・ツール呼び出し・ターン終了・終了・シャットダウン) で
    `agent_text` として 1 回だけ保存・配信される。UI はストリーミング表示をこの
    `agent_text` で置き換える。トークン単位の行を DB に積まないための選択。
  - `Snapshot = { seq, state: State, sessions: Vec<SessionRecord> }` (`Orchestration::snapshot`。`seq` は最後の durable イベント。`sessions` は Stage 4)。
  - TS 型生成 (Stage 4): **ts-rs 12** (`api/typegen.rs` のテスト)。`src/api/generated/` に型ごとの
    ファイル + `index.ts` を出す。テスト `generated_typescript_is_up_to_date` が生成物とコミット済み
    ファイルの一致を検査し (`cargo test` に含まれる)、`pnpm gen:types` で更新する。`u64` は `number`
    (seq とミリ秒時刻は 2^53 より十分小さい)。`AgentEvent` 内の ACP スキーマ型は `unknown` で出し、
    UI は `src/api/acp.ts` で必要なフィールドだけ実行時に検査して読む (ACP の型を手で写さない)。
- **クライアント同期の規則**: 接続時に `snapshot` (その時点の `seq` を含む) を取り、
  以降 `seq > snapshot.seq` の durable イベントだけ状態に適用する。durable イベント間の取りこぼし
  (seq の飛び) を検知したら `ListEvents` で欠けた分を取る (Stage 4 の実装。snapshot の取り直しでもよい)。live イベントは表示用で、状態には畳み込まない。
  履歴 (過去の会話・エージェント出力) はイベントログから読む (`Orchestration::events` /
  `Store::session_events`、Stage 4 で `ListEvents` に)。
- WS ブリッジのメッセージ型 `WsRequest` / `WsReply` / `WsServerMessage` も `api::wire` に定義済み (§10、ブリッジ本体は Stage 5)。
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
Stage 7b: ハーネスは `HarnessPreset` (§15) として登録し、役割ごとの `HarnessConfig` とモデルの差し込み方をまとめる。
`HarnessConfig` の形は変えていない (Stage 7c-2 で省略可能な `env_remove` だけ追加)。

**Stage 6b (セキュリティレビュー)**: エージェントの**プロセス**はプロジェクトではなくユーザーのホーム
(無ければ `/`) で起動する。作業ディレクトリは ACP の `session/new` / `session/load` の `cwd` でだけ渡す。
プロジェクトで起動すると、信頼できないリポジトリの `.npmrc` (registry の差し替え) や `node_modules` の
同名パッケージを `npx` が読み、エージェントが動く前に任意コードが走りうるため。偽エージェントも
`session/new` / `session/load` の `cwd` に `chdir` する (実エージェントと同じ振る舞い)。
アダプタは `@agentclientprotocol/claude-agent-acp@0.84.0` に完全一致で固定 (`CLAUDE_AGENT_ACP`)。
Yhtye 自身の秘密 (`YHTYE_BRIDGE_TOKEN` / `VITE_YHTYE_BRIDGE_TOKEN`) はエージェントの環境から除く。
回帰テスト `tests/acp_launch.rs`。
`HarnessConfig::claude_code_usage_probe()` は §14 の使用量取得用 (ツールなし・`persistSession: false`・`TZ=UTC`)。

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
- 127.0.0.1 のみに bind (ポートは OS 任せ)。rmcp 既定の `allowed_hosts` (localhost 限定 = DNS rebinding 対策) を維持。
  Stage 6b: `enforce_origin_validation()` (許可リストは空) で **`Origin` ヘッダのある要求 (= ブラウザのページ) を 403**。
  エージェント (Claude Code) は `Origin` を送らない。回帰テスト `mcp_server.rs::browser_requests_with_an_origin_are_forbidden`。
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
  チャネルに流す (runtime が `tool_called` イベントとしてイベントログに保存する。3b)。
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
  `snapshot` / `events(after_seq, limit)` / `store()` / `shutdown`。`OrchestrationConfig` に
  `domain: DomainConfig` (`max_review_rounds`)、`git: Arc<dyn GitService>`、`db_path` (3b) を持つ。
  `start` は DB を開き、プロジェクトの状態があれば読み込んで再起動処理 (§8.1) をする。
  複数プロジェクトを束ねる `Core` は未着手 (Stage 5 までに。DB は `store::db_path(data_dir)` 1 つを共有)。
- `runtime::driver` — ループ本体 (§8)。`runtime::port::LoopPort` が `ToolPort` を実装し、
  ツール呼び出しを oneshot 付きでループへ送る (ループが処理して応答する)。
- `runtime::sessions` — セッションの起動 (裏で `JoinSet`)・プロンプトのキュー・トークンの
  `rebind` / `revoke`・停止。セッションキーは `orchestrator` / `T-n/implementer` (タスク中は同じ
  セッション) / `T-n/review-<step>` (毎回新規)。起動中に止められたセッションは起動完了時に即停止する。
- `runtime::emitter` (3b) — `Emitter` (セッション管理側が持つ。本文をバッファするだけ) と
  `Publisher` (ループが持つ)。`Publisher::flush` はループの 1 周ごとにバッファを取り出し、
  durable イベントに `seq` を振って DB に書いてから購読者へ送る。`Publisher::commit` は
  ドメイン遷移のイベントと現在状態テーブルの変更を 1 トランザクションで書く (§6)。
  したがって購読者は DB に無い durable イベントを見ない。
- `runtime::transcript` (3b) — チャンクを `agent_text` ブロックにまとめる (§2)。
- `runtime::launch` (3b) — セッション起動 (トークン発行・`session/load` 指定・イベント転送)。
  **転送タスクは `session/load` の履歴再生 (`Ready` より前の `Output`) を捨てる** — 既にログにある
  履歴なので、新しい出力として配信・保存しない。起動ごとに launch 番号を振り、置き換えられた
  プロセス (失敗した起動の後に同じキーで起動し直した場合など) のイベントはループで捨てる
  (3b で発見: 失敗した `session/load` のプロセスの `Exited` が新しいセッションの終了と誤認されていた)。
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
    Restart { orchestrator: OrchestratorResume },       // 3b: 再起動時。§8.1
    ResumeTask { task },                                 // 3b: interrupted のタスクを再開
    RetryGroupMerge { group },                           // 5: UI からのマージ再試行
    OrchestratorTurnEnded { outcome, prompt_queued },    // 7a: 落ち着いた active グループの催促 → 代行
}                                                        //     (orchestration-model §2.1)

pub enum Effect {
    RunStep { agent: AgentRef, group, prompt, workdir },  // 無ければセッションを起動してから送る
    ResumeStep { agent, group, prompt, fallback, workdir }, // 3b: session/load で復元して prompt、
                                                          // 復元できなければ新セッションに fallback
    PromptAgent { agent: AgentRef, text },                // help の返答・催促
    StopAgent { agent: AgentRef },                        // レビュー済みのレビュアー
    StopTaskAgents { task },                              // タスク終端
    Git(GitOp),     // CreateGroupBranch / PrepareWorkspace / FinishTask / RemoveWorkspace / MergeGroup
                    // MergeGroup.trigger (7a): FinishGroup = ツールの戻り値 / UserRetry・Yhtye = merge_result
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
  `git_rules.rs` (git 結果) / `restart.rs` (3b: 再起動と再開)。Step kind の追加は `flow.rs::start_step` と
  `agent_rules.rs::turn_ended` に閉じる。
- 規則 (「active グループは 1 つ」「`done` は末尾」「review の自動再挿入は N 回まで」など) は
  すべてここに置き、単体テスト (`domain/tests/`: 遷移表・フロー・エラー・git・エージェント) で網羅する。

## 6. `store` モジュール

Stage 3b で実装。

- sqlx **0.9** (`sqlite-bundled`, `runtime-tokio`, `migrate`, `macros`)。コンパイル時に DB を要する
  `query!` マクロは使わず `query_as` + `FromRow` にする (ビルドを単純に保つ)。マイグレーションは
  `crates/yhtye-core/migrations/*.sql` を `sqlx::migrate!` で埋め込み、`Store::open` で適用する。
  WAL + `synchronous = NORMAL` (コミット済みトランザクションはアプリのクラッシュで失われない。
  電源断では直近が失われうるが、トランザクションが半端に残ることはない)。
- 場所: アプリはデータディレクトリの `yhtye.sqlite3` (`store::db_path(data_dir)`)。
  `OrchestrationConfig::db_path` で指定する (テストは一時ディレクトリ)。
- テーブル (`0001_init.sql`): `projects` (id, path, config, counters) / `task_groups`
  (`groups` は SQL の予約語のため改名) / `tasks` / `task_deps` / `steps` / `helps` / `inbox` /
  `agent_sessions` (session_key, role, task, acp_session_id, status, turn_running) /
  `events (project_id, seq, ts_ms, kind, session, payload JSON)` — 主キー `(project_id, seq)`、
  `kind` は `domain.task_created` / `agent.turn_ended` / `agent_text` など。enum 列は JSON と同じ
  snake_case 名。
- **`Transition` 1 回 = 1 トランザクション**: イベント追記 + 現在状態テーブル更新
  (`Store::commit`)。現在状態は遷移前後の `State` を比べて変わった行だけ書く (タスクが変われば
  その steps / deps を書き直す。受信箱は増減分)。コミット後に `ApiEvent` を配信する。
  DB 書き込みに失敗した遷移は捨てる (状態を変えず、Effect も実行せず、ツール呼び出しには
  `internal` を返す)。ドメイン以外の durable イベントは `Store::append` (まとめて 1 トランザクション)。
- `agent_sessions` はセッションのイベント (`session_started` / `session_stopped` /
  `session_interrupted` / `prompted` / `agent.turn_ended`) から同じトランザクションで導く。
  status は `live` / `stopped` (Yhtye が止めた・プロセスが終了した) / `suspended` (Yhtye の終了で
  止めた) / `interrupted` (起動時に live か suspended だった)。`stopped` 以外は次に同じキーの
  セッションを使うとき `session/load` で復元する。
- **MCP トークンは保存しない。** 復元したセッションにも `session/load` の `mcpServers` で新しい
  トークン (新しいポート) を渡す。実 Claude Code が新しい URL で `report_step_done` できることを
  確認した (§8.1)。
- 起動時は現在状態テーブルから `State` を組み立てる (`Store::load_state`。イベントの再生はしない)。
  `Store::replay_state` はイベントログだけから `State` を作る (検査用)。テストでは偽エージェントの
  全シナリオの最後に「ライブの状態 == テーブルから読んだ状態 == イベント再生の状態」と
  「ログの最後の seq == 配信した seq」を検査している。
- `DomainConfig` は設定なので、起動時は保存値ではなく現在の設定で上書きする。

## 7. `git` モジュール

状態機械からは `GitOp` (§5) として要求され、runtime が `GitService` トレイト経由で実行する
(Stage 3a で定義。本番の実装は 3c の `GitCli` (§7.1)。テスト用の `NoopGit` は何もしない — 作業場所はプロジェクトディレクトリ、
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

### 7.1 `GitCli` (Stage 3c)

本番用の実装は `git::GitCli` (`git` CLI を `tokio::process` で実行)。git2 / gix は使わない:
worktree・マージ・コンフリクト・フック・`.gitignore`・ユーザー設定の挙動を git 本体と完全に
一致させたいこと (orchestration-model §6 の決定)、libgit2 の worktree / merge 支援が限定的なこと、
依存を増やさないこと、が理由。操作はすべて短い (ネットワークなし) のでループ内で await する (§8)。

- 構成: `git/run.rs` (実行: `LC_ALL=C`・`GIT_TERMINAL_PROMPT=0`・`GIT_EDITOR=true`、
  `GIT_DIR` 等の継承を除去、stdin は null、1 コマンド 120 秒のタイムアウトで kill) /
  `git/repo.rs` (照会と commit / merge / stash) / `git/worktree.rs` (worktree の作成・再利用・削除) /
  `git/cli.rs` (`GitCli`: `GitOp` ごとの手順)。
- 生成: `GitCli::new(project_dir, git::worktree_root(data_dir, project_id))`
  (`<data_dir>/worktrees/<projectId>`)。`NoopGit` は git を使わないテスト用に残す。
- `GitOp::PrepareWorkspace` / `FinishTask` は `group_branch` / `base_branch` を持つ (3c で追加):
  消えた group ブランチ・統合 worktree を作り直すため。ブランチ名は `domain::group_branch` /
  `domain::task_branch` (`yhtye/<groupId>-<taskId>`) の 1 か所で決める。
- トレイトに `workspace_changes(workdir, group_branch)` を追加 (既定実装は空)。再起動のフォールバック
  プロンプトに `git status --short` + 分岐点からの `git diff` (16 KiB まで) を添えるのに使う
  (runtime の `Effect::ResumeStep` 処理)。
- 各 `GitOp` の手順と冪等性・安全性の規則は orchestration-model §6「Stage 3c で決めた細部」。
  予期された結果 (`Conflict` / `Dirty` / `Blocked`) 以外の失敗は `Failed{message}` → ドメインが
  `git_failed` の help (タスク) か `internal` (create_group) にする。
- テスト: `tests/git_cli.rs` (一時リポジトリで 17 件)、`tests/orchestration_fake_git.rs`
  (偽エージェント + 実 git で 4 シナリオ)、`tests/orchestration_claude_git.rs` (実 Haiku、`#[ignore]`)。
  一時リポジトリは `tests/common/repo.rs::TempRepo` (リポジトリと data ディレクトリを同じ一時
  ディレクトリの別の子に置く。DB も data 側)。

## 8. `runtime` モジュール

- 単一のイベントループ (tokio タスク) が `DomainCommand` を受けて
  `domain::decide` → `store` に永続化 (3b) → `ApiEvent` 配信 → `Effect` 実行 を**直列に**行う。
  状態の同時更新を排除する。Effect は DB に書けた遷移についてだけ実行する。
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

### 8.1 再起動と再開 (Stage 3b)

[`orchestration-model.md`](orchestration-model.md) §10 の実装。

1. `Orchestration::start` が DB からプロジェクトの状態を読む。`agent_sessions` のうち復元対象
   (`stopped` 以外) を `session_interrupted` として配信し、そのキー → ACP セッション ID を
   セッション管理に渡す (各キーで 1 回だけ使う)。
2. オーケストレータを起動する。保存済みセッションがあれば `session/load`、失敗したら
   新しいセッション (`session_failed` に理由)。
3. ドメインに `Restart { orchestrator: { had_session, restored, turn_was_running } }`:
   `running` / `merging` のタスク → `interrupted`。help の返答待ちだったエージェントは
   `help_agent_lost` (プロセスは Yhtye と一緒に消えている。`answer_help(resume)` で Step を
   やり直す — そのセッションは `session/load` で復元され、返答は note として付く)。
   `finishing` のグループ (git のマージ中に落ちた) → `merge_blocked` + 受信箱 `merge_result`。
   オーケストレータのセッションを復元できなかった → 受信箱 `restarted` に状態の要約。
   復元できたがターン中に止まった → `restarted` に「ターンが途中で切れた」。
4. `interrupted` の各タスクに `ResumeTask`: エージェントの Step なら `ResumeStep`
   (`session/load` で復元して短い `[yhtye:resume]` プロンプト。復元できない / ハーネスが
   非対応なら新しいセッションに Step の全プロンプト + 「再起動で前のセッションを失った。
   作業ツリーを確認せよ」の注記)。報告済みでターン終了だけ失われた Step は次へ進む。
   `done` Step (マージ中) は `FinishTask` をやり直す (git 操作の冪等性は 3c)。作業場所の
   準備中だったタスクは準備からやり直す。
- 停止 (`shutdown`) はタスクの状態を変えない。止めたセッションは `session_stopped { suspended: true }`
  になり、次回の起動で復元対象になる。

## 9. Tauri アプリ層 (`src-tauri`)

- `Core::start` を `setup` で呼び `manage` する。コマンドは
  `#[tauri::command] async fn yhtye_command(cmd: ApiCommand) -> Result<ApiResponse, ApiError>`
  の 1 本だけ。`subscribe` した `ApiEvent` を `app.emit("yhtye://event", ev)` で流す
  (broadcast の `Lagged` は捨てる。UI が seq 飛びで補う)。
- ウィンドウ終了時に `Core::shutdown` を await してエージェントプロセスを残さない。
- Stage 5 の実装 (`src-tauri/src/lib.rs`): データディレクトリは `YHTYE_DATA_DIR`、なければ Tauri の
  app data dir (Linux: `~/.local/share/io.github.tom039224.yhtye`)。モデルは `YHTYE_MODEL` (既定 `haiku`。
  再構築中の検証方針に合わせた)。起動後に `resume_unfinished` (§2)。終了は `RunEvent::Exit` で
  `block_on(core.shutdown())`。SIGINT / SIGTERM (`pnpm tauri dev` の Ctrl+C など) は `app.exit(0)` に
  変換して同じ経路で止める。ログは `tracing-subscriber` (`RUST_LOG`)。
- **CSP (Stage 6b)**: `tauri.conf.json` の `app.security.csp` を `null` から
  `default-src 'self'; script-src 'self'; style-src 'self' https://fonts.googleapis.com; font-src 'self' https://fonts.gstatic.com;
  img-src 'self' data:; connect-src 'self' ipc: http://ipc.localhost; object-src 'none'; base-uri 'none'; form-action 'none'; frame-src 'none'`
  に。Tauri が自前のスクリプトに nonce を足す。React の `style={{}}` は CSSOM なので `unsafe-inline` 不要。
  `devCsp` は設定しない: デスクトップの dev は devUrl (Vite) を直接読み込むので Tauri は CSP を注入しない。
  本番バンドルに同じ CSP (IPC の代わりに `ws://127.0.0.1:1422`) を HTTP ヘッダで付けて Chrome で動かし、違反なしを確認した。
  opener の権限は `opener:default` から `allow-open-url` + `allow-default-urls` (http/https/mailto/tel) に絞った
  (`reveal-item-in-dir` は使わない)。
- `WEBKIT_DISABLE_DMABUF_RENDERER=1` (Stage 7a で決定): アプリの `run()` の最初に
  `src-tauri/src/webkit_env.rs` が **Wayland (`WAYLAND_DISPLAY` が空でない、または `XDG_SESSION_TYPE=wayland`。
  `GDK_BACKEND=x11` なら対象外) かつ NVIDIA ドライバが読み込まれている (`/proc/driver/nvidia` または
  `/sys/module/nvidia` がある)** ときだけ設定する。ユーザーが既に設定していれば (値にかかわらず) 触らない。
  プロセスを起こさない安価な判定で、スレッド起動前に呼ぶ。判定は注入した入力で単体テスト。
  dev の `src-tauri/.cargo/config.toml` の `[env]` はそのまま (設定済みなので検出は何もしない)。
- データディレクトリの移行: identifier を `com.tom039224.yhtye` から `io.github.tom039224.yhtye` に変えたので、
  `src-tauri/src/legacy_data.rs` が既定パスのときだけ (`YHTYE_DATA_DIR` 未設定)、旧ディレクトリだけが存在すれば
  新ディレクトリへ `rename` し、移動した `worktrees/*/*/*` (`.git` ファイルあり) で `git worktree repair` を実行する。
  両方ある・旧がない・rename 失敗 (別デバイスなど) は何もせず警告のみで、新しい空ディレクトリで起動する。

## 10. 開発用 WS ブリッジ (`yhtye-dev-bridge`)

- `yhtye-dev-bridge --data-dir <dir> --port 1422` で Core を起動し、`ws://127.0.0.1:1422/ws` を公開。
  **既定ポートは 1421 → 1422 に変更 (Stage 5)**: Vite の HMR が `TAURI_DEV_HOST` 設定時に 1421 を使うため。
- メッセージ: 要求 `{ "id": n, "cmd": ApiCommand }` → 応答 `{ "id": n, "ok": ApiResponse }` /
  `{ "id": n, "err": ApiError }`。イベントは `{ "event": ApiEvent }` をプッシュ。
- 127.0.0.1 のみに bind。ブラウザ上の他サイトから叩かれないよう、`Origin` を
  `http://localhost:1420` / `http://127.0.0.1:1420` に限定し、起動時に表示する
  ランダムトークンを `?token=` で要求する (Vite には env で渡す)。
- 開発・E2E 専用。配布物には含めない。
- Stage 5 の実装 (`crates/yhtye-dev-bridge`、lib = `router` / `serve` + bin): axum 0.8 の `ws`。
  `Origin` ヘッダが**ある**接続 (= ブラウザ) だけ Origin を検査し、無い接続 (スクリプト・テスト) は
  トークンだけで通す。拒否は 403 (Origin) / 401 (トークン)。要求は接続ごとに並行実行 (OpenProject の
  間もキャンセルが通る。応答は順不同で id で対応付け)。接続が切れても実行中の要求は中断しない
  (半端な OpenProject を残さない)。id の無い不正メッセージは無視、id があれば `invalid_argument`。
  トークンは `--token` / `YHTYE_BRIDGE_TOKEN`、無ければランダムで、起動時に stdout に
  `YHTYE_BRIDGE_URL=ws://...?token=...` を 1 行出す。データディレクトリは `--data-dir` /
  `YHTYE_DATA_DIR`、無ければ `$XDG_DATA_HOME/yhtye-dev-bridge` (アプリの DB とは分ける)。
  SIGINT / SIGTERM で `Core::shutdown` してから終了。起動後に `resume_unfinished`。
- `pnpm dev:browser [ブリッジの引数]` (`scripts/dev-browser.mjs`) がブリッジをビルドして Vite と一緒に
  起動し、同じランダムトークンを両方に渡す。片方が終わるか Ctrl+C で両方止める。

## 11. フロントエンドの transport 抽象

```ts
export interface Transport {
  readonly kind: "tauri" | "websocket" | "memory"; readonly target: string;
  invoke(cmd: ApiCommand): Promise<ApiResponse>;              // CommandError (code = ApiErrorCode | "transport") で reject
  subscribe(onEvent: (ev: ApiEvent) => void): () => void;
  onStatus(h: (s: ConnectionStatus) => void): () => void;    // connecting / open / closed{reason, retryInMs}
  close(): void;
}
// createTransport(): Tauri 内 (window.__TAURI_INTERNALS__ がある) なら TauriTransport,
// それ以外は WsTransport (VITE_YHTYE_BRIDGE_URL 既定 ws://127.0.0.1:1422/ws, VITE_YHTYE_BRIDGE_TOKEN)。
```

Stage 4 の構成 (`src/`):
- `api/` — `transport.ts` / `tauri.ts` (`yhtye_command` と `yhtye://event`) / `ws.ts` (要求 id の対応付け、
  切断で保留中の要求を失敗させる、バックオフで再接続、180 秒の応答タイムアウト) / `create.ts` /
  `acp.ts` / `generated/`。テスト専用の `MemoryTransport` と `FakeCore` は `src/test/`。
- `store/` — `domain.ts` (`State::apply` の移植)、`sessions.ts` (agent_sessions の導出の移植)、
  `transcript.ts` (セッションごとの会話・出力、ストリーミング)、`project.ts` (純粋な畳み込み)、
  `app.ts` (唯一のストア `AppStore`。同期: 読み込み中のイベントはバッファ → snapshot →
  `ListEvents` を先頭からページング (履歴) → バッファを流す / `seq <= cursor` は重複として捨てる /
  飛びは `ListEvents` で補う / `cursor` より新しい live イベントも飛びとみなす / `cursor` より古い
  live チャンクは捨てる (二重表示防止) / 再接続では `OpenProject` (冪等) してから `cursor` 以降を補い、
  途中のストリーミングは捨てる)。`useSyncExternalStore` で React に渡す。
- `ui/` — 素の画面 (トークンの CSS 変数のみ): プロジェクト一覧と開く欄、オーケストレータの会話
  (ストリーミング・思考は折りたたみ・ツール呼び出し・待機中のユーザーメッセージ)、コンポーザ
  (Enter 送信 / Shift+Enter 改行 / IME 変換中は送らない / ターン中も送れて受信箱で待機 / ターン中止)、
  グループ・タスク・Step と help / interrupted / merge_blocked、タスクのエージェント出力、
  接続状態とエラーのバナー。
- Stage 6a: `ui/` を Claude Design のレイアウトに置き換えた (対応表は
  [`docs/design/orchestrator-desktop.md`](../design/orchestrator-desktop.md) §8)。ストアに `git` (GitOverview と
  エラー)・`lastEventAt`・履歴の遅延読み込み (`historyStart` / `loadOlderHistory`)・前回のプロジェクトの記憶
  (`Prefs`、アプリでは `localStorage`。読めなければ覚えないだけ。接続後にコアが知っているパスなら自動で開く) を追加。
- TS の reducer が Rust と一致することは、偽エージェントで本物のコアを動かして記録したイベント列
  (`src/test/fixtures/*.json`、`pnpm record:fixtures`) を畳み込んで最終 snapshot と比べて検査する。

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
| 結合 (ブリッジ) | `cargo test -p yhtye-dev-bridge` | 実 WebSocket + 本物のコア + 偽エージェント: アクセス制御・要求-応答・エラー・イベント配信 |
| E2E | Chrome + WS ブリッジ + 実 Haiku | 依頼 → グループ完了まで (Stage 5 は手動の自動操作) |

## 14. 使用量 / quota (`usage`、Stage 6b)

調べたデータ源 (2026-09-23、Claude Code 2.1.280 / claude-agent-acp 0.81.0):

| 源 | 内容 | 判断 |
|---|---|---|
| ACP `usage_update` (live で既に配信) | コンテキストの `used` / `size` とターンの `cost` のみ | quota ではない |
| `usage_update._meta["_claude/rateLimit"]` | SDK の `rate_limit_event` (`status`, `rateLimitType`, `utilization?`, `resetsAt?`)。**変化したときだけ**・1 窓ずつ | 実機の 1 ターンでは来なかった。常時表示の源にならない (補助には使える) |
| **`/usage` (ローカルコマンド)** | アダプタが SDK の `usage_EXPERIMENTAL…()` を Markdown に整形: プラン名・5 時間・週 (全モデル)・モデル別週・リセット時刻 | **採用**。モデルを呼ばない。実機で 5〜7 秒 |
| `~/.claude/.credentials.json` の OAuth トークンで `api/oauth/usage` | 非公開 API + 資格情報の読み出し | 不採用 |
| statusline の `rate_limits` | 対話 UI の statusline にだけ渡る | 使えない |

実装 (`crates/yhtye-core/src/usage/`):
- `probe_usage`: `HarnessConfig::claude_code_usage_probe()` (ツール `[]`・MCP なし・`persistSession: false` で
  `~/.claude/projects` に会話を残さない・mode / model を切り替えない・`TZ=UTC`) を `data_dir/usage-probe` で起動 →
  `/usage` → 返答の Markdown を `parse_usage_markdown` で読む → 停止。
- `parse_usage_markdown`: `> Claude <plan> subscription usage` と `**<label>** — **<n>%** · Resets <Mon D, h:mm AM UTC>`。
  年は表示されないので「今に最も近い年」。読めない行は捨て、何も読めなければ `Unreadable` (**推測しない**)。
  書式はアダプタのバージョン (固定) に依存する。実機テスト `acp_claude_real::real_usage_command_reports_the_subscription` で検知。
- `UsageService`: 単一実行 + キャッシュ (成功 60 秒、失敗と明示の再取得は 10 秒)、`close()` で実行中のプローブを止める。
- UI: 接続時と 5 分ごとに `get_usage`、メーターのクリックで `refresh: true`。

## 15. エージェント (ハーネス × モデル × effort) の選択 (Stage 7b、7d で effort と用途メモを追加)

ユーザーの決定 (PLAN.md Stage 7b): 役割ごとに**候補集合**と**既定値**を持ち、全体の既定値の上にプロジェクトの
既定値を重ねる。モデル一覧はハーネスから ACP で取る (ハードコードしない)。変更は新しく起動するセッションから効く。
**7d**: 候補は「行」= ハーネス × モデル × effort (任意) + 用途メモ (自由記述)。同じハーネス × モデルを effort 違いで
何行でも置ける。effort とモデルごとの effort 一覧も ACP の `configOptions` から取る。

### 15.1 型 (`crates/yhtye-core/src/agents/`)

```rust
pub enum AgentRole { Orchestrator, Implementer, Investigator, Reviewer }   // 設定の役割
pub struct AgentChoice { harness: String, model: Option<String>, effort: Option<String> }  // model None = ハーネスの既定、effort None = 設定しない
pub struct Candidate { harness, model, effort, note: String }              // 候補の 1 行 (7d)。choice() = 用途メモを除いた AgentChoice
pub struct RoleSettings { candidates: Vec<Candidate>, default: AgentChoice }     // default = どれかの行の choice()
pub struct AgentSettings { orchestrator, implementer, investigator, reviewer: RoleSettings }       // 実効値
pub struct AgentSettingsLayer { orchestrator, implementer, investigator, reviewer: Option<RoleSettings> } // 1 層
```

- **役割の対応**: オーケストレータのセッション = `orchestrator`、`code` タスクの implementer セッション = `implementer`、
  `investigate` タスクの implementer セッション = `investigator`、レビューのセッション = `reviewer`
  (ドメインの `Role` は 3 つのまま。`investigator` は設定だけの役割)。
- **重ね方** (`agents::effective`、純粋関数): 役割ごとに「プロジェクトの層 → 全体の層 → 組み込みの既定
  (`CoreConfig::default_agent` だけを候補にした RoleSettings)」の最初にあるもの。層は役割単位で丸ごと置き換える
  (候補と既定は一緒に上書き・継承する)。
- **検査** (`RoleSettings::validate`): 候補が 1 つ以上・既定が候補のどれかと (ハーネス × モデル × effort で) 完全一致・
  ハーネスが登録簿にある・モデル / effort が空文字でない・用途メモは 400 文字まで。同じ組 (ハーネス × モデル × effort) の行は
  最初のものを残して取り除く。モデル名・effort の値は検査しない (一覧は遅れて取るため。UI は一覧にあるものだけを選ばせる。
  実際にそのモデルが effort を持たなければ**セッション起動が失敗する** (§15.4))。
- **上書きの選び方** (`RoleSettings::pick(harness?, model?, effort?)`): すべて省略なら既定。指定があれば一致する行
  (省略したものは任意) を探す: 0 行 → `PickError::NoMatch` (全行を返す)。1 行 → それ。複数行で、すべて同じハーネス × モデルで
  effort が省略されている → `PickError::NeedsEffort` (その行を返し、effort の指定を求める)。それ以外の複数行 →
  既定が含まれれば既定、無ければ最初の行。つまり「ハーネス × モデル × effort が行と完全一致」が原則で、effort を省略できるのは
  そのハーネス × モデルの行が 1 行だけのとき (その行の effort が使われる)。
- **7d の移行**: 設定・`tasks.agent` は JSON なので、古い保存値の `effort` (なし) と `note` (空) は serde の既定値で読める
  (書き換え不要、単体テストあり)。SQL の移行はセッションに effort を残す `0004` だけ。

### 15.2 ハーネスの登録簿

`HarnessPreset { id, label, orchestrator, implementer, investigator, reviewer: HarnessConfig, probe: Option<HarnessConfig>, model_env: Option<String> }`。
`HarnessPreset::config(role, model, effort)` は役割の `HarnessConfig` を複製し、モデルが指定されていれば `model`
(`set_config_option`、config id は既存の `ModelSelect` のもの、無ければ `"model"`) と `model_env` の環境変数
(Claude Code は `ANTHROPIC_MODEL`) を差し替える。`None` なら何も変えない (ハーネスの既定)。**7d**: effort が
あれば `HarnessConfig.effort` (`ModelSelect { config_id: "effort", value }`) を設定する。起動手順 (`acp/startup.rs`) は
**モデル → effort の順**に `set_config_option` を送り、どちらも「要求した値 = 応答の currentValue」を確かめる
(effort の選択肢はモデルごとに変わり、モデルを変えると作り直されるため、必ずモデルが先)。effort を持たないモデルに
effort を指定したら**起動を失敗させる** (`session/set_config_option` の `Startup` エラー: 黙って別の effort で動かさない。
UI は一覧にある値しか選ばせず、`create_task` は行との完全一致を要求するので、古い設定でしか起きない)。
`HarnessPreset::claude_code(model)` は Stage 6b までの 3 役割の設定そのもの (probe = `claude_code_usage_probe`)。
7c で OpenCode を足すときは preset を 1 つ登録するだけ (`CoreConfig::harnesses`)。

**Stage 7c-2**: preset に `requires_model` (選択は必ずモデルを持つ。`AgentCatalog::validate` が検査) と
`orchestrator_read_only` (オーケストレータの設定がファイルを書けない。`false` は UI で警告) を追加し、`HarnessInfo` にも載せる。
`HarnessPreset::opencode(fallback_model, env_remove)` ([`acp-harnesses.md`](acp-harnesses.md) §7.6)。
アプリと開発ブリッジは `CoreConfig::installed(data_dir, model)` = `agents::installed_presets`: Claude Code は常に、OpenCode は
`PATH` に実行可能な `opencode` があるときだけ登録する。

**Stage 7e (Codex)**: preset に `model_config_env: Option<String>` (モデルと effort を JSON で渡す環境変数。Codex は `CODEX_CONFIG`。設定すると
`HarnessPreset::config` が `{"model","model_reasoning_effort"}` を組み立てて入れ、effort は option として送らない)、`effort_config_id` (effort の config id。
既定 `effort`、Codex は `reasoning_effort`。以前のグローバル定数 `EFFORT_CONFIG_ID` は既定値としてだけ残る)、`model_source: ModelSource { Acp, Codex }`
(§15.6) を追加。`HarnessPreset::codex(codex_path)` ([`acp-harnesses.md`](acp-harnesses.md) §9): 全役割 `HarnessConfig::codex` (npx で `codex-acp@2.0.0`、
`CODEX_PATH`、`agent-full-access`、`FirstPrompt`)、`requires_model = true`、`orchestrator_read_only = false`。`installed_presets` は `codex` が `PATH`
にあるときだけ登録する (`CODEX_COMMAND`)。

`AgentCatalog` (`Arc`、全プロジェクトで共有) が登録簿・組み込みの既定・全体とプロジェクトの層 (メモリ上の写し) を持つ。
`OrchestrationConfig::agents: Arc<AgentCatalog>` (以前の `orchestrator / implementer / reviewer` の 3 フィールドを置き換え。
テスト用に `AgentCatalog::fixed(orch, impl, reviewer)` = 1 つだけのハーネス `default`)。

### 15.3 保存 (マイグレーション `0003_agent_settings.sql`)

- `agent_settings (scope, role, settings JSON, updated_ms)`、主キー `(scope, role)`。`scope = ''` が全体、それ以外はプロジェクト ID。
  行が無い = その層では継承。`Core::start` で全行を `AgentCatalog` に読み込み、`set_agent_settings` は DB に書いてから写しを更新する。
- `tasks.agent` / `tasks.review_agent` (JSON の `AgentChoice`、NULL = 上書きなし) — オーケストレータの `create_task` の上書き。
  ドメインの `Task.agent` / `Task.review_agent` (イベント `task_created` に含まれる。古いイベントは `serde(default)` で無し)。
- `agent_sessions.harness` / `agent_sessions.model` — そのセッションが実際に使ったもの (`session_started.agent`)。
  **7d (マイグレーション `0004_agent_effort.sql`)**: `agent_sessions.effort` を追加 (復元するセッションは記録した effort のまま)。

### 15.4 セッション起動時の解決 (runtime)

ドライバがセッションごとに `AgentPick { role: AgentRole, over: Option<AgentChoice> }` を作る (タスクの kind と
`Task.agent` / `Task.review_agent`)。`Sessions::launch_parts` が:
1. `session/load` で復元するとき、`agent_sessions` に記録されたハーネス × モデルが登録簿にあればそれを使う
   (動いていた・中断したセッションは設定の変更に影響されない。記録の無い古い行は下の解決)。
2. それ以外は `AgentCatalog::resolve(project, role, over)`: `over` があればそれ (create_task の時点で候補内と検査済み。
   後で候補から外されても、そのタスクはそのまま使う)、無ければ**起動時点の**実効値の既定。`over` のハーネスが
   登録簿から消えていたら既定に戻す (警告ログ)。
   **Stage 7c-2**: `resolve` は `Resolved { choice, harness, replaced }` を返し、登録簿に無くて飛ばした選択 (タスクの上書き・役割の既定・
   復元するセッションの記録) を `session_started.replaced` に載せる。UI は会話 / エージェント出力に
   「`<harness/model>` is not available (not installed?); started … instead」をエラー行で出し、設定パネルはその役割に
   「⚠ … が見つかりません … 既定 (…) で起動します」を出す (設定は消さずに残す)。
3. `HarnessPreset::config(role, model)` で起動し、`session_started.agent` に記録する。
- したがって**設定の変更は新しく起動するセッションから効く**: 動いているセッション (タスク中の implementer) は
  そのまま。レビューの Step は毎回新しいセッションなので、次のレビューから新しい既定になる。
- **レビューのハーネス × モデル**: `Task.review_agent` (create_task の `review_harness` / `review_model`) があればそれ、
  無ければ reviewer の役割の既定。

### 15.5 `create_task` の上書きとオーケストレータへの提示

- `create_task` の任意引数 `harness` / `model` / `effort` (implementer か investigator の候補から) と
  `review_harness` / `review_model` / `review_effort` (reviewer の候補から) ([`mcp-tools.md`](mcp-tools.md) §3)。ドライバが
  状態機械に渡す前に `RoleSettings::pick` で検査し、**組が行と完全一致しなければ** `invalid_argument` (message に全行
  `harness=… model=… effort=… (用途メモ)`)。同じハーネス × モデルの行が複数あって effort が無ければ「`effort` も指定して」と
  行を挙げて拒否する。通れば引数を解決済みの組 (effort も埋める) に書き換えて渡す (状態機械は純粋なまま、`Task.agent` に記録するだけ)。
- `get_status` の戻り値に `agents: { implementer, investigator, reviewer: { default, allowed: [行 (用途メモ付き)] } }` をドライバが足す
  (呼んだ時点の実効値)。オーケストレータのシステムプロンプトには起動時点の同じ一覧を**役割ごとに行 + 用途メモ**で付け
  (`prompts::agent_choices_prompt`)、「普段は省略する」「用途メモを読んで行を選ぶ」「行と完全一致させる」「最新は get_status」と書く。

### 15.6 モデル一覧 (`agents/models.rs`)

- `list_harness_models{harness, refresh?}`: preset の `probe` (無ければ implementer の設定から mode / model の切り替えを外したもの) を
  `data_dir/model-probe` で**プロンプトを送らずに**起動し (`session/new` だけ。モデルは呼ばない)、`Ready` の `config_options` から
  モデルの選択肢を読んで止める。モデルの選択肢 = `category: model` の select、無ければ id が preset のモデル config id (`"model"`) の select。
  グループ付きの選択肢は平らにする。見つからなければ `models: []` (そのハーネスはモデルを選べない = 既定だけ)。
- 応答 `HarnessModels { harness, models: [{value, name, description?, efforts}], current (ハーネスの既定値), fetched_at_ms }`。
- キャッシュ: 成功 10 分・失敗 10 秒、`refresh` でも 10 秒以内の結果は再利用。1 度に 1 プローブ。終了時に実行中のプローブを止める (使用量と同じ)。
- **effort 一覧 (7d)**: effort の選択肢 (`id: effort` の select、無ければ `category: thought_level`) は**現在のモデルによって変わる**
  (Claude Code アダプタは `supportedEffortLevels` から作り、先頭に古いクライアント用の `default` を足す。モデルを変えると作り直す。
  OpenCode は variant)。そこで同じプローブのセッションで**モデルを 1 つずつ `set_config_option` で選び、返ってきた
  `configOptions` の effort を読む** (モデルの呼び出しは無い)。`default` の値は「指定なし」と同じなので一覧から除く。
  - モデルが 12 個以下 (Claude Code は 5 個) のハーネスは、モデル一覧のプローブの中で全モデルを読む (`ModelOption.efforts = Some(..)`、
    空 = そのモデルに effort は無い)。実測: Claude Code のプローブ全体が約 2.7 秒 (モデル一覧だけのときは約 2 秒)。
  - それより多い (OpenCode は約 480 個) ハーネスは一覧では読まず (`efforts = None`)、UI が選ばれたモデルについて
    `list_model_efforts{harness, model}` で**そのとき 1 つだけ**読む (プローブのセッションを 1 つ起動: OpenCode で約 1.2 秒)。
    結果は (ハーネス, モデル) ごとに 10 分 (失敗 10 秒) キャッシュし、モデル一覧のキャッシュに載っていればそれを返す。
    480 個を全部読むと 480 回の `set_config_option` になるので避けた。
  - ハーネスが拒否するモデルは `efforts = None` のまま (一覧のプローブ) / `unavailable` (`list_model_efforts`)。

- **モデルの出どころ (7e、`ModelSource`)**: `Acp` (上のとおり) か `Codex`。`Codex` の preset は一覧を取る時に**ユーザーの Codex 設定** (`$CODEX_HOME/config.toml`、
  `agents/codex_config.rs`) のトップレベル `model_provider` を読み、`openrouter` なら OpenRouter の公開 API (`agents/openrouter.rs`、認証なし、`tools` 対応のモデルだけ、
  effort は `reasoning.supported_efforts`) から `HarnessModels` を作る (`efforts` は全モデルで `Some`、`current` は `None`)。それ以外はアダプタの一覧 (プローブ)。
  `ModelService` は環境の参照 (`with_env`) と取得先 URL (`with_openrouter_url`) を差し替えられる (テスト)。`list_model_efforts` は OpenRouter のときは一覧から返す。
  詳細は [`acp-harnesses.md`](acp-harnesses.md) §9.4。
- `probe_models` / `probe_efforts` / `efforts_from_options` は先頭または末尾に `&Secrets` / `config_id` を取る (プローブのエージェントにも秘密の環境変数を渡すため、
  effort の config id が preset ごとのため)。

### 15.7 API

- `get_agent_settings{project?}` → `agent_settings{settings: AgentSettingsView{harnesses: [{id, label}], builtin, global, project, project_layer, effective}}`
  (`project` があればその実効値。プロジェクトは登録済みであること、開いていなくてよい)。
- `set_agent_settings{project?, role, settings: RoleSettings | null}` → 同じ `agent_settings` (更新後)。`null` = その層の役割を消す
  (プロジェクトなら全体を継承、全体なら組み込みの既定)。検査に通らなければ `invalid_argument`。
- `list_harness_models{harness, refresh?}` → `harness_models{models}`。未知のハーネスは `not_found`、起動・取得の失敗は `unavailable`。
- `list_model_efforts{harness, model}` (7d) → `model_efforts{efforts: {harness, model, efforts: [{value, name, description?}]}}`。エラーは同上。
- `list_secret_env` / `set_secret_env{name, value}` / `delete_secret_env{name}` (7e) → `secret_env{names}` (§16)。
- **設定 UI (7d)**: アプリ全体の設定モーダル ([`orchestrator-desktop.md`](../design/orchestrator-desktop.md) §8)。API は上のとおりで、
  行の編集はすべてクライアント側で「役割の設定全体を `set_agent_settings` で置き換える」形 (同じ組の重複は UI が先に断る)。

## 16. 秘密の環境変数 (Stage 7e、`crates/yhtye-core/src/secrets/`)

Codex の認証キーのように、**エージェントプロセスの環境**に置きたい秘密をユーザーが登録する (アプリ全体、プロジェクトごとではない)。

- **保存**: 値は **OS のキーリング** (Linux は Secret Service = gnome-keyring / KWallet、macOS は Keychain、Windows は Credential Manager)。`keyring` crate 3.6
  (Linux は `sync-secret-service` + `crypto-rust`、`libdbus` が要る)、service `yhtye`、account = 変数名。Yhtye の DB (マイグレーション `0005_secret_env.sql`、
  テーブル `secret_env_names`) は**名前だけ**。値をログ・エラー・API の応答・`Debug` 出力に出さない (`SecretValue` / `SecretEnv` は `Debug` で伏せる。
  エラーメッセージは keyring のエラー文だけで値を含まない)。値は保存後にフロントエンドへ返さない (UI は「登録済み」、上書き・削除のみ)。
- **型**: `trait SecretBackend { set, get, delete }` (`KeyringBackend` = 本番、`MemoryBackend` = 単体テスト用、失敗を注入できる)、`Secrets { backend, names }`
  (`set` / `remove` / `names` / `env`)。ブロッキングの呼び出しは `spawn_blocking`。`CoreConfig::secret_backend` で差し替え (`CoreConfig::claude_code` は `KeyringBackend`)。
- **検査** (`validate_secret_name` / `validate_secret_value`): 名前は `[A-Za-z_][A-Za-z0-9_]*` で 128 文字まで、Yhtye 自身の `YHTYE_BRIDGE_TOKEN` /
  `VITE_YHTYE_BRIDGE_TOKEN` (エージェントに渡さない変数)、プログラムの読み込み・探索を変える `PATH` / `HOME` / `NODE_OPTIONS` / `PYTHONPATH` / `PYTHONSTARTUP` /
  `LD_*` / `DYLD_*` は拒否。DB の行を手で書き換えて禁止名を入れても、`Secrets::env` が飛ばす。値は空でなく NUL を含まず 64 KiB まで。ストアを触る前に検査する。
- **注入**: `spawn_agent_with_secrets(secrets, harness, cwd, options, events)` が全登録変数の値を読んで `SpawnOptions.secret_env` に入れ、`acp/process.rs` が
  子プロセスの環境に足す。**適用順は 秘密 → 削除 (`PRIVATE_ENV` と `HarnessConfig.env_remove`) → ハーネスの `env`** なので、ハーネスの `env`
  (`ANTHROPIC_MODEL`、`CODEX_PATH`、`CODEX_CONFIG` …) も削除も衝突時は秘密に勝つ (秘密が起動設定を壊せず、消した変数を復活させられない)。サブエージェント・オーケストレータ (`runtime/launch.rs`・`sessions.rs`)、モデル一覧のプローブ (`agents/models.rs`)、
  使用量のプローブ (`usage/probe.rs`) の**すべて**が通る (`AgentCatalog::secrets()` / `ModelService` / `UsageService` が `Arc<Secrets>` を持つ)。
  値が読めない (キーリングが使えない・ロック中) ときは、登録が 1 つでもあれば**起動を失敗させる** (`AgentError::Startup`、step `secret environment variables`、
  メッセージにキーリングのエラー)。キーが黙って抜けたエージェントが後から分かりにくい失敗をするより明確なため。登録があるのに値が無い名前は警告ログだけで飛ばす。
- **整合**: `set_secret_env` は名前を DB に登録してから値をキーリングに保存し、後者が失敗したら前者を取り消す (もともと登録済みの名前は残す)。
  `delete_secret_env` は DB から消してからキーリングを消し、失敗したら名前を戻す。DB とメモリ / キーリングが食い違わない。
- **既知の限界 (レビューで確認、今回は対応しない)**: すべての秘密が**すべてのエージェント**に渡る (ハーネスごとの絞り込みは無い。UI の説明にも書いた)。エージェントが
  自分の環境を stderr に出すと、その行が Yhtye のログ・エラーに載りうる (値の伏せ字化はしていない)。メモリ上の値の zeroize はしない。
  キーリングが使えないと、登録が 1 つでもあれば**全エージェントの起動が失敗する** (フェイルクローズ、ユーザー決定の範囲内)。
- **API**: `list_secret_env` → `secret_env{names}` (ソート済み)。`set_secret_env{name, value}` (キーリングに保存してから名前を DB に登録。上書き可) と
  `delete_secret_env{name}` (値と名前を消す。無い名前でも成功) は更新後の `secret_env{names}` を返す。不正な名前・値は `invalid_argument`、キーリングが使えなければ
  `unavailable` (メッセージにキーリングの理由)。`ApiCommand` の `Debug` でも値は `<redacted>`。
- **UI**: 設定モーダルの項目「秘密の環境変数」 ([`orchestrator-desktop.md`](../design/orchestrator-desktop.md) §8)。
- **テスト**: 単体 (名前と値の検査、`Debug` に値が出ない、キーリング不可の扱い)、`tests/acp_secret_env.rs` (実プロセスの環境に入る・ハーネスの `env` が勝つ・読めなければ起動しない)、
  `tests/secret_env_api.rs` (Core の 3 コマンド、再起動後も名前が残る、値が応答に出ない)、`store` のテスト、実キーリングの往復 1 件 (`--ignored`)。
