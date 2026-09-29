# 調査: OpenAI Codex CLI を Yhtye のハーネスにする (codex-acp)

> **状況 (2026-09-29): この調査に基づいて Stage 7e で実装済み。** 採用した仕様・ユーザー決定・実機の結果は [`../acp-harnesses.md`](../acp-harnesses.md) §9 と
> [`PLAN.md`](../../PLAN.md) の Stage 7e。以下は調査時点の記録で、そのまま残す (Codex 0.159 + `~/.codex` では、アダプタ自身のモデル一覧が OpenRouter のモデルも
> 含むなど、§4 の記述と実測が変わった点は acp-harnesses.md §9.4 に書いた)。


調査日 2026-09-29。Codex CLI 0.159.0 (`~/.local/bin/codex`)、`@agentclientprotocol/codex-acp` 2.0.0。
コード変更・コミットなし (この文書のみ)。ラベル: **[実測]** 実機で確認、**[文書/コード]** README・`dist/index.js` を読んだ、**[推測]**。
検証には無料枠のモデル `stealth/space-bunny-alpha` (ユーザーの `~/.codex/config.toml` の既定、OpenRouter 経由) を一行プロンプトで数回だけ使った。

## 1. 結論

| 項目 | 結論 |
|---|---|
| 推奨する入口 | **`@agentclientprotocol/codex-acp`** (bin `codex-acp`、Node 22 で動作確認)。Zed 製 `@zed-industries/codex-acp` (0.16.0、Rust) は **アーカイブ済み・npm で「replaced by @agentclientprotocol/codex-acp」と非推奨**。後継は ACP 組織のリポジトリ (TypeScript、2026-09-28 に v2.0.0、直近 1 か月でほぼ週次リリース、活発) |
| Codex 本体の ACP | **無い [実測]**。`codex acp` はサブコマンドではない。`codex app-server` は Codex 独自の JSON-RPC (ACP ではない)。アダプタが `codex app-server` を子プロセス (stdio) として起動して ACP に翻訳する |
| 動くか | **動く [実測]**。initialize → session/new (HTTP MCP 付き) → prompt (シェル実行・MCP ツール呼び出し) → cancel → 別プロセスで session/load まで全部通った。1 ターン 2.5〜7 秒 |
| Yhtye の現構造でそのまま足りるか | **ほぼ足りるが、ギャップが 6 つある** (§7)。特に (a) モデル・effort が「実行時に set_config_option」ではなく `CODEX_CONFIG` env で入れる方が現実的、(b) 失敗 (認証・課金) が JSON-RPC エラーではなく**普通の応答テキスト + `end_turn`** で返る、(c) 読み取り専用オーケストレータが作れない |

- 他の候補: `cola-io/codex-acp` (第三者の Rust 実装、`gh search`)、Codex の app-server 直叩き (ACP ではないので Yhtye の ACP クライアントが使えない)。どちらも推奨しない。
- 依存: npm パッケージは `@openai/codex@^0.158.0` を同梱 (`node_modules` 約 441 MB。Codex のネイティブバイナリを含む)。ユーザーの Codex を使うには `CODEX_PATH=~/.local/bin/codex` を渡す。0.159.0 でも動作確認済み [実測] (0.159 の方がモデルの effort 情報が取れて `reasoning_effort` の config option が出た。§4)。
- 起動: `npx -y @agentclientprotocol/codex-acp@2.0.0` (Claude Code と同じく完全一致で固定) か、グローバル導入した `codex-acp`。初回 `npx` は 441 MB のダウンロードになるので、`startup_timeout` (120 秒) の余裕と、導入済み前提の検出 (`codex-acp` が PATH にあるか) が要る [推測]。

## 2. initialize / session/new [実測]

