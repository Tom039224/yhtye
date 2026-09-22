# オーケストレーションモデル

Yhtye が何をどう組み立てて動かすかの機能設計。ここに書いた内容はユーザーと合意済みの
**確定事項**であり、実装はこれに従う。変える場合はまずこのファイルを更新する。

関連:
- MCP ツールの引数・戻り値・遷移: [`mcp-tools.md`](mcp-tools.md)
- モジュール構成・インターフェース: [`core-design.md`](core-design.md)
- ACP / ハーネス: [`acp-harnesses.md`](acp-harnesses.md)

## 1. 基本原則

- **ユーザーはオーケストレータとのみ対話する。** サブエージェントに直接指示しない。
- **指示と報告の経路は Yhtye がホストする MCP サーバーのツールだけ。**
  オーケストレータもサブエージェントも、Yhtye の状態を変えるのは MCP ツール呼び出し
  (ACP `session/new` の `mcpServers` で各セッションに渡す) に限る。
  エージェントの自然文出力を Yhtye がパースして状態を推測することはしない。
- **工程 (Step) 間の遷移は Yhtye が機械的に進める。** オーケストレータ (LLM) を起こすのは
  次の場合だけ (§5):
  1. ユーザーのメッセージ
  2. `checkpoint` 工程に到達した
  3. `help` が上がった
  4. 依存が解消したが `instruction` が空のタスクがある
  5. グループ内の全タスクが終端状態になった
- **全エージェントは独立した ACP セッション。** サブエージェントはオーケストレータの
  子プロセスではなく、Yhtye が起動する同格のセッション。ハーネス固有のサブエージェント
  機能 (Claude Code の Task ツール等) には依存しない。
- **コアは汎用 ACP / MCP にのみ依存する。** 当面のハーネスは Claude Code のみだが、
  ハーネス固有の知識は設定 (起動コマンド・env・モード名) に閉じ込める。

## 2. 構造

```
Project (git リポジトリ 1 つ)
 └─ Group (プロジェクトごとに active は最大 1)
     └─ Task (kind, depends_on, instruction, steps)
         └─ Step (implement / review / checkpoint / done)
```

### 2.1 Group

| 属性 | 内容 |
|---|---|
| `id` | Yhtye が採番 (`G-<連番>`) |
| `title` / `summary` | オーケストレータが `create_group` で与える |
| `base_branch` | グループ作成時にプロジェクトのメイン作業ツリーでチェックアウトされていたブランチ |
| `group_branch` | `yhtye/<groupId>` — `base_branch` の HEAD から作成 |
| `status` | `active` → `finishing` → `done` / `cancelled` / `merge_blocked` |

- **active なグループはプロジェクトごとに最大 1 つ。** データモデル上は複数持てるが、
  `create_group` 時の 1 か所のチェックでのみ制約する (将来の並行グループに備える)。
  マージ中 (`finishing`) も数える。`merge_blocked` は数えない (ユーザーが整えるまで次の依頼を
  止めないため。Stage 3a で明確化)。
- 全タスクが終端 (`done` / `cancelled`) になると Yhtye はオーケストレータを起こす。
  オーケストレータはタスクを追加するか、`finish_group` を呼ぶ。
- `finish_group` で Yhtye は `group_branch` を `base_branch` に自動マージする (§6)。

### 2.2 Task

| 属性 | 内容 |
|---|---|
| `id` | `T-<連番>` (プロジェクト内で一意) |
| `title` | 短い表示名 |
| `kind` | Yhtye 定義。当面 `code` と `investigate` の 2 種 (§3) |
| `depends_on` | 先行タスク ID の配列。全て `done` になるまで開始しない |
| `instruction` | 担当エージェントへの指示。**空でもよい** (「今定義し、後で指示する」) |
| `steps` | 工程の列。オーケストレータが組む |
| `status` | §4 の状態機械 |

- 依存が解消し `instruction` が空なら、Yhtye はオーケストレータを起こし
  `set_instruction` を求める。依存先タスクの結果 (最終報告) を起こすメッセージに含める。

