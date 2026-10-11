# 計画中の機能: Android クライアント (Tailscale 経由で PC の Yhtye を操作)

- 日付: 2026-10-11
- 状態: 設計確定。段階 1 (PC 側サーバーと認証 T-55、設定画面 T-56、[`core-design.md`](../architecture/core-design.md) §18) と
  段階 2 (Android の基盤: 雛形 T-58、型生成 T-59、通信・ペアリング・保存・最小画面 T-61) は実装済み。段階 3 (GUI) は画面構成と操作を HTML モックで決めた段階 (下の「段階 3: GUI の決定事項」、T-62 / T-63)。同期層 (T-66)、3 パネルの骨格・会話画面 (T-67)、左右パネルとタスク詳細 (T-69) は実装済み。設定・通知・常駐 (T-71) は未着手
- ブランチ: `feature/android_client`
- 目的: Tailscale でつながった Android 端末から、PC で動いている Yhtye (コア) を操作する。

## 決定事項

| # | 項目 | 決定 | 理由・却下した案 |
|---|---|---|---|
| 1 | クライアントの技術 | **Kotlin + Jetpack Compose** | タッチ・スクロール・ジェスチャ・IME・戻るジェスチャが純正で最も馴染む。PC 側 GUI は GPUI へ移行予定なので React 流用は利点にならない。却下: Tauri 2 (WebView)、Flutter (自前描画で細部がずれる)、React Native / Expo |
| 1a | iOS への備え | UI 以外 (通信・認証・データモデル) は **Kotlin Multiplatform の共通モジュール** (Ktor + kotlinx.serialization) に置く。Android 固有 (Keystore・カメラ・通知) は差し替え可能な境界の向こう | 今は iOS 予定なし。将来は UI (Compose Multiplatform も可) と端末固有部分を足すだけにする |
| 2 | 認証 | **ペアリングコード + 端末ごとのトークン** | 却下: Tailscale の端末識別のみ (Android では Tailscale が VPN なので、同じ端末の他アプリも PC のポートに届く)、鍵ペア + 署名 (重い。枠組みは共通なので後から強化できる) |
| 2a | 多重防御 | 待ち受けは **Tailscale のアドレスのみ**。接続元を `tailscale whois` で照合し、**PC と同じ Tailscale ユーザーの端末だけ**許可 | |
| 2b | 通信路の暗号 | **`ws://` のまま** (TLS・独自 E2EE なし) | Tailscale (WireGuard) が暗号化と相手の保証をする。Orca の NaCl E2EE は自社リレー経由の接続のため |
| 3 | サーバーの所在 | **デスクトップアプリに組み込み**。設定で「リモート接続」をオンにした時だけ待ち受け、アプリが動いている間だけ使える | データ (DB・ワークツリー) を扱えるプロセスは 1 つだけ。却下: 常駐デーモン化 (GUI とコアの間がすべて通信になる大改造。やるなら GPUI 移行と一緒に)。GUI 無し起動モード (`--headless`) は必要になったら後付け |
| 4 | ペアリングの手順 | **QR + 手入力の予備** | 下記「ペアリング」 |

## 参考: Orca (stablyai/orca 1.4.x) との違い

| | Orca | Yhtye |
|---|---|---|
| クライアント | Expo (React Native) | Kotlin + Compose |
| 通信 | `ws://` :6768、tweetnacl box で E2EE。自社リレー経由の接続もある | `ws://`、Tailscale のみ。E2EE なし |
| Tailscale | QR に載せるアドレスとして 100.64.0.0/10 を優先するだけ | 待ち受けを Tailscale アドレスに限定 + `whois` で照合 |
| 認証 | QR に deviceToken + デスクトップの公開鍵 | QR に一度きりのコード → 端末トークンを発行 |
| 失効 | 端末一覧から revoke、接続も切る | 同じ |

## PC 側 (サーバー)

- 既存の `yhtye-dev-bridge` の WS 処理 (要求 `{id, cmd}` → 応答 `{id, ok|err}`、イベント `{event}` のプッシュ。
  [`core-design.md`](../architecture/core-design.md) §10) を共有ライブラリに切り出し、
  開発用ブリッジとデスクトップアプリの両方から使う。認証部分だけ差し替えられるようにする。
- 待ち受け: `tailscale ip` の 100.x (と必要なら fd7a:…) アドレス、既定ポート **1423** (設定で変更可)。
  Tailscale が動いていなければ起動せず、理由を設定画面に出す。
- 接続の認証: `Authorization: Bearer <端末トークン>` を検査 (定数時間比較、DB にはハッシュだけ)。
  さらに接続元 IP を `tailscale whois` で照合し、PC 自身と同じユーザーでなければ拒否。
- 端末の保存: コアの DB に端末表 (id・名前・トークンのハッシュ・作成日時・最終接続日時)。
  失効したら行を消し、その端末の接続を即座に切る。
- サーバー ID: 初回にランダムに作って DB に保存。QR に載せ、スマホは接続先の取り違えを検出する。
- スマホに許す操作: コアの API (`ApiCommand`) をすべて許可。ただし **リモート接続の設定・ペアリングの発行・
  端末の失効は PC の GUI からだけ** (スマホから新しい端末を増やしたり、自分を消したりはできない)。
- 再接続: スマホ側は切断後に指数バックオフで再接続し、状態を取り直す (イベントの取りこぼしは再取得で埋める)。

### 実装で決めたこと・変えたこと (T-55)

- WS 処理の共有ライブラリは別 crate ではなく `yhtye-core` の `ws` モジュール (リモートサーバーの開始/停止を `ApiCommand` から
  操作するので、Core と同じ crate にないと循環依存になる)。開発用ブリッジもそれを使う。
- 端末トークンは 256 bit を hex 64 文字で渡す。サーバー ID は 128 bit (hex 32 文字)、端末 ID は `dev-` + hex 16 文字。
- whois は**ペアリングコードを見る前**に行う (他ユーザーの端末にコードの試行回数を使わせない)。whois に失敗した接続も 403。
- HTTP の応答: `/pair` は 200 / 400 (JSON・端末名) / 401 (コードが違う・無い) / 403 (whois)、`/ws` は 401 (トークン) / 403 (whois)。
  エラーの本文は `{"error": "..."}`。失効やサーバー停止で WS を閉じるときは close コード 1001。スマホは再接続で 401 なら
  「この PC から外された」とみなす (失効専用の close コードは設けていない)。
- 待ち受けは IPv4 (100.x) が必須、IPv6 (fd7a:…) は bind できれば併用。QR の `ip` は IPv4。MagicDNS が無効なら `host` を省く。
- 「リモート接続の設定・ペアリングの発行・端末の失効」に加えて **状態の取得と端末一覧もローカル専用**、
  リモート接続の状態・端末の変化のイベント (`remote_status_changed` / `remote_devices_changed`) もリモートには流さない。
- Tailscale のアドレスやログイン状態が起動後に変わっても追従しない (再起動か設定のオン/オフで取り直す)。

### 実装で決めたこと (T-58: Android の雛形)

- Gradle プロジェクトは `mobile/` (`shared` = KMP、`androidApp` = Compose)。手順は [`DEVELOPMENT.md`](../DEVELOPMENT.md)。
- applicationId / namespace はデスクトップと同じ `io.github.tom039224.yhtye` (OS が違うので衝突しない)。
  共通モジュールの namespace とパッケージは `io.github.tom039224.yhtye.shared`。
- AGP 9 では KMP の `androidTarget()` (`com.android.library`) が非推奨のため、`com.android.kotlin.multiplatform.library` を使う。
- バックアップ・端末間転送は無効 (端末トークンは Keystore の鍵で暗号化するので、別端末に戻しても使えない)。

### 実装で決めたこと (T-59: API 型の Kotlin 生成)

- 方式は **schemars の JSON Schema + Rust 内の小さな生成器** (typeshare は内部タグ付き enum を Kotlin に出せない)。
  ts-rs と同じテストの運用 (`pnpm gen:kotlin` で更新、生成物とずれたら `cargo test` が失敗)。詳細は
  [`core-design.md`](../architecture/core-design.md) §2。
