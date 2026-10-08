# ACP と対象ハーネス

## 1. なぜ ACP か

Yhtye は自前でコーディングエージェントを実装しない。既存のハーネス (CLI/エージェント) を
プロセスとして起動し、[ACP (Agent Client Protocol)](https://zed.dev/acp) 経由でやりとりする。
ACP は元々 Zed が Claude Code / Gemini CLI などのエージェントをエディタに統合するために
作ったプロトコルで、「クライアント (エディタ・GUI) ↔ エージェント (ハーネス)」の
セッション管理・メッセージ・ツール呼び出し・パーミッション確認を JSON-RPC ベースで
標準化している。Yhtye にとっての利点:

- ハーネスごとに個別の統合コードを書かずに済む (対応ハーネスが増えても Yhtye 側は
  ACP クライアント実装 1 つで足りる)
- ハーネスの内部実装や CLI の出力フォーマットに依存しない
- セッションの生存期間・パーミッション要求・ツール実行の可視化が
  プロトコルレベルで定義されている

## 2. 対象ハーネス

**当面は Claude Code のみ**を実装・検証の対象にする。コアは汎用 ACP / MCP だけに依存し、
ハーネス固有の知識 (起動コマンド・環境変数・モード名・モデル指定方法) は
ハーネス設定 (`HarnessConfig`、[`core-design.md`](core-design.md) §3.4) に閉じ込める。
他ハーネスは実運用後に設定の追加で足す。

将来の候補 (ACP 対応状況は追加時に個別調査してこの表を更新する):

| ハーネス | 状態 |
|---|---|
| Claude Code | **採用** (`@agentclientprotocol/claude-agent-acp` 経由、§4) |
| OpenCode | **採用 (Stage 7c)** — 組み込みの `opencode acp` (2.0.12)、§7。`opencode` が見つかれば全役割で選べる (7c-2、§7.6) |
| Codex | **採用 (Stage 7e)** — `@agentclientprotocol/codex-acp` 2.0.0 (npx、Codex 本体はユーザーの `codex`)、§9。`codex` と `npx` が見つかれば全役割で選べる。調査: [`research/codex-acp.md`](research/codex-acp.md) |
| Devin | **実装済み・基本動作は実機確認済み (T-5〜T-10、T-21)** — Devin CLI の `devin acp`、§10。`devin` が見つかれば全役割で選べる。devin 3000.11.3・無料プラン (SWE-1.6 Slow) で起動・`bypass`・モデル設定・1 ターン・HTTP MCP の接続、実装エージェントとしての `report_step_done` を確認 (§10.0、§10.9)。未ログイン時、`session/load`、cancel は未確認 (§10.9) |
| MiniMax Code | **実装済み・基本動作は実機確認済み (T-26)** — MiniMax Code CLI の `mcode acp`、§11。`mcode` が見つかれば全役割で選べる (`PATH` に無くても `~/.minimax-code/bin` を探す)。mcode 0.6.2 で起動・モデル一覧・モデルと effort の設定・1 ターンを確認。`permissionMode` はユーザーの全体設定に書き込まれるので**触らない** (§11.2) |
| Grok Build | **実装済み・基本動作は実機確認済み (T-29)** — xAI の Grok Build CLI の `grok agent --no-leader stdio`、§12。`grok` が見つかれば全役割で選べる (`PATH` に無くても `~/.grok/bin` を探す)。grok 1.0.46・grok.com の Free プラン (モデルは `grok-4.7` だけ) で起動・モデル一覧・モデルと effort の設定、実装エージェントとしての `report_step_done` (Yhtye の MCP 経由) まで確認。許可要求の自動応答 (実機では要求が来なかった)、`session/load`、cancel は未確認 (§12.8) |
| Google Antigravity | **実装済み・基本動作は実機確認済み (T-34, T-36)** — Google の公式 ACP サーバー `agy_acp_server` (v1.3.0)、§13。`agy_acp_server.par` が見つかれば全役割で選べる (`PATH` に無くても `$AGY_ACP_SERVER_HOME`、`~/.local/share/agy-acp-server`、`~/.gemini/antigravity-acp/bin` を探す)。Yhtye は `authenticate` を呼ばない (ログインは利用者が先に済ませる、§13.7)。effort は無い (思考レベルはモデルの id に含まれる)。Google アカウントでのログイン後、`gemini-3.8-flash-low` の実装エージェントとしてドキュメント変更のタスクを `report_step_done` (Yhtye の MCP 経由) まで完了。許可要求の自動応答 (要求が来なかった)、`session/load`、cancel は未確認 (§13.8) |
| Cursor CLI / Gemini CLI / GitHub Copilot | 未着手 |
| Muse Code | 未着手。サードパーティ製 ACP アダプタが要る可能性 |

## 4. Rust クライアント: `agent-client-protocol` 2.2

調査日 2026-09-23。crate ソース `~/.cargo/registry/src/*/agent-client-protocol-2.2.0`
(スキーマは `agent-client-protocol-schema =1.9.1`) を読んだ結果。行番号は crate ルートからの相対。

### 4.1 接続モデル (0.x とは別物)

- 2.x には `ClientSideConnection` も「実装すべき `Client` トレイト」も無い。
  `Client.builder()` (`src/role/acp.rs:48-74`) にハンドラを `.on_receive_request(..)` /
  `.on_receive_notification(..)` で登録し、`.connect_with(transport, async |cx| {..})`
  (`src/jsonrpc.rs:1871`) で駆動する。クロージャが返るまで接続が生きる。
- サブプロセス起動は `AcpAgent::new(AcpAgentConfig::new(cmd).args(..).env(k, v))`
  (`src/acp_agent.rs:53-189`)。stdin/stdout/stderr の配線と、drop 時のプロセスグループ kill
  (`acp_agent.rs:140, 311-321`) まで面倒を見る。
  ただし終了コードを返さないため、Yhtye は使わず `tokio::process` で同じこと
  (`process_group(0)` + グループ kill) を自前で行い、`ByteStreams` で接続する
  ([`core-design.md`](core-design.md) §3.5)。
- 接続先プロセスが stdout を閉じても `connect_with` はエラーにならない (未応答リクエストが
  失敗し、`cx.incoming_closed()` が完了する)。コマンドループはこれも待つ。
- 未登録の通知は無視、未登録のリクエストは `method_not_found` で応答される
  (`src/jsonrpc/incoming_actor.rs:612-620`)。接続は落ちない。
- ランタイム非依存。`Send + 'static` が前提で `LocalSet` / `spawn_local` は不要。
  `ConnectionTo<Agent>` は `Clone` (`jsonrpc.rs:3429-3440`) で、1 接続上で並行リクエスト可。
- **ハンドラはディスパッチループ上で実行される。** ハンドラ内で応答待ち (`block_task`) を
  するとデッドロックする (`src/concepts/ordering.rs:7-60`)。重い処理は `cx.spawn` に逃がし、
  `responder` を move して後から応答する。

### 4.2 メソッド (クライアント → エージェント)

| メソッド | 型 | 備考 |
|---|---|---|
| `initialize` | `InitializeRequest::new(ProtocolVersion::V1)` | 応答の `agent_capabilities.load_session` / `mcp_capabilities{http,sse}` / `session_capabilities` を保存する |
| `session/new` | `NewSessionRequest{cwd, additional_directories, mcp_servers}` | 応答: `session_id`, `modes`, `config_options` |
| `session/load` | `LoadSessionRequest{session_id, cwd, mcp_servers, ..}` | `load_session` が true のときのみ。履歴を session/update で再生する |
| `session/resume` | `ResumeSessionRequest{..}` | 再生なしの再開 |
| `session/prompt` | `PromptRequest::new(session_id, Vec<ContentBlock>)` | 応答 `{stop_reason}` |
| `session/cancel` | `CancelNotification::new(session_id)` (**通知**) | |
| `session/set_mode` | `SetSessionModeRequest{session_id, mode_id}` | |
| `session/set_config_option` | `SetSessionConfigOptionRequest{session_id, config_id, value}` | **モデル選択はこれ** (v1 に `set_session_model` は無い)。stable |

`McpServer` は `Http{name,url,headers}` / `Sse{..}` / `Stdio{name,command,args,env}`
(schema `agent.rs:2652-2892`)。

### 4.3 エージェント → クライアント

- `session/request_permission`: `options: Vec<PermissionOption{option_id, name, kind}>`、
  `kind` は `allow_once` / `allow_always` / `reject_once` / `reject_always`
  (schema `client.rs:968-1100`)。応答は `Selected(option_id)` か `Cancelled`。
- `session/update` の `SessionUpdate` (schema `client.rs:92-168`, `non_exhaustive`):
  `user_message_chunk` / `agent_message_chunk` / `agent_thought_chunk` / `tool_call` /
  `tool_call_update` / `plan` / `available_commands_update` / `current_mode_update` /
  `config_option_update` / `session_info_update` / `usage_update`。
  未知のバリアントは握りつぶさず `Unknown` イベントとして記録する。
- fs (`read_text_file` / `write_text_file`) と terminal 系はクライアント能力として
  **広告しない** (エージェント自身のツールで作業させる)。未登録リクエストは
  `method_not_found` になる (`src/jsonrpc/incoming_actor.rs:620`)。
- `StopReason`: `end_turn` / `max_tokens` / `max_turn_requests` / `refusal` / `cancelled`。

### 4.4 ターン中キャンセルの正しい形

以前の実装はコマンド受信ループの中で prompt の応答を await していたため、
ターン中の cancel が届かなかった。2.2 では:

```text
connect_with(agent, async |cx| {
    loop {
        match cmd_rx.recv().await {
            Prompt(p) => { let cx = cx.clone();
                           cx.spawn(async move { let r = cx.send_request(p).block_task().await;
                                                 emit(TurnEnded(r)); Ok(()) })?; }
            Cancel    => cx.send_notification(CancelNotification::new(sid))?,
            ...
        }
    }
})
```

- **prompt の future を drop / abort してキャンセルしない。** 応答前に `SentRequest` を
  drop すると JSON-RPC の `$/cancel_request` が飛ぶだけで (`jsonrpc.rs:5440-5460`)、
  ACP の `session/cancel` にはならない。
- `cx.spawn` のタスクが `Err` を返すと接続ごと落ちる。エラーはイベントにして `Ok(())` を返す。

## 5. Claude Code: `@agentclientprotocol/claude-agent-acp`

調査日 2026-09-23、npm 最新 0.81.0 (bin `claude-agent-acp`、Node >= 22、
依存 `@anthropic-ai/claude-agent-sdk` 0.3.280)。`dist/` を読み、実際に
initialize / session/new / set_config_option まで疎通確認した。行番号は `dist/` 相対。
(2026-09-30: Sonnet 5.5 を選べるよう **0.84.0** (SDK 0.3.284 / Claude Code 2.1.284) に更新。
Claude Code は `claude` CLI ではなく SDK 同梱のバイナリなので、モデルの追加はアダプタの更新で入る。)

| 項目 | 結論 |
|---|---|
| 起動 | `npx -y @agentclientprotocol/claude-agent-acp` またはグローバル導入した `claude-agent-acp`。stdout は ACP 専用、ログは stderr |
| 認証 | ローカルの Claude Code ログイン (`~/.claude`) をそのまま使う。`claude` CLI ではなく SDK 同梱バイナリを使う (`CLAUDE_CODE_EXECUTABLE` で上書き可、`acp-agent.js:377-400`) |
| **モデル** | `session/set_model` は**無い** (-32601)。初期値は adapter プロセスの env **`ANTHROPIC_MODEL=haiku`** が最優先 (`session-model.js:296-318`)。セッション後は `session/set_config_option {configId:"model", value:"haiku"}` で変更可 (実測で成功)。選択肢: `default` / `opus[1m]` / `sonnet` / `haiku` ほか |
| **権限モード** | `default` / `acceptEdits` / `plan` / `auto` / `bypassPermissions` (`session-mode.js:77-113`)。session/new 時にモードは指定できない (`_meta` の `permissionMode` は上書きされる、`acp-agent.js:6115`)ので、**session/new 直後に `session/set_mode {modeId:"bypassPermissions"}`**。root 実行時は `IS_SANDBOX` が無いと bypass 不可 (`permissions/modes.js:2-3`) |
| permission の選択肢 | `allow-once` (allow_once) / `allow-with-updates` (allow_always、条件付き) / `reject` (reject_once) ほか。id ではなく kind で選ぶ |
| session/load | `loadSession: true`。`sessionCapabilities` に resume / fork / list / close / delete。セッション ID は Claude Code のセッション UUID |
| MCP | `mcpCapabilities {http: true, sse: true}` と stdio。session/new の `name` がキーになり、ツール名は `mcp__<name>__<tool>` (`acp-agent.js:5937-5961`) |
| システムプロンプト | session/new の `_meta.systemPrompt`: 文字列なら全置換、`{append: "..."}` なら Claude Code 既定に追記 (`acp-agent.js:5963-5982`)。Yhtye は **append** を使う |
| 使用量 | `usage_update` (`used`, `size`, `cost`)、prompt 応答の `usage` と `_meta.quota` |
| キャンセル | `session/cancel` で SDK クエリを中断し、実行中ターンは `stop_reason: cancelled` で返る (`acp-agent.js:4506-4570`) |

Yhtye での Claude Code 用 `HarnessConfig` (初期値):

```toml
[harness.claude-code]
command = "npx"
args = ["-y", "@agentclientprotocol/claude-agent-acp@0.84.0"]   # Stage 6b で完全一致に固定 (npx -y は解決したものを実行するため)
env = { ANTHROPIC_MODEL = "haiku", ENABLE_TOOL_SEARCH = "false", ENABLE_CLAUDEAI_MCP_SERVERS = "false", CLAUDE_CODE_DISABLE_BACKGROUND_TASKS = "1" }  # §5.2
mode_after_new = "bypassPermissions"
model = { config_id = "model", value = "haiku" }   # set_config_option。応答の現在値で検証する
system_prompt = "meta_append"          # _meta.systemPrompt.append
session_meta = { claudeCode = { options = { strictMcpConfig = true } } }   # §5.2 (オーケストレータは + tools)
startup_timeout = 120                  # 秒。各起動段ごと
```

(`HarnessConfig::claude_code("haiku")` がこの値を返す。)

(Stage 7b: 役割ごとの設定は `HarnessPreset::claude_code(model)` (id `claude-code`、`model_env = ANTHROPIC_MODEL`) として
登録し、選んだモデルを `model` と `ANTHROPIC_MODEL` に差し込む。[`core-design.md`](core-design.md) §15。)

**モデル一覧 (Stage 7b、実機 2026-09-23、0.81.0)**: `session/new` の `configOptions` の `id: "model"` の select
(カテゴリ付き) に、`default` (Default (recommended)) / `opus[1m]` (Opus 5.5) / `claude-fable-5-1[1m]` (Fable 5.1) /
`sonnet` (Sonnet 5) / `haiku` (Haiku 4.5) が並ぶ。プロンプトを送らない短命のセッション
(`claude_code_usage_probe` の設定) で約 2 秒、現在値は `ANTHROPIC_MODEL` の値 (`haiku`)。
`tests/acp_claude_real.rs::real_model_list_comes_from_the_adapter`。

**0.84.0 (実機 2026-09-30)**: `default` / `opus` (Opus 5.5) / `claude-fable-5-1` (Fable 5.1) / `sonnet` (**Sonnet 5.5**) /
`haiku` (Haiku 4.5) に加え、旧版の `claude-sonnet-5` / `claude-opus-5` / `claude-fable-5` / `claude-opus-4-8` /
`claude-opus-4-7` / `claude-opus-4-6` / `claude-sonnet-4-6` の計 12 個。
- **値から `[1m]` が消えた。** `opus[1m]` を `ANTHROPIC_MODEL` / `set_config_option` で渡すと現在値は `opus` と返る
  (`claude-fable-5-1[1m]` はそのまま返る)。Yhtye は要求した値と現在値の完全一致で検証する (`startup.rs`) ので、
  `opus[1m]` を保存した設定は起動エラーになる。移行処理は入れず、アプリの設定で選び直す (2026-09-30 決定)。
- 12 個は `EAGER_EFFORT_MODELS` (12) ちょうどなので全モデルの effort をまとめて読み、一覧の取得に約 30〜50 秒かかる
  (1 モデル約 2.5 秒)。一覧は 10 分キャッシュされる。モデルが 1 つ増えると effort は選んだときの個別読み取りに切り替わる。

### 5.1 実機での観察 (Stage 1、2026-09-23、adapter 0.81.0 + Haiku)

- 起動 (npx キャッシュ済み) から `Ready` まで数秒。`Ready` 前に `available_commands_update` と
  `config_option_update` が届き、プロンプト直後にもう一度 `available_commands_update` が来る。
  ターン中は `usage_update` が頻繁に来る。`current_mode_update` は set_mode では来なかった。
- `bypassPermissions` ではファイル作成 (Write ツール) で `session/request_permission` は
  **来なかった** (tool_call → tool_call_update のみ)。自動承認は偽エージェントでのみ検証済み。
- 長文生成中の `session/cancel` は約 35 ms (2 回とも) で `stop_reason: cancelled` が返った。
- `session/load` は同じセッション ID で復元でき、履歴 (ユーザー・エージェント両方のチャンク) を
  `Ready` 前に再生する。前の会話内容を覚えていた。
- `Shutdown` 後、プロセスグループ (npx → node → claude) に生存プロセスは残らなかった。

### 5.2 Yhtye から起動するときの隔離設定 (Stage 2)

Stage 2 の実機確認で、何も指定しないと Yhtye のエージェントにユーザー自身の Claude Code 環境の
**MCP サーバーがすべて付く** (`.mcp.json`・プラグイン・claude.ai コネクタ。この環境では
Todoist / Claude Docs / chrome-devtools など 100 個超) ことが分かった。ツールが埋もれて
Haiku が迷ううえ、エージェントがユーザーの外部サービスを触れてしまう。
また既定では MCP ツールが ToolSearch の後ろに遅延ロードされる。
`HarnessConfig::claude_code` は次を設定する:

| 設定 | 経路 | 効果 |
|---|---|---|
| `strictMcpConfig: true` | `_meta.claudeCode.options` (SDK の `--strict-mcp-config`) | session/new で渡した MCP サーバー (= `yhtye`) だけを使う |
| `ENABLE_CLAUDEAI_MCP_SERVERS=false` | プロセス env | claude.ai のコネクタを付けない (デバッグログで "Disabled via env var" を確認) |
| `ENABLE_TOOL_SEARCH=false` | プロセス env | MCP ツールを遅延ロードせず最初から見せる |
| `CLAUDE_CODE_DISABLE_BACKGROUND_TASKS=1` | プロセス env | (Stage 6a) Bash の `run_in_background`・長いコマンドの自動背景化などの背景タスクを無効にする。ACP ではターンが終わるとエージェントを起こす手段が無く、背景のコマンドを待つつもりでターンを終えると報告なしのターン (`protocol_violation`) になる (Stage 5 で観察、実 Haiku の `real_implementer_waits_for_a_long_command_before_reporting` で再現・修正を確認) |

(Stage 8d 以降、オーケストレータも組み込みツールを制限しない。以前は `tools: ["Read","Glob","Grep"]` で絞っていた。
[`orchestration-model.md`](orchestration-model.md) §8.1。)
`settingSources` (ユーザーの CLAUDE.md・フック・プラグイン・スキル) は既定のまま = **常に有効**
(Stage 7a でユーザーが決定、[`orchestration-model.md`](orchestration-model.md) §11)。MCP サーバーだけは上の
`strictMcpConfig` で隔離する。

実機での観察 (Stage 2、2026-09-23、adapter 0.81.0 / Claude Code 2.1.280 + Haiku):

- Claude Code は MCP の **2026-07-28 版** で接続する (デバッグログ `protocolEra: modern`)。
  `tools/list` に `ttlMs` / `cacheScope` が無いと `INVALID_RESULT` で 3 回再試行後に
  諦め、ツールが 1 つも無い状態でターンが始まる。このときエージェントは「Yhtye のツールが
  無い」と答える ([`core-design.md`](core-design.md) §4)。原因調査には
  `_meta.claudeCode.options.debugFile` で Claude Code のデバッグログを出すのが有効。
- MCP サーバーへの接続は非同期 ("running fully async (nonblocking)") だが、ローカル HTTP
  なので 30 ms 程度で終わり、最初のプロンプトに間に合っていた。
- 「README に 1 行足すタスクを作って」相当の依頼で、Haiku のオーケストレータは
  `create_group` → `create_task` (steps `[implement]`、自己完結した instruction 付き) を呼び、
  サブエージェント (Haiku) が README を編集して `report_step_done` を呼び、
  `group_settled` で起こされたオーケストレータが `finish_group` を呼んでユーザーに報告した。
  全体で約 30 秒。2 回実行して 2 回とも同じ流れ。

### 5.3 再起動後の `session/load` (Stage 3b、2026-09-23、Haiku)

`tests/orchestration_claude_restart.rs` (実 Haiku、2 回実行して 2 回とも成功):
サブエージェントがターン中 (最初の出力の直後) に Yhtye を止め (ターンは `session/cancel` で
打ち切られる)、同じ DB で起動し直した。

- サブエージェントもオーケストレータも、保存していた ACP セッション ID (Claude Code の
  セッション UUID) で `session/load` でき、`resumed = true` で同じ ID に戻った。
- **`session/load` で渡した新しい `mcpServers` (新しいポート・新しいトークンの URL) が使われる。**
  復元したサブエージェントは新しい URL で `report_step_done` を呼べた。したがってトークンを
  DB に保存する必要はない (core-design §6)。
- 短い `[yhtye:resume]` プロンプト (「再起動でターンが中断された。続けよ」) だけで、Haiku は
  履歴から元の指示を読み取り、README に 1 行足して報告した。途中まで進んでいた作業を
  二重に行うこともなかった (README の行は 1 つ)。
- 履歴の再生 (`Ready` より前の `Output`) は Yhtye 側で捨てている (core-design §4.1)。

## 6. フォールバック: Claude Code が ACP から脱退した場合

Claude Code は Yhtye にとって最重要ハーネスであるため、Claude Code が将来 ACP サポートを
打ち切る (または対応が不十分なまま止まる) リスクに備えて、代替手段をあらかじめ決めておく。

### 6.1 画面をメイン画面とターミナル画面に分ける

- **メイン画面** — 現行の設計 ([`docs/design/orchestrator-desktop.md`](../design/orchestrator-desktop.md))
  をそのまま使う。ACP 接続できるハーネス (オーケストレータ・サブエージェント問わず) は
  通常どおりこの画面で扱う。
- **ターミナル画面** — Claude Code 専用。ただの端末 (PTY) を埋め込んで Claude Code の
  対話型 CLI をそのまま表示するだけで、Yhtye 側は会話やタスクの構造を解釈しない。
  オーケストレータ/サブエージェントのどちらであっても、Claude Code が担当する分は
  この画面に出る。

2 画面の切り替えはアイコンレール (`docs/design/orchestrator-desktop.md` §3.2 の `work` /
`runs` と同列) に追加する想定。表示するタスク一覧・git グラフなどメイン画面側の機能を
ターミナル画面が引き継ぐ必要はない — あくまで生の CLI を出すだけの窓。

### 6.2 チャット履歴は生の JSONL を解析して描画

Claude Code はセッションログを JSONL で `~/.claude/projects/` 以下に書き出している。
ACP が使えない場合、この JSONL をファイル監視 (tail) して Yhtye 側でパースし、
メイン画面の会話 UI に近い見た目で再構成して表示する。

- 双方向の制御 (ツール呼び出しの承認など) は ACP のようにプロトコル経由でできないため、
  ユーザーの入力は別途ターミナル画面 (6.1) 経由で Claude Code に渡すか、
  Claude Code 自体の非対話モード (`-p` 等) をラップして呼び出す形になる。
  どちらにするかは実装時にあらためて決める。
- JSONL のスキーマは Claude Code のバージョンに追従する必要がある。壊れた行は
  無視して継続する (パース失敗で画面全体を止めない)。

### 6.3 適用条件

このフォールバックは「Claude Code が ACP から抜けた場合」専用であり、平時は使わない。
ACP 経由で問題なく動いている間は 6.1/6.2 のどちらも実装しない —
両方とも Claude Code 個別対応であり、他のハーネスには適用しない。

## 7. OpenCode: 組み込みの `opencode acp` (Stage 7c-1)

調査日 2026-09-23、OpenCode **2.0.12** (`/usr/bin/opencode`、Bun でコンパイルされた単一バイナリ)。
アダプタは不要で、OpenCode 自身が ACP サーバーを持つ。バイナリに埋め込まれた JS
(`strings` で読める) の ACP 実装を読み、実機 (モデルは常に `opencode/muse-spark-1.3-contributor-free`、
ユーザー決定) で確認した。

### 7.1 結論

| 項目 | 結論 |
|---|---|
| 起動 | **`opencode acp`** (フラグ不要)。stdout は ACP 専用。`opencode acp` は自分の子として **`opencode serve --stdio --port 0`** (専用サーバー、`--standalone` 相当) を起動し、HTTP API 経由で操作する。ユーザーのバックグラウンドサービス (`opencode serve --service`) には繋がない |
| 認証 | `~/.local/share/opencode/auth.json` (ユーザーのログイン) をそのまま使う。`initialize` の `authMethods` は `opencode-login` (端末で `opencode auth login`) のみ |
| initialize | `loadSession: true`、`mcpCapabilities {http: true, sse: false}`、`promptCapabilities {image, embeddedContext}`、`sessionCapabilities {close, delete, fork, list, resume}`、`agentInfo {name: "OpenCode", version: "2.0.12"}` |
| **cwd** | **ACP の `cwd` を守る。** Yhtye はプロセスを `$HOME` で起動するが、ファイル作成・シェル (`pwd`)・相対パスはすべて session/new の `cwd` で行われた (`real_opencode_works_in_the_acp_cwd`)。プロジェクトディレクトリには OpenCode のファイルは何も残らない (MCP を足しても) |
| **モード** | session/new の応答に **`modes` が無い**。モードは config option **`mode`** (category `mode`) としてだけ出る: `build` (既定、全ツール許可) / `plan` (edit 禁止 + 「実装するな」のリマインダ)。`session/set_mode {modeId}` も `set_config_option {configId:"mode"}` も使える。Yhtye は `build` を set_mode する。`AgentInfo::current_mode()` は `modes` が無いとき `mode` config option を返すようにした (追加のみ) |
| **自動承認** | `bypassPermissions` 相当のモードは無いが、**`build` はもともと「全部 allow、ただし `external_directory` (セッション外のパス) と `*.env` の read は ask」**。ask は `session/request_permission` で来て、選択肢は固定の `once` (allow_once) / `always` (allow_always) / `reject` (reject_once)。Yhtye の自動応答 (kind で `allow_always` 優先) がそのまま効く (`real_opencode_external_directory_permission_is_auto_approved`: 外部ファイル read で 1 回来て `always`)。ファイル作成・シェルでは来なかった。`question` ツールのフォームは ACP 側で自動キャンセルされる |
| **モデル** | config option **`model`** (値は `<provider>/<model>`、選択肢は認証済みプロバイダ全部で 475 件)。`effort` (バリアント) もある。**既定は OpenCode の「最後に使ったモデル」** (この環境では `opencode/gpt-6-sol`) なので、**最初のプロンプトより前の `set_config_option {configId:"model"}` が必須**。応答の `currentValue` で検証でき、Yhtye の `set_config_option` 検証 (要求値 = 現在値) がそのまま通る。`session/set_model` は無い |
| **システムプロンプト** | `_meta` は見ない (session/new のハンドラは `cwd` と `mcpServers` だけ使う)。**`SystemPromptStyle::FirstPrompt`** (最初のプロンプトの前にテキストブロックとして付ける) を使う。会話の一部なので後のターンにも効き、`session/load` 後も履歴に残る (`real_opencode_system_prompt_reaches_the_agent`)。設定のエージェント定義 (`agent.<名前>.prompt`) で本物のシステムプロンプトにする案は下の 7.3 の理由で使えない |
| **session/load** | 使える。セッション ID は `ses_…`。**cwd がセッション作成時のディレクトリと違うと拒否**される (`en` エラー)。履歴は **応答より前に** `session/update` で順に再生される: `available_commands_update` → `user_message_chunk` → (`agent_thought_chunk`) → `agent_message_chunk` → ツールは `tool_call` + 状態に応じた `tool_call_update`。つまり Yhtye では `Ready` より前に届き、Claude Code と同じく捨てられる。前の会話内容を覚えていた |
| 更新の種類 | `agent_message_chunk` / `agent_thought_chunk` / `tool_call` / `tool_call_update` / `usage_update` (ターン末に 1 回、`used`・`size`・`cost`) / `available_commands_update` (`init`, `review`)。`current_mode_update` / `config_option_update` は来なかった |
| **stop reason** | `end_turn` / `cancelled` / `max_tokens` (finish=length) / `refusal` (content-filter)。プロバイダ認証エラーは `-32000 Authentication required` の **JSON-RPC エラー**で返る (stop reason ではない) |
| **キャンセル** | `session/cancel` で `session.interrupt`。長文生成中の cancel は **約 18〜20 ms** (3 回) で `stop_reason: cancelled` |
| **MCP (HTTP)** | 使える。session/new の `mcpServers` の `Http{name,url,headers}` を `mcp.add({server: name, location: {directory: cwd}, config: {type:"remote", url, headers, oauth:false}})` で**そのセッションのサーバーに実行時に足す** (設定ファイルには書かない)。SSE は不可 (`sse:false`)。`ttlMs` / `cacheScope` は不要だが、付いていても問題ない (Yhtye の `tools/list` のまま動いた) |
| **MCP ツールの呼び方** | OpenCode 2 は MCP ツールを**コードモード**で出す: エージェントは `execute` ツールで `await tools.yhtye.report_step_done({ result: "…" })` のような JS を実行する。ACP には `tool_call {title: "execute", kind: other, rawInput: {code}}` として見える (`mcp__yhtye__…` のような名前は出ない)。Yhtye の MCP サーバー側の `ToolCalled` 記録は Claude Code と同じ |
| **後始末** | `opencode serve --stdio` は **別セッション (setsid、自分のプロセスグループ)** で動くので、Yhtye のプロセスグループ kill は届かない。ただし stdin が `opencode acp` からのパイプなので、`opencode acp` が終わる (stdin EOF で 0.02 秒で exit 0、SIGKILL でも) とサーバーも終わる。実テストでは毎回、グループと新しい `serve --stdio` の両方が残っていないことを確認している (`tests/common/opencode.rs::assert_no_new_servers`)。失敗したテスト (panic) の後も残存 0 |

Yhtye での OpenCode 用 `HarnessConfig` (`HarnessConfig::opencode(model)`):

```toml
[harness.opencode]
command = "opencode"
args = ["acp"]
env = { OPENCODE_DISABLE_AUTOUPDATE = "1" }
mode_after_new = "build"
model = { config_id = "model", value = "opencode/muse-spark-1.3-contributor-free" }  # <provider>/<model>
system_prompt = "first_prompt"
startup_timeout = 120
```

全役割で同じ設定を使う (7.3)。

### 7.2 ユーザーの OpenCode 設定の扱い

Claude Code では「`~/.claude` は常に有効、MCP サーバーだけ隔離」(Stage 7a の決定) にした。OpenCode も
**ユーザー設定はそのまま有効** (隔離しない) にした。理由と実測:

- OpenCode 2 の設定は グローバル (`~/.config/opencode/opencode.json(c)`、または **`OPENCODE_CONFIG_DIR` がその場所を置き換える**) →
  `OPENCODE_CONFIG` のファイル → プロジェクト (`cwd` から祖先へ辿った `opencode.json(c)` と `.opencode/`) → `OPENCODE_CONFIG_CONTENT` の順に重なる。
- この環境のユーザー設定 (`~/.config/opencode`: ollama プロバイダ、npm プラグイン `oh-my-openagent`、`plugins/gk-hooks.js`) は
  2.0.12 では**実質効いていない**: ローカルプラグインは V2 のプラグイン API に合わず `failed to load plugin`、
  `oh-my-openagent` のエージェント (sisyphus ほか、ClinePass のモデル指定付き) は ACP のモード一覧に出ない。
  ユーザー設定に `mcp` は無い。したがって現時点で隔離して得るものが無く、隔離のコスト (Yhtye 専用の設定ディレクトリを
  持つ・ユーザーの AGENTS.md などが効かなくなる) だけが残る。
- **MCP の隔離手段は無い** (Claude の `strictMcpConfig` 相当が無い)。ユーザーやプロジェクトの `mcp.servers` は Yhtye のエージェントにも付く。
  名前を知らずに無効化する設定も無い。将来ユーザーが OpenCode に MCP を足した場合の対策は 7c-2 以降の検討事項
  (Yhtye 専用の `OPENCODE_CONFIG_DIR` を持つ案が最有力。`OPENCODE_CONFIG_DIR` はグローバル設定ディレクトリを**置き換える**ことを確認済み)。
- **注意 (開発環境)**: Orca の端末は `OPENCODE_CONFIG_DIR=~/.config/orca/opencode-hooks/shared` を設定しており、Yhtye (やテスト) を
  そこから起動するとそれが継承される (Orca の AGENTS.md・プラグインが読まれる。プラグインは V2 非対応で読み込み失敗)。
  デスクトップから起動した Yhtye では継承されない。
- **継承した `OPENCODE_CONFIG_DIR` の扱い (7c-2 で決定)**: 「ユーザーの OpenCode 設定を有効にする」を守るため、
  **`OPENCODE_CONFIG_DIR` が `ORCA_OPENCODE_CONFIG_DIR` と同じ値 (= Orca の端末が注入したもの) のときだけ**エージェントの環境から外す
  (`HarnessConfig::env_remove`、`agents::inherited_opencode_env_remove`)。ユーザーが自分で設定した値 (`ORCA_…` と違う・無い) はそのまま渡す。
  こうすると Orca から起動してもデスクトップから起動しても同じ `~/.config/opencode` が効く。Yhtye 専用の `OPENCODE_CONFIG_DIR` は
  (MCP の隔離が要るまで) 持たない。

### 7.3 役割ごとの状況

実テスト (`opencode/muse-spark-1.3-contributor-free`、`--test-threads=1`):

| 役割 | 状況 |
|---|---|
| implementer | **動く。** 実行時に足した HTTP MCP 経由で `report_step_done` を呼び、README を編集した (`acp_opencode_real::real_opencode_implementer_reports_through_mcp`、オーケストレーション経由でも) |
| reviewer | **動く。** implement → review のタスクで別セッションのレビュアーが `verdict: approve` 付きで `report_step_done` を呼んだ (`orchestration_opencode_real::real_opencode_implement_then_review`) |
| orchestrator | **動くが読み取り専用にできない。** `create_group` → `create_task` → (サブエージェント完了) → `group_settled` で自分で `finish_group`。約 30 秒。催促 (`group_finish_reminded`) は 0 回。ただしモードは `build` (全ツール可) — 下記 |

オーケストレータを読み取り専用にする手段 (Claude の `tools: [Read, Glob, Grep]` 相当) は 2.0.12 の ACP では見つからなかった:

- **`plan` モード**: edit は拒否されるが shell は可。さらに OpenCode が「Plan モード中は変更しない・サブエージェントにも頼まない。
  実装を頼まれたらエージェントを切り替えるよう伝えよ」というリマインダを入れるため、実測でオーケストレータが
  `create_group` / `create_task` を**拒否**した (「Plan モードではグループ/タスクを作れない。build に切り替えて」と返答してターン終了)。使えない。
- **設定で独自のプライマリエージェントを定義** (`agent.yhtye-orchestrator: {mode: primary, permission: {edit: deny, bash: deny, task: deny}}`) して
  `set_mode` する案: `OPENCODE_CONFIG_CONTENT`・`OPENCODE_CONFIG_DIR` のファイル・`agents/*.md`・プロジェクトの `opencode.json` の
  **どれで定義しても ACP のモード一覧に出ず**、`set_mode` は `mode not found` (-32602)。同じプロジェクト設定はバックグラウンドサービス経由の
  `opencode debug agents` には出るので、ACP が使う専用サーバーのエージェント一覧に設定のエージェントが載らない (原因は未確定。ACP 実装は
  ディレクトリごとのカタログを最初の session/new で作ってキャッシュする)。
- 補足の観察: プロジェクトの `opencode.json` に `permission: {edit: deny, bash: deny}` を置くと、プロンプトが `-32000 Authentication required` で失敗した
  (`{}` や `edit: deny` だけなら通る)。設定の `permission` は ACP 経由でも一部効いているが挙動が読めないので使わない。

したがって OpenCode のオーケストレータは `build` モード。(**Stage 8d でユーザーが決定: 読み取り専用は求めない。** 全ハーネスのオーケストレータが
書き込めるので、この制限できない点は問題ではなくなった。`orchestration-model.md` §8.1。)

### 7.4 実機での観察と実行結果 (2026-09-23)

- 起動から `Ready` まで約 1 秒 (initialize 0.6 秒、session/new 0.4 秒)。1 ターンの応答は数秒 (無料モデル)。
- `tests/acp_opencode_real.rs` 7 件 (プロンプト / ACP の cwd でファイル・シェル / 外部ディレクトリの権限の自動承認 / ターン中キャンセル /
  FirstPrompt のシステムプロンプト / session/load と履歴の再生順 / MCP 経由の `report_step_done`) と
  `tests/orchestration_opencode_real.rs` 2 件 (全役割 OpenCode で依頼 → タスク → 報告 → `finish_group` / implement → review) を
  それぞれ 2 回実行して全件成功 (ACP の 1 回目はシステムプロンプトのテストが「毎回タグを付けよ」の指示を無料モデルが無視して失敗したので、
  指示の伝達 (合言葉を後のターンで答える) を検査する形に直した)。テスト後に `opencode acp` / `opencode serve --stdio` の残存 0。
- 1 回だけ観察: プロジェクトに `opencode.json` があると上記の認証エラーになる場合がある (7.3 の補足)。Yhtye のテストのプロジェクトには置いていない。

### 7.5 7c-2 (7b の設定への組み込み) への申し送り (→ 7.6 で対応済み)

- ハーネスは `HarnessConfig::opencode(model)` を全役割で使う。モデル一覧は session/new 応答の `model` config option
  (値 `<provider>/<model>`) から取れる — 7b の「configOptions から取得」にそのまま載る。選択肢が 475 件と多いので UI で絞り込みが要る。
- 既定モデルが OpenCode の最後に使ったモデル (有料のことがある) なので、**モデル未指定で OpenCode を起動しない** (`model` を必須にする)。
- オーケストレータを OpenCode にした場合は読み取り専用にならない (7.3)。→ 7c-2 で警告を出す形にしたが、Stage 8d で全ハーネスが書ける方針に変えたので警告は無くなった。
- ACP の `tool_call` は MCP 呼び出しが `execute` (コードモード) に見える。UI でツール名を出す箇所は `rawInput.code` を見せるか、
  MCP サーバー側の `ToolCalled` を使う。
- MCP の隔離が無い (7.2)。必要になったら Yhtye 専用の `OPENCODE_CONFIG_DIR` (アプリのデータディレクトリ配下、他ユーザーが書けない場所) を渡す。
- 再起動復元: session/load は `cwd` が作成時と同じでないと拒否される。Yhtye は同じ作業ディレクトリ (プロジェクト / worktree) で load するので問題ないはずだが、
  パスが変わった場合 (worktree の作り直しなど) は load が失敗する。オーケストレータは失敗時に新しいセッションへフォールバックするが、
  サブエージェントの経路でも同様に扱えるかを 7c-2 で確認する。FirstPrompt のシステムプロンプトは新しいセッションの最初のプロンプトにだけ付き、
  load で復元したセッションには付けない (履歴に残っている)。

### 7.6 ランタイムへの組み込み (Stage 7c-2、2026-09-23)

- **登録**: `HarnessPreset::opencode(fallback_model, env_remove)` (id `opencode`、全役割 `HarnessConfig::opencode`、probe は mode / model の
  切り替えなし)。アプリ (`src-tauri`) と開発ブリッジは `CoreConfig::installed(data_dir, model)` を使い、**実行ファイルが見つかったハーネスだけ**登録する
  (無ければ UI に出ない)。当初 (7c-2) は「`PATH` に実行可能な `opencode` があるときだけ登録、Claude Code は常に登録」だったが、
  Devin 対応 (T-6) で全ハーネスに一般化した: 検索は `PATH` (絶対パスの項目) → 既知の場所 (`~/.local/bin`・`~/.cargo/bin`・`~/.bun/bin`・`/usr/local/bin`) の順、
  Claude Code は `npx` が見つかったときだけ登録される (常には登録されない)。手動パスと再検出 (設定 › ハーネス) もある。詳細は
  [`core-design.md`](core-design.md) §15.2。組み込みの既定は claude-code > opencode > devin > minimax-code > grok-build > codex の最初の見つかったもの。
- **モデルは必須**: preset の `requires_model = true`。設定の検査 (`AgentCatalog::validate`) が OpenCode の `model: null` を
  `invalid_argument` で拒否し、UI も「既定のモデル」の候補を出さない。それでも model 無しで起動される場合 (古い設定など) は
  preset の設定に入っている `OPENCODE_FALLBACK_MODEL` (= 無料の `opencode/muse-spark-1.3-contributor-free`) を set_config_option する —
  **OpenCode の「最後に使ったモデル」では起動しない**。
- **モデル一覧**: 実機で 475 件、約 1.5 秒 (`real_opencode_model_list_comes_from_the_preset_probe`)。値はすべて `<provider>/<model>`。
  プローブも OpenCode のセッションを 1 つ作る (`data_dir/model-probe` の cwd、OpenCode の履歴に残る。プロンプトは送らない)。
- **オーケストレータ**: 選べる (ユーザー決定)。(7c-2 では読み取り専用にできない (7.3) ので preset の `orchestrator_read_only = false` を UI に渡して
  「⚠ 書き込み制限なし」を出していたが、**Stage 8d で全ハーネスのオーケストレータが書ける方針に変え、このフラグと警告は削除した**。)
- **MCP 呼び出しの表示**: サーバー側の `tool_called` 記録 (ハーネス非依存) を従来どおり会話・エージェント出力に `yhtye <tool>` として出す。
  加えて ACP の `execute` ツールは `rawInput.code` の `tools.<server>.<tool>(` を読み、「tool execute → yhtye.report_step_done」と表示する
  (`src/api/acp.ts::codeToolCalls`)。実機では `search({query})` (コードモードのツール検索) も `execute` で来る。
- **session/load のフォールバック**: 別ディレクトリでの load は約 0.65 秒で `Invalid params: session … does not belong to cwd` の
  エラーになる (`real_opencode_session_load_in_another_directory_fails_fast`)。起動エラーなので、サブエージェントもオーケストレータも
  既存のフォールバック (新しいセッションに Step の全プロンプト + 注記、`retry_fresh`) に乗る。フォールバック自体は偽エージェントの
  `orchestration_fake_restart::restart_starts_new_sessions_when_session_load_fails` で検証済み (ハーネス非依存)。
- **実行結果 (実機、`--test-threads=1`)**: `tests/orchestration_mixed_real.rs`
  - `real_mixed_claude_orchestrator_opencode_implementer_claude_reviewer` (Claude Haiku のオーケストレータ + OpenCode 無料モデルの implementer +
    Claude Haiku のレビュアー、一時 git リポジトリで implement → review → `finish_group` → main にマージ): **2 回成功** (約 60 秒)。
    各セッションの `Ready` のモデルが設定どおり (haiku / muse-spark / haiku)、implementer の `execute` の `rawInput.code` に
    `await tools.yhtye.report_step_done(...)`。
  - `real_opencode_orchestrator_with_a_claude_implementer` (OpenCode のオーケストレータ + Claude Haiku の implementer): 成功。
    `create_group` → `create_task` → `finish_group`、マージ済み。
  - 終了後 `opencode acp` / `opencode serve --stdio` / `claude-agent-acp` の残存 0 (ユーザーの `opencode serve --service` は元から動いているもの)。

### 7.7 OpenCode 2.0.18 の上流バグと、テスト専用のウォームアップシム (2026-09-29)

- **バグ**: anomalyco/opencode#50236 (修正 PR #50619 は未マージ)。`session/new` のモデル一覧が cwd ごとに、プロバイダの読み込み完了**前**に
  スナップショットされ、プロセスの生存中キャッシュされる。実際には **`opencode acp` プロセスの最初の `session/new` だけ**が `opencode/*` のモデルを
  1 つも含まない一覧になり、`set_config_option model=opencode/muse-spark-1.3-contributor-free` が `Invalid params: model not found` で失敗する。
  同じプロセスの 2 つ目以降の `session/new` (別の cwd) は正しい一覧になる。実測: ウォームアップ無し 3/3 失敗、捨ての `session/new` を 1 回挟むと 5/5 成功 (+約 0.2 秒)。
  (7d で「git リポジトリの cwd で失敗する」と見えていたのはこれ。壊れた OpenCode プラグインを外すことも必要だった。)
- **Yhtye 本体は回避しない (ユーザー決定)**: 本番のハーネス設定・コアの挙動は変えない。上流の修正待ち。
- **テストだけウォームアップのシムを使う**: `crates/yhtye-fake-agent/src/bin/opencode-warmup-shim.rs` (アプリには含まれない、テスト用クレートのバイナリ)。
  `opencode acp` を起動して改行区切りの JSON-RPC を両方向に中継し、クライアントの `initialize` 応答を転送した直後に、文字列 id `yhtye-test-warmup`・
  新しい一時ディレクトリの cwd・`mcpServers: []` の捨ての `session/new` を注入する。応答が来るまでクライアントからのメッセージは保留し、その応答と
  捨てのセッションの通知は捨てる (それ以外は無変更)。stdin の EOF は OpenCode の stdin を閉じて伝え (`serve --stdio` も一緒に終わる)、OpenCode が
  終わるとシムも終わり、一時ディレクトリを消す。実 OpenCode のテストはヘルパー (`tests/common/opencode.rs` の `opencode_harness` / `opencode_preset`)
  でコマンドをシムに差し替える (`acp_opencode_real`・`orchestration_opencode_real`・`orchestration_mixed_real`)。モデルは常に無料モデル、Claude Code は常に Haiku のまま。
- 実機の結果 (シム経由): `orchestration_mixed_real` 3 件・`orchestration_opencode_real` 2 件は成功、`acp_opencode_real` 10 件中 9 件 (残る
  `real_opencode_system_prompt_reaches_the_agent` は無料モデルが合言葉を答えたり答えなかったりする不安定さで、再実行で通る)。終了後の残存プロセス 0。

## 8. effort (思考の深さ) の設定 (Stage 7d、2026-09-29)

ACP のセッション設定 `configOptions` に **`effort`** があるハーネスでは、Yhtye は行 (ハーネス × モデル × effort) の effort を
`session/set_config_option` で設定する ([`core-design.md`](core-design.md) §15)。

| | Claude Code (`claude-agent-acp` 0.84.0) | OpenCode (`opencode acp` 2.0.12) |
|---|---|---|
| config option | id `effort`、`category: thought_level`、select | id `effort` (OpenCode の variant) |
| 選択肢 | **モデルごと** (そのモデルの `supportedEffortLevels`)。先頭に古いクライアント用の `default` 行。モデルが effort を持たなければ option 自体が無い。**モデルを変えると作り直される** (`session-effort.js`、`acp-agent.js`) | モデルごと (variant) |
| 実測 (このマシン) | `default` / `opus` / `claude-fable-5-1` / `sonnet` ほか = `low, medium, high, xhigh, max` (`claude-opus-4-6` / `claude-sonnet-4-6` は `xhigh` 無し)、**`haiku` = effort 無し** | `opencode/muse-spark-1.3-contributor-free` = `minimal, low, medium, high, xhigh` |
| 未知の値 | `Invalid value for config option effort: no-such-effort` (JSON-RPC の内部エラー) → 起動失敗 | — |

- **順序**: 必ずモデルを先に設定してから effort (`acp/startup.rs`)。モデルを変えると effort の選択肢が作り直されるため。
  どちらも「要求した値 = 応答の `currentValue`」を検証する。
- **一覧の取り方**: モデルの選択肢は現在のモデルに依存するので、プローブのセッションでモデルを 1 つずつ選んで effort を読む
  ([`core-design.md`](core-design.md) §15.6)。Claude Code (5 モデル) はモデル一覧と一緒に約 2.7 秒、OpenCode (約 480 モデル) は
  選んだモデル 1 つだけ約 1.2 秒 (`list_model_efforts`)。先頭の `default` 行は「指定なし」と同じなので Yhtye の一覧から除く。
- **未対応モデルに effort を指定したら**: セッション起動を**失敗させる** (黙って別の effort で動かさない)。
- 実機での確認 (Claude Code は Haiku のみ長い実行をする。Haiku に effort は無いので、effort の適用だけは **プロンプトを送らず**
  `sonnet` のセッション起動とプローブのみ): `acp_claude_real::real_effort_is_applied_after_the_model` (sonnet に `low` を設定 →
  アダプタが `model=sonnet effort=low` を報告、未知の effort は `set_config_option` で失敗)、
  `acp_opencode_real::real_opencode_efforts_are_read_per_model_and_applied` (無料モデルの effort 一覧を読み `minimal` を設定 →
  `effort=minimal` を報告)、`orchestration_mixed_real::real_effort_rows_are_matched_exactly_and_applied` (Haiku のオーケストレータが
  存在しない effort を指定して拒否 → 行の一覧と用途メモを読んで OpenCode の `minimal` の行を選び、implementer が `effort=minimal` で動いて main にマージ)。

## 9. Codex: `@agentclientprotocol/codex-acp` (Stage 7e、2026-09-29)

調査の全記録は [`research/codex-acp.md`](research/codex-acp.md) (ラベル [実測] / [文書/コード] / [推測])。ここは採用した仕様と実装後の実測。
Codex 本体は ACP を話さない (`codex app-server` は独自の JSON-RPC)。アダプタ `@agentclientprotocol/codex-acp` (TypeScript、旧 Zed 製 Rust 版の後継)
が `codex app-server` を stdio の専用の子プロセスとして起動して ACP に翻訳する。デーモンは使わず、終了時に一緒に終わる
(実機: 終了後の残存 0、ユーザーの `~/.codex` のデーモンは起動前後で同じ PID)。

### 9.1 起動 (`HarnessConfig::codex` / `HarnessPreset::codex`)

- コマンド: `npx -y @agentclientprotocol/codex-acp@2.0.0` (**バージョン完全固定**、`claude-agent-acp` と同じ理由)。初回は約 440 MB のダウンロード
  (`startup_timeout` 120 秒)。
- **`CODEX_PATH`** = `PATH` で見つけたユーザーの `codex` (アダプタ同梱の 0.158 ではなくユーザーの 0.159。`config.toml` との互換が取れる)。
- **`codex` と `npx` の両方が見つかったときだけ** preset を登録する (T-6 で一般化した検出、`agents/detect.rs`。当初は `codex` が `PATH` にあるときだけだった。
  `CODEX_PATH` には見つけた `codex`、起動するコマンドには見つけた `npx` の絶対パスを使う。無ければ UI に出ない)。
- モード: **全役割 `agent-full-access`** (承認なし・サンドボックスなし = Claude の `bypassPermissions` / OpenCode の `build` 相当) を `session/new` /
  `session/load` の後に `session/set_mode` で設定する (`session/load` するとモードは既定の `agent` に戻るため、Yhtye が毎回設定する現行の作りのまま)。
  `agent-full-access` はネットワークもホスト全体も制限なしで、承認要求はゼロ (実機で確認)。
- システムプロンプト: `SystemPromptStyle::FirstPrompt` (`_meta` に systemPrompt の口が無い。最初のプロンプトの前置。`session/load` 後も履歴に残る)。
- 環境: **`CODEX_HOME` はそのまま継承する** (ユーザーの `config.toml`・skills・`AGENTS.md`・hooks が有効。OpenCode の Orca `env_remove` のような処理はしない)。
  注意: Orca の端末はこの端末のように `CODEX_HOME` を Orca 自身の runtime home に注入する。そこから Yhtye を起動すると Orca の設定が使われる (ユーザー決定: 特別扱いしない)。
  実機テストは `CODEX_HOME=$HOME/.codex` を明示する。
- ユーザーの `~/.codex/sessions` に Yhtye のセッションが残る (ユーザー決定: 削除しない)。モデル一覧のプローブのセッションも同様。

### 9.2 モデルと effort — `CODEX_CONFIG`

- アダプタは**カタログに無いモデルを `set_config_option` で拒否する** (`-32602`)。そこで選んだモデルと effort を起動時の環境変数
  **`CODEX_CONFIG` (JSON)** で渡す: `{"model":"<model>","model_reasoning_effort":"<effort>"}` (effort が無ければそのキーは省く)。
  `HarnessPreset.model_config_env = Some("CODEX_CONFIG")`。アダプタはこれを `thread/start` の `config` にマージする。
- **モデルの検証**は従来どおり (`HarnessConfig.model` = `ModelSelect{model, value}` を `session/set_config_option` して「要求値 = 応答の currentValue」を確認)。
  `CODEX_CONFIG` で先に入れた値と同じ値の `set_config_option` は通る (実機で確認: 全セッションで `config_value("model")` = 指定モデル)。
- **effort は option として送らず、`CODEX_CONFIG` にだけ入れる** (検証しない)。OpenRouter のモデルはアダプタがメタデータを持たず、`reasoning_effort` の
  option が出ないことがあるため、option を要求すると起動が失敗する。(Claude Code / OpenCode のような「effort の無いモデルに指定したら起動失敗」は Codex では起きない。)
- **effort の config id は `reasoning_effort`** (Claude / OpenCode は `effort`)。global 定数 `EFFORT_CONFIG_ID` に頼らず `HarnessPreset.effort_config_id` に持たせ、
  `HarnessPreset::config` と effort 一覧の読み取り (`efforts_from_options(options, config_id)`) がそれを使う。
- **Codex が受け付ける effort の値**: Codex 0.159 の app-server スキーマ (`ReasoningEffort`) は「モデルが示す**空でない任意の文字列**」で、値の列挙を持たない。
  `codex -c model_reasoning_effort="<値>"` は `max` / `xhigh` / `none` / `minimal` のどれでも (`bogus` でも) 設定の読み込みに通る。つまり Codex 自身は値を検証せず**そのまま
  プロバイダに渡す**。実際に通るかは**プロバイダ (OpenRouter) の `reasoning.supported_efforts`** で決まるので、Yhtye はそのモデルの `supported_efforts`
  (このマシンの 2026-09-29 の一覧では `none` / `minimal` / `low` / `medium` / `high` / `xhigh` / `max` が現れる) **だけ**を提示する。effort を持たないモデル
  (`reasoning` 無し) は選択肢が出ない。**実機での確認** (テスト用の `nvidia/nemotron-3-super-120b-a12b:free`、OpenRouter の `supported_efforts` = `medium` / `low`):
  `CODEX_CONFIG` の `model_reasoning_effort = "low"` で起動し 1 プロンプトを送ると、Codex のロールアウト (`~/.codex/sessions/.../rollout-*.jsonl`) の全 `turn_context` が
  `effort: "low"` (`acp_codex_real::real_codex_effort_from_codex_config_is_applied`)。effort を渡さないときはユーザーの設定の値が使われる。
- 注意: ユーザーの `~/.codex/config.toml` の `model_reasoning_effort` は既定値として効く (Yhtye が何も指定しないとその値で動く)。

### 9.3 オーケストレータは読み取り専用にできない (Stage 8d で問題ではなくなった)

`read-only` モードは「承認が要る」で、Yhtye の自動承認 (allow_always 優先) だと書けてしまい、`reject_once` を返すとターンごと `cancelled` になる。
そこで OpenCode と同じ扱い (ユーザー決定): オーケストレータも `agent-full-access`。7e では preset の `orchestrator_read_only = false` (「⚠ 書き込み制限なし」) と
プロンプトでの禁止を付けていたが、Stage 8d で全ハーネスのオーケストレータが書ける方針に変え、フラグ・警告は削除、プロンプトは
「検証のいらない小さな変更だけ自分でしてすぐコミット」に変えた (`orchestration-model.md` §8.1)。

### 9.4 モデル一覧はプロバイダで変わる (`agents/openrouter.rs`、`agents/codex_config.rs`)

`ModelSource::Codex` の preset は、モデル一覧を取る時に**ユーザーの Codex 設定の `model_provider`** を読む
(`$CODEX_HOME/config.toml`、既定 `~/.codex/config.toml`、継承した `CODEX_HOME` に従う。ファイルは読むだけで、トップレベルの `model_provider` 以外は見ない):

- **`openrouter`**: OpenRouter の公開 API **`GET https://openrouter.ai/api/v1/models`** から作る。**認証ヘッダなし** (公開エンドポイント。キーは読まない・送らない。
  テスト `codex_models` でリクエストに `Authorization` が無いことを確認)。`data[].id` のうち `supported_parameters` に **`tools`** を含むものだけ (エージェントとして
  使えない対話専用モデルを除く。2026-09-29 は 460 件中 392 件)。モデルごとの effort は `reasoning.supported_efforts` (無ければ effort なし = `efforts: Some([])`)。
  effort は一覧に含まれるので `list_model_efforts` は追加のプローブを起こさない。キャッシュは他のハーネスと同じ (成功 10 分・失敗 10 秒、`refresh` でも 10 秒)。
  取得に失敗したら空の一覧ではなく `unavailable` のエラー。
- **それ以外** (プロバイダ無し = OpenAI / ChatGPT、他のカスタムプロバイダ): 他のハーネスと同じくアダプタの `configOptions` のモデル一覧 (プローブのセッション、プロンプトなし)。
  他のカスタムプロバイダ専用の処理はしない (アダプタが返すものをそのまま出す)。
- 実測の補足: Codex 0.159 (`CODEX_PATH`) + `~/.codex` (openrouter) では、**アダプタ自身の一覧も OpenRouter のモデルを含む** (464 件、`tools` 非対応も含み、effort は
  option を持つモデルだけ・現在のモデル `stealth/space-bunny-alpha` が先頭に足される)。調査時 (同梱 0.158 / Orca の `CODEX_HOME`) は OpenAI のカタログ 8 件だけだった。
  OpenRouter の一覧を使うのは、`tools` での絞り込みと**全モデルの effort を 1 回の取得で得られる**ため (ユーザー決定)。

### 9.5 秘密の環境変数 (認証キーの渡し方、`crate::secrets`)

ユーザーの Codex 設定は OpenRouter のキーを `[model_providers.openrouter.auth] command = "sh"、args = ["-c", "echo $OPENROUTER_API_KEY_CODEX"]` で取り、
この `auth.command` は**アダプタ (= Yhtye が起動したプロセス) の環境**で動く。デスクトップから起動した Yhtye は fish の設定を継承しないので、キーが空になり
`provider auth command sh produced an empty token` (応答テキスト + `end_turn`) になる。そこで Yhtye に**アプリ全体の「秘密の環境変数」**を持たせた
([`core-design.md`](core-design.md) §16): 名前と値を登録し、値は OS のキーリング (Secret Service。`keyring` crate、service `yhtye`、account = 変数名) にだけ保存、
Yhtye の DB は**名前だけ**、起動する**すべてのエージェントプロセス**の環境に注入する。ユーザーの端末・`config.fish` は変えない。

### 9.6 失敗の見え方 (未対応・ユーザー決定)

実機で何度も見た: OpenRouter の無料モデルが 429 を返すと、Codex は再試行して諦め、**`exceeded retry limit, last status: 429 Too Many Requests, request id: …` という普通のメッセージ + `end_turn`**
でターンを終える (`session/load` のリプレイにも「ユーザー発言だけでエージェントの発言が無い」形で残る)。実装者では「報告なしのターン」として `agent_crashed` の help になり、オーケストレータが
`cancel_task` する。無料モデルは時間帯で 3〜6 分/回の遅延 (`nvidia/nemotron-3.5-lightning:free`、この日は障害) や 429 が起きるため、実機テストは 1 イベント 240 秒・1 実行 20 分で打ち切り、
モデルは `nemotron-3-super-120b-a12b:free` に変えた (ユーザー決定)。

認証エラー・課金上限・プロバイダの 400/403 は JSON-RPC エラーではなく **`agent_message_chunk` のテキスト + `stopReason: end_turn`** (usage が null か 0) で返る。
Yhtye はこれを「エージェントがエラーを喋った」と「作業した」と区別しない (報告ツールが呼ばれなければ既存の `protocol_violation` / 催促として現れる)。今回は検出しない。
カタログに無いモデルでは毎ターン先頭に `Warning: Model metadata for … not found. Defaulting to fallback metadata; …` が `agent_message_chunk` として混ざる
(そのまま表示する、ユーザー決定)。

### 9.7 実機の結果 (`nvidia/nemotron-3-super-120b-a12b:free`、OpenRouter 経由、Codex 0.159 + アダプタ 2.0.0)

実行結果は [`PLAN.md`](../PLAN.md) Stage 7e の結果メモ。Codex の実装者は実機でマージまで通った (`stealth/space-bunny-alpha`)。

**Codex をオーケストレータにすると `create_task` が呼べない**: Codex の既知バグ [openai/codex#13746](https://github.com/openai/codex/issues/13746) で、
MCP ツールスキーマの `$defs` / `$ref` が解決されずモデルに崩れたスキーマが渡る (`steps` を `{"item": …}` や文字列で送る)。
Yhtye 側で `$ref` を展開すれば回避できるが、メインは Claude のため当面放置 (ユーザー決定、2026-09-29)。

## 10. Devin: `devin acp` (T-5〜T-10、T-21 で**基本動作を実機確認**、一部未検証)

> 実装は偽エージェントを Devin 風に振る舞わせたテスト (`tests/acp_fake_devin.rs`) と単体テストで作り、T-21 で **devin 3000.11.3 (9c803229faa4)・無料プラン
> (使えるモデルは SWE-1.6 Slow だけ)** の実機で確かめた (2026-10-01、プロンプト 2 回)。結果は §10.0 にまとめ、各項目にも書き足した。以下のラベル: **[文書]** = 公式ドキュメント・レジストリ、
> **[他実装]** = 他の ACP クライアント (acpx、RepoPrompt) の実装・PR から読み取れること、**[Yhtye]** = このリポジトリの実装、**[実測]** = T-21 で実機で見たこと。
> 実機を呼ぶテストはリポジトリに置いていない (無料プランの枠が小さいため。確認は一時的なスクリプトで行った)。未確認の項目は §10.9。

一次情報:

- [Devin Desktop の ACP](https://docs.devin.ai/desktop/acp) (外部 ACP エージェントの登録の説明。Devin は `devin acp` を `PATH` から起動する。MCP・`clientInfo` の記述は無い)
- [CLI のコマンド一覧](https://docs.devin.ai/cli/reference/commands) (`devin acp`、`devin auth`、`--permission-mode`、環境変数)
- [CLI の基本コマンド](https://docs.devin.ai/cli/essential-commands)
- [ACP レジストリの `devin/agent.json`](https://github.com/agentclientprotocol/registry/blob/main/devin/agent.json) (バージョン `3000.11.3`、全プラットフォームの引数は `acp`)
- [Zed の ACP ページ (Devin)](https://zed.dev/acp/agent/devin)
- acpx: [`agents/Devin.md`](https://github.com/openclaw/acpx/blob/main/agents/Devin.md)、[PR #560](https://github.com/openclaw/acpx/pull/560) (組み込みエージェントとして追加)
- RepoPrompt: [PR #1029](https://github.com/repoprompt/repoprompt-ce/pull/1029) (権限モードと選択肢の除外)、[PR #1057](https://github.com/repoprompt/repoprompt-ce/pull/1057) (権限レベルとモデルごとの推論 effort)

### 10.0 実測のまとめ (T-21、devin 3000.11.3、無料プラン)

すべて **[実測]**。Yhtye の実際の起動経路 (`HarnessPreset::devin(..).config(Implementer, Some("swe-1-6-slow"), None)`、Yhtye の `McpHost` の HTTP MCP を渡す) と、
`devin acp` に直接 JSON-RPC を送るスクリプトの両方で見た。

| 項目 | 結果 |
|---|---|
| 検出 | `~/.local/bin/devin` が `PATH` から見つかる (`PATH` から外すと既知の場所 `KnownDir` として見つかる) |
| `initialize` | `clientInfo` は **`yhtye` のままで通る** (上書き不要、§10.4)。`agentInfo` = `affogato` / "Devin Agent" / `0.0.0-dev`。`authMethods` = `[{id: "devin-browser"}]` だが、ログイン済みなら `authenticate` 無しで使える |
| `agentCapabilities` | `loadSession: true`、`mcpCapabilities {http: true, sse: true}`、`promptCapabilities {image: true, audio: false, embeddedContext: true}`、`sessionCapabilities {list, delete, additionalDirectories}`、`_meta` に多数の `cognition.ai/*`。応答の `_meta.mcpConfigPath` = `~/.config/devin/mcp_config.json` |
| モード | `session/new` 直後は `accept-edits`。一覧は `accept-edits` (Code) / `smart` / `ask` / `plan` / **`bypass`** ("Bypass Permissions")。`session/set_mode bypass` が通り、`current_mode` が `bypass` になる |
| config options | `mode` (category `mode`) と **`model`** (id `model`・category `model`・select) の 2 つだけ。モデルの値は **`swe-1-6-slow`** ("SWE-1.6 Slow") の 1 つ (このプラン)。**`thought_level` (effort) の option は無い** |
| モデル一覧 (`ModelService`) | 約 0.35 秒で `swe-1-6-slow` (effort 一覧は空 `Some([])`)、current = `swe-1-6-slow`。プロンプトを送らないプローブのセッションは Devin のセッション DB に残らない |
| 起動 → 1 ターン | 起動 約 0.2 秒、`bypass` と `model = swe-1-6-slow` が `Ready` に反映。極小プロンプトが `EndTurn` まで 3.7 秒 (入力 約 14k トークン) |
| HTTP MCP | `session/new` の HTTP MCP (`yhtye`) は受け付けられる。接続は**遅延** (最初に使うとき)。Devin は MCP のツールを自分のツールとして並べず、**メタツール `mcp_list_tools` (と呼び出し用のメタツール) 経由**で使う。`mcp_list_tools {server_name: "yhtye"}` で Yhtye の MCP につながり、`report_step_done`・`help` が見えた (§10.7) |
| 許可要求 | `bypass` では MCP のツール呼び出し (`mcp_list_tools`) でも `session/request_permission` は来なかった |
| ベンダー拡張 | `_cognition.ai/request_diagnostics` は**送られてこない** (Devin のログ上、クライアント能力 `request_diagnostics=false`)。通知 `_cognition.ai/mcp/serversChanged`・`_cognition.ai/output`・`_cognition.ai/thinking_complete`・`_cognition.ai/turn_stats`・`_cognition.ai/agent_stopped` が来るが、Yhtye は無視して問題ない |
| ユーザーの MCP 設定 | Devin は `~/.config/devin/mcp_config.json` のサーバーも (Yhtye の MCP と並べて) つなぐ。ユーザーの設定を残す方針は他のハーネスと同じ |
| 後始末 | 終了後に `devin acp` のプロセスは残らない |

T-21 の実測を受けた変更 **[Yhtye]**: 役割プロンプトの後ろに Devin 専用の注記 (`DEVIN_MCP_NOTE`、MCP ツールはメタツールで一覧・呼び出しする) を付ける (§10.7)、
継承した `RUST_LOG` を Devin の環境から除く (Yhtye 用の `RUST_LOG` で Devin 自身のログ `~/.local/share/devin/cli/logs` が空になっていた)、
読み取り済みの effort 一覧に無い effort (effort の無いモデルの effort を含む) を設定の保存時に拒否する (§10.3)。

### 10.1 起動と認証 (`HarnessConfig::devin` / `HarnessPreset::devin`)

- **起動**: `devin acp` (stdio の ACP サーバー) **[文書]**。Yhtye は検出で見つけた `devin` の絶対パスを `command` にし、引数は `acp` だけ **[Yhtye]**。
  `npx` は要らない。起動のタイムアウトは他と同じ 120 秒。環境の追加は無く、継承した `RUST_LOG` だけを除く (Devin も `tracing` でログを出すので、
  Yhtye 用の値が Devin 自身のログを空にする。**[実測]** で空になっていた)。ほかはユーザーの環境を継承する。
- **認証**: `devin auth login` (`devin auth status` / `logout` もある) で保存した認証情報を使う **[文書]**。環境変数 **`WINDSURF_API_KEY`** が設定されていると
  「ACP サーバーの認証情報として、保存済みの認証情報より優先」される **[文書]**。Yhtye の**秘密の環境変数** (§9.5、`core-design.md` §16) に `WINDSURF_API_KEY` を
  登録すれば、起動するエージェントの環境に渡る (ただし**すべてのエージェント**に渡る)。
- **未認証のとき**: acpx の PR #560 は、未認証の `devin acp` がセッションを作りプロンプトを受けて "login-required" の ACP エラーを返したと書いている **[他実装]**。
  エラーが `session/new` で返るのか `session/prompt` で返るのかは、この記述だけでは分からない (→ §10.9)。
  Yhtye は ACP の `authenticate` を呼ばない (Devin の `authMethods` を使わない) **[Yhtye]**: 保存済みの認証情報か `WINDSURF_API_KEY` が前提。
  ログイン済み (`devin auth login`) なら `authMethods` (`devin-browser`) があっても `authenticate` 無しで `session/new` から最初のターンまで通る **[実測]**。
  未ログインのときのエラーの出方は**未確認** (ログアウトが要るため試していない)。
  `session/new` の失敗なら「agent startup failed at `session/new`: Authentication required …」として起動エラーになる (偽エージェントで確認、`an_authentication_error_reads_as_a_startup_error`)。
  `session/prompt` の失敗なら `agent request `session/prompt` failed: …` のターンのエラーになる。

### 10.2 モード: `session/set_mode bypass`

- Devin が出すモードは `accept-edits` / `smart` / `ask` / `plan` / `bypass` (RepoPrompt PR #1029 が「Devin advertises modes」として挙げている) **[他実装]**。
  実機でもこの 5 つ (`session/new` 直後は `accept-edits`)。`set_mode bypass` が通り、現在のモードが `bypass` になる **[実測]**。
- Yhtye は全役割を **`bypass`** (権限確認なし) で動かし、`session/new` / `session/load` の後に **`session/set_mode {modeId: "bypass"}`** を送る **[Yhtye]**
  (`DEVIN_BYPASS_MODE`。他のハーネスと同じ、Claude Code の `bypassPermissions`・OpenCode の `build`・Codex の `agent-full-access` 相当)。
  応答の `modes.availableModes` に `bypass` が無ければ、起動を失敗させる (`Unsupported`、利用できるモード名を並べたメッセージ)。
- **`--permission-mode` は `devin acp` に効かない**。これはトップレベルのフラグで、`acp` サブコマンドは無視する (RepoPrompt PR #1029: 「`--permission-mode` is a top-level Devin CLI
  argument that the `acp` subcommand ignores」) **[他実装]**。CLI の文書は `--permission-mode` の値を `normal` (`auto`) / `accept-edits` / `smart` / `dangerous`
  (`yolo`, `bypass`) / `autonomous` (要 `--sandbox`) とし、環境変数 `DEVIN_PERMISSION_MODE` も挙げる **[文書]**。ACP のモード名 `bypass` と CLI の値の名前 (`dangerous`) は
  同じではない。だから起動フラグや環境変数ではなく `session/set_mode` を使う。
- 承認なし・サンドボックスなしなので、README / SETUP の注意 (エージェントはあなたのリポジトリのコピーで何でも実行する) は Devin にも当てはまる。

### 10.3 モデルと effort (`thought_level` の id 解決)

- **モデル**: 「Devin の ACP のモデル設定オプション (advertised ACP model config option)」で選ぶ (acpx の `--model`) **[他実装]**。Yhtye は Claude Code と同じ形で扱う:
  `HarnessPreset::devin` は `requires_model = false` (選ばなければ Devin の既定モデル)、モデル一覧は `session/new` の `configOptions` (プローブのセッション、プロンプトなし、§8 と同じ)、
  選んだモデルは `session/set_config_option` で `model` に設定して「要求値 = 応答の `currentValue`」を検証する **[Yhtye]**。オプションは id `model`・category `model` の select で、
  無料プランの値は `swe-1-6-slow` だけ。設定と検証が通る **[実測]**。
- **effort**: Devin はモデルごとの推論の強さ (reasoning effort) を持つ (RepoPrompt PR #1057) **[他実装]**。ACP では config option (category `thought_level`) として出ると想定しているが、
  **無料プラン (SWE-1.6 Slow) には `thought_level` の option が無い** (effort 一覧は空) **[実測]**。有料プランのモデルで出るか、その id・category は**未確認**。`effort` と同じとは限らないので、Yhtye は effort を設定するとき (起動手順・effort 一覧の読み取りとも) **`effort_option`** で option を探す **[Yhtye]**: id が `effort` の select があればそれ、無ければ
  `category: thought_level` の select が**ちょうど 1 つ**のときそれ (複数あるときは推測しない)。見つからなければ id `effort` のまま送って Devin に拒否させ、起動が見える形で失敗する。
  偽エージェントで「id `reasoning`・category `thought_level` の option に `effort` の値が設定できる」ことを確認 (`effort_is_set_on_the_thought_level_option_whatever_its_id`)。
  モデルごとに effort の選択肢が変わるか、`default` のような「指定なし」の行があるかは**未確認** (Claude Code のように先頭の `default` を除く処理が Devin にも当たる)。
- 順序は §8 と同じ (モデル → effort)。
- effort の無いモデル (一覧が空) に effort を付けた行は、UI では選べず (effort の欄が無効)、設定の保存でも拒否する **[Yhtye]** (T-21): 新しい行の effort を
  `ModelService` が読み取り済みの一覧と照合する (`RoleSettings::check_efforts`)。一覧がまだ無ければ受け付ける (起動時に Devin が拒否して見える形で失敗する)。
  モデル未指定 (ハーネスの既定) の行の effort も拒否する。保存済みの行はそのまま残せる。

### 10.4 `clientInfo` (`yhtye` で通る、[実測])

- **既定は正直に `yhtye`** を名乗る (`initialize` の `clientInfo`)。他社製品の名前を無断で名乗らない。バージョンは Yhtye の crate のもの **[Yhtye]**。
- **acpx は `windsurf` を名乗っている**: 「`clientInfo.name` は互換のため `windsurf` のまま」、バージョンは既定 `1.110.1` (環境変数 `ACPX_DEVIN_WINDSURF_VERSION` で変えられる)、
  PR #560 は「scoped Windsurf compatibility behavior」を保った **[他実装]**。つまり Devin が**知っているクライアントにだけ**正しく振る舞う (未知の名前だと拒否する、
  あるいは一部の機能を出さない) 可能性がある。本当にそうかは**分からない** — 文書には書かれておらず、acpx がそうしていることから推測しているだけ。
- Yhtye には**上書きの受け口だけ**ある: `HarnessConfig::client_info: Option<ClientInfoOverride { name, version }>` (`None` = `yhtye`)。**設定画面や環境変数からは変えられない**。
  試すには `HarnessConfig::devin` (`acp/config.rs`) の `client_info` に `Some(ClientInfoOverride { name: "windsurf".into(), version: "1.110.1".into() })` を入れて再ビルドする。
  偽エージェントで「既定は `yhtye`、上書きするとその名前が `initialize` に載る」を確認 (`the_client_introduces_itself_as_yhtye_unless_overridden`)。
  名前を偽ることの是非は、`yhtye` で通らないと分かってから決める (ユーザー判断)。
- **[実測] `yhtye` のままで通る**: `initialize` → `session/new` (HTTP MCP 付き) → `set_mode` → `set_config_option` → プロンプト → `EndTurn` まで問題なし。上書きは要らない
  (受け口は残す)。他社名に変えた試行はしていない。

### 10.5 ベンダー拡張 `_cognition.ai/request_diagnostics`

- Devin はクライアントに診断情報を求めるベンダーの**リクエスト** `_cognition.ai/request_diagnostics` を送る。acpx はこれに **`{}`** で応答し、ベンダーの通知
  (`_cognition.ai/…`) は method-not-found にせず受け付ける。クライアントの能力として `_meta["cognition.ai/requestDiagnostics"] = true` も送る **[他実装]**。
- Yhtye は `_cognition.ai/request_diagnostics` に **`{}`** で応答する (`acp/session.rs`、報告する診断が無い) **[Yhtye]**。それ以外の未知のリクエストは接続層が返す
  method-not-found、未知の通知は無視する。偽エージェントで「応答後もセッションが続く」ことを確認 (`diagnostics_requests_are_answered_with_an_empty_object_and_the_session_goes_on`)。
- Yhtye は `_meta["cognition.ai/requestDiagnostics"]` を**送らない** (`ClientCapabilities::default()`)。
- **[実測]** `request_diagnostics` は送られてこない (Devin のログ上もクライアント能力 `request_diagnostics=false`)。送らなくても止まらない。応答の受け口はそのまま残す。
  `_cognition.ai/*` の通知 (`mcp/serversChanged`、`output`、`thinking_complete`、`turn_stats`、`agent_stopped`) は無視して問題ない。

### 10.6 許可の自動応答: `PermissionPolicy::OnceOnly`

- `bypass` でも `session/request_permission` は来うる。Devin の選択肢には、モードやプランを切り替えるもの (`switch_*`、`plan_*`) と、現在のリクエストを超えて効く許可
  (`*_always`、`*_global`) がある **[他実装]** (RepoPrompt PR #1029 はこれらを自動でも代替でも選ばない)。Yhtye の既定の方針 (`allow_always` を優先) だと、セッションの動き方を
  変えてしまう選択肢を選びかねない。
- Devin の preset は **`PermissionPolicy::OnceOnly`** **[Yhtye]** (`acp/permission.rs`): kind が **`allow_once`** で、id が `switch_` / `plan_` で始まらず `_always` で終わらないものだけを選ぶ
  (T-29 で、id の除外を `always` / `for_session` を**含む**ものへ広げた。Grok Build の `allow_always_bash`・`allow_edits_for_session` のため、§12.2)。
  無ければ `Cancelled`。位置ではなく kind で選ぶ点は他と同じ。偽エージェントで確認 (`permissions_never_pick_mode_switches_plans_or_standing_grants`)。
  RepoPrompt が除く `*_global` は id では見ない (kind が `allow_once` のものだけを選ぶので、通常は同じ結果。`allow_once` の kind で `_global` の id が来たら選んでしまう)。
- 他のハーネス (Claude Code・OpenCode・Codex) は従来の方針のまま (`PermissionPolicy::Default`)。
- **[実測]** `bypass` では、MCP のツール呼び出し (`mcp_list_tools`) を含めて `session/request_permission` は来なかった。ファイル作成・シェル実行では試していない。

### 10.7 MCP (HTTP) — 接続は通る。ツールはメタツール経由

- Yhtye のオーケストレーションは、Yhtye がホストする **HTTP の MCP サーバー**をエージェントの `session/new` (`mcpServers` の `Http`) に渡し、エージェントがそのツール
  (`report_step_done` など) を呼ぶことで進む ([`mcp-tools.md`](mcp-tools.md))。これが通らないと Devin は**タスクを報告できない** (実装者は報告なしのターン、オーケストレータは `create_group` を呼べない)。
- Devin が `initialize` で `mcpCapabilities.http` を出すか、`session/new` の HTTP MCP を使えるかは、調べた範囲の文書・PR のどれにも書かれていない (**不明**)。Yhtye は能力を見ずに HTTP のサーバーを渡す
  (Claude Code の `mcpCapabilities {http, sse}`、OpenCode の `{http: true, sse: false}` と違い、確かめていない) **[Yhtye]**。
- **[実測]** `mcpCapabilities` は `{http: true, sse: true}`。`session/new` の HTTP MCP はそのまま受け付けられ、stdio のブリッジは要らない。接続は遅延で、最初に使うときに
  つながる (トークン付きの URL・状態なし・JSON 応答の Yhtye の MCP で問題なし)。
- **[実測] Devin は MCP のツールを自分のツールとして並べない**。モデルにはサーバー名 (`yhtye`) だけが見えていて、ツールはメタツール **`mcp_list_tools`** (`{server_name}`) で
  一覧し、別のメタツールで呼ぶ。`mcp_list_tools {server_name: "yhtye"}` で `report_step_done`・`help` (実装者の道具) が見えた。役割プロンプトの
  `mcp__yhtye__report_step_done` という名前は Devin には無いので、Yhtye は役割プロンプトの後ろに注記 `DEVIN_MCP_NOTE`
  (`HarnessConfig::system_prompt_note`) を付ける **[Yhtye]**: 「Yhtye のツールは MCP サーバー `yhtye` にある、MCP の一覧ツールで一覧し、MCP の呼び出しツールで呼ぶ、
  `mcp__yhtye__xxx` は `yhtye` の `xxx`」。全役割 (オーケストレータも) に付き、Devin 以外には付かない。
- 実際に `report_step_done` を呼んでタスクが完了まで進むこと **[実測]**: 注記 `DEVIN_MCP_NOTE` を入れた版で、`devin` / `swe-1-6-slow` の実装エージェントに
  「何もせず完了を報告する」タスク (T-22、`code`) を流し、Yhtye に完了報告と結果が届いた (§10.9)。
- Devin はユーザーの `~/.config/devin/mcp_config.json` のサーバーも並べてつなぐ **[実測]** (つながらないサーバーは警告をログに出すだけ)。

### 10.8 検出と UI

- 検出は `devin` の実行ファイルを `PATH` → 既知の場所の順に探すだけ (実行はしない)。手動パス・再検出は設定 › ハーネス ([`core-design.md`](core-design.md) §15.2)。
- 組み込みの既定の順は claude-code > opencode > **devin** > minimax-code > grok-build > codex (Devin は、モデルの指定が要る Codex より先。モデルは未指定で Devin の既定)。
- システムプロンプト (役割の指示) は `SystemPromptStyle::FirstPrompt`: 最初のプロンプトの前にテキストとして付ける (後ろに §10.7 の注記) **[Yhtye]**。Devin が `_meta` の system prompt を受け付けるかは調べていない。
  `session/load` で復元したセッションには付けない (履歴に残る前提)。Devin は `loadSession: true` を出す **[実測]** が、`session/load` で履歴を再生するか・復元が通るかは**未確認**。

### 10.9 手動検証チェックリスト (Devin を持つ人向け)

前提: `devin auth login` 済みで、Yhtye の設定 › ハーネスで Devin が「インストール済み」。設定 › エージェント で Devin の行を implementer などに足して、小さなタスクを流す。
結果 (成否・観察・Devin のバージョン `devin --version`) を、この節に **[実測]** として書き足す。

**T-21 の状況** (devin 3000.11.3、無料プラン): **済** = 1 (起動・`initialize`)、3 (`bypass`。許可要求は MCP のメタツールでは来ない。ファイル作成・シェルは未)、
4 (`model` / `swe-1-6-slow`、effort の option は無い)、5 (`yhtye` で通る)、6 (`request_diagnostics` は来ない)、7 のうち `mcpCapabilities.http` と MCP の接続・ツール一覧 (§10.0)。T-22 で 7 の `report_step_done` の呼び出しとタスクの完了も済。T-23 で Devin 自身がこの行を書き換えてコミットし、ファイル変更を伴うタスクも済。
**未** = 2 (未ログイン時のエラー)、8 のうち `session/load`・`session/cancel`・`WINDSURF_API_KEY`
(子プロセスが残らないこと、プローブのセッションが Devin の DB に残らないことは確認済み)。

1. **`devin acp` が起動する**: 端末で `devin acp` を起動し、標準入力に `initialize` (`{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":1,"clientCapabilities":{},"clientInfo":{"name":"yhtye","version":"0"}}}`)
   を送って応答を見る。記録するもの: `agentInfo`、`agentCapabilities` の `loadSession` / `mcpCapabilities` / `promptCapabilities`、`authMethods`。Yhtye から起動したとき、エージェント出力に起動エラーが出ないか。
2. **未認証のとき login-required が読める形で出る**: `devin auth logout` (または `WINDSURF_API_KEY` なし・別の `HOME`) の状態で起動する。エラーが `session/new` の起動エラーとして出るか、
   `session/prompt` のターンのエラーとして出るか。メッセージに「ログインが必要」と読める文言があるか (オーケストレータ・エージェント出力・エラー行のどこに出るか)。`authMethods` が返るなら、`authenticate` 無しで動くか。
3. **`bypass` が提供される**: `session/new` の応答の `modes.availableModes` に `bypass` があり、`session/set_mode` が通り、`current_mode` が `bypass` になるか。ファイル作成・シェル実行で
   `session/request_permission` が来ないか (来たら、選択肢の id と kind を記録する: §10.6 の除外に当たるか)。無いなら Yhtye は起動を失敗させ、利用できるモード名を出す。
4. **effort の id と値**: `session/new` の `configOptions` に `thought_level` の option があるか。その **id・category・選択肢の値**、モデルによって変わるか、「指定なし」の行 (`default` など) があるか。
   `model` の option の id と選択肢 (Yhtye の設定 › エージェント にモデル一覧が出るか、行を足して effort を選べるか)。選んで起動して「要求値 = 応答の `currentValue`」の検証が通るか。
   `thought_level` の option が複数あると Yhtye は推測せず、id `effort` のまま送って失敗する。
5. **`clientInfo` を `yhtye` で通るか**: 既定 (`yhtye`) で `initialize` から最初のターンまで通るか。通らないなら、`HarnessConfig::devin` の `client_info` に `windsurf` / `1.110.1` を入れて再試行し、
   通った・通らなかったを記録する (§10.4)。通ったときの違い (拒否のメッセージ、出てくる能力の差) も。
6. **`request_diagnostics` が来るか**: `_cognition.ai/request_diagnostics` のリクエストが来るか (ログに出す。`{}` で応答して続くか)。来ないなら、`_meta["cognition.ai/requestDiagnostics"]` を
   能力として送る必要があるかも見る (§10.5)。
7. **HTTP MCP が渡せるか**: `initialize` の `mcpCapabilities.http` が `true` か。implementer のタスク (`code`、小さな変更) を流し、エージェントが `report_step_done` を呼んでタスクが完了まで進むか
   (`tool_called` の記録があるか)。`session/new` が MCP のせいで失敗するなら、エラーメッセージを記録する。
8. **そのほか** (余裕があれば): 役割の指示が最初のプロンプトで効くか / 再起動後の `session/load` (`loadSession`、履歴の再生、cwd の制約) / `session/cancel` でターンが止まるか /
   終了後に `devin` の子プロセスが残らないか / Devin のセッションの履歴が残る場所と量 (Yhtye のプローブのセッションも残るか) / `WINDSURF_API_KEY` を秘密の環境変数に登録して認証が通るか。

### 10.10 自動テストで確認したこと (偽エージェント、Devin 本体ではない)

- 役割プロンプトの後ろの注記 `DEVIN_MCP_NOTE` が Devin にだけ付くこと (`acp/startup.rs` の単体テスト、`tests/acp_fake_devin.rs` の最初のプロンプト)、
  `RUST_LOG` を除くこと (`acp/config.rs`)、読み取り済みの一覧に無い effort を保存で拒否すること (`agents/settings.rs`、`tests/agent_selection.rs`) は T-21 で追加。

- `tests/acp_fake_devin.rs`: `bypass` の設定と役割の指示が最初のプロンプトに 1 回だけ付くこと、effort が id の違う `thought_level` の option に設定されること (と未知の値で起動が失敗すること)、
  `clientInfo` が既定 `yhtye`・上書きで変わること、`request_diagnostics` に `{}` で応答してセッションが続くこと、`switch_*` / `plan_*` / `*_always` の選択肢を選ばないこと、認証エラーが起動エラーとして読めること。
- `crates/yhtye-core/src/agents/detect.rs` の単体テスト、`tests/harness_detection.rs`: 検出 (`PATH` / 既知の場所 / 手動パス)、再検出、手動パスの検査と保存、壊れた手動パス、未インストールの行の非表示。

## 11. MiniMax Code: `mcode acp` (T-26、mcode 0.6.2 で**基本動作を実機確認**)

> 実装は Devin (§10) と同じ形 (`HarnessConfig` / `HarnessPreset` / 検出 / 偽エージェント) で、**mcode 0.6.2** の実機調査 (JSON-RPC を直接送る調査、2026-10-02) で分かったことに合わせた。
> その後 Yhtye 自身の acp 層 (`HarnessPreset::minimax_code(..).config(Implementer, Some(既定のモデル), Some("low"))`) から、プロンプトなしの起動とモデル一覧、
> **プロンプト 1 回**を実機で確かめた (§11.8)。その後 T-28 で、許可要求と `allow-once` の自動応答を見るために**プロンプトをさらに 2 回**送った (§11.8 の「許可要求の確認」。`auto` では要求が来なかった)。
> 利用クレジットが少ないので、実機を呼ぶテストはリポジトリに置いていない (確認は一時的なテストで行い、消した)。
> ラベル: **[調査]** = 実機を直接調べて分かったこと、**[実測]** = Yhtye の acp 層から実機で確かめたこと、**[Yhtye]** = このリポジトリの実装。

### 11.0 調査のまとめ (mcode 0.6.2)

| 項目 | 結果 |
|---|---|
| 起動 | `mcode acp` (引数はこれだけ、stdio)。標準のパスは **`~/.minimax-code/bin/mcode`** (シェルスクリプトの起動ラッパー。環境変数 `MCODE_INSTALL_ROOT` でルートを変えられる)。**`PATH` に入らない**ことが多い (fish など)。`mcode --version` は `0.6.2` (番号だけ) **[調査]** |
| `initialize` | `authMethods` は無い。`clientInfo` は `yhtye` のままで通る (`client_info: None`)。`loadSession: true`。`agentInfo` = `minimax-code` / "MiniMax Code" / `0.6.2` **[調査][実測]** |
| 未認証 | `session/new` が `-32000 "Authentication required: Run \`mcode login\` and try again."` を返す **[調査]**。Yhtye では起動エラー (step = `session/new`) として読める (§11.7) |
| モード | `default` と `plan` だけ。bypass は無い (`session/new` 直後は `default`) **[調査][実測]** |
| config options | `permissionMode` (category `_permission`、値 `default` / `auto` / `bypassPermissions`、既定 `auto`)、`model` (category `model`)、`thinkingEffort` (category `thought_level`) の 3 つ。`thinkingEffort` は effort のあるモデル (Flash Preview の thinking) を選んでいるときだけ出る **[調査][実測]** |
| モデル | `models` フィールドは無く、`model` の config option で選ぶ。値は **`m:minimax:<Model>:v:<variant>`**: `m:minimax:MiniMax-M3.1-Flash-Preview:v:thinking` (既定)、`m:minimax:MiniMax-M3:v:`、`m:minimax:MiniMax-M3:v:thinking`、`m:minimax:MiniMax-M2.7:v:thinking`、`m:minimax:MiniMax-M2.7-highspeed:v:thinking`。`m:minimax:MiniMax-M3.1-Flash-Preview:v:` (thinking なし) は**一覧に出るが、選ぶと `-32603 "Invalid model reasoning"` で拒否される** **[調査]** |
| effort | `thinkingEffort` の値は `default` / `low` / `medium` / `high` / `xhigh` / `max`。モデルを切り替えるとこの option は消える (消えるのは Flash Preview thinking 以外のモデル) **[調査][実測]** |
| システムプロンプト | `_meta.systemPrompt` は**無視される** → 最初のプロンプトの前に付ける (`FirstPrompt`) **[調査]** |
| MCP | HTTP の `mcpServers` を受け付け、ツールは **`mcp__yhtye__<tool>` の名前のまま**見える (Devin のようなメタツールの注記は要らない) **[調査]** |
| 許可要求 | 選択肢は `allow-once` (kind `allow_once`)、`allow-always` (`allow_always`)、`deny` (`reject_once`)。既定の `auto` では、作業ディレクトリ内の書き込みと MCP は許可要求なしで通る **[調査]**。シェルによる作業ディレクトリ外への書き込み・削除も要求なしで通った **[実測]** (§11.8)。要求が来たのは `permissionMode` = `default` のときに、シェルで作業ディレクトリ外へ書き込んだ場合 **[調査]**。`auto` で要求が来る操作は見つかっていない |
| ベンダー拡張 | ベンダー独自の**リクエスト**は来ない。通知 `$/cancel_request` が来るが、Yhtye は未知の通知を無視するので問題ない **[調査]** |
| キャンセル | `session/cancel` を送ると `stopReason: cancelled` で終わる **[調査]** |

### 11.1 起動と検出 (`HarnessConfig::minimax_code` / `HarnessPreset::minimax_code`)

- **起動**: `mcode acp` **[調査]**。Yhtye は検出で見つけた `mcode` の絶対パスを `command` にし、引数は `acp` だけ **[Yhtye]**。`npx` は要らない。環境の追加・除去は無い
  (ユーザーの環境をそのまま継承する。`MCODE_INSTALL_ROOT` もそのまま渡る)。起動のタイムアウトは他と同じ 120 秒。
- **検出** (`agents/detect.rs`): ほかのハーネスと同じ `PATH` → 既知の場所の順に加え、MiniMax Code には**自分用の探す場所**がある **[Yhtye]**
  (`HarnessSpec::root_env` / `extra_dirs`): `PATH` → **`$MCODE_INSTALL_ROOT/bin`** (絶対パスのときだけ) → 共通の既知の場所 (`~/.local/bin` など) → **`~/.minimax-code/bin`**。
  見つかった場所は `known_dir` として出る。`mcode` の実行ファイル 1 つだけを要求する (`mcode --version` は実行しない)。ほかのハーネスはこの場所を探さない。
  `~/.minimax-code/bin/mcode` は `current` ファイルの指すリリースの実体を `exec` するラッパーなので、`PATH` に無くてもそのまま起動できる **[調査]**。
- **ハーネスの id は `minimax-code`、表示名は `MiniMax Code`**。組み込みの既定の順は claude-code > opencode > devin > **minimax-code** > grok-build > codex
  (MiniMax Code は、Grok Build と、モデルの指定が要る Codex より先。モデルは下の既定 `MINIMAX_CODE_DEFAULT_MODEL`)。
- **認証**: `mcode login` で済ませておく **[調査]**。Yhtye は ACP の `authenticate` を呼ばない (`authMethods` が無い)。

### 11.2 モードと `permissionMode` には**一切触れない** (ユーザーの全体設定に書き込まれるため)

- **`permissionMode` を `session/set_config_option` で変えると、ユーザーのグローバル設定 `~/.minimax/config.yaml` に永続的に書き込まれる** **[調査]**。
  Yhtye が 1 回の起動のために変えると、ユーザーが普段 `mcode` を使うときの権限モードまで変わってしまう。だから Yhtye は `permissionMode` を設定しない。
  モード (`default` / `plan`) も同じ扱いにして `session/set_mode` を送らない (`mode_after_new = None`)。
- 既定の `auto` でも、作業ディレクトリ内の書き込みと MCP は許可要求なしで通る **[調査]**。つまり Yhtye の使い方 (エージェントはワークツリーの中で作業し、MCP で報告する) に足りる。
  シェルによる作業ディレクトリ外への書き込みと削除も、`auto` では要求なしで通った **[実測]** (§11.8)。`auto` で `session/request_permission` が来る操作はまだ見つかっていない。
  来たときは §11.4 の自動応答が `allow-once` で通す (実機では未確認)。`permissionMode` を `default` にしているユーザーでは、作業ディレクトリ外へのシェルの書き込みで要求が来る **[調査]**。
  README / SETUP の注意 (エージェントはあなたのリポジトリのコピーで何でも実行する。サンドボックスは無い) は MiniMax Code にも当てはまる。
- **[実測]** Yhtye から起動・モデル設定・effort 設定・1 ターンを行っても、`~/.minimax/config.yaml` のハッシュは変わらなかった
  (モデルと effort の `set_config_option` は、このファイルには書き込まない。`defaultModel` はそのまま)。
- 偽エージェントで「`session/set_mode` も `permissionMode` の `set_config_option` も 1 回も送らない」ことを確認 (`neither_the_mode_nor_the_permission_mode_is_ever_written`)。

### 11.3 モデルと effort

- **モデル**: 一覧は `session/new` の `configOptions` の `model` (プローブのセッション、プロンプトなし、§8 と同じ)。選んだモデルは `session/set_config_option` で `model` に設定して
  「要求値 = 応答の `currentValue`」を検証する **[Yhtye]**。値の形式は `m:minimax:<Model>:v:<variant>`。
- **既定のモデルは `m:minimax:MiniMax-M3.1-Flash-Preview:v:thinking`** (`MINIMAX_CODE_DEFAULT_MODEL`、クレジット節約のため。thinking のある Flash Preview) **[Yhtye]**。
  `HarnessPreset::minimax_code` は `requires_model = true`: 設定の行は必ずモデルを持ち、モデル無しの選択 (古い設定など) は mcode 自身の既定 (ユーザーの
  `~/.minimax/config.yaml` の `defaultModel`、高いモデルかもしれない) ではなくこのモデルで動く。組み込みの既定の選択もこのモデル。
- **一覧から外すモデル** (`HarnessPreset::unusable_models`、`MINIMAX_CODE_UNUSABLE_MODELS`): `m:minimax:MiniMax-M3.1-Flash-Preview:v:` (thinking なしの Flash Preview)。一覧には出るが、
  選ぶと `-32603 "Invalid model reasoning"` で拒否される **[調査]**。`probe_models` が一覧から除く (effort の読み取りもこれに対しては行わない) **[Yhtye]**。
  **[実測]** 実機の一覧 (`option` は 6 つ) から除いて 5 つ: Flash Preview thinking (effort `low` / `medium` / `high` / `xhigh` / `max`)、`MiniMax-M3` (thinking なし)、
  `MiniMax-M3` thinking、`MiniMax-M2.7-highspeed` thinking、`MiniMax-M2.7` thinking (この 4 つに effort は無い)。現在の値は Flash Preview thinking。一覧の読み取りは約 3.8 秒
  (全モデルを順に選んで effort を読む。設定ファイルは変わらない)。
- **effort**: `thinkingEffort` (category `thought_level`) に設定する。preset の `effort_config_id` は実際の id の **`thinkingEffort`** (`MINIMAX_CODE_EFFORT_CONFIG_ID`。
  id が違っても category で見つけるロジック `effort_option` (§10.3) もそのまま効く) **[Yhtye]**。**この option はモデルを選んだ後にだけ出る**ので、順序は §8 と同じ
  **モデル → effort** (`startup.rs` はモデルの応答の `configOptions` を見てから effort を設定する)。effort の無いモデルに effort を付けると、起動が
  `session/set_config_option` で失敗する (拒否が見える)。`default` は effort 一覧から除かれる (「指定なし」)。
  偽エージェントで「モデルの後に `thinkingEffort` が設定される (`options=model,thinkingEffort`)、別のモデルへ変えると option が消える、effort の無いモデルへの effort は起動失敗」を確認
  (`the_effort_is_set_on_thinking_effort_after_the_model`)。

### 11.4 許可の自動応答: `PermissionPolicy::OnceOnly` (ハイフン形の id)

- 選択肢は `allow-once` (kind `allow_once`)、`allow-always` (kind `allow_always`)、`deny` (kind `reject_once`) **[調査]**。`allow-always` を選ぶとユーザーの設定に永続化する恐れがあるので、
  preset は Devin と同じ **`PermissionPolicy::OnceOnly`** にする **[Yhtye]** (`acp/permission.rs`): kind が `allow_once` のものだけを選ぶ。
- OnceOnly の id による除外は `switch_` / `plan_` で始まるものと、`always` / `for_session` を含むもの (T-29 で Grok Build のために広げた、§12.2。それまでは `_always` で終わるもの)。
  mcode の id はハイフン形 (`allow-always`) で、`allow-always` は kind が `allow_always` なので kind で選ばれず (id も `always` を含むので除外にも当たる)、
  `allow-once` (kind `allow_once`、id はどの除外にも当たらない) が選ばれる。
  **ハイフン形の id でも正しく動く**ことを偽エージェントで確認 (`permissions_pick_the_hyphenated_allow_once_and_never_allow_always`): `allow-always` / `deny` / `allow-once` の並びから `allow-once`、
  `allow-always` と `deny` だけのときは `Cancelled`。位置ではなく kind で選ぶ点は他と同じ。
- **[実測]** 既定の `auto` では、作業ディレクトリ外へのシェルの書き込みと削除でも許可要求は来なかった (§11.8、2026-10-02)。そのため、実機の mcode が出す許可要求に対して
  `allow-once` が選ばれるところは**未確認** (偽エージェントでの確認だけ)。`auto` を変えずに確かめられる操作は、今のところ見つかっていない。

### 11.5 システムプロンプトと MCP

- `_meta.systemPrompt` は無視されるので、`SystemPromptStyle::FirstPrompt` (最初のプロンプトの前にテキストとして付ける。`session/load` で復元したセッションには付けない) **[調査][Yhtye]**。
  役割プロンプトへの追加の注記 (`system_prompt_note`) は**無し**: ツールは `mcp__yhtye__<tool>` の名前のまま見える **[調査]**。
  偽エージェントで「役割プロンプトが最初のプロンプトにだけ付き、注記が無い」ことを確認 (`the_role_prompt_rides_the_first_prompt_only_and_nothing_is_added_to_it`)。
- Yhtye の HTTP の MCP は `session/new` の `mcpServers` に渡す (stdio のブリッジは要らない)。
  **[未検証]** Yhtye の `McpHost` に実際につないで `report_step_done` を呼ばせるところは、プロンプト 1 回の制約で試していない (§11.8)。

### 11.6 そのほか

- **`clientInfo`**: 既定の `yhtye` で通る (上書き不要) **[調査][実測]**。
- **ベンダー拡張**: 独自のリクエストは来ない。`$/cancel_request` の通知は無視してよい **[調査]**。Yhtye の acp 層は未知の通知を無視する (`acp/session.rs`)。
- **ターン中に来る更新**: `available_commands_update` (`/help` `/new` `/model` など)、`config_option_update`、`session_info_update`、`usage_update` (`used` / `size` / `cost`) が来る **[実測]**。
  Yhtye は型付きの `AgentOutput` として受け取り、問題なく処理する。
- **キャンセル**: `session/cancel` で `stopReason: cancelled` で終わる **[調査]**。Yhtye のキャンセル (§4.4) と同じ形で扱えるはず (実機での Yhtye 経由の確認は未)。
- **料金**: 利用はユーザーの MiniMax のクレジットを使う (`usage_update` の `cost` は `0.0` と出たが、実際にいくら引かれるかは未確認)。既定のモデルを Flash Preview thinking にしているのはそのため。

### 11.7 失敗の見え方: 未認証

- 未認証のとき `session/new` が `-32000 "Authentication required: Run \`mcode login\` and try again."` を返す **[調査]**。Yhtye では
  「agent startup failed at `session/new`: Authentication required …」として**既存の起動エラー**になる (`AgentError::Startup { step: "session/new", .. }`) **[Yhtye]**。メッセージに
  `mcode login` が出るので、ユーザーは何をすればよいか分かる。偽エージェントで確認 (`an_authentication_error_reads_as_a_startup_error`)。

### 11.8 実機の結果 (Yhtye の acp 層から、mcode 0.6.2、2026-10-02、プロンプト 1 回)

すべて **[実測]**。`HarnessPreset::minimax_code("~/.minimax-code/bin/mcode").config(Implementer, Some("m:minimax:MiniMax-M3.1-Flash-Preview:v:thinking"), Some("low"))`、
作業ディレクトリは一時ディレクトリ。システムプロンプトを付けて最初のプロンプトの前置を使った。

| 項目 | 結果 |
|---|---|
| 検出 | `~/.minimax-code/bin/mcode` (`PATH` 外) を見つけて起動できる |
| 起動 | 約 2.6 秒で `Ready`。`agent` = `minimax-code` / "MiniMax Code" / `0.6.2`。モード = `default` (`default` / `plan`)、`permissionMode` = `auto` (**変更していない**)、`model` = Flash Preview thinking、`thinkingEffort` = `low` (モデルの後に設定) |
| モデル一覧 | §11.3 のとおり (5 つ、thinking なしの Flash Preview は除外)。約 3.8 秒、プロンプトなし (モデル呼び出しなし) |
| 1 ターン | 「`pong` とだけ答えて。ツールは使わないで」→ `EndTurn`、約 3.8 秒、応答は `pong`。`usage_update` は `used` = 15014、`size` = 512000、`cost` = 0.0。許可要求は来なかった |
| 設定ファイル | 一覧の読み取り・起動・1 ターンの前後で `~/.minimax/config.yaml` のハッシュは同じ (`permissionMode` も `defaultModel` も変わっていない) |
| 後始末 | 終了コード 0。`mcode` のプロセスは残らない |

**未検証** (クレジットの都合でプロンプトは少数に限った): 許可要求が来る操作と `allow-once` の応答 (下の「許可要求の確認」で 2 回試したが、`auto` では要求が来なかった)、
Yhtye の MCP (`McpHost`) 経由の `report_step_done` (実装エージェントとしてのタスク完了)、
`session/load`、`session/cancel` を Yhtye 経由で、thinking なしの Flash Preview を選んだときの拒否の見え方、未ログイン時のエラー (調査の結果はあるが Yhtye 経由では未)。
Devin の §10.9 にならい、mcode を使える人は小さなタスクを流して、この節に **[実測]** として書き足す。

#### 許可要求の確認 (T-28、2026-10-02、mcode 0.6.2、プロンプト 2 回)

すべて **[実測]**。§11.4 の自動応答 (`allow-once` を選び `allow-always` を選ばない) が実機の許可要求で働くかを確かめた。条件は上の表と同じ (Flash Preview thinking、effort `low`、
`permissionMode` = `auto` のまま、モードも `permissionMode` も変えない)。作業ディレクトリと書き込み先は、別々の `mktemp -d` で作った一時ディレクトリ。
mcode を `tee` で包んだ起動ラッパーを `command` にして、JSON-RPC の両方向を記録した (`AgentEvent::PermissionAutoAnswered` と、ワイヤ上の `session/request_permission` の両方で見るため)。

| 操作 (プロンプト) | 結果 |
|---|---|
| 1. 「シェルで `date > <作業ディレクトリ外の一時ディレクトリ>/outside.txt` を実行して」 | `bash` (kind `execute`) が `in_progress` → `completed` (終了コード 0)。**ファイルは書かれた** (日付の 1 行)。**許可要求は来なかった**。`stopReason` = `EndTurn`、約 11 秒 |
| 2. 「シェルで `rm <作業ディレクトリ外の一時ディレクトリ>/victim.txt` を実行して」 (自分で作った一時ファイル) | 同じく `completed`、**許可要求は来なかった**。mcode の `rm` は `mavis-trash: moved to trash: …` と出て、ファイルをユーザーのごみ箱 (`~/.local/share/Trash`) へ移した (後でそのエントリを消した)。`EndTurn`、約 7 秒 |

- **許可要求は 2 回とも 0 件**: `AgentEvent::PermissionAutoAnswered` は出ず、記録したワイヤにもエージェント → クライアントの**リクエストは 1 つも無かった** (`session/update` の通知だけ)。
  したがって **`allow-once` の自動選択は実機では確認できていない** (「`auto` では要求が来ず未確認」)。
- 結論: `auto` では、**シェルによる作業ディレクトリ外への書き込みも削除も許可なしで通る**。§11.0 の調査にあった「作業ディレクトリ内の書き込みと MCP 以外は要求が来る」は、
  シェルについては `auto` では当てはまらない (要求が来たのは `permissionMode` = `default` のとき、**[調査]**)。`auto` で要求が来る操作 (ほかのツール、ネットワーク、機密のパスなど) は見つけていない。
  Yhtye の使い方 (`auto` のまま) では、許可要求がほとんど (あるいは全く) 来ない可能性がある。
- **設定ファイル**: 2 回のプロンプトの前後で `~/.minimax/config.yaml` の sha256 は同じ (`234e4e7c…a7e`、`permissionMode: auto`)。`set_config_option` の `permissionMode` も `session/set_mode` も送っていない。
  `permissionMode` を `default` にすれば要求は来るはずだが、それは**ユーザーの全体設定を書き換える**ので、この確認ではやっていない。
- 副作用: mcode は自分の記録 (`~/.minimax/background-tasks/bg_*/` にシェルの出力、`~/.minimax/v2/sessions/`、`~/.minimax/v2/observability/logs/`) をいつもどおり `~/.minimax/` に書いた (設定ファイルではない)。
  Yhtye やこの確認が `~/.minimax/` に書いたものではなく、mcode 自身の状態。
- 後始末: 終了コード 0、`mcode` のプロセスは残らなかった。一時テスト (ラッパーと一時ディレクトリ) はリポジトリに残していない。
- 確かめるには: `permissionMode` を自分で `default` (Ask) にしている人が、同じ操作 (作業ディレクトリ外へのシェルの書き込み) を Yhtye から流して、選ばれた選択肢の id (`allow-once`) を見る (§11.10 の 2)。

### 11.9 自動テストで確認したこと (偽エージェント、mcode 本体ではない)

- `tests/acp_fake_minimax_code.rs` (6 本。偽エージェントは `permission_modes` / `report_writes` で mcode 風に振る舞う): `session/set_mode` も `permissionMode` も書かず、モデルだけを設定する
  (モデル無指定の選択は既定のモデルになる)、役割プロンプトが最初のプロンプトにだけ付き注記が無い、effort が `thinkingEffort` に**モデルの後**に設定され、別のモデルで option が消え、effort の無いモデルへは
  起動が失敗する、許可の自動応答がハイフン形の `allow-once` を選び `allow-always` を選ばない (選べなければ `Cancelled`)、認証エラーが起動エラーとして読める、
  モデル一覧が拒否されるモデルを除き残りの effort を読む。
- `agents/catalog.rs` の preset の単体テスト (`minimax_code_preset_never_touches_the_mode_and_defaults_to_its_cheapest_model`)、
  `agents/detect.rs` の単体テスト (`mcode_is_found_in_its_own_install_directory_and_in_the_root_the_environment_names`: `~/.minimax-code/bin`、`MCODE_INSTALL_ROOT`、`PATH` が先、ほかのハーネスは探さない)、
  `agents/installed.rs` (登録と既定の順)、`tests/harness_detection.rs` (Core 経由で `mcode` が見つかり登録される)。

### 11.10 手動検証チェックリスト (mcode を持つ人向け)

前提: `mcode login` 済みで、Yhtye の設定 › ハーネスで MiniMax Code が「インストール済み」。設定 › エージェント で MiniMax Code の行を implementer などに足して、小さなタスクを流す。
結果 (成否・観察・mcode のバージョン `mcode --version`) を、この節に **[実測]** として書き足す。**済** / **未** を各項目の末尾に書いて、進んだ形を残す。

1. **実装エージェントとしてタスクを完了できる**: 実装エージェント (kind `code`) の行に MiniMax Code を足して、小さな変更 (1 ファイル) のタスクを流す。
   `McpHost` 経由で `report_step_done` が呼ばれ、タスクが完了状態まで進むか。ツール名が `mcp__yhtye__report_step_done` のまま見えるか、`tool_called` の記録があるか。
2. **許可要求が来る操作で `allow-once` が自動で選ばれる**: 作業ディレクトリ外 (`mktemp -d` で作った一時ディレクトリなど。`~/.minimax/` や他の作業ツリーは使わない) への書き込み・シェル実行を含むタスクを流す。
   `session/request_permission` が来るか、来たときに選ばれた選択肢の id が `allow-once` かどうか、`allow-always` を選ばないこと (`allow-once` しか無ければ `Cancelled` になる)。
   あと、選ばれた許可が次のターンに残らないこと (§11.4)。**未** (2026-10-02、T-28: 既定の `auto` では、作業ディレクトリ外へのシェルの書き込みも削除も許可要求が来なかったので `allow-once` の自動選択は実機で見られていない。
   許可要求が来るのは `permissionMode` を `default` にしているときなので、`permissionMode` を自分で `default` にしている人の確認を待つ。§11.8)
3. **`session/load` と `session/cancel` を Yhtye 経由で**: タスクを 1 つ流して Yhtye を再起動し、そのタスクのセッションが再開できるか
   (履歴の再生、`loadSession` の能力の扱い、cwd が制約されるか)。実行中に `session/cancel` をするとターンが `stopReason: cancelled` で止まるか。
4. **thinking なしの Flash Preview を選んだときの拒否の見え方**: 設定の行のモデルに `m:minimax:MiniMax-M3.1-Flash-Preview:v:` (thinking なし、§11.3 で一覧から除くモデル) を指定してタスクを流してから、
   一覧から選択して除外されているか、または拒否されたときに `-32603 "Invalid model reasoning"` が読みやすい形でどこに出るか。
5. **未ログイン時のエラー表示**: `mcode logout` の状態で implementer のタスクを流す。`session/new` の起動エラーとして出るか、ターンのエラーとして出るか、
   メッセージに「ログインが必要」 (`mcode login` を含む) が読める形で出るか (§11.7)。
6. **設定ファイルが変わらない**: 1 〜 5 をやる前後で `~/.minimax/config.yaml` のハッシュを比較する。特に `permissionMode` が `auto` のままであること、`defaultModel` も変わらないこと
   (Yhtye はモードと `permissionMode` に一切触れない、§11.2。起動・モデル一覧の読み取り・1 ターン・許可要求のいずれでも変わらないことを確かめる)。

## 12. Grok Build: `grok agent --no-leader stdio` (T-29、grok 1.0.46 で**基本動作を実機確認**)

> 実装は Devin (§10)・MiniMax Code (§11) と同じ形 (`HarnessConfig` / `HarnessPreset` / 検出 / 偽エージェント)。**grok 1.0.46 (2765805b9442、stable)・grok.com の Free プラン**の実機を調べ (2026-10-02、
> JSON-RPC を直接送るスクリプト。`initialize` → `session/new` を数回と、**プロンプト 1 回**)、その後 Yhtye の acp 層から、プロンプトなしのモデル一覧と、
> Yhtye の MCP をつないだ実装エージェントの 1 ターン (**プロンプト 1 回**) を確かめた (§12.6)。プロンプトは合計 2 回。実機を呼ぶテストはリポジトリに置いていない
> (利用枠を使うため。確認は一時的なテストで行い、消した)。
> ラベル: **[調査]** = 実機を直接調べて分かったこと、**[実測]** = Yhtye の acp 層から実機で確かめたこと、**[Yhtye]** = このリポジトリの実装、**[推測]** = 確かめていない推論。

### 12.0 調査のまとめ (grok 1.0.46)

| 項目 | 結果 |
|---|---|
| 起動 | **`grok agent stdio` が ACP (JSON-RPC over stdio) を話す**。`grok agent` のサブコマンドは `stdio` / `headless` / `serve` / `leader`、オプションは `--no-leader` / `--leader` / `--always-approve` / `-m` / `--reasoning-effort` / `--agent-profile` / `--plugin-dir` など。`grok agent stdio` 自身のオプションは `--debug` / `--debug-file` / `--leader-socket` だけ。`--permission-mode` はトップレベル (TUI) のオプションで、`grok agent` は `unexpected argument` で受け付けない。実体は `~/.grok/bin/grok` (`~/.grok/downloads/grok-1.0.46-linux-x86_64` へのシンボリックリンク) **[調査]** |
| `initialize` | `protocolVersion` 1。**`agentInfo` は返さない**。`agentCapabilities`: `loadSession: true`、`promptCapabilities {image: false, audio: false, embeddedContext: true}`、`mcpCapabilities {http: true, sse: true}`、`sessionCapabilities {list, resume, close}`、`_meta` に `x.ai/*`。`authMethods` = `cached_token` ("Cached token from ~/.grok/auth.json") / `grok.com`。ログイン済みなら `authenticate` 無しで使える。`clientInfo` は `yhtye` のままで通る **[調査][実測]** |
| `session/new` | 約 0.1〜0.3 秒。**`modes` は無い**。`configOptions` は 2 つ: **`model`** (category `model`、値は `grok-4.7` だけ) と **`reasoning_effort`** (category `thought_level`、値 `xhigh` / `high` / `medium` / `low`、既定 `high`)。ほかに `models` (model state、current `grok-4.7`) と `_meta` (`x.ai/sessionConfig` など)。応答の前後にベンダーの**通知** `_x.ai/session/setup` (段階 auth → resolve_workspace → folder_trust → plugin_registry → mcp_merge → … → response_ready) が並ぶ **[調査]** |
| `grok models` / `grok inspect` | "You are logged in with grok.com."、既定 `grok-4.7`、一覧も `grok-4.7` だけ (Free プラン)。`inspect` は、ユーザーの `~/.claude/rules/*.md` (Claude Code の規則) を Project Instructions として、スキル約 400 (ユーザー・同梱・Claude Code のプラグイン) を読み込むと出す **[調査]** |
| ベンダー拡張 | クライアントへの**リクエストは来ない**。通知: `_x.ai/session/setup`、`_x.ai/mcp/servers_updated`・`init_progress`・`server_status`、`_x.ai/models/update`、`_x.ai/settings/update`、`_x.ai/announcements/update`、`_x.ai/session_notification` (`hook_execution`、`model_changed`、`pending_interaction`、`interaction_resolved`、`response_completed`、`turn_completed` など) **[調査]**。Yhtye の acp 層は未知の通知を無視する |
| ターン中の `session/update` | 標準の `agent_message_chunk`・`agent_thought_chunk`・`tool_call`・`tool_call_update`・`config_option_update`・`session_info_update`・`available_commands_update` に加え、標準外の `tool_call_delta_chunk`・`hook_run_started` も来る **[調査]** (Yhtye では `AgentOutput::Unknown` としてそのまま残る) |
| 許可要求 | 調査のプロンプト (作業ディレクトリにファイルを作らせた) で、`session/request_permission` は**来なかった**。grok 自身が `_x.ai/session_notification` の `pending_interaction` (kind `permission`) → `interaction_resolved` で解決し、ファイルは書かれた **[調査]**。このマシンの `~/.grok/config.toml` に `[ui] permission_mode = "always-approve"` があるためと考えられる **[推測]**。選択肢の id は実行ファイル中の文字列から `allow_once` / `allow_always` / `allow_edits_for_session` / `allow_always_bash` / `allow_always_bash_glob` / `allow_always_domain` / `allow_always_mcp_tool` / `allow_always_mcp_server` / `reject_once` / `reject_always_bash` / `reject_always_mcp_tool` / `reject_always_domain` と読める (それぞれの ACP の kind は**未確認**) **[調査]** |
| ユーザーの設定の読み込み | ユーザーの MCP サーバー (このマシンでは `avatar`・`fusion`)、hooks (`~/.grok` のグローバルなものと、Claude Code のプラグインのもの。`session_start` で走った)、Claude Code の規則とスキルも読み込む **[調査]** |
| `RUST_LOG` | grok は `tracing` で `RUST_LOG` に従う。`RUST_LOG=trace` だと `initialize` → `session/new` の 3 秒で stderr に 2.6 MB 出た。設定しなければ stderr はほぼ空 (つながらない MCP サーバーの ERROR 1 行) **[調査]** |
| セッションの記録 | **プロンプトを送らないセッションも** `~/.grok/sessions/<URL エンコードした cwd>/<session id>/` に残る (Yhtye のモデル一覧のプローブも残る) **[調査][実測]** |
| 設定ファイル | 調査と実測の前後で `~/.grok/config.toml`・`~/.grok/trusted_folders.toml` の sha256 は同じ。変わったのは grok 自身のキャッシュ (`models_cache.json`・`settings_cache.json`) とセッションの記録だけ **[調査][実測]** |

### 12.1 起動と検出 (`HarnessConfig::grok_build` / `HarnessPreset::grok_build`)

- **起動**: `grok agent --no-leader stdio` (`GROK_BUILD_ARGS`) **[Yhtye]**。検出で見つけた `grok` の絶対パスを `command` にする。`npx` は要らない。起動のタイムアウトは他と同じ 120 秒。
- **`--no-leader` を付ける理由**: `grok agent --help` によると、`--leader` は「新しいエージェントを起動せず、共有の leader プロセスにつなぐ。既定は config.toml の `[cli] use_leader`」、
  `--no-leader` は「config が leader モードを有効にしていても新しいエージェントを起動する」**[調査]**。Yhtye はエージェントごとにプロセス (グループ)・作業ディレクトリ・環境を持たせ、
  終了時にグループごと止めるので、共有の leader につながるとそれが成り立たない。ユーザーの設定によらず常に付ける。このマシンの設定では leader は有効でない
  (付けなくても同じ動き) ので、leader を有効にした設定での挙動は**未確認** (§12.8)。
- **`--always-approve` は付けない** **[Yhtye]**: 許可要求は Yhtye の方針 (§12.2) で応答し、`AgentEvent::PermissionAutoAnswered` として記録に残す。
- **環境**: 継承した `RUST_LOG` だけを除く (`GROK_BUILD_ENV_REMOVE`、Devin の §10.1 と同じ理由。§12.0 の表) **[Yhtye]**。ほかはユーザーの環境をそのまま継承する (`GROK_HOME` なども)。
- **認証**: `grok login` 済み (`~/.grok/auth.json`) が前提。Yhtye は ACP の `authenticate` を呼ばない。ログイン済みなら `authMethods` があっても `session/new` から最初のターンまで通る **[実測]**。
  未ログインのときのエラーの出方は**未確認** (ログアウトが要るため試していない)。`session/new` の失敗なら既存の起動エラー (step = `session/new`) として出る。
- **検出** (`agents/detect.rs`): `PATH` → **`$GROK_HOME/bin`** (絶対パスのときだけ) → 共通の既知の場所 (`~/.local/bin` など) → **`~/.grok/bin`** の順 **[Yhtye]**。
  ハーネス専用の探す場所は MiniMax Code (§11.1) と同じ `HarnessSpec::root_env` / `extra_dirs` で足す。見つかった場所は `known_dir` として出る。
  `~/.grok/bin` はインストーラの標準の場所で、fish などでは `PATH` に入らないことがある。`GROK_HOME` が grok のホーム (`~/.grok`) を移すことは確かめた
  (`GROK_HOME=/tmp/x grok du` が "Disk usage for $GROK_HOME" を出す) **[調査]** が、インストーラがそのとき `$GROK_HOME/bin` に置くかは**未確認** **[推測]**。
- **同名の別コマンドに注意**: `grok` という名前の別のコマンド (サードパーティ製の Grok 向け CLI など) が `PATH` の先にあると、そちらが見つかって起動に失敗する (`grok agent` が無い)。
  そのときは設定 › ハーネス で `~/.grok/bin/grok` を手動パスにする。
- **ハーネスの id は `grok-build`、表示名は `Grok Build`**。組み込みの既定の順は claude-code > opencode > devin > minimax-code > **grok-build** > codex
  (モデルは未指定で grok の既定)。

### 12.2 モードと許可

- **モードは無い** (`session/new` に `modes` が無い) ので、`session/set_mode` は送らない (`mode_after_new = None`) **[調査][Yhtye]**。
- grok の権限モード (`--permission-mode`: `default` / `acceptEdits` / `auto` / `dontAsk` / `bypassPermissions` / `plan`) は TUI のトップレベルのオプションで、`grok agent` は受け付けない。
  ACP からも変えられない (モードも、権限の config option も無い) **[調査]**。ユーザーの `~/.grok/config.toml` の `[ui] permission_mode` が ACP のセッションにも効くと考えられる **[推測]**
  (このマシンは `always-approve` で、許可要求は 2 回のプロンプトとも 1 つも来なかった)。Yhtye はこの設定を書き換えない。セッションの `/always-approve` コマンドも使わない。
- **許可要求が来たときは `PermissionPolicy::OnceOnly`** **[Yhtye]** (`acp/permission.rs`): kind が `allow_once` で、id が除外に当たらないものだけを選ぶ (無ければ `Cancelled`)。
  grok の選択肢には現在の要求を超えて効くもの (`allow_always_bash` などの永続する許可、`allow_edits_for_session` のセッション全体の許可) がある。kind が分からないので、
  **T-29 で除外を id に `always` / `for_session` を含むものへ広げた** (以前は `_always` で終わるもの。Devin の `*_always`・`allow_always` もそのまま除かれる)。
  偽エージェントで、これらに kind `allow_once` を付けて前に並べても `allow_once` が選ばれ、`allow_once` が無ければ `Cancelled` になることを確認
  (`permissions_pick_allow_once_and_never_a_standing_or_session_wide_grant`、単体テスト `once_only_skips_grok_builds_scoped_and_session_wide_grants_whatever_their_kind`)。
- 権限確認の既定 (ask) を使っているユーザーで要求が来るか、来たときの選択肢の kind は**未確認** (ユーザーの設定を書き換える必要があるので試していない、§12.8)。
  README / SETUP の注意 (エージェントはあなたのリポジトリのコピーで何でも実行する。サンドボックスは無い) は Grok Build にも当てはまる。

### 12.3 モデルと effort

- **モデル**: 一覧は `session/new` の `configOptions` の `model` (プローブのセッション、プロンプトなし、§8 と同じ)。選んだモデルは `session/set_config_option` で `model` に設定して
  「要求値 = 応答の `currentValue`」を検証する **[Yhtye]**。`requires_model = false` (選ばなければ grok の既定。Free プランでは `grok-4.7` しか無い)。
- **effort**: `reasoning_effort` (category `thought_level`) に設定する。preset の `effort_config_id` は実際の id の `reasoning_effort` (`GROK_BUILD_EFFORT_CONFIG_ID`。
  category で見つけるロジック `effort_option` (§10.3) もそのまま効く) **[Yhtye]**。値は `xhigh` / `high` / `medium` / `low` (「指定なし」の `default` 行は無い)、既定 `high`。順序は §8 と同じモデル → effort。
  **[実測]** `low` を設定でき、`Ready` の `configOptions` に反映された (grok は応答に加えて `config_option_update` と `_x.ai/session_notification` の `model_changed` も送る)。
- **[実測] モデル一覧** (`ModelService`): 約 2.2 秒、`grok-4.7` ("Grok 4.7") と effort 4 つ (説明付き)、current = `grok-4.7`。プロンプトなし。

### 12.4 システムプロンプトと MCP

- システムプロンプトは `SystemPromptStyle::FirstPrompt` (最初のプロンプトの前にテキストとして付ける。`session/load` で復元したセッションには付けない) **[Yhtye]**。
  grok が `_meta.systemPrompt` を受け付けるかは調べていない (`--system-prompt-override` / `--rules` は TUI のトップレベルのオプションで、`grok agent` には無い)。
- Yhtye の HTTP の MCP は `session/new` の `mcpServers` に渡す (stdio のブリッジは要らない)。MCP の接続は `session/new` の応答の後に非同期で進む (`_x.ai/mcp/init_progress`) が、
  最初のターンで Yhtye の MCP を使えた **[実測]**。
- **[実測] grok は MCP のツールを自分のツールとして並べない** (モデルに見えるツールは `run_terminal_command`・`read_file`・`search_replace`・…・`search_tool`・`use_tool` などの 26 個)。
  MCP のツールは**遅延ツール**で、`search_tool` (`{"query": "yhtye report_step_done", "limit": 5}`) で探し、`use_tool` (`{"tool_name": "yhtye__report_step_done", "tool_input": {…}}`) で呼ぶ。
  名前は **`yhtye__<tool>`** (`mcp__` の接頭辞なし)。役割プロンプトの `mcp__yhtye__report_step_done` とは名前が違うが、**注記なしで grok は自分で探して呼べた** (1 回の実測)。
- そのため役割プロンプトへの注記 (`system_prompt_note`、Devin の §10.7 と同じ仕組み) は**付けていない** **[Yhtye]**: 注記を付けた版は実機で確かめていないので、確かめた形のまま出す。
  オーケストレータ (`create_group` などツールが多い) や別のタスクで見つけ損なう (報告なしのターンになる) ようなら、注記
  (「Yhtye のツールは MCP サーバー `yhtye` にある。`search_tool` で探して `use_tool` で呼ぶ。`mcp__yhtye__xxx` は `yhtye__xxx`」) を足すのが次の手 (§12.8)。
- grok はユーザーの MCP サーバー (`~/.grok` や Claude Code の設定のもの) も並べてつなぐ **[調査]**。ユーザーの設定を残す方針は他のハーネスと同じ。

### 12.5 そのほか

- **`clientInfo`**: 既定の `yhtye` で通る (上書き不要) **[調査][実測]**。
- **ベンダー拡張**: 独自のリクエストは来ない。`_x.ai/*` の通知と標準外の `session/update` は無視してよい (§12.0) **[調査]**。偽エージェントで「`session/new` の応答の前とターンの頭に未知のメソッドの通知が来ても、
  起動もターンも続く」ことを確認 (`no_mode_is_set_vendor_notifications_are_ignored_and_the_role_prompt_rides_the_first_prompt`)。
- **入力の大きさ**: grok はユーザーの Claude Code の規則・スキルなども読み込むので、1 ターンの入力が大きい。調査のプロンプト (ファイル作成、モデル呼び出し 2 回) は入力 約 109k トークン
  (キャッシュ 55k)、実装エージェントの 1 ターン (モデル呼び出し 6 回) は入力 約 340k トークン (キャッシュ 284k) **[調査][実測]**。利用は grok.com の利用枠から。
- **hooks**: ユーザーのグローバル hooks と Claude Code のプラグインの hooks が `session_start` などで走る **[調査]**。
- **セッションの記録**: Yhtye が起動したセッション (モデル一覧のプローブを含む) は `~/.grok/sessions/` に残る **[調査][実測]**。grok は `sessionCapabilities.close` を出すが、Yhtye は使っていない。

### 12.6 実機の結果 (Yhtye の acp 層から、grok 1.0.46、2026-10-02、プロンプト 1 回)

すべて **[実測]**。モデル一覧は `ModelService::get(&HarnessPreset::grok_build("~/.grok/bin/grok"), false)`。1 ターンは
`HarnessPreset::grok_build(..).config(Implementer, Some("grok-4.7"), Some("low"))` に、Yhtye の `McpHost` (受けたツール呼び出しを記録する `ToolPort`) の HTTP MCP と
実装者の役割プロンプト (`system_prompt(Role::Implementer)`) を付け、`step_prompt` の実装ステップ (「README.md に `hello from yhtye` の行を足す」) を送った。作業ディレクトリは一時ディレクトリの git リポジトリ。

| 項目 | 結果 |
|---|---|
| 検出 | `~/.grok/bin/grok` を見つけて起動できる |
| モデル一覧 | §12.3 のとおり。約 2.2 秒、プロンプトなし |
| 起動 | 約 0.32 秒で `Ready`。`agent` = なし (grok は `agentInfo` を返さない)、`modes` = なし、`model` = `grok-4.7`、`reasoning_effort` = `low` |
| 1 ターン | `EndTurn`、約 29.7 秒。ツール呼び出しは `grep` → `search_tool` → `run_terminal_command` → `read_file` → `search_replace` → `use_tool`。README.md に行が足され、**`report_step_done` (`result`: "Appended the line `hello from yhtye` to README.md. No other files were changed.") が Yhtye の MCP に届いた**。許可要求は 0 件、`Stderr` のイベントも 0 件 |
| 後始末 | `grok agent` のプロセスは残らない |
| 設定ファイル | 前後で `~/.grok/config.toml`・`~/.grok/trusted_folders.toml` の sha256 は同じ |

### 12.7 自動テストで確認したこと (偽エージェント、grok 本体ではない)

- `tests/acp_fake_grok_build.rs` (4 本。偽エージェントは `"modes": []` と `vendor_notifications` で grok 風に振る舞う): モードを設定せず (`session/set_mode` を送ると偽エージェントが拒否する)、
  `_x.ai/...` の通知を無視して起動とターンが続き、役割プロンプトが最初のプロンプトにだけ付き注記が無い / モデルの後に `reasoning_effort` が設定され、無い値は起動失敗として見える /
  許可の自動応答が `allow_once` を選び `allow_always*`・`allow_edits_for_session` を選ばない (選べなければ `Cancelled`) / モデル一覧が `grok-4.7` と effort 4 つを読む。
- `acp/config.rs` (`grok_build_runs_its_own_stdio_agent_without_modes_or_always_approve`)、`acp/permission.rs` (上記)、`agents/catalog.rs`
  (`grok_build_preset_runs_every_role_the_same_and_sets_reasoning_effort`)、`agents/detect.rs`
  (`grok_is_found_in_its_own_install_directory_and_in_the_home_the_environment_names`: `~/.grok/bin`、`GROK_HOME`、`PATH` が先、ほかのハーネスは探さない)、
  `agents/installed.rs` (登録と既定の順)、`tests/harness_detection.rs` (Core 経由で `grok` が見つかり登録される)、フロントエンドの `HarnessSettings.test.tsx` (設定 › ハーネス に Grok Build の行が出る)。

### 12.8 未検証の項目と手動検証チェックリスト (grok を持つ人向け)

前提: `grok login` 済みで、Yhtye の設定 › ハーネスで Grok Build が「インストール済み」。設定 › エージェント で Grok Build の行を足して、小さなタスクを流す。
結果 (成否・観察・`grok --version`) を、この節に **[実測]** として書き足す。

1. **許可要求と `allow_once` の自動応答**: `~/.grok/config.toml` の `[ui] permission_mode` を自分で既定 (確認する設定) にしている人が、作業ディレクトリ外への書き込みやシェル実行を含むタスクを流す。
   `session/request_permission` が来るか、選択肢の id と **kind** (特に `allow_edits_for_session`・`allow_always_*`) を記録し、選ばれたのが `allow_once` か (§12.2)。**未** (T-29: このマシンは `always-approve` で要求が来なかった)
2. **オーケストレータとしての MCP**: オーケストレータに Grok Build を割り当て、`create_group` / `create_task` を `search_tool` → `use_tool` で呼べるか。見つけ損なうなら、§12.4 の注記を
   `HarnessConfig::grok_build` の `system_prompt_note` に入れて再試行する。**未** (実装エージェントの `report_step_done` は済、§12.6)
3. **未ログイン時のエラー**: `grok logout` の状態でタスクを流す。`session/new` の起動エラーか、ターンのエラーか、`grok login` が読める文言か。**未**
4. **`session/load` と `session/cancel`**: タスクを流して Yhtye を再起動し、セッションが再開できるか (`loadSession: true`)。実行中の中止で `stopReason: cancelled` で止まるか。**未**
5. **leader を有効にした設定**: `[cli] use_leader = true` の人が、`--no-leader` で独立したプロセスとして動き、終了時に残らないか。**未**
6. **`$GROK_HOME/bin`**: `GROK_HOME` を変えてインストールした人の `grok` がそこにあるか (検出が見つけるか)。**未**
7. **設定ファイルが変わらない**: 1〜6 の前後で `~/.grok/config.toml` のハッシュを比べる (Yhtye は grok の設定に書き込まない)。T-29 の範囲 (§12.6) では**済**。

## 13. Google Antigravity: `agy_acp_server` (T-34/T-36、v1.3.0 で**基本動作を実機確認**)

> 実装は Devin (§10)・MiniMax Code (§11) と同じ形 (`HarnessConfig` / `HarnessPreset` / 検出 / 偽エージェント) で、**agy_acp_server 1.3.0** の実機調査
> (JSON-RPC を直接送る調査と、同梱の Python ソースを読んだこと、2026-10-08) で分かったことに合わせた。
> その後 Yhtye 自身の acp 層 (`HarnessPreset::antigravity(..).config(..)`、`ModelService`) から、**プロンプトなし**で起動・モデル一覧・未認証の失敗を実機で確かめた (§13.8)。
> さらに Google アカウント (`oauth-personal`) でログインした状態でのモデル一覧も実機で確認した (T-36、2026-10-08、§13.3/§13.8)。その状態で `gemini-3.8-flash-low` を実装エージェントにし、ドキュメントだけを変えるタスクを 1 つ流して完了まで確認した (T-36、§13.8)。
> 認証は利用者の `~/.gemini` に永続的に書き込まれ、プロンプトは利用枠を使うため、ダミーの API キーでの確認は
> `HOME` と `GEMINI_HOME` を `/tmp` の隔離ディレクトリにして、ダミーの API キーで `session/new` まで通す範囲に限った (実ユーザーの `~/.gemini` には何も作っていない)。
> 実機を呼ぶテストはリポジトリに置いていない (確認は一時的なテストで行い、消した)。
> ラベル: **[調査]** = 実機を直接調べて分かったこと、**[ソース]** = 同梱の Python ソースを読んで分かったこと (実機では試していない)、**[実測]** = Yhtye の acp 層から実機で確かめたこと、**[Yhtye]** = このリポジトリの実装。
> 節番号: §12 は Grok Build (T-29)。

### 13.0 調査のまとめ (agy_acp_server 1.3.0)

| 項目 | 結果 |
|---|---|
| 配布 | ACP レジストリの `antigravity-acp`。Linux x86_64 は `https://dl.google.com/agy-extensions/releases/linux/agy-acp-server-1.3.0-linux-x86_64.zip` (約 320 MB)。展開すると **`agy_acp_server.par`** (実行ファイル、884 MB の Python アーカイブ) と **`localharness_external`** (125 MB) の 2 つが出て、**同じディレクトリに置く必要がある**。Antigravity IDE (`/opt/Antigravity`) には同梱されていない。`agy` CLI 自体に ACP モードは無い **[調査]** |
| 起動 | **引数なし**で動く (レジストリにある `--uid=` は不要)。フラグは `--debug` と `--notices` だけ。`initialize` まで約 2〜2.5 秒 **[調査][実測]** |
| `initialize` | `agentInfo` = `antigravity-acp` / "Google Antigravity" / `1.3.0`。`loadSession: true`、`mcpCapabilities` = http・sse、`sessionCapabilities` = list・resume。`authMethods` = `oauth-personal` / `oauth-business` / `gemini-api-key` / `agent-platform` **[調査][実測]** |
| 未認証 | `session/new` が `-32000 "Authentication required"` を返す。`data.message` に「`authenticate` を呼ぶか、`settings.json` の `auth.type` を…」と出る。Yhtye では起動エラー (step = `session/new`) として読める (§13.7) **[調査][実測]** |
| 認証の保存 | `authenticate` を呼ぶと `$GEMINI_HOME/antigravity-acp/settings.json` に**ユーザーの認証方式が永続的に書き込まれる**。トークンは `$GEMINI_HOME/antigravity-acp/acp_token.json` (個人) / `acp_business_token.json`。`GEMINI_HOME` が無ければ `~/.gemini` **[調査][ソース]** |
| モード | `default` / `auto_edit` / `yolo` (`session/new` 直後は `default`) **[調査][実測]** |
| config options | `model` (category `model`、select) と `mode` (category `mode`、select) の 2 つ。**effort 用の option は無い**。`session/new` の応答に `models` (`availableModels`) も付く **[実測]** |
| モデル | **11 個** (Google アカウント / `oauth-personal` ログイン時、2026-10-08 実測): `gemini-3.8-flash-high|medium|low`、`gemini-3.7-flash-high|medium|low`、`gemini-3.6-flash-high|medium|low`、`gemini-pro-agent`、`gemini-3.1-pro-low`。API キー時の 14 個と違い 3.5 系と `gemini-3.1-pro-high` が無く `gemini-pro-agent` がある。Gemini 以外は含まれない。思考レベルはモデルの id に含まれる。既定 (`currentModelId`) はバイナリ内の定数 `gemini-3.8-flash-high`。Yhtye の既定 `gemini-3.8-flash-low` は含まれる。`session/set_model` と `session/set_config_option` (`model`) は動く **[調査][実測]** |
| 設定ファイル | `session/set_mode`・`mode` と `model` の `set_config_option`・`session/set_model` は `settings.json` を**書き換えない** (API キーの環境で確認) **[実測]** |
| システムプロンプト | 受け取る口は見つからなかった → 最初のプロンプトの前に付ける (`FirstPrompt`) **[調査]** |
| `clientInfo` | `yhtye` のままで通る (`client_info: None`)。`clientInfo.name` が zed / JetBrains / xcode のときだけ Gemini 以外のモデルも出る (Yhtye は名前を偽装しない) **[調査][実測]** |
| MCP | `mcpServers` (http / sse / stdio) を受け付け、グローバル設定とマージされる (同名ならクライアント側が優先) **[調査]** |
| 許可要求の選択肢 | `allow` (kind `allow_once`)、`deny` (`reject_once`)、`allow_always` (`allow_always`。セッションのメタデータに永続化される) **[ソース]** |
| スラッシュコマンド | `available_commands_update` で `plan` と `logout` が来る。`/logout` は**ユーザーの認証を消す** **[実測]** |
| セッションの記録 | `session/new` のたびに `$GEMINI_HOME/antigravity-acp/conversations/<uuid>.db` と `.meta` が作られる (モデル一覧の読み取りのセッションも) **[実測]** |

### 13.1 入手・配置・検出 (`HarnessSpec`)

- **入手と配置** (自動インストールはしない。検出と手動パスだけ) **[Yhtye]**: 上の zip を `unzip` で展開し (実行権限は zip が保つ)、**2 つのファイルを同じディレクトリに置く**。
  `agy_acp_server.par` は自分が起動されたパスのディレクトリ (か `PATH`) から `localharness_external` を探し、リンクの実体のディレクトリは見ない。**`.par` だけを別のディレクトリへ
  シンボリックリンクすると `session/new` が `-32603 "Could not find default localharness binary…"` で失敗する [実測]**。リンクを使うなら、環境変数 `ANTIGRAVITY_HARNESS_PATH` に
  `localharness_external` の絶対パスを渡す (リンク経由でも `session/new` が通ることを確かめた)。Yhtye はこの環境変数に触らず、起動した環境のまま継承する。
- **検出** (`agents/detect.rs`): 実行ファイル名は **`agy_acp_server.par`** (設定 › ハーネス に出る名前、手動パスで置き換えるのもこれ)。`agy_acp_server` も別名として受け付ける
  (`HarnessSpec::aliases`: 同じディレクトリでは `.par` を先に見る)。探す順は `PATH` → **`$AGY_ACP_SERVER_HOME`** と **`$AGY_ACP_SERVER_HOME/bin`** (絶対パスのときだけ。zip を展開した
  ディレクトリそのものが入る) → 共通の既知の場所 (`~/.local/bin` など) → **`~/.local/share/agy-acp-server`** → **`~/.gemini/antigravity-acp/bin`**
  (`HarnessSpec::root_subdirs` / `extra_dirs`)。`PATH` の別名が既知の場所の `.par` より先に見つかる。見つかった場所は `known_dir` として出る。
  実行ファイルは走らせない (`--version` もない)。実行可能な通常ファイルかだけを見るので、`.par` という拡張子は判定に関係しない (手動パスも同じ)。`localharness_external` の有無は検査しない。
  ほかのハーネスはこれらの場所を探さない。
- **ハーネスの id は `antigravity`、表示名は `Google Antigravity`**。組み込みの既定の順は claude-code > opencode > devin > minimax-code > **antigravity** > codex
  (Codex はモデルの指定が要るので最後)。組み込みの既定の選択は `antigravity` のモデル `gemini-3.8-flash-low`。
- **利用規約**: Antigravity の利用規約が、Google 自身のクライアント以外 (Yhtye のような第三者のクライアント) からの利用をどう扱うかは、ここでは調べていない。
  使う前にあなた自身で確認してほしい (SETUP.md にも書いてある)。

### 13.2 起動、モード、設定ファイル (`HarnessConfig::antigravity` / `HarnessPreset::antigravity`)

- **起動**: `agy_acp_server` (引数なし、stdio)。`command` は検出で見つけた絶対パス **[Yhtye]**。`npx` は要らない。環境の追加・除去は**無い**: ユーザーの環境をそのまま継承する
  (`GEMINI_HOME` も `GEMINI_API_KEY` もそのまま渡る。Yhtye は `GEMINI_HOME` を上書きしない)。起動のタイムアウトは他と同じ 120 秒。
  **これは `session/new` にも掛かる**: ブラウザでのログインを待つ間もこの時間以内に済ませる必要がある (§13.7)。
- **モード**: `session/set_mode` を送らない (`mode_after_new = None`)。`default` のままで、書き込みやシェルの許可確認は `session/request_permission` で来て、Yhtye が自動で
  `allow` (1 回だけ) を返す (§13.4)。`auto_edit` / `yolo` は設定ファイルを書き換えない [実測] が、決定として使っていない (許可確認の自動応答で足りる)。
- **ユーザーの設定ファイルに書かない**: Yhtye が書くのはセッションのモデルだけで、`settings.json` は変わらない **[実測]** (起動・モデル一覧・モデルの設定の前後で内容が同じ。
  ただし API キーの環境で、OAuth ログイン後は未確認)。
- **記録が溜まる**: `session/new` のたびに `$GEMINI_HOME/antigravity-acp/conversations/` に 2 ファイル (`.db`、`.meta`) が増える。Yhtye は後片付けをしない
  (モデル一覧の読み取りのセッションでも増える)。これはサーバーの仕様で、Devin / MiniMax Code の「何も残さない」一覧のセッションとは違う。

### 13.3 モデルと effort

- **モデル**: 一覧は `session/new` の `configOptions` の `model` (プローブのセッション、プロンプトなし、§8 と同じ)。選んだモデルは `session/set_config_option` で `model` に設定して
  「要求値 = 応答の `currentValue`」を検証する **[Yhtye]**。**既定のモデルは `gemini-3.8-flash-low`** (`ANTIGRAVITY_DEFAULT_MODEL`、クレジット節約のため。サーバー自身の既定は
  `gemini-3.8-flash-high`) で、設定の行がモデルを持たなくてもこれで動く。ただし `HarnessPreset::antigravity` の `requires_model = false`:
  サーバーの既定はバイナリの定数で、ユーザーの設定ではない (MiniMax Code や OpenCode と違って、ユーザーの高いモデルで意図せず動く心配がない)。
  認証方式によってモデル一覧は異なる **[実測]**:
  - **Google アカウント (`oauth-personal`) ログイン時** (2026-10-08 実測、`currentModelId` は `gemini-3.8-flash-high`): 次の **11 個**
    `gemini-3.8-flash-high`, `gemini-3.8-flash-medium`, `gemini-3.8-flash-low`, `gemini-3.7-flash-high`, `gemini-3.7-flash-medium`, `gemini-3.7-flash-low`, `gemini-3.6-flash-high`, `gemini-3.6-flash-medium`, `gemini-3.6-flash-low`, `gemini-pro-agent`, `gemini-3.1-pro-low`。
  - **ダミーの API キーで認証時**: **14 個** (`gemini-3.8-flash-high|medium|low`、`gemini-3.7-flash-…`、`gemini-3.6-flash-…`、`gemini-3.5-flash-…` (各 3 段階)、`gemini-3.1-pro-high|low`)。
  - **差分と注意点**: OAuth ログイン時の一覧は、API キー時と異なり 3.5 系 (各 3 段階) と `gemini-3.1-pro-high` が無く、代わりに `gemini-pro-agent` が含まれる。Gemini 以外のモデルは含まれない。Yhtye の既定モデル `gemini-3.8-flash-low` はどちらの一覧にも含まれるため問題なく動作する。使えないモデルの除外 (`unusable_models`) は無し。
- **effort は対応しない**: `HarnessPreset::effort_config_id` が `None` (新設。Devin / MiniMax Code は effort の option を持つので `Some`)。**[Yhtye]**
  - モデル一覧の読み取り (`probe_models`) は、モデルを 1 つずつ選んで effort を調べることをせず、全モデルを `efforts: Some([])` ("effort 無し") にする。
    モデルが多数あるので、`set_config_option` をモデル数分呼んで、結果が空と確かめることを省く。`get_efforts` (モデル 1 つの effort) もサーバーを起動せずに空を返す。
  - 設定に新しく保存する行に effort が付いていれば、既存の `check_efforts` が「このモデルには effort が無い」と拒否する (Devin と同じ)。UI とエージェント一覧には effort が出ない。
  - 手書きの設定などで effort が付いた選択が起動まで来ても、`HarnessPreset::config` は effort を送らない (無い option に `set_config_option` を送って起動を失敗させない)。
  - 偽エージェントで確認 (`the_listing_has_no_efforts_and_asks_the_server_for_none`、`the_efforts_of_a_model_are_none_without_starting_the_server`、`an_effort_is_not_sent_because_there_is_no_effort_option`)。
    **[実測]** 実機の一覧でもモデルすべてが `efforts: []` (API キー時の 14 個すべてで確認)。

### 13.4 許可の自動応答: `PermissionPolicy::OnceOnly`

- 選択肢は `allow` (kind `allow_once`)、`deny` (kind `reject_once`)、`allow_always` (kind `allow_always`、セッションのメタデータに永続化される) **[ソース]**。
  preset は Devin と同じ **`PermissionPolicy::OnceOnly`** にする **[Yhtye]** (`acp/permission.rs`): kind が `allow_once` のものだけを選ぶ。`allow_always` は kind でも
  `_always` で終わる id でも選ばれない。位置ではなく kind で選ぶ点は他と同じ。
- 偽エージェントで確認 (`permissions_pick_allow_and_never_allow_always`): `allow_always` / `deny` / `allow` の並びから `allow`、`allow_always` と `deny` だけのときは `Cancelled`。
- **[未検証]** 実際に許可要求が来る操作 (`default` モードでのファイル編集・シェル) と、`allow` の自動応答が実機で通ること。認証が要るので試していない (§13.8)。

### 13.5 システムプロンプト、`clientInfo`、MCP

- システムプロンプトを受け取る口が見つからないので、`SystemPromptStyle::FirstPrompt` (最初のプロンプトの前にテキストとして付ける。`session/load` で復元したセッションには付けない)。
  役割プロンプトへの追加の注記 (`system_prompt_note`) は**無し** **[調査][Yhtye]**。偽エージェントで確認 (`the_role_prompt_rides_the_first_prompt_only_and_yhtye_keeps_its_own_name`)。
- `clientInfo` は既定の `yhtye` で通る。zed / JetBrains / xcode の名前を名乗ると Gemini 以外のモデルも出るが、**他社製品の名前は名乗らない**方針なので `client_info: None` のまま
  (Gemini のモデルだけが出る) **[Yhtye]**。
- Yhtye の HTTP の MCP は `session/new` の `mcpServers` に渡す。**[未検証]** 実際につないで `report_step_done` を呼ばせること、ツール名がどう見えるか (`mcp__yhtye__<tool>` のままか、
  メタツール経由か)。ユーザーのグローバル設定の MCP サーバーとマージされる (同名なら Yhtye の側が優先) **[調査]**。

### 13.6 そのほか

- **`/logout` は Yhtye から送られない**: サーバーは `available_commands_update` で `plan` と `logout` を出し、`/logout` は**ユーザーの認証 (`acp_token.json` など) を消す**。
  Yhtye がエージェントに送るプロンプトは、必ず `[yhtye:step]` や `[yhtye:user_message]` などの見出しで始まる (`domain/inbox.rs` の `render_batch`、役割プロンプトの前置)。
  利用者がチャットに `/logout` と打っても `[yhtye:user_message]\n/logout` として届き、行頭は見出しなのでスラッシュコマンドにならないはず (**[未検証]**: サーバーが行頭以外のスラッシュを解釈するかは見ていない)。
  Yhtye が生のスラッシュコマンドを送るのは Claude Code の使用量取得 (`/usage`) だけ。
- **`available_commands_update` が `Ready` より前に来る**: `session/new` の直後にサーバーが送るので、`Ready` の前に `Output(AvailableCommands)` が 1 つ届く。
  Yhtye の処理には問題ない (`session/load` で復元するときの再生の破棄 (`runtime/launch.rs`) とは別)。
- **料金**: 利用は Google アカウント (または API キー) の利用枠を使う。既定のモデルを Flash の `low` にしているのはそのため。Yhtye は課金を管理しない。

### 13.7 ログインと失敗の見え方

- **`agy_acp_server` に端末でのログインの手順は無い**: `--help` のフラグは `--debug` と `--notices` だけで、ログイン用のサブコマンドやフラグは無い **[実測]**。端末で起動しても stdin の ACP の
  JSON-RPC を待つだけで、「ターミナルで一度起動してログインする」ことはできない。ソースでも、ログインを始めるのは ACP の `authenticate` と `session/new` の認証の確認だけ。
  Yhtye は `authenticate` を**呼ばない** (呼ぶと `settings.json` に利用者の認証方式が永続的に書き込まれるため。選ぶのは利用者)。
  代わりに、`settings.json` の `auth.type` が `oauth-personal` なら **`session/new` が (キャッシュされたトークンが無いとき) ブラウザの Google ログインを始める** **[ソース]**。
  また Zed や JetBrains のような `authenticate` を呼ぶ ACP クライアントで一度ログインしても、同じ `settings.json` とトークンが残るはず **[ソース]**。
- **未ログインの失敗** **[実測]**: Yhtye の acp 層で起動すると、`initialize` は通り (約 2.4 秒)、`session/new` が `-32000` で失敗する。メッセージは次のとおり
  (`HarnessConfig::login_hint`、`ANTIGRAVITY_LOGIN_HINT` が既存の起動エラーのメッセージの後ろに付く):

  ```text
  agent startup failed at `session/new`: Authentication required: {
    "message": "No authentication method selected. Either call the `authenticate` method (supports oauth-personal, gemini-api-key, agent-platform), or set `auth.type` in settings.json (<GEMINI_HOME>/antigravity-acp/settings.json) to one of: oauth-personal, gemini-api-key (requires GEMINI_API_KEY env var), …"
  }
  Google Antigravity にログインしていません (Yhtye は代わりにログインしません)。`$GEMINI_HOME/antigravity-acp/settings.json` (GEMINI_HOME が無ければ …) に `{"auth": {"type": "oauth-personal"}}` を書いて もう一度始めると、ブラウザで Google のログインが開きます (…)。詳しくは SETUP.md の「Google Antigravity」。
  ```

  モデル一覧の読み取り (設定 › エージェント) でも同じ文が「could not start the harness: …」として出る。ヒントは `session/new` / `session/load` の認証エラー (`-32000`) にだけ付き、ほかのエラーには付かない
  (`HarnessConfig::login_hint` が `Some` のハーネスだけ。Antigravity だけが持つ)。偽エージェントで確認 (`an_authentication_error_says_how_to_log_in_and_other_errors_do_not`)。
- **API キーで使う場合**: `settings.json` の `auth.type` を `gemini-api-key` にして、環境変数 `GEMINI_API_KEY` を Yhtye の起動環境に入れる (設定 › 秘密の環境変数に登録すると**すべてのエージェント**に渡る)。
  `GEMINI_API_KEY` だけでは認証方式として選ばれない (環境変数だけの選択は廃止されている) **[ソース]**。ダミーのキーでも `session/new` まで通る (少なくともそこまではキーを検証しない) **[実測]**。
- **時間**: ブラウザでのログインを待つ間、`session/new` は 120 秒 (`startup_timeout`) で `Timeout` になる。初回は長めに見て、タイムアウトしたらもう一度始める **[未検証]**。

### 13.8 実機の結果 (Yhtye の acp 層から、agy_acp_server 1.3.0、2026-10-08、プロンプトなし)

すべて **[実測]**。`HarnessPreset::antigravity("/tmp/agy-acp/agy_acp_server.par")` を `HOME` と `GEMINI_HOME` を `/tmp` の隔離ディレクトリにして使った。実ユーザーの `~/.gemini` は前後で変化なし
(`~/.gemini/antigravity-acp` は作られていない、`settings.json` のハッシュも同じ)。

| 項目 | 結果 |
|---|---|
| 未認証 (設定ファイルなし) | `initialize` は通り、約 2.6 秒で `session/new` が `Authentication required` で失敗。メッセージに §13.7 のヒントが付く。隔離ディレクトリには何も作られない (`settings.json` も) |
| モデル一覧 (ダミーの API キー、`settings.json` を手で書いた隔離環境) | 約 3.1 秒で 14 個、全部 `efforts: []`、現在のモデルは `gemini-3.8-flash-high`。`settings.json` は変わらない |
| モデル一覧 (Google アカウント / `oauth-personal` ログイン時) | 11 個 (`gemini-3.8-flash-high|medium|low`、`gemini-3.7-flash-high|medium|low`、`gemini-3.6-flash-high|medium|low`、`gemini-pro-agent`、`gemini-3.1-pro-low`)、現在のモデルは `gemini-3.8-flash-high`。API キー時 (14 個) と違い 3.5 系と `gemini-3.1-pro-high` が無く `gemini-pro-agent` がある。Gemini 以外は無し。Yhtye の既定 `gemini-3.8-flash-low` は含まれる (2026-10-08 実測) |
| セッションの開始 (同上) | 約 2.7 秒で `Ready`。モードは `default`、モデルは `gemini-3.8-flash-low` (サーバーの既定 `gemini-3.8-flash-high` から置き換わる)。effort 付きの選択 (`high`) でも起動は成功 (effort は送られない)。`gemini-3.1-pro-low` を選ぶとそのモデルになる。`Ready` の前に `available_commands_update` が 1 つ届く |
| シンボリックリンク | `.par` へのリンク (`agy_acp_server`) は `session/new` が `-32603 "Could not find default localharness binary…"`。`ANTIGRAVITY_HARNESS_PATH` を渡すと通る |
| 後始末 | 終了後に `agy_acp_server` のプロセスは残らない。隔離ディレクトリには `conversations/<uuid>.db` と `.meta` がセッションごとに残る |

**実測 (T-36、Google アカウントでログイン済み、2026-10-08)**: ログインは、端末で `agy_acp_server.par` に `initialize` と `authenticate` (`methodId: "oauth-personal"`) の JSON-RPC を標準入力から送って済ませた。ブラウザで Google ログインが開き、`settings.json` (`oauth-personal`) と `acp_token.json` が作られた。その後 Yhtye の実装エージェント (`gemini-3.8-flash-low`) に、ドキュメント 2 ファイルを直して commit するタスクを流し、編集・`git commit`・Yhtye の MCP 経由の `report_step_done` まで完了した (システムプロンプトは `FirstPrompt` のままで足りた)。この間に許可要求は来なかった (`default` モードのまま)。タスクの前後で `settings.json` の内容は変わらなかった。

**未検証**: Yhtye の中で始めたときの OAuth のブラウザの流れ (`startup_timeout` との兼ね合い)、実際の許可要求と `allow` の自動応答、MCP のツール名の見え方の詳細、`session/load`、`session/cancel`、`yolo` / `auto_edit` の挙動、
Windows / macOS のビルド。

### 13.9 自動テストで確認したこと (偽エージェント、`agy_acp_server` 本体ではない)

- `tests/acp_fake_antigravity.rs` (7 本): `session/set_mode` を書かずモデルだけを設定する (モデル無指定の選択は既定のモデルになり、選べばそのモデルになる)、役割プロンプトが最初のプロンプトにだけ付き
  `clientInfo` が `yhtye` のまま、effort が送られない、許可の自動応答が `allow` を選び `allow_always` を選ばない (選べなければ `Cancelled`)、認証エラーにログインのヒントが付き
  ほかのエラーには付かない、モデル一覧が `set_config_option` なしで全モデルを `efforts: []` にし保存済みでない effort を拒否する、`get_efforts` がサーバーを起動せず空を返す。
  偽エージェントは新しい機能を足さずに済んだ (既存の `fail_kind: auth_required`、`permission_options`、`report_writes`、`report_state`、`report_client_info`、モードとモデルの一覧で足りる)。
- `agents/catalog.rs` の preset の単体テスト (`antigravity_preset_runs_on_its_cheapest_model_and_offers_no_effort`)、`acp/config.rs` (`antigravity_runs_without_arguments_touches_nothing_and_points_to_the_login`)、
  `agents/detect.rs` の単体テスト (`agy_acp_server_is_found_by_either_name_in_its_own_directories_and_in_the_root_the_environment_names`: 両方の名前、`AGY_ACP_SERVER_HOME` とその `bin`、
  `~/.local/share/agy-acp-server`、`~/.gemini/antigravity-acp/bin`、`PATH` が先、ほかのハーネスは探さない、`a_manual_path_with_an_extension_is_an_executable_file_like_any_other`)、
  `agents/installed.rs` (登録と既定の順・既定のモデル)、`tests/harness_detection.rs` (Core 経由で `agy_acp_server.par` が見つかり登録される)。

### 13.10 手動検証チェックリスト (Google アカウントで Antigravity を使える人向け)

前提: §13.1 のとおり 2 つのファイルを同じディレクトリに置き、Yhtye の設定 › ハーネスで Google Antigravity が「インストール済み」。ログインは §13.7 のどちらかで済ませる。
設定 › エージェント で Google Antigravity の行を implementer などに足して、小さなタスクを流す。結果 (成否・観察・バージョン `1.3.0` のほか) を、この節に **[実測]** として書き足す。
**済** / **未** を各項目の末尾に書いて、進んだ形を残す。**前後で `~/.gemini/antigravity-acp/settings.json` と `acp_token.json` のハッシュを比べる** (項目 7)。

1. **ログイン**: `settings.json` を `{"auth": {"type": "oauth-personal"}}` にして Yhtye で Antigravity のエージェントを始める。ブラウザの Google ログインが開くか、
   完了すると `session/new` が通るか、120 秒以内に済むか (超えたときの `Timeout` の見え方)。(**未**。端末から `authenticate` を送る方法でのログインは**済**、§13.8)
2. **実装エージェントとしてタスクを完了できる**: 小さな変更 (1 ファイル) のタスクを流す。`McpHost` 経由で `report_step_done` が呼ばれ、タスクが完了状態まで進むか。
   ツール名が `mcp__yhtye__report_step_done` のまま見えるか、メタツール経由か (経由なら Devin の `DEVIN_MCP_NOTE` のような注記が要る)。(**済**: T-36 で注記なしに `report_step_done` まで完了)
3. **許可要求が来る操作で `allow` が自動で選ばれる**: 既定の `default` モードで、ファイルの編集・シェルの実行を含むタスクを流す。`session/request_permission` が来るか、
   選ばれた選択肢の id が `allow` か、`allow_always` を選ばないこと。選ばれた許可が次のターンに残らないこと。(**未**)
4. **ログイン後のモデル一覧**: 設定 › エージェント のモデルの選択肢が、ダミーのキーで見た 14 個と同じか。アカウントで違うなら、`gemini-3.8-flash-low` が無いときの起動の失敗の見え方。(**確認済み**: 2026-10-08 実測。Google アカウント (`oauth-personal`) でログインした状態では 11 個 (`gemini-3.8-flash-high|medium|low`、`gemini-3.7-flash-high|medium|low`、`gemini-3.6-flash-high|medium|low`、`gemini-pro-agent`、`gemini-3.1-pro-low`) で、`currentModelId` は `gemini-3.8-flash-high`。API キー時の 14 個と異なり 3.5 系と `gemini-3.1-pro-high` が無く `gemini-pro-agent` がある。Gemini 以外のモデルは含まれず、Yhtye の既定 `gemini-3.8-flash-low` は含まれるため問題なく動作する)
5. **`session/load` と `session/cancel`**: タスクを 1 つ流して Yhtye を再起動し、セッションが再開できるか (履歴の再生と `Ready`、`loadSession` の扱い)。実行中に中断するとターンが
   `stopReason: cancelled` で止まるか。(**未**)
6. **プロンプトのストリーム**: メッセージ・思考・ツール呼び出し・`usage_update` が Yhtye の画面に期待どおり出るか。`Ready` 前の `available_commands_update` が画面に出ないか。(**一部済**: T-36 でターンが流れて完了した。画面の表示の細部は未確認)
7. **設定ファイルが変わらない**: 1〜6 の前後で `~/.gemini/antigravity-acp/settings.json` と `acp_token.json` のハッシュを比べる (`auth.type` はログインのときに自分で書いた分だけが変わる)。
   `/logout` が Yhtye 経由で送られないこと (チャットに `/logout` と打ってもログアウトしないこと)。(**未**)