### 2.3 Step

| kind | 実行者 | 内容 | 完了条件 |
|---|---|---|---|
| `implement` | タスクの担当エージェント (タスク中は同一セッションを使い続ける) | 指示を実行する。`investigate` タスクでは読み取り専用の調査 | `report_step_done(result)` |
| `review` | **毎回新しい**レビュアーセッション | 直前までの変更 (タスクブランチの group ブランチからの差分) を審査 | `report_step_done(result, verdict)`。`verdict` は `approve` / `needs_changes` |
| `checkpoint` | Yhtye → オーケストレータ | 直前までの結果を添えてオーケストレータを起こす | オーケストレータが `resolve_checkpoint` を呼ぶ |
| `done` | Yhtye | 終端。`code` タスクならタスクブランチを group ブランチへマージ | マージ成功 |

- `steps` の最後は必ず `done`。無ければ Yhtye が末尾に補う。`done` を途中に置くのはエラー。
- 各 Step は `instruction` (任意) を持てる。無ければタスクの `instruction` が使われる。
- **review が `needs_changes` を返した場合**、Yhtye はその review の直後に
  `implement` (レビュー所見を指示として付与、同じ担当セッション) と `review` を自動挿入する。
  自動挿入はタスクあたり `max_review_rounds` 回 (既定 2) まで。超えたら自動で `help`
  (`kind = review_rounds_exhausted`) を上げてオーケストレータに判断を委ねる。
- Step の kind は今後の実運用を見て追加できるよう、Yhtye 側の列挙型 + 「Step 開始時の
  ハンドラ」「完了時のハンドラ」の組として実装する ([`core-design.md`](core-design.md))。

### 2.4 help (割り込み)

- どのエージェントも**いつでも** `help` ツールを呼べる。方針レベルの相談
  (「アプローチを変えたい」「仕様が曖昧」) にも使う。いつ使うべきかはシステムプロンプトで誘導する (§8)。
- `help` が上がるとタスクは `handling` になり、オーケストレータは即座に起こされる
  (グループの完了を待たない)。
- Yhtye 自身も `help` を上げる: マージコンフリクト、レビュー回数超過、
  エージェントがツールを呼ばずにターンを終えた (§7)、エージェントプロセスの異常終了。
- オーケストレータは `answer_help` で返答する (担当エージェントへ返答を送って再開 /
  工程の変更後に再開 / タスク中止)。

## 3. タスク kind

| kind | 作業場所 | 書き込み | 完了時 |
|---|---|---|---|
| `code` | タスク専用の git worktree + ブランチ `yhtye/<groupId>/<taskId>` (group ブランチから分岐) | 可 | 未コミット変更を Yhtye がコミット → group ブランチへマージ |
| `investigate` | group の統合 worktree (group ブランチ。共有、§6) | 不可 (プロンプトで禁止し、終了時に `git status` が汚れていたら `help`) | マージなし。結果テキストのみ |

kind は Yhtye 定義の列挙で、オーケストレータは選ぶだけ。追加は実運用後。

## 4. タスクの状態機械

```
             deps 未解消            deps 解消 & instruction 空
 [pending] ──────────────▶ (待機) ─────────────────────────▶ [awaiting_instruction]
     │ deps 解消 & instruction あり                                 │ set_instruction
     ▼                                                              ▼
 [running] ◀────────────────────────────────────────────────────────┘
   │  current_step = implement / review : エージェントのターンが進行中
   │  current_step = checkpoint         : → [checkpoint] (オーケストレータ待ち)
   │  current_step = done               : → [merging]
   │
   ├─ help ─────────────────▶ [handling] ── answer_help(resume) ──▶ [running]
   ├─ cancel_task ──────────▶ [cancelled]
   └─ (Yhtye 再起動時) ──────▶ [interrupted] ── 自動再開 ─────────▶ [running]

 [checkpoint] ── resolve_checkpoint(continue) ──▶ [running] (次の Step)
              ── resolve_checkpoint(abort) ─────▶ [cancelled]
 [merging]    ── 成功 ──▶ [done]
              ── コンフリクト ──▶ [handling] (help kind = merge_conflict)
```

