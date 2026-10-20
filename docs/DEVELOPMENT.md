# 開発ガイド

Yhtye 本体を開発する人向け。使うだけなら [`SETUP.md`](../SETUP.md) を参照。
設計は [`architecture/`](architecture/)、進捗は [`PLAN.md`](PLAN.md)。

## 構成

| 層 | 技術 |
|---|---|
| シェル | Tauri 2 (Rust 2021) |
| フロントエンド | React 19 / TypeScript / Vite |
| ハーネス接続 | [ACP](https://agentclientprotocol.com) (Agent Client Protocol) — Claude Code / OpenCode / Codex |
| 指示・報告の経路 | Yhtye がホストする MCP サーバー (streamable HTTP) |
| パッケージマネージャ | pnpm |

採用理由は [ADR-0001](adr/0001-tauri-react-vite.md) を参照。

```
docs/design/         Claude Design から起こした画面仕様・デザイントークン・原本スナップショット
docs/architecture/   オーケストレーションモデル・MCP ツール・コア設計・ACP ハーネスの機能設計
docs/PLAN.md         再構築の段階計画と進捗
docs/adr/            設計判断の記録
docs/e2e/            各 Stage の実機確認のスクリーンショット
docs/planned/        未着手の機能案
src/                 React フロントエンド
src-tauri/           Tauri シェル (Rust)
crates/              yhtye-core (ドメインコア・ACP・MCP)、yhtye-dev-bridge (開発用 WS ブリッジ)
mobile/              Android クライアント (Gradle。Kotlin Multiplatform の共通モジュール + Compose アプリ)
```

## 前提

| 必要なもの | 確認コマンド |
|---|---|
| Rust 1.98+ | `rustc --version` |
| Node 22+ | `node --version` |
| pnpm 11+ | `pnpm --version` |

Linux ではさらに WebKitGTK が要る。

```sh
pkg-config --modversion webkit2gtk-4.1 javascriptcoregtk-4.1 libsoup-3.0
```

Arch / CachyOS 系で足りない場合:

```sh
sudo pacman -S --needed webkit2gtk-4.1 base-devel curl wget file openssl \
  appmenu-gtk-module libappindicator-gtk3 librsvg
```

Debian / Ubuntu 系のビルド依存は [`.github/workflows/ci.yml`](../.github/workflows/ci.yml) の
`Install system dependencies` を参照。

## 起動

```sh
pnpm install
pnpm tauri dev      # デスクトップアプリとして起動 (本物のコア + Claude Code)
pnpm dev:browser    # ブラウザで開発: WS ブリッジ + Vite を同時に起動 → http://localhost:1420
cargo run -p yhtye -- --headless --data-dir /tmp/yhtye-headless   # GUI 無し (DISPLAY 不要) で本物のコア + リモート接続
```

- 既定のエージェントは Claude Code (ACP、`npx @agentclientprotocol/claude-agent-acp`) をローカルログインで使う。
  モデルは `YHTYE_MODEL` (既定 `haiku`)。ハーネス (Claude Code / OpenCode / Codex / Devin / MiniMax Code / Google Antigravity / Grok Build) は、必要なコマンド (`npx` / `opencode` / `codex` + `npx` / `devin` / `mcode` / `agy_acp_server.par` / `grok`) が
  `PATH` か既知の場所 (`~/.local/bin` など。`mcode` は `~/.minimax-code/bin` と `$MCODE_INSTALL_ROOT/bin`、`agy_acp_server.par` は `$AGY_ACP_SERVER_HOME`・`~/.local/share/agy-acp-server`・`~/.gemini/antigravity-acp/bin`、`grok` は `~/.grok/bin` と `$GROK_HOME/bin` も) にあるときだけ選べる。設定を開いたときと「再検出」で見つけ直し、パスは手動でも指定できる
  ([`core-design.md`](architecture/core-design.md) §15.2)。
- アプリのデータ (SQLite と worktree) は `--data-dir` / `YHTYE_DATA_DIR`、無ければ `~/.local/share/io.github.tom039224.yhtye`
  (`src-tauri/src/data_dir.rs`。GUI とヘッドレスで共通)。
  以前の識別子 `com.tom039224.yhtye` のディレクトリが残っていれば、初回起動時に新しい名前へ自動で移す
  (両方あるときは触らない。`src-tauri/src/legacy_data.rs`)。
  Yhtye はプロジェクトのリポジトリには何も書かない (マージ以外)。
- 同じデータディレクトリで 2 つ目のアプリは起動しない (`yhtye.lock` を排他ロックし、取れなければ理由を出して終了する。
  終了コードは GUI が 1、ヘッドレスが 3)。並べて動かすときは `YHTYE_DATA_DIR` を変える。開発用ブリッジは別のデータなので関係ない。
  **例外 (T-97)**: 既定のデータディレクトリの GUI がもう動いていて、同じく既定のディレクトリで GUI を起動したときは、ロックの前に
  `tauri-plugin-single-instance` が引数を動いている方へ渡し (ウィンドウが前に出る)、2 つ目は終了コード 0 で終わる (`yhtye://pair?…` のリンクを開く OS の
  起動がこの経路。`--data-dir` / `YHTYE_DATA_DIR` を指定した GUI には働かない。ヘッドレスが既定のディレクトリを持っているときの GUI は従来どおりロックで終了コード 1)。
- アプリを閉じる / Ctrl+C で全エージェントを止めてから終了する。未完了の作業があるプロジェクトは
  次の起動時に自動で開き、中断したタスクを再開する。

### ヘッドレス起動 (`yhtye --headless`)

ウィンドウを開かずコアとリモート接続だけを動かす実行時オプション (cargo feature ではない。`main()` が引数を最初に見るので、`DISPLAY` の無い環境でも落ちない。
設計は [`core-design.md`](architecture/core-design.md) §9、使い方は [`SETUP.md`](../SETUP.md))。

```sh
yhtye --headless [-d] [--port N] [--data-dir DIR] [--model NAME]   # -d: デーモン (切り離して戻る。ログは <データディレクトリ>/headless.log)
yhtye remote pair | devices | revoke <id> | status | stop [--data-dir DIR]   # 動いているヘッドレスへ (<データディレクトリ>/yhtye.sock)
env -u DISPLAY -u WAYLAND_DISPLAY target/release/yhtye --headless --data-dir /tmp/yhtye-headless   # 実機での確認の例
```

- 確認のしかた: 上のように DISPLAY を外して起動し、別の端末で `yhtye remote status` / `pair` が答え、SIGTERM で「all agents stopped」を出して終了コード 0 になること。
  フラグ無しで DISPLAY を外すと案内を出して終了コード 1。Tailscale の無い環境では、起動時に「remote access is not running: … (trying again every 30 s)」が出る (これが再試行の確認になる)。
- `-d` の確認: 起動した側が戻り (終了コード 0)、デーモンが新しいセッションのリーダーとして残り、`remote status` が PID を答え、2 つ目の `-d` が終了コード 3、
  `remote stop` で終わって制御ソケットが消えること (`src-tauri/tests/headless_daemon.rs` が本物のバイナリで確かめる)。端末を閉じる確認は、`script` などの疑似端末から `-d` を起動して端末側を落とす。
- ログは stderr、stdout は人間向けの表示 (端末のときだけペアリングの案内)。`RUST_LOG` で調整する。デーモンではどちらもログファイルへ行く。
- テスト: 引数 (`cli.rs`)、データディレクトリの規則 (`data_dir.rs`)、制御口のプロトコルとクライアント (`headless/control.rs`)、デーモンの起動 (`headless/daemon.rs`)、
  表示 (`headless/report.rs`)、リモートの再試行 (`headless/mod.rs`)、本物のバイナリでのデーモン (`tests/headless_daemon.rs`) は `cargo test -p yhtye`。コアの `Ping` が `headless` を返すことは `crates/yhtye-core/tests/core_facade.rs`。

### 別のホストへの接続 (PC の GUI、T-97)

PC の GUI が別の Yhtye (別の PC、`--headless` のホスト) に接続する。設計は [`core-design.md`](architecture/core-design.md) §9「PC のクライアント」・§11・§18.10。
ブラウザ (`pnpm dev:browser`) では使えない (Tauri のシェルが WS を張る)。

```sh
# ホスト (1 台目): ウィンドウ無しで起動し、ペアリングコードを出す (Tailscale が要る)
yhtye --headless --data-dir /tmp/yhtye-host && yhtye remote pair --data-dir /tmp/yhtye-host    # → yhtye://pair?… と QR
# クライアント (2 台目): GUI を起動して、出た URI を引数に別のプロセスとして開く (OS がリンクのクリックでする起動と同じ)
yhtye 'yhtye://pair?v=1&host=…&ip=100.x.y.z&port=1423&code=ABCD2345&sid=…'    # 動いている GUI があればそこへ渡る
```

- リンクを開くと設定が「接続先」で開き、リンクが入った状態になる (行き先のホストとポート・端末名が出る)。「接続」を押すまでペアリングしない (T-98。T-97 の確認ダイアログは
  この節に置き換えた)。ペアリングを済ませると、プロジェクトの一覧にホストのプロジェクトがこの PC のものの後ろに並び、名前の右にリモートのアイコンが付く
  (同じ名前があるときだけホスト名も添える)。ホスト側の `yhtye remote devices` に `desktop` として載る。手入力 (ホスト・ポート・コード) も同じ節。
- **試すときは環境を分ける**: `XDG_DATA_HOME` / `XDG_CONFIG_HOME` / `XDG_CACHE_HOME` / `XDG_RUNTIME_DIR` / `HOME` を一時ディレクトリにして `dbus-run-session` の中で動かすと、
  普段のアプリのデータ・キーリング・単一インスタンスの D-Bus 名・`yhtye://` の既定のハンドラ (debug ビルドは `register_all()` が `xdg-mime default` を実行する) に触れない。
  画面が無ければ Xvfb (`Xvfb :98 -screen 0 1440x900x24`、`DISPLAY=:98`、`WEBKIT_DISABLE_DMABUF_RENDERER=1 LIBGL_ALWAYS_SOFTWARE=1`)。
  キーリングを試すなら同じバスの中で `echo -n pw | gnome-keyring-daemon --login` (無ければトークンはそのプロセスの間だけで、再起動後は自動接続しない)。
  1 台でホストを用意するなら `cargo run -p yhtye-core --example remote_server -- --port 14231` (§18.6。標準入力の `open <path>` / `code` / `devices`)。
- テスト: Tauri 層 (`src-tauri/src/remote_hosts/tests.rs`: エラーの対応・イベントと通知の JSON・ペアリングの入力・起動時の自動接続、`deep_link.rs`: リンクの待ち行列と
  設定ファイル、`cli.rs`: リンクの引数)、WebView の `src/api/tauri.test.ts` (ホストごとの送信とイベントの振り分け・接続状態)、`src/api/remoteHosts.test.ts`、
  `src/store/hosts.test.ts` (登録簿)、`src/store/appHosts.test.ts` (キーの前置・ホストの版)、`src/ui/Hosts.test.tsx` (プロジェクトの選択とリモートのアイコン・
  接続の帯・フッタ)、`src/ui/HostsSettings.test.tsx` (設定の「接続先」・リンクで開く流れ・リモートのホストが出ているときの「リモート接続」)、`src/ui/hostLabels.test.ts` (文言)。
  Rust の `the_messages_the_screen_tells_apart_start_as_it_expects` は、UI がペアリングの失敗の文言を選ぶ手がかり (英語のメッセージの先頭) を固定する。

### ブラウザで動かす (開発・E2E 用)

素の Chrome には Tauri の IPC が無いので、同じ API を WebSocket で出す開発用ブリッジ
`yhtye-dev-bridge` を使う ([`core-design.md`](architecture/core-design.md) §10)。

```sh
pnpm dev:browser                              # ブリッジ (ws://127.0.0.1:1422/ws) + Vite (1420)
pnpm dev:browser --data-dir /tmp/yhtye-dev    # 以降の引数はブリッジへ (--model / --token など)
pnpm dev:browser --port-base 15420            # Vite 15420 + ブリッジ 15422 (稼働中のアプリと並べる)
```

- **ポートの指定**: 既定は Vite 1420 / ブリッジ 1422 (`pnpm tauri dev` も 1420 を使う)。`pnpm tauri dev` や別の
  `pnpm dev:browser` と並べて動かすときは変える。コマンドラインの指定が環境変数に勝つ。

  | コマンドライン | 環境変数 | 意味 |
  |---|---|---|
  | `--vite-port N` | `YHTYE_VITE_PORT` | Vite のポート |
  | `--bridge-port N` (ブリッジ自身の `--port N` も同じ) | `YHTYE_BRIDGE_PORT` | ブリッジのポート |
  | `--port-base N` | `YHTYE_PORT_BASE` | Vite を N、ブリッジを N+2 に (上の 2 つを個別に指定すればそちらが勝つ) |

  ```sh
  pnpm dev:browser --vite-port 15420 --bridge-port 15422 --data-dir /tmp/yhtye-dev   # → http://localhost:15420
  YHTYE_PORT_BASE=15420 pnpm dev:browser
  ```

  スクリプトがブリッジの `--port` と、そのページの Origin (`--allow-origin http://localhost:N` /
  `http://127.0.0.1:N`)、Vite の `--port` (`--strictPort`) と、ページがつなぐ WebSocket の URL
  (`VITE_YHTYE_BRIDGE_URL`) を揃えて渡す。ポートが使用中なら、ビルドの前にどのポートかを示して終了する。
  `pnpm tauri dev` が使う `vite.config.ts` の 1420 と `tauri.conf.json` の `devUrl` は変わらない。

- ブリッジは 127.0.0.1 のみで待ち受け、`Origin` が `http://localhost:1420` / `http://127.0.0.1:1420`
  (Vite のポートを変えたときはそのポート。ブラウザからの接続) で、`?token=` が一致する接続だけ受け付ける。`pnpm dev:browser` はランダムな
  トークンを作ってブリッジ (`YHTYE_BRIDGE_TOKEN`) と Vite (`VITE_YHTYE_BRIDGE_TOKEN`) の両方に渡す。
- ブリッジのデータは既定で `$XDG_DATA_HOME/yhtye-dev-bridge` (アプリとは別)。
- 別々に起動する場合: `pnpm bridge -- --token T` と `VITE_YHTYE_BRIDGE_TOKEN=T pnpm dev`
  (ブリッジのポートを変えたなら `VITE_YHTYE_BRIDGE_URL=ws://127.0.0.1:<ポート>/ws` も、
  Vite のポートを変えたならブリッジに `--allow-origin http://localhost:<ポート>` も要る)。
- **dev ブリッジは単一ユーザーのマシン専用。** トークンは Vite のバンドルに埋め込まれるため、同じマシンの
  他のユーザーは `localhost:1420` から読めてしまう (配布物には含まれない。Stage 7a でこの前提で決定)。

### Wayland + NVIDIA で起動直後に落ちる場合

WebKitGTK の DMA-BUF レンダラが NVIDIA プロプライエタリドライバと噛み合わず、
`Gdk-Message: Error 71 (プロトコルエラー) dispatching to Wayland display` を出して
即クラッシュすることがある。

`src-tauri/.cargo/config.toml` で `WEBKIT_DISABLE_DMABUF_RENDERER=1` を設定済みなので
`pnpm tauri dev` では対処されている。古いビルドなどで必要なら自分で渡す。

```sh
WEBKIT_DISABLE_DMABUF_RENDERER=1 ./target/debug/yhtye
```

アプリ本体も起動時に Wayland + NVIDIA を検出したとき (かつ未設定のとき) だけ自分で設定するので
(`src-tauri/src/webkit_env.rs`、[`core-design.md`](architecture/core-design.md) §9)、
ビルド済みバイナリでも通常は不要。変数が既に設定されていれば (値にかかわらず) アプリは触らない。

## ビルド・検証

```sh
pnpm build                                         # tsc + vite build
pnpm test                                          # Vitest
pnpm check:layout                                  # 実ブラウザで「ページ全体がスクロールしない」ことを検査
cargo test --workspace                             # Rust (偽エージェント・一時 git リポジトリ)
cargo test -p yhtye-core -- --ignored --test-threads=1   # 実エージェント (Claude Code は Haiku)
cargo clippy --workspace --all-targets && cargo fmt --check
pnpm gen:types                                     # Rust の API 型から TS (src/api/generated) と Kotlin (mobile/…/api/generated) を再生成
pnpm check:types                                   # 生成物が Rust の API 型と一致するか (cargo test にも含まれる)
pnpm gen:kotlin / pnpm check:kotlin                # Kotlin の型と契約サンプルだけ
pnpm tauri build --debug --no-bundle               # アプリのデバッグビルド (target/debug/yhtye)
pnpm tauri build                                   # 配布用バイナリ
```

### レイアウトの検査 (`pnpm check:layout`)

Yhtye は画面全体を 1 枚の固定レイアウト (`.app`) で使い、スクロールするのは各パネルの内側だけ。
html / body / `#root` は `height: 100%` + `overflow: hidden` で固定してあるが、それは最後の砦にすぎない。
はみ出す要素がそもそも無いことを、次の 2 つで守る。

- **`src/ui/pageFrame.test.ts` (Vitest)**: jsdom はレイアウトを計算しないので、CSS の規則だけを見る
  (html / body / `#root` の高さと overflow、`.app` の位置と `min-width` が無いこと、`min-height: 0` / `min-width: 0`、
  スクロール領域がすべて `position` を持つこと)。
- **`pnpm check:layout` (`scripts/check-layout.mjs`)**: playwright-core + Chromium (headless) で、
  `layout-harness.html` (本物の `App` をメモリ上のコアで動かす開発専用ページ。`src/test/layoutHarness.tsx`) を
  画面の状態ごとに開き、`document.scrollingElement` の `scrollHeight <= innerHeight`、`scrollWidth <= innerWidth`、
  `scrollX / scrollY == 0`、枠 (`.app` `.body` `.sidebar` `.conversation` `.right` など) の中身がはみ出していないこと、
  ポップアップ (⋯ メニュー・プロジェクトのプルダウン・モデル一覧) がウィンドウ内に収まっていることを確かめる。
  状態は、プロジェクト無し・記録した実行・大量の会話/タスク/ブランチ・ホイール操作・各ポップアップ・設定・
  エラーバナー・パネルの限界までのリサイズ・Tab で全部の部品に触れる・別のホストを持つ状態 (`hosts`: 長い名前・失効・再接続待ちのホスト。
  プルダウン・リモートのプロジェクトを開いた画面・設定の「接続先」・リンクを受けた画面)・下部パネルに 8 個のタスクタブを開いた状態
  (タブ列が横スクロールして全部に届くこと、git の見出しやセッションの選択が潰れないこと) を 1060x600 から 1920x1080 まで。
  Rust のコアは要らない (Vite だけを空いているポートで起動する)。画面を足す・直すときは実行しておく。

```sh
pnpm check:layout
YHTYE_CHROMIUM=/usr/bin/google-chrome pnpm check:layout   # ブラウザの指定 (既定: /usr/bin/chromium など。無ければ Playwright の)
YHTYE_LAYOUT_URL=http://localhost:1420 pnpm check:layout  # 起動中の Vite を使う (既定: 空きポートで新しく起動)
YHTYE_LAYOUT_SHOTS=/tmp/yhtye-shots pnpm check:layout     # 各状態のスクリーンショットも保存する
YHTYE_LAYOUT_ONLY="task tabs" pnpm check:layout           # 名前に文字列を含む状態だけ実行する
```

失敗すると、どの状態で何がはみ出したか (例: `span.sr-only (absolute, in the page) down to y=113505`) を出す。
新しい状態を足すには、`scripts/check-layout.mjs` の `STATES` に 1 行足す (はみ出し以外の検査は `verify` に書く)。

CI は [`.github/workflows/ci.yml`](../.github/workflows/ci.yml) (フロントエンドのビルドとテスト、
`pnpm check:layout` (`layout` ジョブ)、Rust の fmt / clippy / テスト。実エージェントを使う `#[ignore]` のテストは含まない)、
Android クライアントの `./gradlew build` (`android` ジョブ。ユニットテスト・lint・debug/release のビルド。署名鍵は使わず release は未署名、
本物の PC 側サーバーが要るテストは飛ばす。失敗したときはレポートをアーティファクト `android-reports` に残す (7 日))。
`layout` ジョブは ubuntu-latest に入っている Google Chrome を `YHTYE_CHROMIUM=/usr/bin/google-chrome` で使う
(playwright-core は CDP で動かすだけなので、`playwright install` のダウンロードと apt のパッケージが要らない)。
ヘッドレスでディスプレイは無く、Vite のポートは空きから自動で選ぶ。失敗したときは全状態のスクリーンショットを
アーティファクト `layout-shots` に残す (7 日)。手元で CI と同じ条件にするには
`env -u DISPLAY -u WAYLAND_DISPLAY CI=1 YHTYE_LAYOUT_SHOTS=/tmp/yhtye-shots pnpm check:layout`。

## Android クライアント (`mobile/`)

Tailscale 経由で PC の Yhtye を操作する Android アプリ。設計は
[`planned/android-client.md`](planned/android-client.md)。リポジトリ直下の cargo / pnpm とは独立した Gradle プロジェクト。

| モジュール | 中身 |
|---|---|
| `mobile/shared` | Kotlin Multiplatform (今は Android ターゲットのみ)。通信・認証・データモデルを置く。`commonMain` に Android 依存を入れない (将来 iOS を足せるように) |
| `mobile/androidApp` | Jetpack Compose (Material 3) の画面と、Android 固有の部分 (Keystore・カメラ・通知) |

前提は JDK 17 と Android SDK (compileSdk 37 の `platforms;android-37.0`)。SDK の場所は `mobile/local.properties`
(コミットしない) に書く。`ANDROID_HOME` が設定されていれば不要。

```sh
echo "sdk.dir=$HOME/Android/Sdk" > mobile/local.properties
~/Android/Sdk/cmdline-tools/latest/bin/sdkmanager "platforms;android-37.0" "build-tools;37.0.0"   # 足りなければ
```

```sh
cd mobile
./gradlew build                          # 全部 (ユニットテスト・lint・debug/release の APK)
./gradlew :androidApp:assembleDebug      # デバッグ APK → androidApp/build/outputs/apk/debug/androidApp-debug.apk
./gradlew :androidApp:installDebug       # adb でつないだ実機・エミュレータに入れる
./gradlew :shared:allTests               # 共通モジュールのユニットテスト (JVM 上で実行)
./gradlew :androidApp:testDebugUnitTest  # 画面のテスト (Compose UI テストを Robolectric で JVM 上で実行)
./gradlew :androidApp:lint               # Android lint
```

画面のテスト (`androidApp/src/test/`) は Robolectric で動かす (実機・エミュレータ不要)。Robolectric の Android の版は
`src/test/resources/robolectric.properties` の `sdk=34` (35 以上は JDK 21 が要る)。同期層は本物の `AppStore` を偽のコア
(`TestCore`。PC の vitest と同じ記録 `src/test/fixtures/fake-run.json` を読む) の上で動かし、`Dispatchers.Main` の
コルーチンは `settle` で main looper を回して進める (Compose の `waitUntil` だけでは流れないことがある)。
ダイアログ (別ウィンドウ) の中の入力欄に `performTextInput` すると main looper が止まらなくなるので、入力欄はダイアログに入れていない。

### API 型 (Rust から生成)

`mobile/shared/src/commonMain/kotlin/io/github/tom039224/yhtye/shared/api/generated/` は Rust の API 型
(`ApiCommand` / `ApiResponse` / `ApiEvent` / `ApiError` / `WsRequest` / `WsServerMessage` / `/pair` の型など) から生成する。
手で編集せず、Rust の型を変えたら `pnpm gen:types` (TS と Kotlin) か `pnpm gen:kotlin` を実行してコミットする
(`cargo test` / `pnpm check:types` が生成物のズレで失敗する)。仕組みは
[`architecture/core-design.md`](architecture/core-design.md) §2。

- 読み書きは必ず `ApiJson` (`shared/api/ApiJson.kt`) で行う。タグ付きの型は `ApiCommand.Serializer` のような
  明示した serializer を持つ (`ApiJson.encodeToString(ApiCommand.Serializer, cmd)`。プロパティの型としてなら自動で使われる)。
- PC 側が新しくて知らない variant・enum 値は `Unrecognized` になる (例外にならない)。`when` では必ず扱う。
- 契約テスト: Rust が書いた全 variant の JSON (`commonTest/.../api/generated/ContractSamples.kt`、同じく生成) を
  `ContractTest` が読んで書き戻し、同じ JSON になることを確かめる (`./gradlew :shared:allTests`)。
  Rust の型に variant を足したら `crates/yhtye-core/src/api/typegen/kotlin/samples.rs` にサンプルを足す (無いと Rust のテストが失敗する)。

依存のバージョンは `mobile/gradle/libs.versions.toml` にまとめてある。AGP 9 から Kotlin が AGP に内蔵されたので、
`androidApp` には `org.jetbrains.kotlin.android` を適用しない。共通モジュールの Android ターゲットは
旧来の `androidTarget()` (`com.android.library`) ではなく `com.android.kotlin.multiplatform.library` の
`kotlin { android { ... } }` で宣言する (ユニットテストは `androidHostTest`、`commonTest` もそこで走る)。

### 本物の PC 側サーバーとの結合テスト

`shared` の `RealServerTest` (JVM のホストテスト) は、`remote_server` の example ([`architecture/core-design.md`](architecture/core-design.md) §18.6) を起動して本物の Tailscale 越しに
ペアリング → WS で `Ping` / `ListProjects` → 失効 → `Revoked` を通す。この PC が Tailscale につながっていること
(`tailscale ip -4`)。通常の `./gradlew build` では飛ばされる。

```sh
cd mobile
YHTYE_REAL_SERVER_TEST=1 ./gradlew :shared:testAndroidHostTest --tests '*RealServerTest*'   # ポートは YHTYE_REAL_SERVER_PORT (既定 14230)
```

同期層 (`shared/sync/`、T-66) の `SyncRealServerTest` も同じ環境変数で動く (上のコマンドの `*RealServerTest*` に含まれる)。
`cargo build -p yhtye-fake-agent` した偽エージェントを `remote_server --fake-agent` に渡し、2 つの端末をペアリングして、
プロジェクトを開く → チャット作成 → 送信 → グループ・タスク → マージ → オーケストレータの報告 (ユーザーの番) を両方の同期層の状態で確かめる。
ポートは `YHTYE_SYNC_SERVER_PORT` (既定 14231)。

### 実機での動作確認

前提: 実機が USB デバッグで `adb devices` に出ていて、Tailscale アプリで PC と同じ tailnet (同じユーザー) に入っていること。

1. GUI 無しでサーバーを起動し、プロジェクトを開いてペアリングコードを出す (デスクトップアプリや `pnpm dev:browser` の
   「設定 → リモート接続」でもよい)。データは一時ディレクトリ (アプリの DB は触らない)。

   ```sh
   git init -q /tmp/yhtye-demo-project
   cargo run -p yhtye-core --example remote_server -- --port 1423
   # 標準入力で:
   open /tmp/yhtye-demo-project
   code            # → {"type":"pairing_code","pairing":{"code":"…","uri":"yhtye://pair?…"}}
   ```

2. アプリを入れ、ディープリンクでペアリング画面を開く (`&` を含むので URI は引用符で囲み、`adb shell` にもう一段の引用符で渡す)。

   ```sh
   cd mobile && ./gradlew :androidApp:installDebug
   adb shell am start -a android.intent.action.VIEW -d "'yhtye://pair?v=1&host=…&ip=100.x.y.z&port=1423&code=ABCD2345&sid=…'"
   ```

   端末名を確かめて「ペアリングする」→ ホームに接続状態とプロジェクト一覧が出る。

3. 失効: サーバーの標準入力で `devices` → `revoke <端末 ID>`。アプリは「この端末は PC から外されました」の案内付きで
   ペアリングの始めの画面に戻る。
4. 手入力: 「手入力する」でホスト (`tailscale ip -4` の 100.x か MagicDNS 名)・ポート・コード (`code` で新しく出す) を入れる。
   `adb shell input tap <x> <y>` / `adb shell input text <文字列>` でも操作できる (座標は `adb shell uiautomator dump` で調べる)。
5. QR: PC の「設定 → リモート接続」に出る QR (example サーバーなら `code` の `uri` を `qrencode -t ansiutf8 '<uri>'` で
   端末に表示) を、アプリの「QR コードを読む」で映す。初回はカメラの許可を求められる。
6. スクリーンショットは `adb exec-out screencap -p > shot.png` (画面が消えていると真っ黒になる。`adb shell input keyevent KEYCODE_WAKEUP`
   と `adb shell wm dismiss-keyguard` で起こし、`adb shell svc power stayon usb` で消えないようにする。終わったら `stayon false`)。
   `adb shell input text` は端末の IME を通るので、日本語 IME だと全角や仮名になることがある (アプリは全角の英数字を半角として読む)。
   会話の入力では Gboard を英字にしてから送る (キーボードの「あa1」をタップ。空白は `%s`)。
   会話の確認には偽エージェントを使う (`cargo build -p yhtye-fake-agent` して `--fake-agent target/debug/yhtye-fake-agent
   --fake-orchestrator '<シナリオの JSON>'`。形式は `crates/yhtye-fake-agent/src/scenario.rs`。`match` で発言ごとに返事を変えられ、
   `message` を `sleep` で区切るとストリーミングになり、`wait_cancel` で中止ボタンを試せる)。終わったら `quit` でサーバーを止める (一時 DB ごと消える)。
7. 常駐 (Foreground Service) と通知の確認 (T-71。結果は [`planned/android-client.md`](planned/android-client.md) の「実装で決めたこと (T-71)」):
   - 偽エージェントのシナリオに `{"match":"slow","actions":[{"message":"started"},{"sleep":75000},{"message":"done\nmore"},{"end":"end_turn"}]}` のような長いターンを入れ
     (`--fake-agent` / `--fake-orchestrator` には**絶対パス**を渡す。相対パスだとモデル一覧の探索が失敗する)、そのチャットから別のチャットに切り替える・ホームに戻る・画面を消すなどして完了を待つ。
   - サービスと通知の状態は `adb shell dumpsys activity services io.github.tom039224.yhtye` (`isForeground=true`、`types=0x40000000` = specialUse) と
     `adb shell dumpsys notification --noredact` (自分のパッケージの行だけを見る。端末の他のアプリの通知が混ざる)。ロック画面の通知は `adb shell input keyevent KEYCODE_SLEEP` →
     `KEYCODE_WAKEUP` のあと `uiautomator dump` で探してタップできる (通知シェードは動くものがあると `uiautomator` が「idle にならない」と失敗する)。`uiautomator dump` も
     アニメーションが続く画面では失敗して古い /sdcard/ui.xml が残るので、前に消してから取る。
   - Doze: `adb shell dumpsys battery unplug` と `adb shell dumpsys deviceidle force-idle` (深い Doze を強制)、終わったら `dumpsys deviceidle unforce` と `dumpsys battery reset`。
   - 設定が PC に反映されたかは、もう 1 つのクライアント (別の端末としてペアリングした WebSocket クライアント。Python の `websockets` で `get_agent_settings` / `list_secret_env` を送る) で見られる。
   - `BOOT_COMPLETED` はシェルから送れない (保護された放送)。同じ除外を受ける `adb install -r` (= `MY_PACKAGE_REPLACED`) で、アプリを開かずにサービスが始まることを確かめられる。

### リリース (GitHub Releases に署名済み APK を出す)

Android アプリは GitHub Releases に署名済みの APK を添付して配る (Play / F-Droid は使わない)。利用者向けの説明 (インストール・版番号と互換性の規則・アプリ内の更新)
は [`README.md`](../README.md) の「Android 版」。決めた内容の記録は [`planned/android-client.md`](planned/android-client.md) の「リリースと更新 (T-84)」。

#### 版番号

- **タグ**: `android-v<major>.<minor>.<patch>` (例 `android-v0.1.0`)。デスクトップ版のリリースとは別。数字だけで、先頭の 0 は付けない。マイナーとパッチは 0〜99。
- **`versionName`** はタグから (`android-v0.1.0` → `0.1.0`)。Gradle のプロパティ `yhtye.versionName` で渡す (`-Pyhtye.versionName=0.1.0`)。
  渡さなければ `mobile/gradle.properties` の開発用の既定値 (`yhtye.versionName`)。形が違えばビルドが失敗する。
- **`versionCode`** = `major*10000 + minor*100 + patch` (`0.1.0` → `100`、`1.2.3` → `10203`)。マイナーかパッチが 0〜99 の外ならビルドが失敗する。
  上書きの更新は `versionCode` が増える必要があるので、版は必ず上げる。
- アプリは `BuildConfig.VERSION_NAME` で自分の版を読む (`ThisApp.version`)。
- **PC の版**はワークスペースの版 (`Cargo.toml` の `[workspace.package] version`)。`src-tauri` は `version.workspace = true`、`tauri.conf.json` と `package.json` は同じ値を手で合わせ、
  `crates/yhtye-core/tests/app_version.rs` が一致を見る。PC の版を上げるときは 3 か所を一緒に変える。`ping` の応答 (`pong{host, version}`) が PC の版をアプリに伝える。
- **互換性の規則**は `mobile/shared/.../shared/version/Compatibility.kt` (0.x は `0.<minor>` が同じ、1.0 以降はメジャーが同じで PC のマイナー ≥ スマホのマイナー、パッチは見ない)。
  規則を変えたら README の表とテスト (`CompatibilityTest`) も直す。

#### 出し方

1. 版を決める。互換性が変わる変更を含むとき (0.x ならマイナー、1.0 以降は規則に従って) は、PC の版との組み合わせも考える。
2. 出したいコミット (通常は `main` の先頭) にタグを打って push する。

   ```sh
   git tag android-v0.1.0
   git push origin android-v0.1.0
   ```

3. `.github/workflows/android-release.yml` が動く。流れ: タグの形式を検査 → JDK 17 と Android SDK → Secrets から keystore を一時ファイルに復元 →
   `./gradlew :androidApp:assembleRelease -Pyhtye.versionName=<版>` → `apksigner verify` で署名を、`aapt2 dump badging` で `versionName` / `versionCode` を確認 →
   `yhtye-android-<版>.apk` を GitHub Release に添付 (リリース名・本文に「PC の Yhtye 0.1.x と互換」のように互換する PC の版を書く。版が `0.` で始まる間は**プレリリース**。
   Latest にはしない) → keystore の一時ファイルを消す。Actions の画面で成功を確かめ、リリースのページから APK を落として確かめる。
4. 間違えたとき: リリースとタグを消して (`gh release delete android-v0.1.0 --cleanup-tag`)、直してから打ち直す。**配ったあとの版を作り直さない**
   (同じ版の別の APK を出すと、入れた端末は更新と見なさない)。直すなら次のパッチを出す。

ワークフローはタグの push だけで動く (`ci.yml` とは別)。パスワードは Gradle を実行する step の `env` にだけ渡し (Secrets の値はログでマスクされる)、Gradle は設定キャッシュとビルドキャッシュを使わず実行する。keystore の一時ファイルは job の最後に必ず消す。

#### 署名の鍵

- 鍵 (PKCS12 の keystore) は**リポジトリに入れない**。リポジトリの Secrets に次の 4 つを登録してある。

  | Secret | 中身 |
  |---|---|
  | `ANDROID_KEYSTORE_BASE64` | keystore ファイルの base64 (`base64 -w0 yhtye-release.p12`) |
  | `ANDROID_KEYSTORE_PASSWORD` | keystore のパスワード |
  | `ANDROID_KEY_ALIAS` | 鍵の別名 (`yhtye`) |
  | `ANDROID_KEY_PASSWORD` | 鍵のパスワード |

- Gradle は署名の設定を環境変数から読む。**4 つとも**あれば release を署名し、**どれも無ければ**未署名のまま (ローカルの `./gradlew build` はこの状態)。
  一部だけ、またはファイルが無いときは間違いとして失敗する。debug ビルドは今までどおり debug 鍵。

  | 環境変数 | 中身 |
  |---|---|
  | `YHTYE_KEYSTORE_FILE` | keystore ファイルのパス |
  | `YHTYE_KEYSTORE_PASSWORD` | keystore のパスワード |
  | `YHTYE_KEY_ALIAS` | 鍵の別名 |
  | `YHTYE_KEY_PASSWORD` | 鍵のパスワード |

- **鍵を失くすと、入れてある端末に上書きの更新ができなくなる** (別の鍵で署名した APK は、同じアプリとして上書きできない。アンインストール、つまりペアリングなどのデータの消去が要る)。
  keystore とパスワードは Secrets とは別の安全な場所 (パスワードマネージャなど) にもバックアップしておく。署名の証明書の SHA-256 も控えておくと、
  配った APK の鍵が変わっていないことを確かめられる (`apksigner verify --print-certs yhtye-android-0.1.0.apk`)。鍵が漏れたときは新しい鍵に替える必要があり、その場合も利用者は入れ直しになる。
- 鍵の作り方 (最初の 1 回だけ。有効期間は長く取る):

  ```sh
  keytool -genkeypair -keystore yhtye-release.p12 -storetype PKCS12 -alias yhtye -keyalg RSA -keysize 4096 -validity 36500
  base64 -w0 yhtye-release.p12 | gh secret set ANDROID_KEYSTORE_BASE64
  gh secret set ANDROID_KEYSTORE_PASSWORD
  gh secret set ANDROID_KEY_ALIAS --body yhtye
  gh secret set ANDROID_KEY_PASSWORD
  ```

  PKCS12 の keystore では、`keytool` はキーのパスワードを keystore のパスワードに揃える (通常 2 つの Secrets は同じ値になる)。

#### ローカルで署名付きビルドを試す

ワークフローと同じ手順を、使い捨ての keystore で確かめられる (本物の鍵は使わない。作った keystore はコミットせず、終わったら消す)。

```sh
cd mobile
keytool -genkeypair -keystore /tmp/test.p12 -storetype PKCS12 -alias yhtye -keyalg RSA -keysize 2048 -validity 30 \
  -storepass testpass123 -keypass testpass123 -dname "CN=Test"
YHTYE_KEYSTORE_FILE=/tmp/test.p12 YHTYE_KEYSTORE_PASSWORD=testpass123 YHTYE_KEY_ALIAS=yhtye YHTYE_KEY_PASSWORD=testpass123 \
  ./gradlew --no-configuration-cache --no-build-cache :androidApp:assembleRelease -Pyhtye.versionName=0.1.0
$ANDROID_HOME/build-tools/37.0.0/apksigner verify --print-certs androidApp/build/outputs/apk/release/androidApp-release.apk
$ANDROID_HOME/build-tools/37.0.0/aapt2 dump badging androidApp/build/outputs/apk/release/androidApp-release.apk | grep '^package:'   # versionCode='100' versionName='0.1.0'
rm /tmp/test.p12
```

環境変数が無いと APK は `androidApp-release-unsigned.apk` になる (署名されていない)。ワークフローは署名済みの `*-release.apk` がちょうど 1 つ無ければ失敗する。

#### アプリ内の更新の確認

アプリは `GET https://api.github.com/repos/Tom039224/yhtye/releases` (`shared/.../update/Releases.kt` の定数) を読み、`android-v*` のタグのリリースのうち自分より新しい版
(Semantic Versioning。GitHub がプレリリースとした物も含む) があれば知らせる。リポジトリの名前や所有者を変えるときは、この定数 (`RELEASES_URL` / `RELEASE_PAGE_URL`) と
README のリンクを直す。URL は `https://` だけを受け付ける (アプリは PC との通信のために平文を許すので、更新の確認は別の歯止めを持つ)。
ネットワークの制限 (`TailscaleHosts`: PC との平文の接続先を Tailscale のアドレスに限る) は更新の確認とは無関係で、そのまま。
