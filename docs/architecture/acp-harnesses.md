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
| Devin | **実装済み・基本動作は実機確認済み (T-5〜T-10、T-21)** — Devin CLI の `devin acp`、§10。`devin` が見つかれば全役割で選べる。devin 3000.11.3・無料プラン (SWE-1.6 Slow) で起動・`bypass`・モデル設定・1 ターン・HTTP MCP の接続を確認 (§10.0)。実際の `report_step_done`、未ログイン時、`session/load`、cancel は未確認 (§10.9) |
| Cursor CLI / Gemini CLI / GitHub Copilot / Google Antigravity CLI / Grok Build | 未着手 |
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
  [`core-design.md`](core-design.md) §15.2。組み込みの既定は claude-code > opencode > devin > codex の最初の見つかったもの。
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
- Devin の preset は **`PermissionPolicy::OnceOnly`** **[Yhtye]** (`acp/permission.rs`): kind が **`allow_once`** で、id が `switch_` / `plan_` で始まらず `_always` で終わらないものだけを選ぶ。
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
- 実際に `report_step_done` を呼んでタスクが完了まで進むかは**未確認** (プロンプト枠を節約したため、§10.9)。
- Devin はユーザーの `~/.config/devin/mcp_config.json` のサーバーも並べてつなぐ **[実測]** (つながらないサーバーは警告をログに出すだけ)。

### 10.8 検出と UI

- 検出は `devin` の実行ファイルを `PATH` → 既知の場所の順に探すだけ (実行はしない)。手動パス・再検出は設定 › ハーネス ([`core-design.md`](core-design.md) §15.2)。
- 組み込みの既定の順は claude-code > opencode > **devin** > codex (Devin は、モデルの指定が要る Codex より先。モデルは未指定で Devin の既定)。
- システムプロンプト (役割の指示) は `SystemPromptStyle::FirstPrompt`: 最初のプロンプトの前にテキストとして付ける (後ろに §10.7 の注記) **[Yhtye]**。Devin が `_meta` の system prompt を受け付けるかは調べていない。
  `session/load` で復元したセッションには付けない (履歴に残る前提)。Devin は `loadSession: true` を出す **[実測]** が、`session/load` で履歴を再生するか・復元が通るかは**未確認**。

### 10.9 手動検証チェックリスト (Devin を持つ人向け)

前提: `devin auth login` 済みで、Yhtye の設定 › ハーネスで Devin が「インストール済み」。設定 › エージェント で Devin の行を implementer などに足して、小さなタスクを流す。
結果 (成否・観察・Devin のバージョン `devin --version`) を、この節に **[実測]** として書き足す。

**T-21 の状況** (devin 3000.11.3、無料プラン): **済** = 1 (起動・`initialize`)、3 (`bypass`。許可要求は MCP のメタツールでは来ない。ファイル作成・シェルは未)、
4 (`model` / `swe-1-6-slow`、effort の option は無い)、5 (`yhtye` で通る)、6 (`request_diagnostics` は来ない)、7 のうち `mcpCapabilities.http` と MCP の接続・ツール一覧 (§10.0)。
**未** = 2 (未ログイン時のエラー)、7 のうち実際の `report_step_done` の呼び出しとタスクの完了、8 のうち `session/load`・`session/cancel`・`WINDSURF_API_KEY`
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
