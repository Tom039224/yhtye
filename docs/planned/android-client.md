# 計画中の機能: Android クライアント (Tailscale 経由で PC の Yhtye を操作)

- 日付: 2026-10-11
- 状態: 設計確定。段階 1 の PC 側サーバーと認証は実装済み (T-55、[`core-design.md`](../architecture/core-design.md) §18)。設定画面 (GUI) と Android は未実装
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
   (typeshare か JSON Schema 経由かはこの段階で決める)、Ktor の WS クライアント、QR 読み取り (CameraX + ML Kit)
   と手入力、Keystore への保存、再接続。
   完了条件: 実機から Tailscale 経由でペアリングし、最小の画面でプロジェクト一覧が出る。
3. **GUI**: Compose の画面。画面構成・操作 (チャット、タスク状況、通知など) は段階 3 の前に詰める。