- kotlinx.serialization のプラグインが作る多態 (sealed + `classDiscriminator`) は使わず、タグ付き enum は手書きの
  `TaggedSerializer` で読み書きする。知らないタグを `Unrecognized` で受けるため (プラグインの多態は例外になる)、
  隣接タグ (`AgentEvent` の `data`、`AgentOutput` の `update`) とタグ名の違い (`kind` / `code` / `by`) を同じ仕組みで扱うため。
- 前方互換: 知らない応答・イベント・enum 値は `Unrecognized` (生の JSON つき)、増えたフィールドは無視 (`ApiJson`)。
  `Unrecognized` は送れない (エンコードが失敗する)。
- ACP のペイロード (エージェントの出力の中身など) は TS と同じく型を写さず `JsonElement`。画面で要る部分は段階 3 で読む。
- u32 / usize も `Long` にする (`Int` だと u32 の上限を表せない)。
- `/pair` の要求・応答・エラー本文も Rust 側に型 (`remote::{PairRequest, PairResponse, RemoteError}`) を作って生成する。
- 契約テストは Rust が本物の型で書いた全 variant の JSON を Kotlin で読み書きして比べる (180 件)。Kotlin のテストは
  Gradle でだけ走る (CI にはまだ Gradle のジョブが無い。生成物のズレは cargo のテストが CI で検出する)。

### 実装で決めたこと (T-61: 通信・ペアリング・保存・最小画面)

- **共通モジュール** (`shared/commonMain`。Android 依存なし):
  - `pairing/`: `parsePairingUri` / `parseManualEntry` (コードは空白・`-`・小文字を許して正規化、8 文字・PC と同じ文字種でなければ
    その場で誤り。ホスト・ポート・コードとも**全角の英数字・記号は半角にする**: 実機の日本語 IME が `１００．８９…` を入れた)、
    `PairingClient` (`POST /pair`)、`TailscaleHosts`。
  - `credentials/`: `Credentials` (サーバー ID・接続先の候補・ポート・端末 ID・端末名・トークン。`toString` はトークンを伏せる) と
    `CredentialStore` (境界)。
  - `client/`: `YhtyeClient` (WS RPC)、`WsTransport` (接続の境界。テストは偽物)、`KtorWsTransport`。Android の
    `HttpClient` (OkHttp。接続 10 秒、WS の ping 15 秒) は `androidMain` の `createHttpClient()`。
- **接続先は Tailscale のアドレスだけ** (100.64.0.0/10・fd7a:115c:a1e0::/48・`*.ts.net`)。QR・手入力のどちらも読むときに検査する
  (短い MagicDNS 名 `pc` や LAN の IP は受け付けない)。Android の network security config は IP の範囲を書けないので、
  平文 (`http://` / `ws://`) はアプリ全体で許し (`network_security_config.xml`、理由はファイル内)、相手を限るのはこの検査に任せた。
  アプリは PC 以外と通信しない (ML Kit は端末内のモデル)。
- **ペアリング**: 候補を QR の順 (MagicDNS 名 → IP) に 1 つ 10 秒まで試し、**HTTP の応答が返った先で決める** (401 が返ったら
  他の候補は試さない。同じ PC なのでコードの試行回数を無駄にするだけ)。届いた先を候補の先頭にして保存する。
  応答の `server_id` が QR の `sid` と違えば失敗 (`WrongServer`。PC 側には登録されてしまうので案内に書く)。手入力は `sid` が
  無いので照合しない。失敗の種類: コード誤り/期限切れ (401。PC の説明文で区別できるが画面では 1 つの案内)、他ユーザー (403)、
  端末名 (400)、届かない、取り違え、その他。端末名は PC と同じ規則 (制御文字と前後の空白を除いて 1〜64 コードポイント) で先に検査。
- **WS の拒否の理由**: Ktor の OkHttp エンジンはハンドシェイクの失敗で 401 以外の状態コードを例外の文言にしか残さない。
  そこで失敗したら同じ先へ普通の `GET /ws` (同じ `Authorization`) を送り、その状態コードで決める (PC は upgrade の前に
  whois → トークンの順で検査するので同じ答えになる)。応答が無ければ「届かない」として次の候補へ。
- **`YhtyeClient`**:
  - 状態は `Idle` / `Connecting` / `Connected(session)` / `WaitingToReconnect(attempt, delayMs, reason)` / `Revoked` (401) /
    `Rejected(message)` (403)。`Connected` の `session` は接続のたびに増え、利用側はその変化で状態を取り直す (ホームは
    プロジェクト一覧を取り直す)。
  - 再接続の待ちは 1 秒から倍々で 30 秒まで (ジッタなし。相手は 1 台の PC)。つながったら 0 に戻し、切れた後は 1 秒から。
    `start()` は待ち中なら待たずにすぐ試す (アプリが前面に戻ったとき・「今すぐ再接続」)。
  - 401 では再接続をやめる (`Revoked`。`start()` も効かない)。403 は止まるが `start()` でやり直せる (whois の一時的な失敗も 403 なので)。
  - `call(cmd, timeoutMs = 30 秒)`: つながっていなければ時間内につながるのを待つ。失敗は `ApiException` (PC のエラー)、
    `RpcTimeoutException` (送ったが応答なし)、`ConnectionLostException` (応答前に切れた。**再送しない**: PC では実行されたかもしれない)、
    `NotConnectedException` (時間内につながらない・失効・拒否)。知らない形の応答・壊れたメッセージは捨てる。
  - イベントは `SharedFlow` (1024 件。あふれたら古いものから捨てる。取りこぼしは `seq` の飛びで分かる、§10 の同期規則)。
  - `start` / `stop` / `close` は 1 本のコルーチンで順に処理し (どのスレッドから呼んでもよい)、状態と接続と応答待ちは Mutex で守る。
- **Android**:
  - `KeystoreCredentialStore`: Preferences DataStore に保存し、トークンだけ Keystore の AES-256-GCM 鍵 (StrongBox があればそこ、
    取り出し不可、認証不要 = 裏での再接続にも使える) で暗号化。サーバー ID と端末 ID を認証付きデータにする。復号できなければ
    保存内容を捨てて未ペアリングに戻る。解除のときは鍵も消す。
  - アプリが前面にある間だけつなぐ (`ProcessLifecycleOwner`。回転では切れない)。裏に回ったら切り、前面に戻ったらつなぎ直して
    取り直す (通知などで裏でもつなぐかは段階 3 で決める)。
  - 失効 (401) を検出したら端末内の認証情報を消し、ペアリングの始めの画面に案内を出す。
  - 画面は Compose の状態だけで切り替える (ナビゲーションのライブラリは使わない): 始め → QR / 手入力 → 端末名 (初期値は
    `Build.MODEL`) → ホーム (接続状態・プロジェクト一覧 (引っ張って更新)・メニューの「ペアリング解除」)。
  - QR は CameraX の `LifecycleCameraController` + `MlKitAnalyzer` + ML Kit のバーコード (端末内モデルの `barcode-scanning`)。
    カメラを拒否されたら、設定を開くボタンと手入力を出す。`yhtye://pair` 以外の QR は「Yhtye 用ではない」と出して読み続ける。
  - ディープリンク `yhtye://pair?...` (`singleTop`) はペアリング画面の端末名の確認に直接進む。ペアリング済みなら
    「解除して新しくペアリングするか」を聞く。
- PC の example サーバー (`remote_server`) に `open <path>` (プロジェクトを開く) を足した (結合テストと実機確認用)。

### 実装で決めたこと (T-66: 同期層)

PC の GUI と同じ方式でコアの状態を写し取る同期層。画面 (3 パネル) は次のタスク。