- 終端状態は `done` / `cancelled`。
- UI (`docs/design/orchestrator-desktop.md` の `TaskStatus`) への対応:

| 内部状態 | UI 表示 |
|---|---|
| `pending` / `awaiting_instruction` | 待機 (新規に追加する表示) |
| `running` + `implement` | `implementing` |
| `running` + `review` | `reviewing` |
| `checkpoint` / `handling` / `interrupted` | `handling` |
| `merging` / `done` | `done` |
| `cancelled` | 中止 (新規に追加する表示) |

## 5. オーケストレータの起こし方 (受信箱)

オーケストレータは 1 つの ACP セッション。Yhtye は「起こす理由」を**受信箱**に積み、
オーケストレータがアイドル (直前の `session/prompt` が返った状態) のときに
溜まったものを 1 つの `session/prompt` にまとめて送る。

- 受信箱の種類: `user_message` / `checkpoint_reached` / `help_raised` /
  `instruction_needed` / `group_settled` / `merge_result`。
- 各項目は機械可読な見出し + 本文の決まった書式のテキストにする
  (例: `[yhtye:help_raised] help_id=H-3 task=T-110 kind=blocked\n<本文>`)。
- ターン中に新しい項目が来ても、そのターンを中断しない。ユーザーが明示的に
  キャンセルした場合だけ `session/cancel` を送る (中断は §9 のとおり並行に届く設計)。
- 未配達の受信箱は SQLite に保存され、再起動後も失われない。

## 6. git

- **group ブランチ** `yhtye/<groupId>`: `create_group` 時に `base_branch` の HEAD から作る。
  Yhtye 管理の**統合 worktree** (`<data_dir>/worktrees/<projectId>/<groupId>/_group`) に
  チェックアウトする。タスクブランチのマージはここで行う。
- **タスクブランチ** `yhtye/<groupId>/<taskId>`: `code` タスクの開始時に group ブランチの
  現在の HEAD から作り、専用 worktree (`.../<groupId>/<taskId>`) にチェックアウトする。
- worktree はリポジトリの外 (Yhtye のデータディレクトリ) に置き、ユーザーの作業ツリーを汚さない。
- **タスク完了時**: 未コミット変更があれば Yhtye がコミット (メッセージはタスク ID + 最終報告の要約)
  → 統合 worktree で `git merge --no-ff <taskBranch>` → 成功で worktree を削除 (ブランチは残す)。
- **グループ完了時** (`finish_group`): メイン作業ツリーが `base_branch` をチェックアウト中で
  clean なら、そこで `git merge --no-ff <groupBranch>`。そうでない
  (dirty / 別ブランチ) ならグループは `merge_blocked` となり、オーケストレータとユーザーに通知する。
  ユーザーが整えた後に再試行できる。
- **コンフリクト**は `git merge --abort` で巻き戻したうえで `help` (`merge_conflict`) にする。
  オーケストレータは例えば `modify_steps` で「group ブランチを自分のブランチへマージして
  解消せよ」という `implement` を足し、`answer_help(resume)` する。工程が終わると再び `done` →
  マージが再試行される。
- git 操作は `git` CLI をサブプロセスで呼ぶ (worktree / merge の挙動を git 本体と一致させるため)。

## 7. エージェントのターンとプロトコル違反

- Step 開始時、Yhtye は担当セッションに `session/prompt` を送る (工程の指示・必要な文脈を含む)。
- ターンが `end_turn` で終わったのに `report_step_done` も `help` も呼ばれていなければ、
  Yhtye は 1 回だけ催促プロンプトを送る。それでも呼ばれなければ `help`
  (`kind = protocol_violation`) を自動で上げる。
- `max_tokens` / `max_turn_requests` / `refusal` で終わった場合も同様に `help`
  (`kind = agent_stopped`)。