- initialize は約 0.25 秒、session/new は約 0.5 秒 (HTTP MCP 付きでも)。`agentInfo {name: "@agentclientprotocol/codex-acp", version: "2.0.0"}`。
- `agentCapabilities`: `loadSession: true`、`promptCapabilities {image, embeddedContext}`、`sessionCapabilities {resume, list, close, delete, fork, additionalDirectories, subagents}`、**`mcpCapabilities {http: true, sse: false, acp: false}`** (stdio 型はコマンド起動で別途対応)。
- `authMethods`: `api-key` (`CODEX_API_KEY`/`OPENAI_API_KEY`) と `chat-gpt`。**ただしユーザーの `config.toml` にカスタムプロバイダがあると認証は不要**で、`_auth/status_update {kind:"gateway", label:"Custom model gateway", detail:"openrouter"}` の通知が来るだけ (Yhtye は authenticate を呼ばなくてよかった)。ChatGPT ログインのユーザーは未検証 (このマシンに無い) [推測: `~/.codex/auth.json` をそのまま使う]。
- session/new の応答: `sessionId` (UUIDv7)、`models`、`modes`、`configOptions`。
- `cwd`: **守る [実測]**。ツール呼び出しの `cwd` と `pwd` は session/new の `cwd` (`/tmp/cxrepo`)。プロセス自体は `/tmp` で起動。session/new が渡した `cwd` を `projects.<cwd>.trust_level = "trusted"` として設定に足す [コード] (信頼ダイアログで止まらない)。
- 起動時の通知は `available_commands_update` (`plan`, `mcp`, `skills`, `review`, `compact`, `goal` ほか) と `session_info_update`。

## 3. モード・権限 [実測 + コード]

`modes` と config option `mode` の両方に出る (id は同じ)。承認方針とサンドボックスの組で、**ターンごとにアダプタが明示的に `turn/start` に付ける** (`CODEX_CONFIG` の `approval_policy`/`sandbox_mode` では上書きできない [コード])。

| id | 名前 | 承認 | サンドボックス | 実測 |
|---|---|---|---|---|
| `read-only` | Read-only | on-request | readOnly | シェルでファイル作成 → `session/request_permission` (title `Run command`、選択肢 `allow_once` / `allow_always` (id `accept_execpolicy_amendment`) / `reject_once` (id `cancel`))。**Yhtye の自動承認 (allow_always 優先) だと書けてしまう** |
| `workspace-write` | Workspace access | on-request | workspaceWrite | 未実測 |
| `agent` (**既定**) | Auto review | on-request + auto_review | workspaceWrite | ワークスペースへの書き込みは承認なし。`/tmp` への書き込みも通った (workspaceWrite は `/tmp` を含む) |
| `agent-full-access` | Full access | **never** | dangerFullAccess | 承認要求ゼロ。**「yolo」の等価物**。シェルでファイル作成・MCP ツール呼び出しとも承認なしで通った |

- `session/set_mode {modeId}` と `session/set_config_option {configId:"mode"}` のどちらも使える (どちらも 0 ms 程度)。**`session/load` するとモードは既定 (`agent`) に戻る** ので、Yhtye が load 後にも set_mode する現行の作り (`mode_after_new`) のままでよい。`INITIAL_AGENT_MODE` env で初期値も選べる [コード]。
- もう一つの config option **`collaboration_mode`**: `default` / `plan` (category `collaboration_mode`)。`plan` に切り替え可 (`set_config_option`)。Plan モード + `read-only` で MCP ツールを呼ばせると、**呼び出せた**が (OpenCode の plan のように拒否されない)、MCP ツール呼び出しにも `session/request_permission` が来る (選択肢 `allow_once` / `allow_always` (id `allow_session`) / `allow_always` (id `allow_always`) / `reject_once` (id `cancel`)、`toolCall.title` は無し)。
- **reject_once を返すとターンごと `stopReason: cancelled` で終わる** [実測] (`read-only` でシェル書き込みを拒否 → ツール failed → cancelled)。Yhtye は「ユーザーのキャンセル」と区別する必要がある。
- ツール名: MCP は `tool_call {title: "mcp.yhtye.report_step_done", kind: execute, rawInput {server, tool, arguments}}` (Claude の `mcp__yhtye__…` とは違う、OpenCode の `execute` コードモードとも違い引数が構造化されて見える)。シェルは `title = コマンド全文`、`rawInput {command, cwd}`。

## 4. モデルと effort [実測 + コード]