- **置き場所**: `shared/commonMain/.../shared/sync/` (Android 依存なし)。PC の `src/store/*.ts` と、表示用の導出のうちロジックだけのもの
  (`src/ui/taskInfo.ts` / `branchTree.ts` / `projectActivity.ts` / `labels.ts`) をファイル単位で移した:
  `Domain.kt` (domain.ts)、`Sessions.kt` (sessions.ts)、`Chats.kt` (chats.ts)、`Transcript.kt` (transcript.ts)、`Acp.kt` (api/acp.ts)、
  `ProjectView.kt` (project.ts)、`EventRules.kt` (eventRules.ts + activity.ts + projectActivity.ts)、`AppStore.kt` (app.ts)、
  `SettingsCommands.kt` (settingsCommands.ts)、`ClientPrefs.kt` (prefs.ts + drafts.ts)、`TaskInfo.kt` / `BranchTree.kt` / `Labels.kt` (ui の同名)、
  `UsersTurn.kt` (新規: ユーザーの番の検知)。関数名・規則・文言は PC と同じ (`deleteBlocker` などの日本語も同じ文)。
- **`AppStore`** (PC と同名): `state: StateFlow<AppState>` (不変の data class。`ProjectView` は PC と同じ範囲: スナップショット + 畳み込んだ状態、チャット、
  選択中のチャット、未読、起動中、セッション、会話、ストリーミング途中の本文、文脈の使用量、圧縮中、履歴の読み込み位置)。同期の手順は PC と同じ
  (スナップショット → 直近 400 件の履歴 → 溜めたイベント、`seq` の飛びと未知の durable の後の live で `list_events`、`cursor` 以下は捨てる、
  再接続・`resync` で追いつき、`projects_changed` → `list_projects`、`settings_changed` → `settings.changes`、git の読み直しの契機)。
  操作は楽観的に状態を変えずイベントを待つ (PC と同じく、作ったチャットの一覧への追加と改名・削除の応答はすぐ反映)。
  依存する要求は `suspend` で応答を待ってから次を送る (例: `create_chat` → git の読み直し → 選択 → `open_chat`)。
- **コアへの窓口** `client.CoreConnection` (状態・イベント・`call`) を足し、`YhtyeClient` が実装する。同期層のテストは偽のコア (PC の `FakeCore` の移植) で動かす。
  要求と違う種類の応答は `UnexpectedReplyException` (`RpcException` の仲間)。
- **並行性**: 状態の変更は呼び出し側のディスパッチャを `limitedParallelism(1)` にした上で 1 本ずつ (JS のイベントループと同じく、中断点の間で
  交互に進む)。公開の `suspend` 関数はどのスレッドから呼んでもよい。イベントの購読・接続状態の監視・タイマー (git 400 ms、動作中 200 ms、
  使用量 5 分、ping 3 秒) は同じスコープのコルーチン。
- **アクティビティから独立**: `Session` (Application が持つ、プロセスに 1 つ) がペアリングごとに `YhtyeClient` と `AppStore` を作り
  (`Session.Phase.Paired.sync`)、解除・失効で閉じる。画面が無くても接続さえ保てば同期層は動き続ける (裏での常駐は後続タスク。今は T-61 のまま
  裏に回ると接続を止める)。
- **PC と変えたところ**:
  - 再接続は `ConnectionState.Connected.session` の変化で判断する (StateFlow は途中の切断を飛ばし得るので、番号が変われば切断を見ていなくても
    再接続として扱いストリーミングを捨てる)。close 1013 (コアの送信が追いつかない) は `YhtyeClient` がほかの切断と同じく再接続する。
  - 使用量と ping は**前面にある間だけ** (`setForeground`。裏で電池を使わない)。前面に戻ったら git を読み直す (PC のウィンドウのフォーカスに当たる)。
  - 下書きは端末の設定に残す (`PrefsDrafts`。PC はメモリだけ。Android は裏でプロセスが殺されるため)。最後のプロジェクト (`yhtye.lastProject`)・
    プロジェクトごとのチャット (`yhtye.chat.<id>`) は PC と同じ鍵。保存の境界は `ClientPrefs` で、Android は SharedPreferences (`yhtye.client`、
    秘密は無いので暗号化しない) の `SharedPreferencesPrefs`。ペアリングの解除・失効で全部消す (別の PC のものを残さない)。未読は PC と同じくメモリだけ。
  - `AppState.projectsLoaded` (一覧を一度でも読めたか: 読む前の空とプロジェクトが無い空を分ける) を足した。
  - 失敗の扱い: PC の `run` に当たる操作は `AppState.errors` (5 件まで、`<コマンドの type> failed: <コアの message>`) に積む。PC で reject する操作
    (`createBranch` / `renameChat` / `deleteChat` / 設定) は `RpcException` (コアのエラーは `ApiException`) を投げる。
  - 設定 (`AppStore.settings`): `setAgentSettings(project, role, settings, expectedRevision)` は `Saved` か、`conflict` なら今の設定を読み直して
    `Conflict(latest, message)` を返す (画面は PC と同じ文を出して `latest` を表示する)。リモート接続の設定は PC 専用なので無い。
  - 時間の掛かる要求 (`open_project` / `create_chat` / `create_branch` / `retry_group_merge`) の待ち時間は 180 秒 (PC の WS と同じ)。他は `YhtyeClient` の既定 30 秒。
- **ユーザーの番の検知** (`AppStore.usersTurns: SharedFlow<UsersTurn>`。通知は後続タスク): オーケストレータのターンが正常に終わり (`turn_ended` が
  `end_turn`。中止・エラー・ユーザーが頼んだ `/compact` のターンは除く)、そのチャットの受信箱が空で、走っているグループ (`active` / `finishing`) が無く、
  次のターンが始まっていない (`isUsersTurn`)。`merge_blocked` は含む。表示中のプロジェクトは畳み込んだビューで、スナップショットより後のイベント
  だけ (履歴の読み込みでは出さない。再接続の追いつきで見つけた切断中のターンの終わりは 1 回出す)。表示していないプロジェクトは生のイベントから
  最後の発話と `/compact` だけを覚え、正常なターンの終わりで `get_snapshot` して同じ条件を確かめる (取りこぼしたイベントは追わない)。
  `UsersTurn` はプロジェクト・チャット・題・そのターンの最後の発話 (通知はその 1 行目、`firstLine`)・時刻。表示中かどうかの判断は受け取る側。
- **最小画面**: ホームのプロジェクト一覧は同期層の `state.projects` から出し、PC と同じ規則のリング (`projectRing`: 動作中なら回る緑、そうでなければ
  中を抜いた緑) を付けた。つながったら最後に開いていたプロジェクトを開く (PC の `reopenLast`)。
- **example サーバー**: `remote_server` に `--fake-agent` と各役割のシナリオの引数を足した (core-design §18.6)。
- **確認**: `./gradlew build` (lint を含む) が通る。同期層の `commonTest` 159 件 (下の対応表 + Kotlin で足したケース)。本物のコアとの結合テスト
  `SyncRealServerTest` (DEVELOPMENT.md): 2 つの端末をペアリングし、A がプロジェクトを開く → B の一覧に出る (`projects_changed`) → A がチャットを作る →
  B にも出て選ばれる → A が送る → 両方の会話にユーザーの発言と返事、タスクが `running` を経て `done`、グループがマージされ `hello.txt` が main に入る →
  両方がユーザーの番を 1 回だけ検知 (最後の発話は「Done: hello.txt is on main.」) → B の改名が A に届く。実機は接続されていなかったので
  (`adb devices` が空) 実機での確認はしていない。

#### 移植したテストの対応表 (PC の vitest → `shared/src/commonTest/.../sync/`)

