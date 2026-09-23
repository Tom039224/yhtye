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
| Codex / Cursor CLI / Gemini CLI / GitHub Copilot / Google Antigravity CLI / Grok Build / OpenCode | 未着手 |
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
args = ["-y", "@agentclientprotocol/claude-agent-acp@0.81.0"]   # Stage 6b で完全一致に固定 (npx -y は解決したものを実行するため)
env = { ANTHROPIC_MODEL = "haiku", ENABLE_TOOL_SEARCH = "false", ENABLE_CLAUDEAI_MCP_SERVERS = "false", CLAUDE_CODE_DISABLE_BACKGROUND_TASKS = "1" }  # §5.2
mode_after_new = "bypassPermissions"
model = { config_id = "model", value = "haiku" }   # set_config_option。応答の現在値で検証する
system_prompt = "meta_append"          # _meta.systemPrompt.append
session_meta = { claudeCode = { options = { strictMcpConfig = true } } }   # §5.2 (オーケストレータは + tools)
startup_timeout = 120                  # 秒。各起動段ごと
```

(`HarnessConfig::claude_code("haiku")` がこの値を返す。)

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

オーケストレータはさらに `tools: ["Read","Glob","Grep"]`
([`orchestration-model.md`](orchestration-model.md) §8.1)。
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
