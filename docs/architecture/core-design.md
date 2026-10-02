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
    pub detection: Option<DetectionConfig>,  // T-6: Some のとき、実行ファイルが見つかったハーネスだけを登録する (harnesses / default_agent は無視。§15.2)
    pub mcp_bind: SocketAddr,                // 既定 127.0.0.1:0
    pub domain: DomainConfig,
}
// CoreConfig::claude_code(data_dir, "haiku") が Claude Code だけを登録し、全役割の既定を claude-code × haiku にする (detection なし。テスト用)。
// CoreConfig::installed(data_dir, "haiku") はアプリと開発ブリッジの設定: 上に detection を足し、見つかったハーネスだけを登録する。
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
  **T-6 (§15.2、§15.7)**: `get_harnesses` / `detect_harnesses` / `set_harness_path{harness, path?}` (ハーネスの検出状態・再検出・手動パス)。
  **Stage 8 (§17)**: `send_user_message{project, chat, text}` / `cancel_orchestrator_turn{project, chat}` にチャットが付き、
  `list_chats{project}` / `create_chat{project, branch}` / `create_branch{project, name, from?}` が加わる。
  `ApiResponse` = `projects` / `project` / `snapshot` / `events` / `accepted` / `git_overview` / `usage` /
  `agent_settings` / `harness_models` (T-6 で `harnesses`、Stage 8 で `chats` / `chat`)。
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
**T-5**: `choose_permission(options, policy)` は `HarnessConfig::permission_policy` を取る。`Default` は上のとおり。`OnceOnly` (Devin、MiniMax Code) は kind が `allow_once` で
id が `switch_` / `plan_` で始まらず `_always` で終わらないものだけを選ぶ (無ければ `Cancelled`。[`acp-harnesses.md`](acp-harnesses.md) §10.6)。

### 3.4 `HarnessConfig`