- **`models.availableModels`** (`modelId` は `<model>[<effort>]`、例 `gpt-6-sol[high]`) と config option `model` (`value` は effort 抜きの `<model>`) の両方がある。`session/set_model` (`modelId` は必ず `<model>[<effort>]` 形式) も使える。
- **一覧の出どころは OpenAI の Codex モデルカタログ** (app-server の `model/list`、`gpt-6-astra/sol/luna`、`gpt-5.6-*`、`gpt-5.5`、8 モデル)。**ユーザーの `model_provider = "openrouter"` の OpenRouter モデル一覧は出ない**。OpenRouter で使えないモデルも並ぶ。
- **現在のモデル** (ユーザーの `config.toml` の `model`) は一覧に無くても選択肢の先頭に足される (`stealth/space-bunny-alpha`)。
- **一覧に無い値は `set_config_option` で拒否** (`-32602 Invalid params`) [実測]。**カスタムプロバイダのモデルを ACP の呼び出しだけで選ぶ方法は無い**。代わりに env **`CODEX_CONFIG`** (JSON、`thread/start` の `config` にマージされる) に `{"model": "acme/x", "model_reasoning_effort": "medium"}` を入れる → 現在のモデルが `acme/x[medium]` になり選択肢の先頭にも足される [実測]。その値での `set_config_option {configId:"model", value:"acme/x"}` は成功する (= 現行の「要求値 = 応答の現在値」検証がそのまま通る)。`ANTHROPIC_MODEL` と同じ「env で初期値を入れる」型で、Yhtye の `model_env` の考え方が使える (env 名は `CODEX_CONFIG` で、値は JSON なので差し込み方が違う)。
- **effort**: config option の id は **`reasoning_effort`** (category `thought_level`、Yhtye の `EFFORT_CONFIG_ID = "effort"` とは違う)。値はモデルごと (`gpt-6-luna`: `low, medium, high, xhigh, max`、`minimal` を渡した `CODEX_CONFIG` で `[minimal]` が現在値になる)。**モデルが effort の情報を持たなければ option 自体が出ない** (同梱の 0.158 + `stealth/space-bunny-alpha` では出ず、ユーザーの 0.159 (`CODEX_PATH`) では `reasoning_effort` (現在値 `high`) が出た)。未知の値は `-32602`。`session/set_config_option` は先にモデル、後で effort の順で問題ない (Claude と同じ)。`model_reasoning_effort` を `CODEX_CONFIG` で入れる方が確実 [推測]。
- `fast-mode` option (`off`/`on`) が出るモデルもある。
- **`~/.codex/config.toml` の `model`・`model_reasoning_effort` は既定値として効く** (Yhtye が何も指定しないとユーザーの既定モデルで動く。OpenCode の「最後に使ったモデル」問題と同じ種類 → `requires_model = true` にして必ず指定させるのが安全)。
- 使用量: `usage_update` と prompt 応答の `usage {totalTokens, inputTokens, cachedReadTokens, outputTokens, thoughtTokens}` と `_meta.quota`。

## 5. システムプロンプト

- **session/new の `_meta` に systemPrompt は無い** [コード]。
- **`CODEX_CONFIG` の `developer_instructions`** を入れると `thread/start` に渡り効いた [実測] (合言葉を教えて答えさせた)。プロセスはセッションごとに起動するので env でセッション単位にできる。ただし環境変数の長さ制限 (Linux は 1 引数 128 KB) に注意。
- **最初のプロンプトの前置 (`FirstPrompt`)** は当然使える (OpenCode と同じ)。`session/load` 後も履歴に残る。実装が最も単純 → **`SystemPromptStyle::FirstPrompt` を推奨** (`developer_instructions` は代替案)。
- Codex 標準の **`AGENTS.md` (cwd の) は読まれる** [実測] (合言葉テスト)。

## 6. デーモン・CODEX_HOME・環境変数

- **アダプタはデーモンを使わない [実測]**。`codex-acp → node …/@openai/codex/bin/codex.js app-server → codex app-server` (stdio) の**専用の子プロセス**を起動し、終了時に一緒に終わる (プロセスグループ kill でも、stdin EOF でも全消滅、常駐プロセスは残らなかった。ユーザーの `~/.codex` のデーモン `app-server --listen unix:// --managed-daemon` は起動前後で同じ PID のまま)。
- したがって **プロバイダの `auth.command` (`sh -c 'echo $OPENROUTER_API_KEY_CODEX'`) はアダプタ (= Yhtye が起動した) プロセスの環境で動く**。デーモンのようにキーが古い環境に固定される問題は無い。
  - fish 経由 (キーを export した環境) で起動 → 成功。**素の bash (キー未設定) で起動 → `provider auth command sh produced an empty token` (応答テキスト + `end_turn`、約 7 秒、usage null)** [実測]。
  - → **Yhtye をデスクトップから起動すると fish の設定は継承されない**ので、ユーザーは Yhtye の環境 (例 `~/.profile`、`environment.d`、systemd user env) にキーを置くか、Yhtye が設定として保持する必要がある。秘密の扱い (Yhtye の設定に平文で持たない) はユーザー決定事項 (§8)。