| PC (vitest) | Kotlin (commonTest) | 備考 |
|---|---|---|
| `store/domain.test.ts` | `DomainTest` | 記録 (`src/test/fixtures/fake-*.json`) は同じファイルをリポジトリから読む (`readFixture`、JVM の actual)。3 つの記録を空から・開始スナップショットから畳んでコアの最終状態とセッションに一致。「入力を変えない」は不変の data class なので最終状態と入力の比較だけ。知らないイベントで状態が変わらないことを追加 |
| `store/sessions.test.ts` | `ChatsAndSessionsTest` (前半) | |
| `store/chats.test.ts` | `ChatsAndSessionsTest` (中ほど) | 空の引数の `chatOfUserCall` を追加 |
| `store/drafts.test.ts` | `ChatsAndSessionsTest` (後半) | `memoryDrafts` → `PrefsDrafts(MemoryPrefs())`。同じ保存先なら残ることと、空の下書きを保存先に残さないことを追加 |
| `store/transcript.test.tsx` | `TranscriptTest` | 描画 (`TranscriptView`) の確認は画面のタスクへ。前のページとのツール呼び出しの合流 (`prependTranscripts`) を追加 |
| `store/project.test.ts` | `ProjectViewTest` | 「印の無い古いログの `session_failed`」は Kotlin の型で `retrying` が必須なので無い |
| `store/activity.test.ts` | `ActivityTest` (前半) | |
| `ui/projectActivity.test.ts` | `ActivityTest` (後半) | |
| `ui/branchTree.test.ts` | `BranchTreeTest` | `awaiting` のチャットを木のディレクトリ名の下に出すことを追加 |
| `store/app.test.ts` | `AppStoreTest` | 同期・遅延の履歴・バックグラウンドの動作中。途中の切断を見ずに番号が変わった再接続、最初の接続で最後のプロジェクトを開く、全部読んだ履歴が最初から読んだものと一致、を追加 |
| `store/appChats.test.ts` | `AppStoreChatsTest` | チャットの選択と記憶・`open_chat`・送信・作成・改名・削除・下書き・ping。PC で reject するものは例外。前面の間だけ ping と使用量を読み、前面に戻ると git を読み直すことを追加 |
| `store/multiClient.test.ts` | `MultiClientTest` | `projects_changed` / `settings_changed` / `resync` / 再接続のヒント / `session_starting` / `Snapshot.starting` / 他で削除・作成されたチャット / git の `awaiting`。`expected_revision` の保存と `conflict` (PC は `ui/SettingsSync.test.tsx` の画面で確かめていたもの) を追加 |
| (新規) | `UsersTurnTest` | ユーザーの番: 条件の境界 (受信箱・グループの状態ごと・次のターン・消えたチャット)、中止・エラー・サブエージェント・`/compact`、履歴、切断中のターン、表示していないプロジェクト (スナップショット) |
| `client` 側の既存 | `YhtyeClientTest` | 切断で再接続し `session` が増えることは T-61 のテストが確かめている (close コードによらず同じ経路なので 1013 も同じ) |

### 実装で決めたこと (T-67: 3 パネルの骨格と会話画面)

ペアリング後のホームを T-61 の最小画面 (`HomeScreen`) から 3 パネルに置き換えた。左右パネルは骨格の確認用の仮の中身。

- **置き場所** (`androidApp/.../ui/`): `MainScreen.kt` (ホーム。`MainViewModel` がメンションを持つ)、`shell/PanelShell.kt` (骨格とジェスチャ)、
  `conversation/` (中央: `ConversationScreen` / `ConversationModel` (同期層の状態から画面の値を出す純関数) / `TranscriptList` /
  `TranscriptItems` / `Composer` / `ChatMenu` / `RenameField` / `Markdown` + `MarkdownModel` / `Mentions`)、`panels/PlaceholderPanels.kt`
  (仮の左右パネル)、`common/` (線のアイコン `YIcons` は PC・モックと同じ lucide の形、リング、空の状態)。`Theme.kt` に Yhtye の色。
- **骨格** (`PanelShell`): モックの規則どおり。開いたとき中央を 64dp 残し、そこ (覆い) のタップで閉じる。1 回のスワイプで動くのは今の位置から
  隣まで。離したとき 350dp/s (モックの 0.35px/ms) を超えていればその向き、そうでなければ近い方。開閉が変わったら `HapticFeedbackType.GestureEnd`。
  - 端: `WindowInsets.systemGestures` の左右から始まった指は扱わない (ジェスチャナビゲーションでは Android の「戻る」。3 ボタンでは幅 0)。
  - 向き: モックの 10px ではなく Compose の `touchSlop` で決める (子のスクロールと同じしきい値にしないと、子と親のどちらが先に取るかがずれる)。
  - 優先: 親は Main パスで子の後に見て、子 (縦のリスト・はみ出したコードブロックや表の `horizontalScroll`) が先に動きを取っていたら手を引く。
    横に決まった後は Initial パスで先に取り、子に渡さない。**端まで行った余りはパネルに渡さない** (nested scroll で親に渡さない。モックと同じく、
    はみ出した要素の上の横スワイプはその要素のもの)。はみ出していないコードブロック・表はスクロールを付けない (`enabled = maxValue > 0`)
    ので、その上からでもパネルが開く。
  - 戻る: `PredictiveBackHandler`。開いているパネルだけを閉じ、指に合わせて中央が戻る (取り消すと元に戻る)。中央では扱わない (アプリを閉じる)。
  - 幅の広い端末 (`WindowSizeClass` の幅 Expanded、840dp 以上): 左 320dp・中央・右 360dp を並べて常時表示し、ジェスチャ・☰・タスクのボタンは出さない。
    回転 (横向きの Pixel 8a は 1078dp) でも切り替わる。
  - 隠れているパネルは合成しない (閉じると中のスクロール位置は戻る。T-69 で要れば状態を持ち上げる)。
- **中央** (PC の Conversation とモックの中央):
  - 上部バー: ☰、タイトル (無ければ「新しいチャット」、チャットが無ければ PC と同じ「orchestrator」) + 作業ツリーのブランチのピル
    (`placeLabel`、見つからなければ警告のアイコン)、オーケストレータの状態 (PC の SessionBadge と同じ形と言葉)、タスク (右パネル)、メニュー。
    他のチャットで起きたことの印は PC と同じくチャットの一覧の未読の点だけ (☰ に印は付けない)。
  - メニュー (決定事項「会話の上部のメニュー」): コンテキストの使用量 (PC の ContextBar のメーターと文言)、「コンテキストを圧縮」(押せない理由は
    PC の文)、名前を変更、削除。**名前の変更はタイトルがその場で入力欄になる** (PC の RenameForm と同じ。✓ / IME の完了で保存、✕ / 戻るで取り消し)。
    ダイアログにしなかったのは PC に合わせたのと、Robolectric でダイアログ内の入力欄が試せないため。削除は確認のダイアログ (PC の DeleteConfirm の文。
    `deleteBlocker` の理由があれば「閉じる」だけ)。
  - 会話: ユーザーの発言は吹き出し、返事は Markdown、ツール呼び出しは 1 行、Yhtye の操作 (`yhtye_tool`) と考えたこと・プロンプトは折りたためる 1 行、
    受信箱の通知は破線の枠、グループのカード、ターンの終わり・起動と停止の 1 行 (すべて PC の TranscriptView と同じ種類と文)。考えたことは PC と同じく
    アイコンだけの閉じた折りたたみ (ストリーミング中はアイコンが呼吸する)。時刻はモックと同じく出さない (PC もホバーだけ)。本文は長押しで選択できる。
  - ストリーミング途中の本文は届くたびに Markdown として描き直し、末尾に点滅するカーソル (モック)。
  - 自動スクロール (PC の useAutoScroll): 下端から 48dp 以内にいればユーザーが読んでいるとみなして新しい行・断片・入力欄やキーボードで高さが
    変わったときに最新へ。ユーザーのスクロールでだけ「付いていく」を切り替える。チャットを替えると最新から。
  - 古い履歴 (PC の OlderHistory): 上端の「読み込み中…」の行が見えたら `loadOlderHistory`、読んだ後も見えていれば次も。失敗は理由と再試行。
    前に足したときは読んでいた行を同じ位置に残す。
  - 末尾にグループ待機バナー (PC の WaitBanner の文をモックのように全部見せる + 進み具合のバー)。タップで右パネル。
  - コンポーザ: 複数行 (128dp まで)、IME に追従 (`ime ∪ navigationBars` の余白。`adjustResize` と edge-to-edge)、送信、ターン中は中止、状態の一言
    (PC と同じ「起動中…」「作業中 · 送信分はターン終了後に届きます」、送れない理由)、メンションのチップ (タップで外す、積むと弾む)。Enter は改行
    (スマホのキーボード)。送信は `sendMessage(withMentions(...))` で、受け付けられたら本文とメンションを空にする (会話には足さない。発言はイベントで出る)。
    下書きは同期層の `drafts` (チャットごと)。
  - 空の状態は PC の文 (「プロジェクト (git リポジトリ) を開いてください。」「サイドバーの「新しいチャット」から始めます」「会話はまだありません」)。
    接続が切れたら PC の ConnectionBanner と同じ文 + T-61 の「今すぐ再接続」、同期層のエラー (`AppState.errors`) は閉じられる帯。
