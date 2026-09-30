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
 └─ Chat (作業ツリー 1 つに紐づく。オーケストレータの ACP セッション 1 つ) — Stage 8 で追加、8e で作業ツリーに、§2.0
     └─ Group (作業ツリーごとに open は最大 1)
         └─ Task (kind, depends_on, instruction, steps)
             └─ Step (implement / review / checkpoint / done)
```

### 2.0 Chat (Stage 8、ユーザー決定済み)

ユーザーがオーケストレータと話す単位。**1 つのチャットは 1 つの作業ツリー (worktree) に紐づく** (Stage 8e で「ブランチ」から変更、ユーザー決定)。
Orca と同じく**作業ツリーが仕事の単位**で、ブランチ名は表示のために読むだけ。作業ツリーには複数のチャットを持てる。
チャットは過去のものも残り、どれでも送信すれば再開できる。

**Stage 8e の理由** (ユーザー判断): 8d はブランチの改名を reflog で検出して紐づけを書き換え、checkout で cwd を取り直していたが、守りすぎだった。
ブランチ名は表示用であり、コマンドの結果に即座に反応して紐づけを書き換えるのは安全でない。確認は**マージの直前に 1 回**だけ行い、想定外のことは
**オーケストレータに返す** (§6)。

| 属性 | 内容 |
|---|---|
| `id` | Yhtye が採番 (`C-<連番>`、プロジェクトごと) |
| `worktree` | 作業ツリーの絶対パス (`git worktree list` の表記)。作成後は変わらない。ブランチは持たない (表示は作業ツリーの今の HEAD を読む、§6.1) |
| `title` | 最初のユーザーメッセージの先頭を切り詰めたもの (最初の送信まで空。表示は「新しいチャット」) |
| `created_ms` / `last_used_ms` | 作成時刻 / 最後にプロンプトを送った時刻 (イベントの時刻から導く。状態機械には入れない) |
| ACP セッション | セッションキー `orchestrator:<chatId>` (`agent_sessions` の 1 行)。チャットが持つのは ID ではなくこのキーの規約だけ |

- 「ブランチ X の新しいチャット」は X の作業ツリーを §6.1 の規則で解決し (既存のチェックアウト、無ければ Yhtye が作る)、**そのパス**に紐づける。
  作業ツリーを直接指定しても作れる (サイドバーの作業ツリーごとの「+ 新しいチャット」)。
- オーケストレータの cwd は**常にチャットの作業ツリー** (動かないので取り直しは無い)。グループのマージもこの作業ツリーで行う。
  Yhtye はどの作業ツリーでもブランチを切り替えない (checkout しない)。
- このチャットで作るグループの `base_branch` (マージ先) は、**`create_group` の時点の作業ツリーの HEAD のブランチ** (§2.1)。
- 作業ツリーのディレクトリが消えたチャットは、履歴は読めるが送信は拒否する (`invalid_state`、「作業ツリー … が見つかりません」)。
- 同じプロジェクトで複数のチャットのオーケストレータが同時に動ける (それぞれ別プロセス)。
- 受信箱・グループ・イベント (ユーザー / ツール / 通知) はチャットに属する (§5)。
  サブエージェントのセッションは従来どおりタスクに属し (キー = タスク ID から導く)、チャットには属さない。
- チャットの削除・ブランチの削除・作業ツリーの後片付けは扱わない (Stage 8 の範囲外)。

### 2.1 Group

| 属性 | 内容 |
|---|---|
| `id` | Yhtye が採番 (`G-<連番>`) |
| `title` / `summary` | オーケストレータが `create_group` で与える |
| `chat` | このグループを作ったチャット (`C-<n>`)。グループ関連の通知はこのチャットのオーケストレータに届く |
| `base_branch` | マージ先。`create_group` の時点でチャットの作業ツリーがチェックアウトしていたブランチを記録する (Stage 8e。detached HEAD なら `create_group` は `invalid_state`)。group / タスクブランチの作成と表示に使う。`finish_group` の `into` でだけ変わる (§6) |
| `group_branch` | `yhtye/<groupId>` — `base_branch` の HEAD から作成 |
| `status` | `active` → `finishing` → `done` / `cancelled` / `merge_blocked` |

- **active なグループは作業ツリーごとに最大 1 つ** (Stage 8 で「プロジェクトごと」から「ブランチごと」、Stage 8e で「作業ツリーごと」に変更。
  別の作業ツリーのチャットは並行してグループを持てる)。データモデル上は複数持てるが、
  `create_group` 時 (と `merge_blocked` からマージをやり直すとき) のチェックでのみ制約する。同じ作業ツリーの別チャットが持つグループも数える
  (`conflict`、エラー文でそのチャットを知らせる)。マージ中 (`finishing`) も数える。`merge_blocked` は数えない (ユーザーが整えるまで次の依頼を
  止めないため。Stage 3a で明確化)。
- 全タスクが終端 (`done` / `cancelled`) になると Yhtye はオーケストレータを起こす。
  オーケストレータはタスクを追加するか、`finish_group` を呼ぶ。
- **(Stage 7a) オーケストレータが `finish_group` を忘れた場合。** 実 Haiku が `group_settled` を受けて
  ユーザーに結果を報告しただけでターンを終え、グループが `active` のまま残った (UI では「進行中」、
  完了タスクに「送信保留」が出続けた。ユーザー報告のバグ)。サブエージェントの催促 (§7) と同じ考え方で、
  オーケストレータのターンが `end_turn` で終わった時点で受信箱が空・次のプロンプトも無く、
  **全タスクが落ち着いた (終端 or 開始不能) active グループ**があれば:
  1. 1 回目: `group_settled` を `reminder=1` 付きで再送 (本文 =「`finish_group` を呼ぶか、タスク追加 /
     `cancel_task` / `cancel_group`。呼ばなければ Yhtye が完了させる」)。回数はグループの `finish_nudges`
     (イベント `group_finish_reminded`)。
  2. それでも終わらなければ (`finish_nudges` ≥ 1) Yhtye が `finish_group` を代行する
     (`finish_summary` = 「Yhtye が完了させた」旨、結果は `merge_result` で知らせる)。
     開始不能タスクが残るグループは `finish_group` が拒否するため代行せず、`active` のまま
     (UI は「完了待ち」と表示)。
  これは「オーケストレータがタスク追加か `finish_group` を選ぶ」という決定を変えるものではなく、
  選ばずにターンを終えたときの安全網である (報告: Stage 7a 結果メモ)。
- `finish_group` で Yhtye は `group_branch` を `base_branch` に自動マージする (§6。直前に作業ツリーのブランチを確かめ、食い違いはオーケストレータに返す)。

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
| `code` | タスク専用の git worktree + ブランチ `yhtye/<groupId>-<taskId>` (group ブランチから分岐。Stage 3c で `/` 区切りから変更、§6) | 可 | 未コミット変更を Yhtye がコミット → group ブランチへマージ |
| `investigate` | group の統合 worktree (group ブランチ。共有、§6) | 不可 (プロンプトで禁止し、終了時に `git status` が汚れていたら変更を stash に退避して `help`) | マージなし。結果テキストのみ |

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

オーケストレータは**チャットごとに 1 つ**の ACP セッション (Stage 8)。Yhtye は「起こす理由」を**チャットごとの受信箱**に積み、
そのチャットのオーケストレータがアイドル (直前の `session/prompt` が返った状態) のときに
溜まったものを 1 つの `session/prompt` にまとめて送る。

- 受信箱の種類: `user_message` / `checkpoint_reached` / `help_raised` /
  `instruction_needed` / `group_settled` / `merge_result` / `restarted` (Stage 3b で追加、§10)。
- 各項目は機械可読な見出し + 本文の決まった書式のテキストにする
  (例: `[yhtye:help_raised] help_id=H-3 task=T-110 kind=blocked\n<本文>`)。
- ターン中に新しい項目が来ても、そのターンを中断しない。ユーザーが明示的に
  キャンセルした場合だけ `session/cancel` を送る (中断は §9 のとおり並行に届く設計)。
- 未配達の受信箱は SQLite に保存され、再起動後も失われない。
- **宛先チャット** (Stage 8): `user_message` = 送ったチャット / グループに関わる項目 (`checkpoint_reached` /
  `help_raised` / `instruction_needed` / `group_settled` / `merge_result`) = そのグループを作ったチャット /
  `restarted` = 復元できなかったチャット。ユーザーが UI からタスク・グループを中止したときの `user_message` も
  そのグループのチャット。各項目は `chat` を持つ (`InboxEntry.chat`)。
- **遅延起動** (Stage 8): チャットのオーケストレータのプロセスは、そのチャットに**最初に送るもの**
  (ユーザーの送信、または受信箱の項目) があって初めて起動する。チャットの作成・選択・表示では起動しない。
- **自動再起動**: オーケストレータのプロセスが終了した (異常終了・Yhtye 以外による kill を含む) 後は、
  そのチャットへの**次の送信または受信箱の項目**で自動的に起動し直す (`session/load` で復元 → 失敗なら
  新しいセッション + 状態の要約、§10)。終了しただけでは起動し直さない (無駄な起動と起動失敗の繰り返しを避ける)。
  起動に失敗したら `session_failed` を出し、受信箱はそのまま残す。再試行は次の送信・項目・アプリ起動のときだけ。
- ターンが途中で切れた (プロセスがターン中に終了した) チャットを起動し直すときは `restarted` を積む (§10)。

## 6. git

- **group ブランチ** `yhtye/<groupId>`: `create_group` 時に `base_branch` (= チャットの作業対象ブランチ) の HEAD から作る。
  Yhtye 管理の**統合 worktree** (`<data_dir>/worktrees/<projectId>/<groupId>/_group`) に
  チェックアウトする。タスクブランチのマージはここで行う。
- **タスクブランチ** `yhtye/<groupId>-<taskId>` (例 `yhtye/G-1-T-2`): `code` タスクの開始時に
  group ブランチの現在の HEAD から作り、専用 worktree (`.../<groupId>/<taskId>`) にチェックアウトする。
  **Stage 3c で変更**: 当初の `yhtye/<groupId>/<taskId>` は git では作れない
  (`refs/heads/yhtye/G-1` があると `refs/heads/yhtye/G-1/T-1` を作れない — ref はファイルなので
  同名のディレクトリを持てない。git 2.55 で確認)。group ブランチ名 `yhtye/<groupId>` は合意どおり。
- worktree はリポジトリの外 (Yhtye のデータディレクトリ) に置き、ユーザーの作業ツリーを汚さない。
- **タスク完了時**: 未コミット変更があれば Yhtye がコミット (メッセージはタスク ID + 最終報告の要約)
  → 統合 worktree で `git merge --no-ff <taskBranch>` → 成功で worktree を削除 (ブランチは残す)。
- **グループ完了時** (`finish_group`、Stage 8e): **チャットの作業ツリー**で、マージの直前に 1 回だけ確認する。
  1. 作業ツリーのディレクトリがあり、**今チェックアウトしているブランチがグループの `base_branch` と同じ**か。
     違う (改名された・別のブランチに切り替わった・detached HEAD・base が消えた・作業ツリーが無い) なら**マージしない**。
     グループは `merge_blocked` になり、**状況をオーケストレータに返す** (「base は X でしたが、作業ツリーは今 Y です / detached です」と対処の案内。ユーザーにではない)。
  2. clean (従来の規則、下記) なら `git merge --no-ff <groupBranch>`。dirty / 中断された操作 / コンフリクトも `merge_blocked`。
  - **返し方**: オーケストレータ自身の `finish_group` なら**ツールの応答** (`merge.ok = false` と説明。同じターンで対処できる)。
    それ以外 (Yhtye の代行、ユーザーの再試行) で走ったマージは受信箱の `merge_result` (従来どおり)。
  - **オーケストレータの対処**: 作業ツリーを直して (全ツールを持つ、§8.1) `finish_group` をもう一度呼ぶ (`merge_blocked` のグループにも使える)、
    または今のブランチを受け入れて **`finish_group` の `into: "<branch>"`** で呼ぶ。`into` はその時点の作業ツリーのブランチと一致しなければならず
    (違えば `invalid_argument`)、グループの `base_branch` を `into` に変えて (`group_base_changed`) からマージする。
  - Yhtye の代行 (§2.1) とユーザーの `RetryGroupMerge` も同じ確認をする。ユーザーが整えた後に UI から再試行もできる。
- **コンフリクト**は `git merge --abort` で巻き戻したうえで `help` (`merge_conflict`) にする。
  オーケストレータは例えば `modify_steps` で「group ブランチを自分のブランチへマージして
  解消せよ」という `implement` を足し、`answer_help(resume)` する。工程が終わると再び `done` →
  マージが再試行される。
- git 操作は `git` CLI をサブプロセスで呼ぶ (worktree / merge の挙動を git 本体と一致させるため)。

Stage 3c で決めた細部 (実装は [`core-design.md`](core-design.md) §7):

- **コミットの方針**: サブエージェントはコミットしなくてよい (プロンプトでそう伝える)。
  タスク完了時に Yhtye が `git add -A` + コミット (未追跡ファイルも含む。`.gitignore` 対象は除く)。
  エージェントが自分でコミットしていても構わない。コンフリクト解消の途中 (MERGE_HEAD あり) なら
  このコミットがマージコミットになる。コンフリクトマーカーが残るファイルがあればコミットせず
  `merge_conflict` の help にする (マーカーが消えていれば `git add` されていなくても解消済みとみなす)。
- **Yhtye 内部のコミット・マージ** (タスクブランチへのコミット、group ブランチへのマージ) は
  フックと署名を飛ばす (`--no-verify`、`commit.gpgSign=false`)。ユーザーのフックや pinentry で
  止まらないため。**base ブランチへのマージ**はユーザーのブランチなのでフック・署名はリポジトリの
  設定どおり。コミットの作者はリポジトリの `user.name` / `user.email`、無ければ `Yhtye <yhtye@localhost>`。
- **base ブランチの「clean」**: 追跡ファイルに未コミットの変更 (ステージ済み含む) が無く、
  マージ / リベース等の途中でないこと。**未追跡ファイルは妨げない** (git 自身がマージで上書きする
  場合は拒否し、それは `merge_blocked` になる)。base とのコンフリクトも `git merge --abort` で
  戻して `merge_blocked` (ツリーは元どおり。事前に clean を確認しているので abort で失われる変更は無い)。
  ユーザーの作業ツリーを Yhtye が変えるのは成功したマージだけ。
- **investigate が統合 worktree を汚した場合**: 変更を `git stash push --include-untracked` で
  退避して (消さない) `dirty_readonly_tree` の help。resume で完了できる (stash は残る)。
- **中断された rebase / cherry-pick / revert**: タスク worktree・統合 worktree でこれらが途中なら
  コミットもマージもせず `git_failed` の help (その作業を黙って捨てないため。マージ途中だけは上記の
  とおりコミットで完了させる)。
- **中止したタスク**: worktree を削除する前に未コミットの変更をタスクブランチに WIP コミットして残す。
- **中止したグループ** (Stage 5): 各タスクの後片付けの後で統合 worktree `_group` も削除する
  (中断マージは abort、残った変更は group ブランチに WIP コミット)。ブランチは残す。
- **マージの再試行** (Stage 5): `merge_blocked` のグループはユーザーが UI から `RetryGroupMerge` で
  `finishing` に戻して base へのマージをやり直せる (他に active / finishing のグループがあれば `conflict`)。
  結果はオーケストレータに `merge_result` で知らせる。
- **後片付け**: タスクのマージ成功で task worktree、base へのマージ成功で統合 worktree を削除する。
  ブランチはすべて残す。
- **冪等性** (再起動で git 操作がやり直されるため): 既存の worktree が正しいブランチなら再利用、
  消えた worktree ディレクトリは作り直す、group ブランチが無ければ base から作り直す、
  既にマージ済み (祖先) のタスク / group ブランチは「マージ済み」として成功、統合 worktree に
  中断されたマージが残っていれば abort してからやり直す。`create_group` で**既存の** group
  ブランチが base と別のコミットを指している場合は採用せず失敗 (前回の DB の残骸などを黙って使わない)。
  メイン作業ツリーに中断されたマージが残っている場合は自動で abort せず `merge_blocked`。
- **DB と worktree の場所**: どちらも Yhtye のデータディレクトリ (DB は `yhtye.sqlite3`、
  worktree は `worktrees/<projectId>/...`)。プロジェクトのリポジトリには何も書かないので
  `.git/info/exclude` 等の設定は不要。
- **レビュアーへの差分の渡し方**: review Step のプロンプトにタスクブランチと group ブランチの名前、
  `git status --short` と `git diff $(git merge-base <groupBranch> HEAD)` (コミット済み + 未コミット)
  を明記する。レビュアーはタスクの worktree で動く。
- **再起動のフォールバック**: 新しいセッションで Step をやり直すとき、プロンプトに worktree の
  `git status --short` と分岐点からの `git diff` (最大 16 KiB) を添える (§10)。

### 6.1 チャットの作業ツリー (Stage 8、8e で作業ツリーへの紐づけに変更。ユーザー決定済み)

[Orca](https://github.com/stablyai/orca) 流: **作業ツリーが仕事の単位**。元のクローン (メイン作業ツリー) も
作業ツリーの 1 つとして扱い、**Yhtye はどの作業ツリーでもブランチを切り替えない**。
チャットは**作業ツリーのパス**に紐づき (§2.0)、オーケストレータの cwd とグループのマージ場所はそのパスで**固定**。

- **チャットの作成**: (a) 作業ツリーを指定 (`git worktree list` にある、存在するもの。`yhtye/*` をチェックアウト中の Yhtye 内部の作業ツリーは不可)、
  または (b) ブランチを指定 → `git worktree list --porcelain` から次の順に作業ツリーを決める (`resolve_branch_worktree(branch)`):
  1. そのブランチがチェックアウトされている作業ツリーがあれば**それを使う** (メイン作業ツリーでも、
     ユーザーが作った Yhtye 管理外の作業ツリーでも)。ディレクトリが消えている Yhtye 管理の登録 (prunable) は外してから 2 へ。
  2. 無ければ Yhtye 管理の作業ツリーを作る:
     `git worktree add <data_dir>/worktrees/<projectId>/branches/<sanitized-branch> <branch>`。
     `<sanitized-branch>` は `/` などディレクトリ名に向かない文字を `-` にしたもの。別のブランチが同名のディレクトリを
     使っていれば `-2`, `-3` … を付ける。
  解決は**チャットの作成時の 1 回だけ** (Stage 8e。以前は起動・マージのたびに解決していた)。
- **ブランチ名は表示だけ** (Stage 8e): サイドバー・会話ヘッダ・ブランチピルは、表示のたびに作業ツリーの今の HEAD を読む
  (`GetGitOverview.worktrees`)。改名 (`git branch -m`)・checkout・detached はそのまま表示に出る。Yhtye はこれらに反応して
  チャットやグループを書き換えない。
- **確認はグループのマージの直前に 1 回** (§6): 作業ツリーのブランチがグループの `base_branch` と違えばマージせず、オーケストレータに返す。
  `create_group` はその時点の HEAD のブランチを `base_branch` に記録する (detached なら拒否)。
- セッションを起動した cwd は `agent_sessions.cwd` に記録する。作業ツリーは動かないので、復元時の「cwd が記録と違う」は通常起きない
  (OpenCode は `session/load` に同じ cwd を要求するため、比較は残す。§10 のフォールバック)。
- **管理外の作業ツリー**も使う (マージは「ブランチが一致し clean」を満たすときだけ行う。ユーザーが作業中なら `merge_blocked`。
  オーケストレータが未コミットの変更を残したときも同じ、§8.1)。
- **メイン作業ツリーが detached HEAD** でもチャットは作れ、会話できる。`create_group` だけが拒否される (「ブランチをチェックアウトしてから」)。
- **作業ツリーのディレクトリが消えた**チャットは残り履歴を読めるが、送信は `invalid_state` で拒否する。Yhtye は作業ツリーを作り直さない。
- Yhtye が作る作業ツリーは**チェックアウトしたまま残す** (後片付けは範囲外)。新しいブランチの作成 (`CreateBranch`) も
  `git worktree add -b <name> <path> <start>` でこの場所に作り、メイン作業ツリーには触れない。
- **Stage 8d からの撤去** (Stage 8e): ブランチ改名の追跡 (reflog の `Branch: renamed …` を見て `chat_branch_changed` で紐づけを書き換える)と、
  動いているオーケストレータの cwd の取り直し (checkout されたら止めて別の作業ツリーで起動し直す) は**無くなった**。
  コマンドの結果に即座に反応して紐づけを変えるのは安全でない、というユーザー判断による。

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
  - **orchestrator**: ユーザーとだけ話す。実装や検証が要る作業は自分でせず、グループ / タスクを MCP ツールで組む
    (検証のいらない小さな変更は自分でしてすぐコミットしてよい。§8.1)。
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

### 8.0 役割ごとのハーネス × モデル × effort (Stage 7b で決定、7d で effort と用途メモ)

- 設定の役割は 4 つ: **orchestrator** / **implementer** (`code` タスク) / **investigator** (`investigate` タスク) /
  **reviewer**。それぞれに候補の行 (ハーネス × モデル × effort (任意) + 用途メモ) と既定の行を持つ。同じハーネス × モデルを
  effort 違いの別の行として置ける。全体の既定値の上にプロジェクトの既定値を役割単位で重ねる。
  UI は左端の細いバーの下の設定モーダルから ([`core-design.md`](core-design.md) §15)。
- オーケストレータは各行の**用途メモ** (「難しい設計の変更に」など) を読んで、タスクごとに「この用途ならこの行」と選ぶ。
  行は `create_task` の `harness` / `model` / `effort` で**完全一致**させる (effort は 1 行しかないハーネス × モデルなら省略可)。
- 変更は**新しく起動するセッション**から効く。動いているセッションはそのまま、`session/load` で復元するセッションも
  記録した組のまま。
- オーケストレータは `create_task` の任意引数でタスクごとに上書きできるが、その役割の**候補集合の中だけ**
  (候補外はツールエラー)。普段は省略して既定を使う。review Step は `review_harness` / `review_model` / `review_effort` があればそれ、
  無ければ reviewer の既定 ([`mcp-tools.md`](mcp-tools.md) §3)。

### 8.1 オーケストレータも書ける (Stage 8d で変更、ユーザー決定)

Stage 2 では「オーケストレータは読み取り専用」と決めていた (Claude Code の
`_meta.claudeCode.options.tools = ["Read", "Glob", "Grep"]` で組み込みツールを絞り、プロンプトでも書くなと指示。
OpenCode / Codex は絞れないので設定パネルに「⚠ 書き込み制限なし」を出していた)。**Stage 8d でこれを取りやめた。**

- **全ハーネスのオーケストレータが、組み込みツールをすべて使える** (Claude Code の `tools` の制限を外す。
  `HarnessConfig::claude_code_orchestrator` と `ORCHESTRATOR_BUILTIN_TOOLS`、preset の `orchestrator_read_only`、
  設定パネルの「⚠ 書き込み制限なし」はすべて無くなった)。ハーネスによる違いは無い。
- **理由**: 設定の定数を 1 つ変える、ブランチ名を `git branch -m` で直す、といった**小さな変更のたびに
  サブエージェントを呼ぶのは大げさ**だから。読み取り専用を守るために Claude Code だけ絞っても、OpenCode / Codex では
  元々守れておらず、ハーネスごとに挙動が違うほうが混乱する。
- **新しい MCP ツールは作らない** (ユーザー決定): git や Edit のような**既存の機能を複製するツールは足さない**。
  オーケストレータは自分の組み込みツール (Bash の `git` など) をそのまま使う。
- **オーケストレータ自身がしてよいこと** (プロンプト `orchestrator.md` で指示):
  - **検証のいらない小さな変更**だけを、**自分の cwd (= チャットの作業ツリー)** で直接行い、**すぐコミットする**。
    例: 設定の定数の変更、ブランチ名の変更 (`git branch -m`)。
  - 実装や検証 (ビルド・テスト) が要る変更は、従来どおり**タスク**に渡す。
  - **作業ツリーを clean に保つ** (未コミットの変更を残さない)。グループのマージは作業ツリーが clean であることを要求する (§6)。
    dirty だと `merge_blocked` になる。
  - **グループは、マージの時点で作業ツリーがチェックアウトしているブランチにマージされる**ことを知っておく。改名・checkout は禁止しない
    (Stage 8e) が、`base_branch` と違えばマージの前に Yhtye が止めて知らせる (§6: 直して `finish_group` をもう一度、または `into`)。
- **守らせる仕組みは無い** (プロンプトのみ)。破られたときの被害は `merge_blocked` (オーケストレータかユーザーが整えて再試行) までで、
  ユーザーのブランチにコミットが増えるだけ。cwd の外への書き込みを防ぐ手段は元々無かった。
- 他ハーネスを足すときも同じ扱い (書き込みの制限は前提にしない)。

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
- オーケストレータのセッションも同様に復元する。**Stage 8 で「アプリ起動時に必ず復元」から変更**:
  - 復元する (起動時に開く) のは、**live だった** (セッションが `stopped` 以外 = ターン中だった・Yhtye の終了で
    止めた) チャット、**open なグループ (`active` / `finishing`) を持つ**チャット、**未配達の受信箱を持つ**チャットだけ。
    それ以外のチャットは起動せず、次にそのチャットへ送るとき (または受信箱の項目が届いたとき) に §5 の遅延起動で復元する。
  - **過去のチャットはどれも再開できる**: 送信すると、保存済みの ACP セッション ID で `session/load`。
    失敗した (ハーネスが非対応・セッションが消えた・cwd が変わった) ら新しいセッションを作り、
    受信箱 `restarted` にそのチャットの状態の要約 (そのチャットのグループ・タスク) を入れる (従来の再起動と同じ扱い)。
  - オーケストレータが**ターン中に終了した**チャットを起動し直すときは、復元できても `restarted` に「ターンが途中で切れた」を入れる。

Stage 3b で実装した細部 (合意事項は変えていない。詳細は [`core-design.md`](core-design.md) §6・§8.1):

- 「現在状態テーブル」には groups (`task_groups`) / tasks / task_deps / steps / helps / inbox /
  agent_sessions を置く。イベントログにはドメインイベントに加えて、セッションの起動・停止、
  プロンプト、ツール呼び出し、エージェントの出力 (チャンクはまとめたブロック単位) を保存し、
  UI の履歴に使う。
- アプリの終了 (`shutdown`) ではタスクの状態を変えない。次回の起動で `running` / `merging` を
  `interrupted` にしてから再開する (UI には `interrupted` → `running` の両方が届く)。
- help の返答待ちだったエージェントは再起動でプロセスを失っているので「エージェント喪失」と
  同じ扱い (§7) にする。`answer_help(resume)` で Step をやり直すとき、そのセッションは
  `session/load` で復元される (返答は note として付く)。
- マージ中 (`finishing`) に落ちたグループは `merge_blocked` にして `merge_result` で知らせる
  (再試行はユーザー / オーケストレータが確認してから)。
- オーケストレータが前のセッションを失った、またはターンの途中で止まった場合は、受信箱の
  新しい種類 `restarted` で知らせる ([`mcp-tools.md`](mcp-tools.md) §5)。

## 11. まだ決まっていないこと

- ハーネスの自動選定 (当面は役割ごとに設定で固定)。
- 同じ作業ツリーでの並行グループ (Stage 8 でブランチごと、8e で作業ツリーごとの並行は可能になった。同一作業ツリーは 1 つのまま)。
- `code` 以外で書き込みを伴う kind (例: ドキュメントのみ) を足すか。
- `max_review_rounds` や並行実行数の上限をどこで設定させるか。
- ~~オーケストレータは自分でコードを書かない方針 (§8) が画面設計の筋書き (「オーケストレータが自分で修正を実行中」) とずれる。~~
  → Stage 8d で小さな変更に限って自分で行えるようにした (§8.1)。
- ~~オーケストレータのセッションもバイパス権限で動くと、メイン作業ツリーを直接書き換えられて
  しまう。~~ → Stage 2 で「読み取り専用」と決めたが、**Stage 8d で取りやめた** (§8.1: 全ツールを許し、小さな変更に限ってコミットまで自分でさせる)。
- ~~Yhtye が起動する Claude Code に**ユーザー自身の設定** (`~/.claude` の CLAUDE.md・フック・
  プラグイン・スキル) をどこまで効かせるか。~~ → **Stage 7a でユーザーが決定: 常に有効。**
  全エージェントがユーザーの `~/.claude` の CLAUDE.md・フック・スキルを読み込む (`settingSources` は既定のまま)。
  MCP サーバーだけは従来どおり隔離する (`strictMcpConfig`、[`acp-harnesses.md`](acp-harnesses.md) §5.2)。
  ユーザーの全体ルールがサブエージェントの動きを変えうる (3c ではレビューが 1 往復増えた) ことは承知の上。