- **`CODEX_HOME`**: この端末は Orca が `CODEX_HOME=~/.config/orca/codex-runtime-home/home` を注入する (`ORCA_CODEX_HOME` が同じ値)。Orca の home は別の `config.toml` (`model_provider = "openrouter"` だけで **`model` 指定なし**)・hooks.json 付き。
  - Orca の home で `codex exec` → 既定モデルが OpenRouter で 403 (`Key limit exceeded (total limit)`) になり **5 回再接続してから失敗、約 7.3 秒** [実測]。実 home (`~/.codex`、`stealth/space-bunny-alpha`) では約 4.0 秒で成功。**「`codex exec` が変に遅い」は Orca の `CODEX_HOME` を使っていたことが原因の可能性が高い** (モデル未指定 → 使えない既定モデル → 再試行)。`codex exec` はこの間デーモンの子としては動かず、自プロセスで動いた [実測]。
  - **推奨: OpenCode の `OPENCODE_CONFIG_DIR` と同じ扱い**。`CODEX_HOME` が `ORCA_CODEX_HOME` と同じ値 (= Orca が注入) のときだけエージェントの環境から外す (`env_remove = ["CODEX_HOME"]`、`agents/installed.rs` の `inherited_opencode_env_remove` の Codex 版)。ユーザー自身が設定した `CODEX_HOME` は残す。
- アダプタの env: `CODEX_PATH` (使う codex)、`CODEX_CONFIG` (JSON、セッション設定)、`MODEL_PROVIDER`、`INITIAL_AGENT_MODE`、`NO_BROWSER`、`APP_SERVER_LOGS` (ログのディレクトリ)。

## 7. その他の実測

- **cancel**: 長文生成中 (4 秒後) の `session/cancel` → **約 10 ms** で `stopReason: cancelled`。続けてプロンプトを送ると `end_turn` で普通に動いた。
- **session/load**: 別プロセスで `session/load {sessionId, cwd, mcpServers}` が約 0.6 秒で成功、履歴 (`user_message_chunk`、`agent_message_chunk`) を**応答より前に**再生 (Yhtye は Ready 前の Output を捨てる作りなので問題なし)。前のターンの合言葉を覚えていた。新しい `mcpServers` が使われる (実行時に `mcp_servers` 設定として渡す設計 [コード])。セッションはユーザーの `~/.codex/sessions` に保存される (Yhtye のセッションも Codex の履歴に出る)。
- **HTTP MCP**: 自作の最小 MCP サーバー (Streamable HTTP、JSON 応答) で、`initialize` → `notifications/initialized` → `tools/list` → `tools/call` が**`Authorization` ヘッダ付きで**届いた (プロトコル版 `2025-06-18`)。`ttlMs`/`cacheScope` は不要。ツール呼び出し (`report_step_done`) は成功。最初の `tools/list` だけヘッダ無しで 1 回来る (アダプタ/Codex の事前確認 [推測])。SSE は不可。**Yhtye の MCP サーバーが認証必須でヘッダが無いリクエストを 401 で返しても、その後の接続には影響しなかった**。
- **警告テキスト**: カタログに無いモデルでは、毎ターンの先頭に `Warning: Model metadata for … not found. Defaulting to fallback metadata; …` が **`agent_message_chunk` として出力に混ざる** [実測] (OpenRouter のカスタムモデル)。
- **失敗の見え方 [実測]**: 認証エラー・プロバイダの 400/403 は **JSON-RPC エラーではなく `agent_message_chunk` のテキスト + `stopReason: end_turn`** (usage が null または 0)。OpenCode は `-32000` の JSON-RPC エラーだったのと違う。Yhtye は「エージェントがエラーを喋った」と「実際の作業」を区別できない。
- **ユーザー設定の混入**: `~/.codex/config.toml` の全体・`~/.codex/skills`・`AGENTS.md`・hooks が有効になる (`mcp_servers` はこのユーザーの設定には無いので混入は未確認)。アダプタは同名の MCP サーバーだけ重複排除する [コード]。**MCP の隔離手段は無い** (Claude の `strictMcpConfig` 相当なし)。個別に `mcp_servers.<名前>.enabled=false` を `CODEX_CONFIG` で足す手はあるが名前を知る必要がある [推測]。隔離するなら専用 `CODEX_HOME` に `config.toml` (プロバイダ・モデル) をコピーする案があるが、`auth.json` / セッション履歴の扱いが増える。OpenCode と同じく **「ユーザー設定は有効のまま」を既定にする**のが自然。
- **遅延・ハング**: 遅い/固まる現象は再現しなかった (実 home。`initialize` 0.25 秒、`session/new` 0.5 秒、1 ターン 2.5〜7 秒)。遅かったのは Orca の `CODEX_HOME` の再試行 (上記) と、キー無しの認証失敗 (約 7 秒) のみ。