```rust
pub struct HarnessConfig {
    pub command: String, pub args: Vec<String>, pub env: BTreeMap<String, String>,
    pub mode_after_new: Option<String>,        // Claude Code: "bypassPermissions"
    pub model: Option<ModelSelect>,            // { config_id: "model", value: "haiku" }
    pub system_prompt: SystemPromptStyle,      // MetaAppend | FirstPrompt
    pub session_meta: Option<Map<String, Value>>, // session/new・load の _meta に足すハーネス固有フィールド (Stage 2)
    pub startup_timeout: Duration,             // 既定 120 秒 (npx の初回ダウンロードを見込む)
    pub client_info: Option<ClientInfoOverride>,   // T-5: initialize の clientInfo の上書き。None (既定) = "yhtye" (acp-harnesses.md §10.4)
    pub permission_policy: PermissionPolicy,   // T-5: Default | OnceOnly (§3.3)
}
// HarnessConfig::claude_code("haiku") が Claude Code 用の既定値、
// オーケストレータも同じ設定 (Stage 8d で組み込みツールの制限 = `claude_code_orchestrator` を廃止。`orchestration-model.md` §8.1)。
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
  `rebind` / `revoke`・停止。セッションキーは `orchestrator:<chatId>` (Stage 8。以前は `orchestrator`、§17) / `T-n/implementer` (タスク中は同じ
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
  `inbox_delivered`。Stage 8 で `chat_created` / `chat_titled` が加わる (§17)。
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
- **`harness_paths`** (T-6、マイグレーション `0008_harness_paths.sql`): `(harness TEXT PRIMARY KEY, path TEXT, updated_ms)`。ユーザーが指定したハーネスの主実行ファイルの
  手動パス。行が無い = 自動検出。`Store::harness_paths()` (全行を `HashMap<id, path>` で) と `Store::set_harness_path(harness, Option<path>, now_ms)`
  (`None` は行の削除)。既存データは壊さない (追加のみ)。使い方は §15.2。

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

> **Stage 8 で変更 (§17.3)**: オーケストレータの起動・復元は**チャットごと**で、復元対象のチャット (live / open グループあり / 未配達の受信箱あり) だけを起動時に復元し、
> 他は最初の送信で遅延起動する。以下の 2・3 は各チャットに適用する。

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

### 8.2 チャットごとのオーケストレータ (Stage 8)

ループはプロジェクトに 1 つのまま、オーケストレータのセッションだけがチャットごとになる。詳細は §17.3。

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
- `pnpm dev:browser [ポートの指定] [ブリッジの引数]` (`scripts/dev-browser.mjs`) がブリッジをビルドして Vite と一緒に
  起動し、同じランダムトークンを両方に渡す。片方が終わるか Ctrl+C で両方止める。
  ポートは既定 Vite 1420 / ブリッジ 1422 で、`--vite-port` / `--bridge-port` / `--port-base` (環境変数
  `YHTYE_VITE_PORT` / `YHTYE_BRIDGE_PORT` / `YHTYE_PORT_BASE`) で変えられる。変えたときは、ブリッジに `--port` と
  ページの Origin (`--allow-origin`)、Vite に `--port` と `VITE_YHTYE_BRIDGE_URL` を渡して揃える
  (`tauri dev` が使う `vite.config.ts` の 1420 と `tauri.conf.json` の `devUrl` は変えない)。

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
  プローブするハーネスは `set_harness` で差し替えられる (ハーネスの再検出で `npx` の解決済みパスが変わったとき。変わらなければ何もしない)。
  差し替えるとキャッシュ済みの結果を捨てる。状態は await をまたいでロックしないので、実行中のプローブを待たず、
  差し替え前に始まったプローブの結果は (呼び出し元には返るが) キャッシュしない (世代番号)。
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
  最初のものを残して取り除く。モデル名は検査しない (一覧は遅れて取るため。UI は一覧にあるものだけを選ばせる)。
  effort は、保存 (`SetAgentSettings`) のときに**新しい行**だけを読み取り済みの effort 一覧と照合する (`RoleSettings::check_efforts`、T-21): 一覧に無い値・
  effort の無いモデル (一覧が空。Devin の無料プランなど) の effort・モデル未指定の行の effort は拒否する。一覧をまだ読んでいなければ受け付け、
  そのモデルが effort を持たなければ**セッション起動が失敗する** (§15.4)。保存済みの行は照合しない (ハーネスが消えた行と同じ扱い)。
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
`orchestrator_read_only` (オーケストレータの設定がファイルを書けない。`false` は UI で警告) を追加し、`HarnessInfo` にも載せた
(**Stage 8d で削除**: 全ハーネスのオーケストレータが書けるので、フラグ・警告とも無い)。
`HarnessPreset::opencode(fallback_model, env_remove)` ([`acp-harnesses.md`](acp-harnesses.md) §7.6)。
アプリと開発ブリッジは `CoreConfig::installed(data_dir, model)`。当初は `agents::installed_presets` が Claude Code を常に、OpenCode を
`PATH` に実行可能な `opencode` があるときだけ登録していたが、T-6 で下の検出に一般化し `installed_presets` は無くなった。

**Stage 7e (Codex)**: preset に `model_config_env: Option<String>` (モデルと effort を JSON で渡す環境変数。Codex は `CODEX_CONFIG`。設定すると
`HarnessPreset::config` が `{"model","model_reasoning_effort"}` を組み立てて入れ、effort は option として送らない)、`effort_config_id` (effort の config id。
既定 `effort`、Codex は `reasoning_effort`。以前のグローバル定数 `EFFORT_CONFIG_ID` は既定値としてだけ残る)、`model_source: ModelSource { Acp, Codex }`
(§15.6) を追加。`HarnessPreset::codex(codex_path)` ([`acp-harnesses.md`](acp-harnesses.md) §9): 全役割 `HarnessConfig::codex` (npx で `codex-acp@2.0.0`、
`CODEX_PATH`、`agent-full-access`、`FirstPrompt`)、`requires_model = true`。登録は `codex` が見つかったときだけ (当初は `PATH` のみ。T-6 で `npx` も要り、既知の場所も探す)。

**T-5 (Devin)**: `HarnessPreset::devin(command)` ([`acp-harnesses.md`](acp-harnesses.md) §10): 全役割 `HarnessConfig::devin` (`devin acp`、`bypass`、`FirstPrompt`、
`PermissionPolicy::OnceOnly`)、`requires_model = false`、`model_source = Acp`、effort の config id は既定の `effort` (実際の option の id は `acp::effort_option` が
`category: thought_level` から解決する)。probe は mode の切り替えなし。**実機未検証**。

**T-26 (MiniMax Code)**: `HarnessPreset::minimax_code(command)` ([`acp-harnesses.md`](acp-harnesses.md) §11): 全役割 `HarnessConfig::minimax_code` (`mcode acp`、**mode も `permissionMode` も設定しない**
(`permissionMode` はユーザーの全体設定 `~/.minimax/config.yaml` に書き込まれるため)、既定のモデル `MINIMAX_CODE_DEFAULT_MODEL`、`FirstPrompt`、`PermissionPolicy::OnceOnly`、注記なし)、
`requires_model = true`、`model_source = Acp`、effort の config id は実際の `thinkingEffort` (モデルの後にだけ出る option)。probe はモデルの切り替えなし。
preset の `unusable_models` (一覧に出るが選ぶと拒否されるモデル。MiniMax Code の thinking なしの Flash Preview) は `probe_models` が一覧から除く。

**ハーネス検出の一般化と登録簿の動的化 (Devin 対応, T-6、`agents/detect.rs` / `agents/installed.rs` / `runtime/core/harnesses.rs`)**:
登録は「インストール済みのものだけ」に一般化した。

- **検出** (`detect_harnesses`、純粋関数): 実行ファイルの検索だけで `--version` は実行しない。`PATH` (絶対パスの項目、実行可能な通常ファイル) →
  既知の場所 (`KNOWN_DIRS` = `~/.local/bin`・`~/.cargo/bin`・`~/.bun/bin`・`/usr/local/bin`、`~` は環境の `HOME`) の順。必要なコマンドは `HARNESS_SPECS`:
  claude-code = `npx`、opencode = `opencode`、codex = `codex` + `npx`、devin = `devin`、minimax-code = `mcode` (claude CLI は不要)。UTF-8 でないパスは数えない。
  `HarnessSpec` の `root_env` / `extra_dirs` でハーネス専用の探す場所を足せる (T-26): minimax-code は `PATH` → `$MCODE_INSTALL_ROOT/bin` (絶対パスのとき) → 共通の既知の場所 → `~/.minimax-code/bin`
  (インストーラの標準の場所で、fish などでは `PATH` に入らない)。ほかのハーネスは足さない。
- **`HarnessDetection { id, label, installed, resolved_path, path_source (override|path|known_dir|none), override_path, override_error, requirements: [{command, found}] }`**
  (ts-rs で `src/api/generated/`)。`installed` = 必要なコマンドがすべて見つかった (手動パスがあるならそれが使える)。`resolved_path` は主実行ファイル (`HarnessSpec::main`)。
- **preset** (`presets_from`): installed なものだけ preset にする。見つけた絶対パスが `HarnessConfig.command` になる (`HarnessPreset::with_command`。claude-code / codex は npx のパス、
  opencode / devin / minimax-code は本体。codex は `CODEX_PATH` にも見つけた `codex` を渡す)。起動時の使用量取得 (`/usage`) のエージェントも、見つけた `npx` で起動する
  (`Detected::with_found_npx`。使用量のプローブは起動時に 1 回だけ作り、再検出では作り直さない)。
- **手動パス** (テーブル `harness_paths`、マイグレーション `0008`、§6): 各ハーネスの主実行ファイル (claude-code は npx) **だけ**を置き換える (codex の `npx` は自動検出のみ)。
  保存時に「絶対パスかつ実行可能な通常ファイル」を検査する (`check_executable_path`、`invalid_argument`)。保存後は検索より優先し、後で壊れたらそのハーネスは
  未インストール扱い (`override_error` に理由。検索にはフォールバックしない)。
- **組み込みの既定** (`default_choice`): claude-code > opencode > devin > minimax-code > codex の最初のインストール済み (モデルは opencode = `OPENCODE_FALLBACK_MODEL`、devin = 未指定、
  minimax-code = `MINIMAX_CODE_DEFAULT_MODEL` (最も安い Flash Preview thinking)、
  codex は他に無いときだけで、起動時に「設定でモデルを選んで」と案内して失敗する)。1 つも無くても Core は起動し、エージェント開始時に
  「使えるハーネスがありません (設定 › ハーネス を確認)」を出す (`AgentError::Setup`、`NO_HARNESS_MESSAGE`)。
- **登録簿の動的化**: `AgentCatalog` の登録簿 (`Registry { presets, builtin }`) は `RwLock` に入り、`AgentCatalog::refresh(presets, builtin)` が**丸ごと入れ替える**
  (動いているセッションには影響しない: 新しく起動するセッションから効く)。`harnesses()` / `harness_ids()` / `preset()` / `builtin()` / `resolve()` / `validate()` は呼んだ時点の登録簿を読む。
  ロックの毒化 (パニック) は無視して続ける。
- **`available()` / `available_settings(project)`**: 実効値から**未登録ハーネスの行を除く** (既定が除かれたら、組み込みの既定が行に残っていればそれ、無ければ最初に残った行、
  行が 1 つも残らなければ組み込みの既定だけ)。オーケストレータのプロンプト (`runtime/launch.rs`)・`get_status.agents`・`create_task` の検証 (`runtime/driver.rs`) はこれを使う。
  保存データと設定パネル用の `AgentSettingsView` は変えない (行は残り、UI が「(見つからない)」を出す)。
  セッション起動の解決 (`resolve`) も既定は同じ `available()` の既定を使う (プロンプトで既定と見せたものと、ハーネス省略のタスクが動くものを一致させる。
  保存済みの既定と違えば `replaced` に載せる)。設定の保存 (`validate(settings, existing)`) は、いま実効の行 (`existing`) にある未登録ハーネスの行 (ハーネス × モデル × effort が同じもの)
  はそのまま通し、新しく足した未知のハーネスの行だけを `unknown harness` で拒む (他の行の編集や note の変更が、戻せるよう残した行のせいで失敗しないように)。
- **`Core` のコマンド** (`runtime/core/harnesses.rs`、`DetectionConfig { env, known_dirs, claude_model }` を `CoreConfig::detection` に持つ):
  - `Core::start` が `harness_paths` を読んで検出し、登録簿を作る (各ハーネスの検出結果を 1 行ずつログに出す)。
  - `GetHarnesses` (設定の「ハーネス」タブを開いたとき) は検出をやり直して登録簿を入れ替え、`HarnessDetection` の一覧を返す。
    入れ替えの前後でプリセットが変わった (起動する実行ファイルが変わった・新しく見つかった・見つからなくなった) ハーネスのモデル / effort のキャッシュ
    (`ModelService::invalidate`) を捨て、Claude Code の使用量プローブの `npx` を見つかったものに更新する (`UsageService::set_harness`)。変わっていなければキャッシュは残る。
  - `DetectHarnesses` (「再検出」ボタン) は同じことに加えて、全ハーネスのモデル一覧のキャッシュを捨てる。
  - `SetHarnessPath{harness, path?}` は未知のハーネスを `not_found`、絶対パスの実行可能な通常ファイルでないものを `invalid_argument` で拒み、`harness_paths` に保存 (`None` は削除) して
    再検出し (そのハーネスのキャッシュは上のとおり捨てられる)、一覧を返す。
  - 並行する検出は `Inner::detecting` (`tokio::sync::Mutex`) で直列化する (最後に終わる検出が最新の手動パスを読んでいる)。
  - `CoreConfig::detection` が `None` (テスト) なら従来どおり静的な `harnesses` / `default_agent` を登録し、3 つのコマンドは空の一覧を返す (`set_harness_path` は保存して検査する)。

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
   後で候補から外されても、そのタスクはそのまま使う)、無ければ**起動時点の**実効値の既定 (未登録ハーネスの行を除いた `available()` の既定)。
   `over` のハーネスが登録簿から消えていたら既定に戻す (警告ログ)。
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
  `invalidate` (実行ファイルが変わったハーネスのモデルと effort を捨てる) はキャッシュ (await をまたいでロックしない) とハーネスごとの世代番号を更新するだけで、
  実行中のプローブを待たない。無効化より前に始まったプローブの結果は (呼び出し元には返るが) キャッシュしない。
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
- `get_harnesses` / `detect_harnesses` → `harnesses{harnesses: [HarnessDetection]}` (T-6, §15.2): 検出をやり直して登録簿を更新し、全ハーネスの状態を返す
  (`detect_harnesses` はモデルのキャッシュも捨てる)。`detection` が無いコアは空リスト。
- `set_harness_path{harness, path?}` (T-6) → 同じ `harnesses`。`path: null` は手動パスの削除 (自動検出に戻る)。未知のハーネスは `not_found`、
  絶対パスの実行可能な通常ファイルでなければ `invalid_argument`。保存後に再検出し、そのハーネスのモデルのキャッシュを捨てる。
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

## 17. チャットとブランチ (Stage 8)

仕様と決定の理由: [`orchestration-model.md`](orchestration-model.md) §2.0 / §5 / §6.1 / §10、画面: [`orchestrator-desktop.md`](../design/orchestrator-desktop.md) §3.3 / §3.4 / §8。
ここでは型・テーブル・API・runtime の変更を定める。**旧データ (単一オーケストレータの履歴) は移行せず破棄する** (ユーザー決定)。

### 17.1 型 (`domain`)

- `Chat { id: String /* "C-1" */, branch: String, title: Option<String> }` — `State.chats: Vec<Chat>` (作成順)。**Stage 8e で `branch` は `worktree: PathBuf` に置き換え** (§17.8)。
  `Counters.chats` を足す。時刻は状態機械に入れない (§5) ので `created_ms` / `last_used_ms` は store が導く。
- `Group.chat: String` を足す。`Group.base_branch` = そのチャットの `branch`。
  `InboxEntry.chat: String` を足す (宛先チャット、orchestration-model §5)。
- `DomainEvent::ChatCreated { chat: Chat }` / `ChatTitled { chat: String, title: String }` (最初の `UserMessage` で 1 回、
  先頭 40 文字 + 改行は空白に、で切り詰める)。
- `DomainCommand`:
  - `CreateChat { branch }` (ブランチの存在確認は runtime が git で行い、ここは形式だけ)、
  - `UserMessage { chat, text }` (未知のチャットは `not_found`)、
  - `Restart { orchestrators: Vec<(chat, OrchestratorResume)> }` (チャットごと。§8.1 の規則を各チャットに適用)、
  - `OrchestratorTurnEnded { chat, outcome, prompt_queued }`、
  - `CreateGroup { chat, args, base_branch }` — `base_branch` は runtime が渡さず **チャットの `branch`** を使う
    (detached HEAD の `invalid_state` は無くなる)。open グループの上限チェックは `base_branch` 単位
    (別チャットのグループがあれば `conflict`。メッセージに `chat` を含める)。
  - `Effect::WakeOrchestrator { chat }`。
- 各 `Tool` 呼び出し (`SessionBinding` はオーケストレータならチャット ID を持つ) は**自分のチャットのグループ・タスクだけ**を操作できる。
  他のチャットのものは `forbidden`。`get_status` も自分のチャットのグループだけを返す。

### 17.2 store (マイグレーション `0006_chats.sql`)

- 旧データを破棄する: 各プロジェクトの `events` / `task_groups` / `tasks` / `task_deps` / `steps` / `helps` / `inbox` /
  `agent_sessions` の行を削除し `projects` のカウンタを 0 に戻す。`projects` の行・エージェント設定・`secret_env_names` は残す。
- `chats (project_id, id, ord, branch, title, created_ms, last_used_ms, PRIMARY KEY (project_id, id))` (`ord` = 作成順、`State.chats` の順)。
  `created_ms` は `chat_created` の、`last_used_ms` はそのチャットのセッションの `prompted` の `ts_ms` (`agent_sessions.updated_ms` と同じ導き方)。
- `task_groups.chat_id` / `inbox.chat_id` を `NOT NULL DEFAULT ''` で追加 (SQLite は既定値なしの `NOT NULL` 列を追加できない。旧行は先に削除済みなので既定値は使われない)。
- `agent_sessions.cwd TEXT` を追加 (復元可否の判断、orchestration-model §6.1)。オーケストレータのセッションキーは `orchestrator:<chatId>`。
  値は `session_started` イベントの新しい `cwd` (起動したディレクトリ) から導く。`SessionRecord` にも `cwd` が加わる。
- `Snapshot.chats: Vec<ChatInfo>` — `ChatInfo { id, branch, title, created_ms, last_used_ms }` (`State.chats` + `chats` テーブル)。
  ブランチが存在するかは含めない (UI は `GetGitOverview.branches` と突き合わせて `branch_missing` を出す)。

### 17.3 runtime

> Stage 8e で、ブランチから作業ツリーを解決するのはチャットの作成時だけになり、起動・マージはチャットの作業ツリー (固定) で行う (§17.8)。

- `Orchestration::start` は**オーケストレータを起動しない**。DB から `State` を読み、復元対象のチャット (orchestration-model §10:
  セッションが `stopped` 以外 / open グループあり / 未配達の受信箱あり) だけを `Restart` に渡して起動する。
  `Core::resume_unfinished` の判定 (active / finishing のグループか未配達の受信箱があるプロジェクトだけ開く) に、「live だったチャットのセッションがある」を足す。
- `runtime::sessions` はオーケストレータを**キー `orchestrator:<chatId>` の通常のセッション**として扱う (数はチャット数まで)。
  MCP トークンの束縛 (`SessionBinding`) がチャット ID を持ち、`create_group` などはここからチャットを知る。
- 起動 (`ensure_orchestrator(chat)`): ① `resolve_branch_worktree(branch)` (§17.4) で cwd を決める (失敗 = `branch_missing` → 送信は `invalid_state`)、
  ② `agent_sessions` に記録があり cwd が一致すれば `session/load`、③ 失敗 / 不一致 / 記録なし なら新しいセッション
  (記録ありだったときは `restarted` に要約)。起動するのは (a) `SendUserMessage`、(b) そのチャット宛ての受信箱の項目があるとき、(c) 復元対象。
- `flush_inbox(chat)` はチャットごと。そのチャットのセッションが**無い / 終了している**ときは起動してから送る (今の「アイドルになるまで待つ」だけの挙動を変える)。
  `on_exited` (オーケストレータ) は `SessionStopped` を出し `turn_running` を下ろすだけで再起動はしない。起動に失敗しても受信箱は残し、自動の再試行はしない。
- `CancelOrchestratorTurn{chat}` はそのチャットのセッションにだけ `session/cancel`。セッションが無ければ `accepted` (何もしない)。
- ユーザーによる `CancelTask` / `CancelGroup` の `user_message` は、そのグループのチャットの受信箱に積む。
- 履歴: `ListEvents` は従来どおりプロジェクト全体 (seq の連続性を保つ)。UI が `session` (`orchestrator:<chatId>`) と、ドメインイベントの `group.chat` /
  `inbox.chat` で会話を絞る。

### 17.4 git

> Stage 8e で、ブランチから作業ツリーを解決するのはチャットの作成時だけになり、起動・マージはチャットの作業ツリー (固定) で行う (§17.8)。

- `GitService` に追加:
  - `resolve_branch_worktree(branch) -> Result<PathBuf, BranchWorktreeError>` — orchestration-model §6.1 の規則
    (`git worktree list --porcelain` → 既存 or `worktree add`)。エラー = `BranchMissing` / `Failed(message)`。
  - `create_branch(name, from) -> Result<PathBuf, ..>` — `git check-ref-format --branch`、`yhtye/` 予約、既存ブランチは `Exists`、
    `git worktree add -b <name> <path> <from>`。`from` の既定はメイン作業ツリーの HEAD (ブランチ名、detached なら SHA)。
  - `list_branches()` — `GitOverview.branches` から `yhtye/*` を除く (UI が使う。`GitOverview` 自体は従来どおり全ブランチ)。
- `GitOp::MergeGroup` に `target_dir` を持たせず、`GitCli` が `base_branch` から毎回 `resolve_branch_worktree` して統合する
  (メイン作業ツリー前提の `cli.rs` の分岐を置き換える)。`NoopGit` は従来どおり (テスト用)。
- `worktree_root(data_dir, project_id)` の下に `branches/<sanitized-branch>` を追加 (グループ・タスクの worktree と同じ root)。

### 17.5 API (`api/command.rs`)

| コマンド | 内容 | 応答 |
|---|---|---|
| `list_chats{project}` | プロジェクトのチャット (`last_used_ms` の新しい順)。開いていなくても DB から読める | `chats{chats: Vec<ChatInfo>}` |
| `create_chat{project, branch}` | そのブランチの新しいチャットを作る (オーケストレータは起動しない)。ブランチが無ければ `not_found`、`yhtye/*` は `invalid_argument`。プロジェクトは開いている必要がある (`unavailable`) | `chat{chat: ChatInfo}` |
| `create_branch{project, name, from?}` | ブランチを Yhtye の作業ツリーに作り (§17.4) 最初のチャットを作る。名前不正・`yhtye/` 始まりは `invalid_argument`、既存は `conflict`、`from` が無ければ `not_found` | `chat{chat}` |
| `send_user_message{project, chat, text}` | (変更) チャットを指定。遅延起動 (§17.3)。`branch_missing` は `invalid_state` | `accepted` |
| `cancel_orchestrator_turn{project, chat}` | (変更) チャットを指定 | `accepted` |

- イベント: `domain { chat_created }` / `domain { chat_titled }` (durable)。ブランチ作成は git の状態なのでドメインイベントにせず、
  UI は `chat_created` と `create_branch` の応答で `GetGitOverview` を読み直す。
- `ProjectInfo` は変えない。`GetSnapshot` の `Snapshot` に `chats` を足す (§17.2)。TS 型は `pnpm gen:types` で再生成。
- テスト: 偽エージェントで — チャット 2 つが別 cwd (別ブランチ) で並行 / 同一ブランチの 2 つ目のグループが `conflict` / 遅延起動 (作成だけでは
  プロセスが起きない) / 終了後の送信で再起動 / `session/load` 失敗で `restarted` / 起動時に live のチャットだけ復元 / グループ通知が持ち主のチャットへ。
  実 git (`TempRepo`) で — 既存の作業ツリーの再利用 (メイン・管理外) / Yhtye 管理の作成 / 消えたディレクトリの再作成 / detached HEAD / ブランチの外部削除。

### 17.6 実装メモ (Stage 8a で仕様から具体化・変えたところ)

- `SessionBinding.chat: Option<String>` を足した (オーケストレータのセッションだけ。サブエージェントと UI からのユーザー操作 (`USER_SESSION`) は `None` = 制限なし)。
  `create_group` は `DomainCommand::CreateGroup { chat, args }` で、ブランチが存在するかの確認 (無ければ `invalid_state`) は runtime が git で行う。
- `DomainCommand::TellOrchestrator { chat, resume }` を足した。`Restart` が各チャットに行うことと同じ (前のセッションを復元できない → 状態の要約、ターンが途中で切れた → その旨を
  `restarted` として受信箱へ) を、アプリ起動以外の遅延起動・自動再起動でも使う。ターンが途中で切れたかは、アプリ起動時は保存済みの `turn_running`、
  実行中は `TurnEnded(Closed)` を見た印 (メモリ上) から分かる。**アプリごと再起動した後は、ターン中に終了して `stopped` になったチャットの「切れた」印は残らない**
  (`session_stopped` が `turn_running` を下ろすため。復元できれば会話は続くので実害は小さいとして許容した)。
- 起動は背景で行う (`Sessions::launch`)。復元 (`session/load`) が起動時に失敗したら、その場で新しいセッションを作り、最初のプロンプトを
  「状態の要約 (`lost_session_note`) + 受信箱のバッチ」に差し替える (サブエージェントの `ResumeStep` の fallback と同じ仕組み)。したがって、**復元を試みて失敗した場合の要約は
  受信箱の `restarted` 項目にはならない** (起動と同時に渡すだけ)。起動前から分かる場合 (記録があるが cwd が違う) は `restarted` 項目として受信箱に積む。
  要約は起動の瞬間の状態なので、再起動で再開したタスクは `interrupted` ではなく `running` と書かれる。
- 起動時に一緒に渡した受信箱の項目は、セッションが立ち上がってから `InboxDelivered` にする。起動に失敗したら受信箱に残り、
  チャットごとに「そのとき最新だった項目の id」を覚えて、それより新しい項目 (または新しい送信・アプリ起動) があるまで再試行しない。
- `GitService` に `branch_exists` も足した (チャット作成・送信・`create_group` の存在確認)。`list_branches` は trait ではなく自由関数 `git::list_branches(dir)`。
  `create_branch` は開始点が `-` で始まる場合を `FromMissing` にする (オプションとして解釈させない)。
- `Orchestration::start` はオーケストレータを起動せずすぐ戻る。起動時に復元するチャットの起動も背景 (ループの最初) で行う。
- `create_chat` / `create_branch` は、登録済みだが開いていないプロジェクトに対して `unavailable`、未登録なら `not_found`。
- フロントエンド (8a の暫定): 型検査を保つため、`AppStore` はチャット `C-1` 固定で送る (無ければメインクローンのブランチで `create_chat`)。オーケストレータのセッションキーは `orchestrator:C-1` 固定。8b で選択中のチャットに置き換える。

### 17.7 Stage 8d: 書けるオーケストレータ・ブランチ改名の追跡・作業ツリーの取り直し

仕様と理由: [`orchestration-model.md`](orchestration-model.md) §6.1 / §8.1。

> **Stage 8e で、この節の「ドメイン」「git の `was_renamed`」「runtime (`follow.rs`)」「フロントエンドの通知行」は撤去した** (§17.8)。残っているのは
> オーケストレータの制限の撤廃と `GitService::worktree_branch`。

- **オーケストレータの制限の撤廃**: `HarnessConfig::claude_code_orchestrator` と `ORCHESTRATOR_BUILTIN_TOOLS` を削除 (オーケストレータは `claude_code` と同じ設定)。
  `HarnessPreset.orchestrator_read_only` と `HarnessInfo.orchestrator_read_only` を削除 (`pnpm gen:types`)。MCP ツールは足さない。プロンプト `orchestrator.md` を書き換える。
- **ドメイン**: `DomainCommand::ChatBranchChanged { chat, to }` (runtime が改名を検出したとき) → `DomainEvent::ChatBranchChanged { chat, from, to }` (`from` は決定時のチャットのブランチ)。
  `apply`: `chat.branch = to`、`group.chat == chat` かつ終端でない (`done` / `cancelled` 以外 = `active` / `finishing` / `merge_blocked`) グループの `base_branch = to`。
  検査: 未知のチャットは `not_found`、`to` は `check_target_branch` (空・`yhtye/*` は `invalid_argument`)、`to` が今のブランチと同じなら何もしない。
  ストアは `chats` 表の `branch` と `task_groups.base_branch` を既存の「差分のある行を書く」で更新する (専用のマイグレーションは無い)。
- **git**: `GitService::worktree_branch(dir) -> Result<Option<String>, String>` を足す (`dir` がチェックアウトしているブランチ。detached HEAD・ディレクトリが無い・git の作業ツリーでない = `None`)。既定は `Ok(None)`。あわせて `was_renamed(from, to) -> Result<bool, String>` (既定 `Ok(false)`) を足す。
- **runtime** (`runtime/follow.rs`):
  - `follow_branch(chat) -> Result<String, ToolError>`: チャットのブランチが存在すればそれ。無ければ、そのチャットの記録された cwd (`agent_sessions.cwd`、ストアから読む) で
    `worktree_branch` を見て、別のブランチ (存在し `yhtye/*` でない) で、かつ `GitService::was_renamed(旧, 新)` (新しいブランチの reflog `git reflog show --format=%gs refs/heads/<新>` に `Branch: renamed refs/heads/<旧> to refs/heads/<新>` がある。無い・reflog が無効なら `false`) なら `ChatBranchChanged` を実行して新しいブランチを返す。それ以外は従来の `invalid_state`
    (「ブランチ `x` が削除または改名された」)。`check_chat_branch` を置き換える。
  - `realign_orchestrator(chat)`: セッションが live のとき `resolve_branch_worktree(branch)` の結果と記録された cwd を比べ、違えば、アイドル (ターン中でなく送信待ちも無い) なら `Sessions::stop`
    (その後の配達が遅延起動 = 新しい cwd・`session/load` せず・`restarted` の要約)、ターン中なら `Orchestrators.stale_cwd` に印を付け、`orchestrator_turn_ended` で再確認する。
    起動途中のセッションは何もしない (起動の解決が最新)。
  - 呼び出し点: `user_message` (`follow_branch`)、`flush_chat` の配達前 (`follow_branch` → `realign_orchestrator`、その後セッションが無ければ遅延起動)、`start_lazily` (`follow_branch`)、
    `create_group` (`follow_branch` + `realign_orchestrator`)、`finish_group` の前と `RetryGroupMerge` の前 (`follow_branch`)、`orchestrator_turn_ended` (`follow_branch`、印があれば `realign_orchestrator`。
    ドメインの `OrchestratorTurnEnded` より前に行い、Yhtye が代行する `finish_group` が新しい `base_branch` を使うようにする)。
    配達前・ターン終了時の `follow_branch` の失敗 (ブランチ削除) は無視して従来どおり進める (拒否するのは送信と `create_group`)。
- **フロントエンド**: `chat_branch_changed` を `chats` と `groups` の畳み込みに足し、会話のトランスクリプトに「ブランチ名が a → b に変わりました」の通知行を出す。ツリーはチャットを新しいブランチの下へ動かす。
- テスト: 偽エージェント + 実 git (`tests/orchestration_fake_chats.rs`、`tests/git_branches.rs`)、ドメイン (`domain/tests/chats.rs`)、ストア (`store/tests.rs`)、vitest。実機 (Claude Code Haiku): `tests/orchestration_claude_real.rs`。

### 17.8 Stage 8e: チャットを作業ツリーに紐づける

仕様と理由: [`orchestration-model.md`](orchestration-model.md) §2.0 / §2.1 / §6 / §6.1、[`PLAN.md`](../PLAN.md) Stage 8e。
理由の要点: ブランチ名は表示用。コマンドの結果に即座に反応して紐づけを書き換えるのは安全でないので、確認はマージの直前に 1 回、想定外はオーケストレータに返す。

- **ドメイン**:
  - `Chat { id, worktree: PathBuf, title }` (`branch` を削除)。`DomainCommand::CreateChat { worktree }` (runtime が解決・検査したパス)。
  - `DomainCommand::CreateGroup { chat, args, base_branch, taken }` — `base_branch` は runtime が作業ツリーの HEAD から読んで渡す。open の上限は
    **作業ツリーごと** (`State::open_group_in(worktree)` = チャットの作業ツリーが同じグループのうち `active` / `finishing`)。
  - `GitOp::MergeGroup { group, worktree, group_branch, base_branch, trigger }` — `worktree` = グループのチャットの作業ツリー。
  - `finish_group` は `active` (全タスク終端) と `merge_blocked` を受け付ける。`merge_blocked` からは `RetryGroupMerge` と同じく同じ作業ツリーの open グループがあれば `conflict`。
    `FinishGroupArgs.into: Option<String>`: `check_target_branch` の後、`base_branch` と違えば `DomainEvent::GroupBaseChanged { group, from, to }` (`group.base_branch = to`) を出してからマージ。
    `into` が作業ツリーの今のブランチと一致するかは runtime がツールを渡す前に確かめる (ドメインは git を読まない)。
  - 削除: `DomainCommand::ChatBranchChanged` / `DomainEvent::ChatBranchChanged` / `State::open_group_on(branch)`。
- **git**:
  - `GitCli::merge_group` は `worktree` で行う。直前の確認 (`base_mismatch`): ディレクトリが無い / detached HEAD / 別のブランチなら `GitResult::Blocked`
    (detail = 「base は X、作業ツリー … は今 Y / detached / 見つからない。何もマージしていない。戻して `finish_group` をもう一度、または `into: "Y"`」。LLM 向けなので英語)。
    その後は従来の clean の確認とマージ。base ブランチの作業ツリーを解決し直す処理は無くなった。
  - `resolve_branch_worktree` は `git worktree list` の表記のパスを返す (新しく作った作業ツリーも一覧から引き直す。UI がパスを文字列で突き合わせるため)。
  - `GitService::find_worktree(dir) -> Result<Option<PathBuf>, String>` (既定は `Ok(Some(dir))`): `dir` が登録済みで存在する作業ツリーなら一覧の表記のパス。
  - `GitService::worktree_branch(dir)` は表示・`create_group`・`into` の確認に使う。`NoopGit` は `base_branch` を返す (テスト用)。
  - 削除: `GitService::was_renamed`、`repo::was_renamed`。
  - `GitOverview.worktrees: Vec<GitWorktree { path, branch: Option<String>, head_sha: Option<String>, is_main, missing }>` (`git worktree list --porcelain`。
    `missing` = ディレクトリが無い)。UI のツリーと表示名はここから作る。
- **store**: マイグレーション `0007_chat_worktrees.sql` — **既存データを破棄** (0006 と同じ範囲: `events` / グループ・タスク / 受信箱 / `agent_sessions` / `chats`。
  カウンタを 0 に)、`chats.branch` を `chats.worktree` に置き換える (表を作り直す)。理由: 8d までのイベントログの `chat_created { chat: { branch } }` や
  `chat_branch_changed` は新しい型で読めず、互換用の欄を残すより破棄のほうが単純 (1a0e566 は未リリース、ユーザー了承済み)。`ChatInfo { id, worktree, title, created_ms, last_used_ms }`。
- **runtime**:
  - `runtime/follow.rs` を削除 (`follow_branch` / `realign_orchestrator` / `Orchestrators.stale_cwd`)。`stored_session` は `chats.rs` へ。
  - `create_chat(branch)` = `check_target_branch` → `resolve_branch_worktree` (無ければ `not_found`) → `CreateChat{worktree}`。
    `create_chat_in(worktree)` = `find_worktree` (無ければ `not_found`) → そのブランチが `yhtye/*` なら `invalid_argument` → `CreateChat`。
    `create_branch` は作った作業ツリーのパスでチャットを作る。
  - 送信 (`user_message`) と起動 (`plan_start`) は作業ツリーのディレクトリが無ければ `invalid_state` / 起動失敗 (「作業ツリー … が見つかりません」)。cwd = `chat.worktree`。
  - `create_group`: `worktree_branch(chat.worktree)` → ブランチ (`yhtye/*` 以外) を `base_branch` に。detached / 無い → `invalid_state`。
  - `finish_group{into}`: `worktree_branch(chat.worktree) == into` でなければ `invalid_argument` (今のブランチを message に)。
  - ターン終了時・配達前・`RetryGroupMerge` 前の追跡・取り直しは削除。
- **API**: `create_chat { project, branch?, worktree? }` (どちらか 1 つ。両方・どちらも無しは `invalid_argument`)。`Snapshot.chats` / `list_chats` は `ChatInfo` (worktree)。
  イベント `domain { group_base_changed }` を追加、`chat_branch_changed` を削除。`pnpm gen:types`。
- **フロントエンド**: ツリー (`ui/branchTree.ts`) = `GitOverview.worktrees` (Yhtye 内部の `yhtye/*` をチェックアウト中のもの・チャットの無い消えたものを除く) → チャット。
  表示名 = ブランチ / `detached @<sha7>` / 見つからない。チャットの作業ツリーが一覧に無ければ「(見つからない作業ツリー)」の下 (送信不可)。
  作業ツリーの無いローカルブランチは末尾の折りたたみ「他のブランチ」(そこからの「+ 新しいチャット」で Yhtye の作業ツリーを作る)。
  会話ヘッダ・ピルも作業ツリーの今のブランチ。git の概要は従来の契機に加えて、ウィンドウのフォーカスとオーケストレータのターン終了で読み直す (外部の改名・オーケストレータ自身の改名が表示に出るように)。
  `chat_branch_changed` の畳み込みと会話の通知行を削除、`group_base_changed` をグループの畳み込みに追加。
- テスト: ドメイン (`domain/tests/chats.rs`: 作業ツリーごとの上限・`create_group` の base・`into`・`merge_blocked` からの `finish_group`)、store、git (`tests/git_branches.rs`: 一覧の表記のパス・`find_worktree`・`worktree_branch`、
  マージ直前の確認)、偽エージェント + 実 git (`tests/orchestration_fake_worktree.rs` が `orchestration_fake_follow.rs` を置き換え: 改名で表示が変わる → 次の `finish_group` は `merge_blocked` で返る → `into` で改名後のブランチへ / メインクローンの checkout → 同じ / detached → 返る /
  代行の `finish_group` は受信箱の `merge_result` / 作業ツリーの消えたチャットは送信不可)、`core_chats`、vitest。実機 (Haiku): `real_chat_follows_a_renamed_branch_and_merges_the_next_group_into_it` を、
  途中で外部から改名し、食い違いを受けた Haiku が `finish_group{into}` でマージを完了するテストに書き換える。

### 17.9 チャットの改名・削除とホストの ping (左パネルの改善)

- **`ping`** → `pong{host}`: コアが動いているマシンのホスト名 (`gethostname` クレート、`runtime/host.rs`。空なら `localhost`) を返す軽い疎通確認。プロジェクトは要らない。
  UI (`AppStore`) は接続が開いている間 3 秒おきに送り (`PING_INTERVAL_MS`)、5 秒で応答が無ければ失敗として数える。左パネルのフッタは「ホスト名 + 最後の ping 応答からの経過時間
  (5 秒以下は `now`)」と接続状態のドット (緑 = 応答あり / 琥珀 = 接続中・初回応答待ち / 赤 = 切断か 12 秒以上応答なし) を出す。Tauri でも WS ブリッジでも同じ `ApiCommand` なので transport は変えない。
- **`rename_chat{project, chat, title}`** → `chat{chat}`: 空白を 1 つに畳んで trim、空は `invalid_argument`、80 文字 (`MAX_RENAMED_TITLE_CHARS`) 超も `invalid_argument`、未知のチャットは `not_found`。
  `DomainCommand::RenameChat` は既存の `DomainEvent::ChatTitled` を出す (同じタイトルなら何も出さない)。`user_message` は `title.is_none()` のときだけ最初のメッセージで題を付けるので、ユーザーの名前は上書きされない。
- **`delete_chat{project, chat}`** → `accepted`: `DomainCommand::DeleteChat` は `DomainEvent::ChatDeleted { chat }` を出す。
  - 拒否: 未知のチャットは `not_found`。**未完了のグループ** (`active` / `finishing` / `merge_blocked`) があれば `invalid_state` (グループを片付けられるのはそのチャットのオーケストレータだけ)。
    **オーケストレータがターン中 (または起動中)** なら runtime が `invalid_state` で断る (ターンを切るのはユーザーの `cancel_orchestrator_turn` の仕事で、削除は勝手に中断しない)。
  - 成功時: 待機中 (live で idle) のオーケストレータのプロセスは `Sessions::stop` で止める。`State::apply(ChatDeleted)` はチャットと、そのグループ・タスク・help・受信箱を落とす (カウンタは戻さない = ID は再利用しない)。
  - store (`projection::write` が before / after の差分で行う): `chats` / `task_groups` / `tasks` / `steps` / `task_deps` / `helps` / `inbox` の行と、`agent_sessions` のオーケストレータ (`orchestrator:<chat>`) とタスクのセッションの行を消し、
    残った `task_groups` / `tasks` / `helps` の `ord` を振り直す (後から入れる行と `ord` が重ならないように)。`chats.ord` は `MAX(ord) + 1`。
  - **イベントログは追記のみ** (UI が `seq` の連続を前提にしているので消さない)。削除したチャットの会話の履歴はログに残り、`chat_deleted` 以降の状態には出ない。
  - 作業ツリーとブランチには触れない (チャットの削除は会話の記録の削除)。
- フロントエンド: `store/domain.ts` と `store/chats.ts` に `chat_deleted` の畳み込み (Rust の `remove_chat` と同じ)、`store/project.ts` の `forgetChat` が会話・セッション・ストリーム・未読の印を捨て、表示中のチャットなら次のチャットを選ぶ。
  `AppStore.renameChat` / `deleteChat` はコアの拒否で reject し (行内の編集・確認が表示する)、成功時はイベントを待たずに反映する。