- **Markdown**: JetBrains の `org.jetbrains:markdown` (GFM) で構文木にし、Compose で自前に描く (表・コードブロックの横スクロールと配色を
  モックに合わせるため)。安全は PC の Markdown.tsx と同じ (生の HTML は出さない、リンクは http(s) / mailto だけで外のブラウザ、画像は代わりの文字)。
  段落の中の改行は PC (`pre-wrap`) と同じく改行のまま。
- **色**: ダークは tokens.md・モックの値 (oklch は sRGB に変換)。**ライトは PC にもモックにも無い**ので、同じ流儀 (暖色寄りのグレー、琥珀、
  ステータスは色相固定) で起こした (`LightYhtyeColors`)。端末のダーク/ライトに従い、Dynamic Color は使わない。Material 3 の部品の配色も同じ値から作る。
- **メンションを積む API** (T-69 のタスクの行のスワイプから): `MainScreen` の `addMention(Mention(task, title))` (選択中のチャットのコンポーザに積み、
  パネルを閉じて中央に戻る)。中身は `MentionsState.add(chat, mention)` (チャットごと、`MainViewModel` が持つので回転でも残る。同じタスクは 1 つだけ)。
  仮の右パネルは PC のタスクカードの `@` と同じボタンでこれを呼ぶ。
- **T-69 が差し込む所**: `MainScreen` の `PanelShell(left = …, right = …)` の `LeftPanelPlaceholder` / `RightPanelPlaceholder` を本番のパネルに替える。
  タスク詳細は中央の上に積む画面として `PanelShell` の後に合成し、開いている間は `PanelShell(swipeEnabled = false)`。詳細の `PredictiveBackHandler` は
  後から合成されるので先に効く (戻るは 詳細 → パネル → 中央)。タスクの行の右→左スワイプは行の中で横の動きを取れば (Main パスで消費すれば)
  親のパネルのドラッグは手を引く。左→右は取らずにおけばパネルを閉じるドラッグになる。パネルの開閉は `LocalPanelShell.current` (`show` / `hide`)。
- **仮の左パネル**: プロジェクト一覧 (タップで開く)、チャット (リング・未読の点・経過、タップで出す)、新しいチャット (メインの作業ツリーに作る)、
  下端にホストとペアリング解除 (スマホの設定画面 T-71 ができるまで)。**仮の右パネル**: 選択中のチャットのグループとタスク (状態の系統のアイコン) と `@`。
- **テスト** (`androidApp/src/test/`、Robolectric。`./gradlew build` に含まれる): `PanelShellTest` (スワイプで左右が開く・覆いのタップで閉じる・
  遅いドラッグは戻り速いフリックは開く・1 回で 1 段・端のスワイプは開かない・縦のスワイプは動かない・はみ出したコードブロックはそれ自身がスクロールし
  パネルは開かない・はみ出していなければ開く・戻るで 1 段閉じて中央では扱わない、幅が広いと常時表示)、`ConversationScreenTest` (本物の `AppStore` を
  偽のコアで: 会話の表示・送信がコアへの `send_user_message` になり受け付けられたら空になる・空では送らない・ターン中の中止・右パネルの `@` で積んだ
  メンションが先頭に付いて送られ、チップのタップで外れる・名前の変更 (空は断る)・削除の確認と取り消し・実行中は削除の理由・チャットが無いときの案内・
  待機バナーの文とタップで右パネル・ストリーミングの表示)、`MarkdownModelTest`、`ConversationModelTest`。
- **実機 (Pixel 8a、ジェスチャナビゲーション)**: example の `remote_server` + 偽エージェント (Markdown を少しずつ流すターン、`wait_cancel` で止まる
  ターン、グループとタスクを作るターン) でペアリング → 左パネルからプロジェクトを開きチャットを作成 → 送信・ストリーミング・Markdown (見出し・
  強調・表・コードブロック・リスト・リンク) → コードブロックの上の横スワイプはコードがスクロールしパネルは開かない → それ以外の横スワイプで右パネル →
  画面端からの戻るジェスチャで右パネルが閉じる → 中央で左端からのスワイプはアプリの戻る (パネルは開かない) → 中止ボタンで `turn ended: cancelled` →
  待機バナー (`0/2 完了 · 2 件 対処中`) のタップで右パネル → `@` でチップ → メニューの名前の変更・削除の理由 → ライト (作り直しの後もメンションが残る) →
  1 回の長いフリックは中央で止まる、を確かめた。横向きでは 3 列の常時表示になった。スクリーンショットは `/tmp/yhtye-android-shots/t67/`。
  終わったら PC 側で端末を失効させた (アプリは失効の案内に戻る)。
- **モックとの違い**: メニュー (改名・削除・コンテキスト・圧縮) と接続・エラーの帯はモックに無い (決定事項と PC にあるもの)。名前の変更は上部バーの
  その場の入力欄。PC にある「session started」などの起動・停止の行を出す (モックには無い)。タイトルの下のピルの文字がわずかに下に寄る。
  左右パネルの中身は仮 (T-69)。

### 実装で決めたこと (T-69: 左右パネルとタスク詳細)

T-67 の仮の左右パネル (`panels/PlaceholderPanels.kt`、削除) を本番に替え、タスク詳細と仮の設定画面を積み重ねの画面として足した。

- **置き場所** (`androidApp/.../ui/`): `panels/LeftPanel.kt` (レール・ビューの切り替え・下端のホストと使用量)、`panels/ProjectPicker.kt`
  (ドロップダウンとリポジトリを開く欄・バックグラウンドのチップ)、`panels/BranchTree.kt` (作業ツリーとチャットの木・新しいブランチのフォーム)、
  `panels/RightPanel.kt` (PC の TaskColumn)、`panels/TaskCard.kt` (PC の TaskCard と行のスワイプ)、`tasks/TaskParts.kt` (状態のチップ・Step の並び・
  help の文・■ の確認。PC の statusIcons)、`tasks/TaskDetail.kt`、`shell/ScreenStack.kt` (積み重ねの画面)、`settings/SettingsScreen.kt` (仮)、
  `common/ActionScope.kt`。共通モジュールに `sync/HostStatus.kt` (PC の `hostStatus.ts` と StatusBar の `planLabel` / `formatRemaining`) と
  そのテスト `HostStatusTest`。
- **左パネル**: モックどおり左端 56dp のレール (コーディング / 履歴、一番下に設定 = PC と同じ sliders のアイコン)。
  - 履歴は PC の RunsView と同じ空の状態「現状は何もありません」。PC は履歴でサイドバーごと置き換えるので、モックと同じく左の中身・中央
    (上部バーは ☰ と「履歴」だけ) とも空の状態にし、右パネルは無い (`PanelShell(rightEnabled = false)`: 右へのドラッグの範囲を 0 までにする)。
  - プロジェクトのドロップダウン: PC の ProjectPicker と同じ一覧 (コアの順、リング・今のものに ✓)・開く欄 (`/path/to/git/repository`)・文言
    (「プロジェクトを選択」「プロジェクトを開く」「まだプロジェクトがありません。」)。開いた一覧はモックと同じく下の中身に重ねて出し、外のタップで閉じる
    (Material の Popup にしなかったのは、モックの見た目と、Robolectric で Popup 内の入力欄が試せないため)。選んでもパネルは開いたまま (PC・モック)。
  - バックグラウンドのチップ・リングは PC の `backgroundProjects` / `projectRing` (T-66 で移植済み)。
  - 木: PC の BranchTree と同じ (作業ツリーの既定の開閉は「チャットがあるかメインクローン」、開閉はプロジェクトごとに `MainViewModel` に残す)。
    チャットの行のタップで選んで中央に戻る。行の「⋯」(改名・削除) はスマホでは会話の上部のメニューにあるので出さない (決定事項)。見つからない
    作業ツリーの見出しは PC ではアイコン + ツールチップだが、スマホにツールチップが無いので「見つからない作業ツリー」の文を添えた。
    新しいチャット・他のブランチの新しいチャット・新しいブランチは、作れたら中央に戻る (モック)。新しいブランチのフォームは PC と同じ
    (名前・開始点 = `HEAD (<head>)` と `yhtye/` 以外のローカルブランチ、✓ / IME の完了、断られたらコアの文をフォームに出す)。
  - 下端: PC のサイドバーのフッタ (接続の点・ホスト名・最後の ping からの経過 = 1 秒ごとに数え直す) とステータスバー (プラン・5h / 7d のメーター・
    リセットまで、タップで `refreshUsage(true)`。報告が無ければ「—」)。モックと同じく 2 行で、狭ければ折り返す。