## 8. Yhtye の要求に対するギャップ

| # | ギャップ | 影響 | 対応案 |
|---|---|---|---|
| 1 | **読み取り専用のオーケストレータを作れない** (`read-only` は「承認が要る」で、Yhtye の自動承認だと書ける。reject するとターンが `cancelled`。`plan` の collaboration_mode でも MCP は呼べるが、シェルによる書き込みは止まらない可能性 (未実測)) | `orchestrator_read_only = false` (OpenCode と同じ警告) | 案 A: `agent-full-access` + プロンプトで禁止 (OpenCode 相当)。案 B: `read-only` + 権限応答ポリシーに「MCP ツールは許可、それ以外は reject」を足す (reject でターンが cancelled になるのを Yhtye が許容する必要あり)。案 C: `CODEX_CONFIG` で `features.shell_tool=false` にしてシェルを無効化 (未実測、ファイルも読めなくなる) |
| 2 | モデル: 一覧が OpenAI カタログで、OpenRouter などカスタムプロバイダのモデルは選べない・出ない | Yhtye の「選択肢からモデルを選ぶ」UI がカスタムプロバイダで使えない | env `CODEX_CONFIG` にモデルを入れる (§4)。一覧は ChatGPT/OpenAI ユーザーでは有効。カスタムプロバイダ用は自由入力が要る (Yhtye は一覧に無い値を選べる?) |
| 3 | effort の config id が `reasoning_effort` (Yhtye は `effort` 固定)、モデルによって option が無い、`CODEX_CONFIG` の `model_reasoning_effort` でも指定できる | Yhtye の effort 設定 (`EFFORT_CONFIG_ID`) が Codex に効かない | `HarnessConfig.effort` の `config_id` を per-preset にする (すでに `ModelSelect` なので型はそのまま)。`catalog.rs` の `EFFORT_CONFIG_ID` 定数の使用箇所を preset 側に寄せる |
| 4 | 失敗が応答テキスト + `end_turn` | 認証切れ・課金上限が「作業した」ように見え、`protocol_violation` (報告なしのターン) として現れる | 「MCP の報告ツールが呼ばれず usage が null」のようなヒューリスティック、または警告文の除去、または `_meta`/通知に失敗が出ないか (AIR の session-failure 拡張は AIR クライアントのみ) 追加調査 |
| 5 | 認証キーが Yhtye の環境に無い (デスクトップ起動) | auth.command が空トークンを返す | ユーザー決定 (§9) |
| 6 | reject → cancelled (§3)、Orca の `CODEX_HOME` (§6)、モデル未指定の既定 (§4) | — | `env_remove`、`requires_model = true`、権限応答ポリシー |

`agent-full-access` は**ネットワークも制限なし**でホスト全体にアクセスできる (「Full access: Unrestricted access to the internet and any file」)。Claude Code の `bypassPermissions` と同等だが、実装者にはこれを使うことになる。`agent` (Auto review) は既定で、ワークスペースの書き込みは承認なし、ネットワーク・ワークスペース外は承認が要る (自動承認で通るはず。未実測)。

## 9. 提案: HarnessConfig / HarnessPreset

全役割共通の土台 (OpenCode と同型):

