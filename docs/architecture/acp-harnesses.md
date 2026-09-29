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
| OpenCode | **採用 (Stage 7c)** — 組み込みの `opencode acp` (2.0.12)、§7。`opencode` がインストールされていれば全役割で選べる (7c-2、§7.6) |
| Codex | **採用 (Stage 7e)** — `@agentclientprotocol/codex-acp` 2.0.0 (npx、Codex 本体はユーザーの `codex`)、§9。`codex` が `PATH` にあれば全役割で選べる。調査: [`research/codex-acp.md`](research/codex-acp.md) |
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

したがって OpenCode のオーケストレータは `build` モードで、読み取り専用はシステムプロンプト (「作業はタスクに委ねる」) だけが頼り。
`orchestration-model.md` §11 の緩和策が OpenCode では効かないことを 7c-2 でユーザーに示す必要がある。

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
- オーケストレータを OpenCode にした場合は読み取り専用にならない (7.3)。UI / ドキュメントで明示するか、OpenCode をオーケストレータ候補から外すかを決める。
- ACP の `tool_call` は MCP 呼び出しが `execute` (コードモード) に見える。UI でツール名を出す箇所は `rawInput.code` を見せるか、
  MCP サーバー側の `ToolCalled` を使う。
- MCP の隔離が無い (7.2)。必要になったら Yhtye 専用の `OPENCODE_CONFIG_DIR` (アプリのデータディレクトリ配下、他ユーザーが書けない場所) を渡す。
- 再起動復元: session/load は `cwd` が作成時と同じでないと拒否される。Yhtye は同じ作業ディレクトリ (プロジェクト / worktree) で load するので問題ないはずだが、
  パスが変わった場合 (worktree の作り直しなど) は load が失敗する。オーケストレータは失敗時に新しいセッションへフォールバックするが、
  サブエージェントの経路でも同様に扱えるかを 7c-2 で確認する。FirstPrompt のシステムプロンプトは新しいセッションの最初のプロンプトにだけ付き、
  load で復元したセッションには付けない (履歴に残っている)。

### 7.6 ランタイムへの組み込み (Stage 7c-2、2026-09-23)

- **登録**: `HarnessPreset::opencode(fallback_model, env_remove)` (id `opencode`、全役割 `HarnessConfig::opencode`、probe は mode / model の
  切り替えなし)。アプリ (`src-tauri`) と開発ブリッジは `CoreConfig::installed(data_dir, model)` を使い、`agents::installed_presets` が
  **`PATH` に実行可能な `opencode` があるときだけ**登録する (無ければ UI に出ない)。Claude Code は従来どおり常に登録し、全役割の組み込みの既定。
- **モデルは必須**: preset の `requires_model = true`。設定の検査 (`AgentCatalog::validate`) が OpenCode の `model: null` を
  `invalid_argument` で拒否し、UI も「既定のモデル」の候補を出さない。それでも model 無しで起動される場合 (古い設定など) は
  preset の設定に入っている `OPENCODE_FALLBACK_MODEL` (= 無料の `opencode/muse-spark-1.3-contributor-free`) を set_config_option する —
  **OpenCode の「最後に使ったモデル」では起動しない**。
- **モデル一覧**: 実機で 475 件、約 1.5 秒 (`real_opencode_model_list_comes_from_the_preset_probe`)。値はすべて `<provider>/<model>`。
  プローブも OpenCode のセッションを 1 つ作る (`data_dir/model-probe` の cwd、OpenCode の履歴に残る。プロンプトは送らない)。
- **オーケストレータ**: 選べる (ユーザー決定)。読み取り専用にできない (7.3) ので preset の `orchestrator_read_only = false` を UI に渡し、
  設定パネルが「⚠ 書き込み制限なし」を出す (オーケストレータの役割で OpenCode の候補を**出す・選ぶ**どちらでも)。
  プロンプトでの禁止 (`orchestrator.md`) はそのまま。
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
- **`codex` が `PATH` にあるときだけ** preset を登録する (`agents::installed_presets`、OpenCode と同じ。無ければ UI に出ない)。
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

### 9.3 オーケストレータは読み取り専用にできない

`read-only` モードは「承認が要る」で、Yhtye の自動承認 (allow_always 優先) だと書けてしまい、`reject_once` を返すとターンごと `cancelled` になる。
そこで OpenCode と同じ扱い (ユーザー決定): オーケストレータも `agent-full-access`、preset の `orchestrator_read_only = false`
(設定パネルが「⚠ 書き込み制限なし」を出す)、役割のプロンプト (`orchestrator.md`) が書き込みを禁じる。

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