- エージェントプロセスが異常終了した場合は `help` (`kind = agent_crashed`)。

Stage 3a で明確化した細部:

- 「呼ばれたか」はドメイン状態で判定する (Step が `done` なら報告済み、タスクが `handling` なら
  help 済み)。催促の回数は Step ごとに数え、help への返答 (新しいプロンプト) で 0 に戻る。
- `cancelled` で終わったターン (Yhtye がタスクを止めた) は何もしない。
- サブエージェントの**起動失敗**も `agent_crashed`。
- 返答待ち (`help` 後) の間にそのエージェントのプロセスが死んだ場合は新しい help を上げず、
  既存の help に「エージェント喪失」を記録する。`answer_help(resume)` は返答をプロンプトで送る
  代わりに、新しいセッションで Step をやり直す (返答は note として付く)。
- Yhtye 起因の help (`protocol_violation` / `agent_crashed` 等) の `resume` は最初の未完了 Step を
  新しいプロンプトでやり直す (セッションが生きていれば同じセッション、死んでいれば新規)。

## 8. 権限とシステムプロンプト

- **権限は自動承認。** ACP `session/request_permission` には、選択肢のうち `kind` が
  `allow_always` のもの、無ければ `allow_once` のものを選んで返す。**「先頭の選択肢」を
  選ぶ実装にはしない。** allow 系が無い場合は `cancelled` を返し、イベントとして記録する。
- Claude Code ではさらにセッション作成直後にバイパス系モードへ切り替え、
  要求自体を発生させない (モード名は [`acp-harnesses.md`](acp-harnesses.md))。
- システムプロンプトは役割ごとに用意する (`orchestrator` / `implementer` / `reviewer`)。
  本文は `yhtye-core` 内のテンプレートとして管理し、各役割について次を明記する:
  - **orchestrator**: ユーザーとだけ話す。自分でコードを書かず、グループ / タスクを MCP ツールで組む。
    Step の組み方の指針 (単純な作業は `implement → done`、リスクがあれば `review`、
    判断が要る分岐点に `checkpoint`)。起こされたときのメッセージ書式の読み方。
    ユーザーへの報告はテキストで行う。
  - **implementer**: 与えられた worktree の中だけで作業する。終わったら必ず
    `report_step_done` を呼ぶ。詰まった・方針を変えたい・指示が曖昧なら `help` を呼び、
    ターンを終えて返答を待つ。`investigate` では書き込み禁止。
  - **reviewer**: 差分を読み、`verdict` 付きで `report_step_done`。自分では修正しない。
- ACP にはシステムプロンプトの標準フィールドが無いため、ハーネス固有の拡張
  (Claude Code の場合は `_meta`) を使い、使えない場合は最初の `session/prompt` の
  先頭に付ける ([`acp-harnesses.md`](acp-harnesses.md))。
- 本文は `crates/yhtye-core/prompts/{orchestrator,implementer,reviewer}.md` (初版は Stage 2)。
  LLM 向けなので英語で書き、オーケストレータには「ユーザーの言語で返答する」と指示する。

### 8.1 オーケストレータに書き込ませない (Stage 2 で決定)

オーケストレータもバイパス権限で動くため、そのままではメイン作業ツリーを直接書き換えられる
(§11 に挙げていた未決事項)。Stage 2 で実機確認し、次のように決めた。

- **組み込みツールを読み取り系だけにする。** Claude Code ではオーケストレータの
  `session/new` の `_meta.claudeCode.options.tools = ["Read", "Glob", "Grep"]`
  (`HarnessConfig::claude_code_orchestrator`)。Agent SDK の `tools` は組み込みツールだけを
  絞るオプションで、MCP ツール (`mcp__yhtye__*`) は残る。`Write` / `Edit` / `Bash` /
  `Agent` (Claude Code 自身のサブエージェント) も消える。
  実 Haiku に「自分のツールで note.txt を作れ」と直接頼んでも作れないことを確認した
  (`tests/orchestration_claude_real.rs::real_orchestrator_harness_cannot_write_files`。
  Haiku は書き込みを試みたがツールが無く、`NO_WRITE_TOOL` と答えた)。