- **右パネル**: PC の TaskColumn / GroupBar / TaskCard と同じ情報と文言。PC ではツールチップの言葉 (状態の名前) はモックと同じくチップに見せる。
  過去のグループはモックと同じく「過去のグループ N」(開閉は `MainViewModel` に残す)。help は状態の表示だけ (カードが対処中の色、種類のアイコン、文)。
  終わったタスク (中止) の Step はコアでは `running` のまま残ることがあるので、動いているようには描かない (モック)。
- **■ の確認**: タスクの ■ とグループの中止は、タッチの誤操作を防ぐため確認のダイアログを挟む (PC は確認なし。文は PC のボタンの説明
  「エージェントを停止 (タスクを中止)」「グループを中止」)。ダイアログは `MainScreen` が 1 つだけ持つ (右パネルと詳細のメニューの両方から)。
- **タスクの行のスワイプ (メンション)**: 行が指に付いて左にずれ、下から `@` が現れる。88dp を超えると `@` が弾み (触覚 `GestureThresholdActivate`)、
  超えて離すか、40dp 以上で 450dp/s を超えてはじくと確定 (`@` が弾んでカードが戻り、300ms 後に `addMention` → 中央へ)。足りなければ戻る。
  132dp より先はゴムのように重くする。**判定の規則**: 行は Main パスで親 (`PanelShell` のドラッグ) より先に指を見る。タッチスロップを越えたとき
  横が縦より大きく、かつ**右→左のときだけ**その指を取り (消費する)、親は消費済みを見て手を引く。左→右・縦は取らないので、左→右は親の
  「右パネルを閉じる」ドラッグ、縦は一覧のスクロールになる。画面の右端の `systemGestures` から始まった指は取らない (Android の戻る)。
  行の中のボタン (`@`・■) の上から始めても同じ (動きを消費するのでボタンの押下は取り消される)。
- **タスク詳細**: 中央の上に積む全画面 (`ScreenStack`)。ヘッダ (戻る・状態のチップ・`T-n · G-n`・題・メニュー)、Step の並びと担当・経過、中断の注記、
  対処中の help の文、中止の理由、セッションのタブ (PC と同じ並びと点。はみ出したら横スクロール)、出力。出力は中央と同じ `TranscriptList` /
  `TranscriptRow` (T-67 の `TranscriptList` を `TranscriptSource` で一般化し、Step の結果を先頭の行として渡せるようにした)。
  メニューは PC にある操作だけ (オーケストレータへ引用 = `addMention`、中止 = ■ と同じ確認)。タスクが消えたら閉じる。
  - 閉じ方: 左→右のスワイプ (画面が指に付いてずれ、幅の 35% か 350dp/s を超えて離すと閉じ、足りなければ戻る。右→左は何もしない。
    はみ出したコードブロック・タブの上の横スワイプはその要素のもの)、ヘッダの戻る、`PredictiveBackHandler` (指に合わせてずれる)。
    積んだ画面は開いたときに合成されるので戻るはパネルより先に効く (詳細 → パネル → 中央)。開いている間は `PanelShell(swipeEnabled = false)`。
  - 設定も同じ `ScreenStack` に積む (`Stacked.Settings`)。
- **操作のスコープ**: 同期層への要求は `MainScreen` のスコープ (`LocalActionScope`) で流す。実機で、ドロップダウンでプロジェクトを選ぶと一覧が閉じて
  その `rememberCoroutineScope` が取り消され、`open_project` が届かなかったため (Robolectric では再現しない順序)。
- **T-71 が設定画面を差し込む口**: `ui/settings/SettingsScreen.kt` の `SettingsScreen(app, onBack, onUnpair)` を本番に替える。開くのは左パネルの
  レールの一番下 (`LeftPanel(onOpenSettings)` → `MainScreen` の `stackedKey = Stacked.Settings.key`)、表示は `ScreenStack` (スワイプ・戻るで閉じる)。
  今の仮の画面は接続先のホストと「ペアリング解除」(確認つき、T-67 の仮の左パネルから移した) だけ。同期層の設定 (`AppStore.settings`) は使っていない。
- **テスト** (Robolectric、`MainHarness` で本物の `AppStore` を偽のコアで動かす。`TestCore` にプロジェクトの一覧・git・`create_chat` / `create_branch` の
  答えを足した): `LeftPanelTest` (ドロップダウンでの切り替え・パスで開く・外のタップで閉じる・バックグラウンドのチップ・木の選択で中央が替わる・
  折りたたみ・新しいチャット・新しいブランチ (開始点の選択)・断られたブランチの理由・他のブランチ・履歴のビュー (右パネル無し)・設定とペアリング解除・
  下端)、`RightPanelTest` (PC と同じ情報・help は状態だけ・行の右→左でメンション・遅い短いスワイプは戻る・速いはじきは確定・左→右はパネルを閉じる・
  画面端からは取らない・■ の確認と中止・過去のグループ・グループの中止・マージの再試行・グループが無いとき)、`TaskDetailTest` (セッションのタブ・
  Step の結果・出力・help・スワイプで閉じる / 足りなければ戻る / 右→左は何もしない・画面端・戻るの順・ヘッダの戻る・メニューの引用と中止)、`HostStatusTest`。
  スワイプの閾値の判定を壊すとテストが落ちることを確かめた。
- **実機 (Pixel 8a、ジェスチャナビゲーション)**: example の `remote_server` + 偽エージェント (グループ・調査タスク・実装中のまま止まるタスク・help を
  上げるタスク・依存で待つタスク、コンフリクトでマージ保留になるグループ) で: ペアリング → ドロップダウン・チップでのプロジェクト切り替え →
  新しいチャット → グループ (完了・実装中・対処中・待機・過去のグループ) → 行のスワイプ (保持した途中の `@`・確定でチップ・短いと戻る・左→右で
  パネルが閉じる) → タスク詳細 (コードブロックの横スワイプはコードがスクロール・保持した 33% のドラッグは戻る・スワイプで閉じる・戻るキーで
  詳細 → パネル → 中央・画面端の戻るジェスチャで詳細が閉じる・メニューの引用) → ■ の確認と中止 → 新しいブランチ (既存の名前で理由・開始点の選択・
  作成で中央) → マージ保留の理由と再試行 (コアに届き再び保留) → グループの中止 → 木でのチャットの切り替え → 履歴のビュー → 横向きの 3 列 →
  設定 (スワイプで閉じる・ペアリング解除) を確かめた。スクリーンショットは `/tmp/yhtye-android-shots/t69/`。終わったらサーバーで端末を失効させ、
  端末の設定 (画面の常時点灯・回転) を戻した。
- **モックとの違い**: 使用量はプラン名が無いと出さない (example サーバーは使用量を読まないので「—」)。ドロップダウンの一覧の幅は欄と同じ、開始点の
  選択は Material のメニュー。見つからない作業ツリーの見出しに文を添えた。■ に確認がある (モックは PC と同じく確認なし)。チャットの行の右の
  「⋯」は無い (会話のメニューにある)。偽エージェントの返事は MCP の結果をそのまま本文に含むので会話が長い (アプリの違いではない)。