```toml
[harness.codex]
command = "codex-acp"                 # または npx -y @agentclientprotocol/codex-acp@2.0.0
args = []
env = { CODEX_PATH = "<ユーザーの codex>", CODEX_CONFIG = '{"model":"<model>","model_reasoning_effort":"<effort>"}' }
env_remove = ["CODEX_HOME"]           # ORCA_CODEX_HOME と同じ値のときだけ (§6)
mode_after_new = "agent-full-access"  # implementer / reviewer / investigator。orchestrator は下記 (§8-1)
model = { config_id = "model", value = "<model>" }         # CODEX_CONFIG で先に入れた値と同じ (検証用)
effort = { config_id = "reasoning_effort", value = "<effort>" }   # option が無いモデルでは付けない
system_prompt = "first_prompt"
startup_timeout = 120
```

- `HarnessPreset::codex(...)`: `id = "codex"`、`label = "Codex"`、`requires_model = true` (ユーザーの config の既定モデルを黙って使わない。OpenRouter で使えないモデルが選ばれる事故も避ける)、`model_env = Some("CODEX_CONFIG")` は**形式が違う** (JSON 全体を組み立てる) ため `model_env` の代わりに専用のモデル差し込み関数が要る。`probe`: プロンプトなしの短命セッション (`session/new` だけで約 0.5 秒) で `configOptions` から一覧を読む。`orchestrator_read_only = false`。
- 役割ごとの案:

| 役割 | モード | 補足 |
|---|---|---|
| orchestrator | `agent-full-access` (+プロンプトで禁止) が最も単純。または `read-only` + 権限ポリシー (§8-1) | 読み取り専用にできない → UI で警告 (OpenCode と同じ) |
| implementer | `agent-full-access` | 承認要求ゼロ (Claude の bypass 相当) |
| reviewer | `agent-full-access` か `agent` | レビューアがファイルを変更しないよう指示文で縛るのは他のハーネスと同じ。`read-only` は承認が来るだけで役に立たない |
| investigator | implementer と同じ | — |

## 10. ユーザーに決めてほしいこと

1. **入口**: `@agentclientprotocol/codex-acp` (推奨、バージョンを完全一致で固定) でよいか。npx (441 MB のダウンロード) か、導入済みの `codex-acp` を要求するか。同梱の Codex 0.158 か、ユーザーの `codex` (`CODEX_PATH`) か (推奨: ユーザーの `codex` があればそれ。設定 (`config.toml`) との互換が取れる)。
2. **オーケストレータの読み取り専用**: 上記 A (プロンプトのみ、OpenCode と同じ) / B (`read-only` + 権限ポリシー) / C (シェル無効) のどれか。
3. **モデルの選び方**: OpenAI カタログのみ (ChatGPT/API キーのユーザー向け) か、カスタムプロバイダ (OpenRouter) のために自由入力を許すか。このユーザー自身は OpenRouter を使っているので自由入力が要る。
4. **認証キーの渡し方**: OpenRouter のキーをどうやって Yhtye の環境に渡すか (デスクトップ起動では fish の設定は継承されない)。Yhtye の設定に持つ (平文は避けたい)、`environment.d`/systemd の user env に置く、または「ユーザーが端末から起動する前提」。
5. **`CODEX_HOME`**: Orca 注入時のみ外す (推奨) でよいか。ユーザー設定 (`config.toml`、skills、AGENTS.md、hooks) を有効のまま (OpenCode と同じ) でよいか (MCP を隔離する手段は無い)。
6. **失敗の扱い** (§8-4): 認証・課金エラーが応答テキストで返る問題を、Yhtye 側のヒューリスティックで検出するか、まず実装しないか。
7. **セッション履歴**: Yhtye が起動したセッションがユーザーの `~/.codex/sessions` に残ること (と終了時に `session/delete` で消すか) を許容するか。

## 11. 検証に使ったもの / 後始末

- ドライバ: `/tmp/drv.py`・`/tmp/s*.py` (JSON-RPC 直叩きの Python)、`/tmp/mcp.py` (最小 MCP サーバー)、`/tmp/cxrepo` (一時 git リポジトリ)、`/tmp/codexacp` (npm i した 441 MB)。リポジトリ (Yhtye) の変更はこの文書のみ。
- `~/.codex/sessions` に検証用のセッションが十数件残る (`session/delete` は使っていない)。ユーザー設定は変更していない。
- 終了時、`codex-acp`・アプリサーバー・`codex exec`・MCP サーバーの残存プロセスは無し。`~/.codex` のデーモン (PID 2856885 と `pid-update-loop` 2856988) は開始時のまま。