- プロンプトでも「自分でファイルを書かない・コマンドを実行しない」と指示する (二重の防御)。
- **採らなかった案**: cwd を読み取り専用の場所にする — オーケストレータは計画のために
  プロジェクトを読む必要があり、また cwd の外への書き込みは防げない。
  `disallowedTools` での除外 — 将来増える書き込み系ツールを取りこぼすので、許可リスト
  (`tools`) の方が安全。
- 他ハーネスを足すときは、そのハーネスで同等の制限ができるかを追加時に確認する。

## 9. キャンセル

- ACP の `session/cancel` は**通知**であり、`session/prompt` の応答待ちと**並行して**
  送れなければならない。以前の実装は「コマンド受信ループの中で prompt の応答を await」
  していたため、ターン中にキャンセルが届かなかった。新設計ではセッションごとに
  prompt の実行を別タスクに切り出し、コマンドの受信は止めない ([`core-design.md`](core-design.md) §3)。
- `cancel_task` / ユーザーによるタスク中止: ターン中なら `session/cancel` → 応答
  (`stop_reason = cancelled`) を待つ (タイムアウトで強制終了) → セッションを閉じ、
  プロセスを終了 → worktree を削除 (ブランチは残す) → `cancelled`。

## 10. 永続化と再開

- SQLite (sqlx)。**イベントログ** (追記のみ) と**現在状態テーブル** (groups / tasks / steps /
  sessions / helps / inbox) を同一トランザクションで更新する。
- 再起動時、`running` / `merging` だったタスク (エージェントのターンまたは git 操作の途中で
  止まったもの) は `interrupted` として表示し、自動で再開する。`checkpoint` / `handling` /
  `awaiting_instruction` はオーケストレータ待ちなのでそのまま (受信箱から再配達)。
  - ハーネスが `session/load` をサポートし ACP セッション ID を保存済みなら、それで復元して
    現在の Step の続きを促すプロンプトを送る。
  - そうでなければ新しいセッションを作り、タスクの instruction・Step の履歴・worktree の
    `git diff` を添えて現在の Step をやり直させる。
- オーケストレータのセッションも同様に復元する。

## 11. まだ決まっていないこと

- ハーネスの自動選定 (当面は役割ごとに設定で固定)。
- 並行グループ (データモデルは許容。制約を外す UI・運用は未設計)。
- `code` 以外で書き込みを伴う kind (例: ドキュメントのみ) を足すか。
- `max_review_rounds` や並行実行数の上限をどこで設定させるか。
- オーケストレータは自分でコードを書かない方針にした (§8) が、画面設計の筋書き
  (`docs/design/orchestrator-desktop.md` §1: 「オーケストレータが自分で修正を実行中」) とは
  ずれる。必要ならオーケストレータ自身が担当する Task (担当役割 = orchestrator) を後で足す。
- ~~オーケストレータのセッションもバイパス権限で動くと、メイン作業ツリーを直接書き換えられて
  しまう。~~ → Stage 2 で決定 (§8.1: 組み込みツールを Read / Glob / Grep に限定)。
- Yhtye が起動する Claude Code に**ユーザー自身の設定** (`~/.claude` の CLAUDE.md・フック・
  プラグイン・スキル) をどこまで効かせるか。Stage 2 では MCP サーバーだけ隔離し
  (`strictMcpConfig`、[`acp-harnesses.md`](acp-harnesses.md) §5.2)、それ以外は読み込んだまま。
  ユーザーの全体ルール (例: 「planner エージェントを使え」) がサブエージェントの動きを変えうる。
  `settingSources` を `["project", "local"]` に絞るかはユーザーと決める。
  **(未決。Stage 3a 時点でも現状の挙動 = 読み込んだまま を維持。PLAN.md の未決事項に記載)**