## ペアリング

| 項目 | 内容 |
|---|---|
| QR の中身 | `yhtye://pair?v=1&host=<MagicDNS 名>&ip=<100.x>&port=<N>&code=<コード>&sid=<サーバー ID>` (ホストは 2 通り載せ、届く方を使う) |
| 手入力 | ホスト名 (または IP) + 8 文字のコード (0/O・1/I/l など紛らわしい文字を除く) |
| コード | 有効 5 分、1 回使ったら無効、5 回間違えたら無効 |
| 交換 | スマホが `{code, device_name}` を送る → PC が端末トークン (256 bit ランダム) と端末 ID を返す |
| 端末名 | 機種名を初期値に、ペアリング時に変更可 |
| スマホでの保存 | トークンは Android Keystore の鍵で暗号化して保存 |
| PC の画面 | 今の React の設定画面に「リモート接続」欄 (オン/オフ・ポート・QR 表示・端末一覧と失効・最終接続日時)。GPUI 移行で作り直す前提なので最小限 |
| スマホの初回 | 起動 → 「QR を読む / 手入力」→ 端末名の確認 → 接続 |

## 段階

各段階の完了条件は「本物で動くこと」。

1. **サーバーと認証 (PC 側)**: WS 処理の切り出し、端末表、ペアリング、`whois` 照合、Tailscale アドレスでの待ち受け、
   設定画面の「リモート接続」欄。
   完了条件: Tailscale 上の別端末 (またはスクリプト) から、ペアリング → トークンでコマンド実行 → 失効後は拒否、が通る。
   トークン無し・誤り・他ユーザーの端末・ローカル専用コマンドは拒否される。
2. **Android の基盤 (通信とペアリング)**: Gradle (KMP 共通 + Android アプリ) の雛形、API 型の Kotlin 生成
   (JSON Schema 経由に決定、T-59)、Ktor の WS クライアント、QR 読み取り (CameraX + ML Kit)
   と手入力、Keystore への保存、再接続。
   完了条件: 実機から Tailscale 経由でペアリングし、最小の画面でプロジェクト一覧が出る。
3. **GUI**: Compose の画面。画面構成・操作 (チャット、タスク状況、通知など) は段階 3 の前に詰める。

## 段階 3: GUI の決定事項

HTML モック [`docs/design/mobile-mock/index.html`](../design/mobile-mock/index.html) (1 ファイル完結、ダミーデータ。T-62 で作り、
T-63 で PC 版の設計に合わせて直した) で画面構成と操作の感触を確かめ、ユーザーが決めたこと。

### 原則

- **ユーザーはオーケストレータとだけ対話する** (orchestration-model §1)。help / checkpoint / 依存の解消 / グループの完了は
  オーケストレータを起こすきっかけで、扱うのはオーケストレータ (`answer_help` など)。スマホも PC と同じく**状態を表示するだけ**:
  タスクカードが「対処中」の色になり、help の種類のアイコンと文を出す。会話の受信箱の通知 (`help_raised` など) は PC と同じ
  破線の控えめな行。「判断待ち」のような、ユーザーに判断を求める表示・警告・赤いカウントは作らない。
- **PC 版に無い機能・情報は作らない。** 文言・状態ラベルは PC 版 (`src/ui/labels.ts`・`taskInfo.ts` など) と同じ。
  操作は PC にあるものだけ: タスクの `@` (オーケストレータへ引用 = コンポーザにメンションチップを積む)、タスクの ■
  (エージェントを停止 = タスクを中止。終端状態のタスクには無い。PC と同じく確認なし)、グループの中止、merge_blocked のグループの
  「マージを再試行」、ターンの中止 (ターン中も送信はでき、受信箱で待つ)、新しいチャット・新しいブランチ、設定。
  見た目は PC と同じダークのトークン (ライト・テーマ切替は無い)。
- 例外はスマホならではの操作の割り当て (スワイプ・パネル) だけ。

### 画面構成

| 場所 | 中身 |
|---|---|
| 中央 | **チャットだけ** (PC の会話: 上にチャットのタイトル + 作業ツリーのブランチ + オーケストレータの状態の点、末尾に PC と同じ**グループ待機バナー** `グループ「…」の全タスク完了まで待機中 — 1/3 完了 · 1 件 対処中`、下にコンポーザ) |
| 左パネル (右方向のスワイプ / ☰) | 左端の縦の列 = PC のアイコンレールと同じ**ビュー切り替え** (コーディング = work / 履歴 = runs、一番下に設定)。履歴ビューは PC と同じ空状態「現状は何もありません」。work ビューは上から **プロジェクトのドロップダウン** (PC の `ProjectPicker`。リング + 名前、一覧でタップして切り替え)、その下に PC と同じ**バックグラウンドで動作中のプロジェクト** (`BackgroundProjects`: リング + 名前のチップ、タップで切り替え)、**ブランチ (作業ツリー) とチャット**の木 (PC の BranchTree: チャットのリング・未読の点・経過、作業ツリーごとの新しいチャット、他のブランチ、+ で新しいブランチ)。下端にコアのホスト (PC のサイドバーのフッタ) とプラン・使用量 (PC のステータスバー) |
| 右パネル (左方向のスワイプ / トップバー右のタスクのボタン) | 選択中のチャットの**グループ** (PC のタスク列): ヘッダ (グループ名 + subagents 数)、グループ行 (状態・ブランチ・x/y 完了・対処中の数・中止・merge_blocked の「マージを再試行」)、タスクカード、過去のグループは折りたたみ |
| タスク詳細 (カードのタップ) | PC の**エージェントの出力**: セッションのタブ (implementer / review-n)、Step の結果、そのセッションの出力 |
| 設定 (レールの一番下) | PC の設定画面の項目のうち エージェント / ハーネス / 秘密の環境変数。**「リモート接続」は PC 専用なので出さない** |

- パネルを開いたとき中央を 64px 残し、その部分のタップで閉じる。プロジェクトのリングは PC の規則 (T-49): そのプロジェクトに
  動作中のもの (オーケストレータのターン、または running / merging のタスク) があれば回る緑、なければ中を抜いた緑。

### ジェスチャ

1. 画面の左右 24px から始まったタッチは Android の「戻る」ジェスチャに任せ、アプリは扱わない。
2. 横にスクロールできる要素 (コードブロック・表・セッションのタブ) の上で、実際にはみ出していれば、その要素のスクロールが優先。
3. 最初の 10px の向きで決め、縦ならその指は縦スクロールのまま (横に切り替えない)。
4. 横なら、始めた場所と向きで 1 つに決める:
   - **タスク詳細が開いている**: 左→右 = **詳細を閉じる** (詳細が指に付いてずれる。幅の 35% を超えるか 0.35px/ms を超える速さで
     離すと閉じ、足りなければ戻る)。右→左は何もしない。
   - **右パネルが開いていて、タスクカードの上で始めた**: 右→左 = **メンション** (カードが指に付いてずれ、下から `@` が現れる。
     88px を超えると `@` が弾み (触覚の代わりの軽いアニメーション)、超えて離す (または 40px 以上で 0.45px/ms 超のフリック) と確定:
     PC の `@` と同じくそのタスクをコンポーザのメンションチップに積み、中央に戻る。足りなければカードが戻る)。
     左→右はパネルを閉じるドラッグ。右パネルが開いた状態のパネルのドラッグは左→右しか動かないので、向きで衝突しない。
     カードの右端 24px は 1. で除外されるため、メンションのスワイプはカードの内側から始める。
   - それ以外: パネルのドラッグ (1 回で動けるのは今の位置から隣まで。速さ 0.35px/ms 超ならその向き、そうでなければ近い方)。
5. Android の戻るは 1 段ずつ閉じる (詳細 → パネル → 中央)。

### 通知

- **通知は作業全体が完了したときだけ。** help / checkpoint / 依存の解消ではオーケストレータが扱うので通知しない。
- 「作業全体の完了」の案: チャットのオーケストレータの**ターンが正常に終わり** (`end_turn`。ユーザーの中止は除く)、
  そのチャットの**受信箱が空**で、**終わっていないグループ (active / finishing / merge_blocked) が無い**とき。
  典型は「グループが完了してマージされ (`merge_result` ok)、それを受けたオーケストレータのターンも終わった」とき。
  グループを作らずにオーケストレータが自分で済ませた依頼も、ターンの終わりで同じ条件を満たせば完了とする。
  アプリが前面でそのチャットを表示中なら出さない。通知のタップでそのチャットを開く。
- 通知の文: `<チャットのタイトル> の作業が完了しました` + オーケストレータの最後の発話の 1 行目。

### 決定 (2026-10-11、段階 3 の本実装前)

- **裏での接続**: 既定は**常に常駐** (ペアリング中は Foreground Service で WS を保つ)。設定で「作業中だけ常駐」(オーケストレータのターンか終わっていないグループがある間だけ常駐し、完了を通知したらやめる) に切り替えられる。FCM は使わない (Tailscale だけで完結させる)。
- **通知の条件**: オーケストレータの手が止まりユーザーの番になったとき (ターンが正常に終わり、受信箱が空で、走っているグループ (active / finishing) が無い)。merge_blocked もこれに含まれる。
- **PC にあってモックに無かったもの**: チャットの改名・削除、コンテキストの量と `/compact` は会話の上部のメニュー、リポジトリを開くはプロジェクトのドロップダウンに入れる。git グラフは最初の版では入れない。
- **ペアリング解除**: スマホの設定画面 (スマホ固有の例外)。
- **操作権ロック**: 保留 (下記「複数クライアントの同時操作」)。リモート端末に許す `Set*` は PC と同等のまま (ハーネスのパスも許可)。

### まだ決めていないこと (上で決まったものを除く)

- merge_blocked (マージがコンフリクトで保留) は「完了」ではないので通知しない案だが、ユーザーが整えるまで止まる状態なので知らせるべきか。
- 裏で通知を受けるための接続 (T-61 では前面にある間だけつなぐ。Foreground Service か、PC からの push か)。
- ペアリング解除の置き場所 (PC の設定には無いスマホ固有の操作。T-61 の最小画面ではメニューにある)。
- PC にあってモックに入れていないもの: git グラフ、チャットの改名・削除、リポジトリを開く欄、コンテキストの量と圧縮。

## 複数クライアントの同時操作 (T-64 の調査、2026-10-11)

PC の GUI と Android が 1 つのコアを同時に操作する場合の調査。**DB・状態が壊れる競合は無い**
(プロジェクトごとに 1 つのドライバータスクが全コマンドを直列に処理し、判定から保存までの間に割り込めない。
`runtime/driver.rs`、`runtime/orchestration.rs`。SQLite は WAL・busy_timeout 5s)。
同じチャットへの同時送信は受信箱に別々に積まれ、二重実行の後勝ち側は `invalid_state` / `not_found` になる。

本実装 (段階 3) で入れる修正 (優先度順)。1〜8・10 は T-65 で対応した (詳細は [`core-design.md`](../architecture/core-design.md) §19。
Android の画面側の対応は後続タスク):

1. コア: 設定用の Mutex で `set_agent_settings` と `set_secret` / `delete_secret` を直列化する (DB とメモリ・キーリングの食い違い防止)。
   **対応済み (T-65)**: `Core` の `settings_lock` が検証から反映 (DB → メモリ / キーリング) まで囲む (`runtime/core/settings.rs`)。
2. コア + PC + Android: アプリ全体向けイベント `projects_changed` (他の端末で開いたプロジェクトが一覧に出ない)。
   **対応済み (T-65)**: `projects_changed { project }` (project 空・seq 0・live、リモートにも流す) をプロジェクトの新規登録・初回の open で出す。
   PC は受けて `refreshProjects`。チャットの無いプロジェクトに他の端末がチャットを作ったら、それを表示する (`open_chat` も送る)。
3. コア + PC: `settings_changed` のヒントイベント、`SetAgentSettings` に期待する版を渡して不一致なら `conflict` (古い設定画面からの上書き防止)。
   **対応済み (T-65)**: `settings_changed { kind: agent_settings|harnesses|secrets, project?, revision? }`。`AgentSettingsView.revision`
   (全層で 1 つ、増えるだけ) を `set_agent_settings.expected_revision` に渡すと不一致で `conflict`。PC の 3 つの設定画面は通知で読み直し、
   編集中ならバナー (「他の端末で設定が変更されました … 再読込」)。エージェント設定は保存に版を付け、`conflict` なら知らせて読み直す。
4. コア + クライアント: WS の送信キューを有界にし、取りこぼし (`Lagged`) で `resync` を送る。クライアントは追いつき処理をする。
   **対応済み (T-65)**: WS の送信キューは 1024。あふれたらプロジェクトの live は捨て、durable とアプリ全体のイベントは接続を閉じる
   (close 1013、再接続に任せる)。応答は捨てない。
   WS セッションと Tauri の `forward_events` の `Lagged` でその接続に `resync` (アプリ全体向け live)。PC は `list_events` で追いつき、
   プロジェクト一覧・git・表示中のチャット・設定画面を読み直す。
5. コア + GUI: 二重実行で負けた側にエラーを出さない (冪等化か、直前のイベントで目標状態なら握りつぶす)。
   **対応済み (T-65)**: すべてコアで冪等化 (目標状態なら `accepted`、何も記録しない): `cancel_task` / `cancel_group` (既に中止)、
   `retry_group_merge` (既にマージ済み)、`delete_chat` (削除済みの `C-<n>`。一度も無い ID は `not_found`)、`compact_chat` (圧縮のターン中)。
   GUI で握りつぶすものは無い。
6. PC: 選択中のチャットが他の端末で削除されたときの切り替え (`openChat` と記憶の更新)、`chat_created` で git の概要を即時に読み直す。
   **対応済み (T-65)**: 切り替え先を記憶して `open_chat`、消えたチャットの下書きを消す。`chat_created` で git を即時に読み直し、
   その間はそのチャットの作業ツリーを「見つからない」でなく不明として扱う (`GitView.awaiting`)。
7. コア: 「起動中」をスナップショットかイベントで共有する (今は要求した端末しか知らない)。
   **対応済み (T-65)**: 両方。live イベント `session_starting { session }` と `Snapshot.starting` (起動中のセッションのキー)。PC は両方から「起動中」を出す。
8. Tauri: 単一インスタンスの保証 (同じデータで 2 つ起動するとコアが 2 つになる)。
   **対応済み (T-65)**: データディレクトリの `yhtye.lock` を排他ロック (`src-tauri/src/data_lock.rs`)。取れなければ理由を出して終了する
   (`tauri-plugin-single-instance` はアプリの識別子単位でデータディレクトリと一致しないため使わない。既存ウィンドウの前面化はしない)。
   開発用ブリッジはロックしない。
9. 要判断: リモート端末に許す `Set*` コマンドの範囲 (特に `SetHarnessPath` は任意の実行ファイルを指定できる)。
   **決定済み**: 今のまま (PC と同等)。
10. 文書化: 同じ接続内でも要求の実行順は保証されない。依存する要求は応答を待ってから送る。
    Android は PC と同じ同期方式 (スナップショット + `list_events` + cursor、git の読み直しの契機、復帰時の一覧の読み直し) を踏襲する。
    **対応済み (T-65)**: core-design §10 と `ws.rs` の doc に書いた。Android の同期層は §19.1 のイベントも扱う。
11. 将来: git 処理の非同期化 (今はマージ中にそのプロジェクトの全コマンドが待つ)、ストリーミング途中の本文をスナップショットに含める。

保留: チャット単位の「操作権」ロック (片方が操作中ならもう片方は閲覧のみ、取得で奪う)。データの整合はコアの直列化で守られるので、
上の追従の修正を入れたうえで、食い違う指示が実際に問題になるなら足す。
