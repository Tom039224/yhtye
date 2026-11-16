# Windows 11 検証用 VM (QEMU/KVM)

Linux ホストの QEMU/KVM 上に Windows 11 Enterprise 評価版 (90 日) の VM を無人インストールし、SSH・スクリーンショット・VNC で
Windows 版 Yhtye を実機確認するためのスクリプト。ISO・ディスク・鍵・パスワードは `~/VMs/win11` (環境変数 `WIN11_VM_DIR`) に置き、
リポジトリには入れない。

## 構成

| 項目 | 値 |
| --- | --- |
| CPU / メモリ / ディスク | KVM (`-cpu host`)、8 vCPU、16 GB、qcow2 128 GB (シン) |
| ファームウェア | UEFI (OVMF `OVMF_CODE.4m.fd`、Secure Boot なし) + swtpm の TPM 2.0 |
| デバイス | SATA (AHCI) ディスク、e1000e NIC、VGA 1600x900、USB タブレット (ドライバの追加は不要) |
| ネットワーク | QEMU user-mode。SSH は 127.0.0.1:2222 → 22 |
| 画面 | VNC 127.0.0.1:5901 (パスワード無し。localhost のみ)、QMP `~/VMs/win11/qmp.sock` で screendump |
| ユーザー | `yhtye` (管理者、自動ログオン、SSH 用) と `日本語ユーザー` (管理者、非 ASCII のユーザー名の検証用)。パスワードは `~/VMs/win11/README.md` |
| SSH | OpenSSH Server (自動起動)、公開鍵認証のみ (`~/.ssh/yhtye_win11_vm_ed25519`)、既定のシェルは Windows PowerShell |
| 開発ツール | Git for Windows、Node.js 22 LTS (npx)、pnpm (`install-tools.ps1`) |

## 必要なもの (ホスト)

`qemu-system-x86_64`、`qemu-img`、OVMF (`edk2-ovmf`)、`swtpm`、`python3`、`/dev/kvm` への書き込み権。
無人インストール用の小さな ISO は `pycdlib` で作る (`uv` があれば `uv run --with pycdlib` で自動。無ければ `pip install --user pycdlib`)。
xorriso / mkisofs は要らない。

### swtpm / qemu-img が動かないとき

部分更新などで、ディストリの `swtpm` が入っていない `libcrypto.so.4` を要求することがある (`swtpm --version` が
`libcrypto.so.4: cannot open shared object file` で落ちる)。sudo を使わずに済ませるには、同じリポジトリの `openssl` (4.x) と
`qemu-img` のパッケージを取ってきて `~/VMs/win11/tools` に展開する。スクリプトは `tools/lib` を `LD_LIBRARY_PATH` に、
`tools/bin` を優先して使う。

```sh
# <版> は `pacman -Sl core | grep ' openssl '` と `pacman -Sl extra | grep ' qemu-img '` で見る
mkdir -p ~/VMs/win11/tools/bin ~/VMs/win11/tools/lib /tmp/x && cd /tmp
curl -fLO https://geo.mirror.pkgbuild.com/core/os/x86_64/openssl-<版>-x86_64.pkg.tar.zst
curl -fLO https://geo.mirror.pkgbuild.com/extra/os/x86_64/qemu-img-<版>-x86_64.pkg.tar.zst
bsdtar -xf openssl-*.pkg.tar.zst -C /tmp/x usr/lib/libcrypto.so.4 usr/lib/libssl.so.4
bsdtar -xf qemu-img-*.pkg.tar.zst -C /tmp/x usr/bin/qemu-img
cp /tmp/x/usr/lib/lib*.so.4 ~/VMs/win11/tools/lib/ && cp /tmp/x/usr/bin/qemu-img ~/VMs/win11/tools/bin/
```

## 作り方

```sh
scripts/windows-vm/download-iso.sh          # Evaluation Center から日本語版 ISO (約 8 GB)。英語版は download-iso.sh en-US
scripts/windows-vm/prepare.sh               # 鍵・パスワード・unattend.iso・qcow2・UEFI 変数・~/VMs/win11/README.md
scripts/windows-vm/vm.sh install            # ISO から起動して無人インストール (20〜30 分、数回再起動)
scripts/windows-vm/vm.sh wait-ssh           # 初回ログオンの設定 (OpenSSH Server) が終わって SSH が通るまで待つ
scripts/windows-vm/vm.sh ssh 'powershell -NoProfile -ExecutionPolicy Bypass -File C:\ProgramData\yhtye-vm\install-tools.ps1'
```

- `vm.sh install` は起動直後に Enter を 20 秒送り続けて「Press any key to boot from CD or DVD」を越える。インストール中の再起動では
  キーを送らないので、そのままディスクから起動する。
- `autounattend.xml.in` はディスク 0 (SATA ディスク) を消して GPT で切る。ISO と `unattend.iso` は CD-ROM なのでディスクに数えられない。
- 初回ログオンで `unattend.iso` の `yhtye-vm\firstlogon.ps1` が動き、OpenSSH Server を入れて (Windows Update の機能、駄目なら
  GitHub の Win32-OpenSSH の MSI) 公開鍵を `C:\ProgramData\ssh\administrators_authorized_keys` に置き、パスワード認証を切る。
  スリープ・画面オフ・ロック画面・ログオン中の自動再起動も止める。`日本語ユーザー` もここで作る (autounattend.xml の
  `LocalAccounts` に日本語の名前を書くと、Setup が `???????` に化かして 0x8007089a で失敗するため)。
  ログは `C:\ProgramData\yhtye-vm\firstlogon.log`。
- インストールが「インストールを完了できませんでした」で止まったら、`vm.sh kill` してディスクの `C:\Windows\Panther\UnattendGC\setuperr.log`
  を見る。`qemu-img convert --image-opts "driver=raw,offset=<C: の開始バイト>,size=<大きさ>,file.driver=qcow2,file.file.filename=disk.qcow2" -O raw c.ntfs`
  で C: だけを取り出し (`sfdisk -d` で位置を見る。生のディスク全体は 128 GB になる)、`7z x c.ntfs 'Windows/Panther/*'` で読める。
- インストール後は `vm.sh start` で ISO 無しで起動する。

## 操作

```sh
scripts/windows-vm/vm.sh start | stop | kill | status
scripts/windows-vm/vm.sh ssh                        # 対話 (PowerShell)
scripts/windows-vm/vm.sh ssh 'Get-ComputerInfo | Select-Object OsName, OsVersion'
scripts/windows-vm/vm.sh screenshot /tmp/win.png    # QMP screendump (PNG)。省略時は ~/VMs/win11/screenshots/
scripts/windows-vm/vm.sh key ctrl-alt-delete        # キー送信 (QEMU の sendkey の書き方)
scripts/windows-vm/vm.sh vnc                        # VNC の宛先を出し、ビューアがあれば開く
scp -i ~/.ssh/yhtye_win11_vm_ed25519 -P 2222 file yhtye@127.0.0.1:'C:/Users/yhtye/'
```

- SSH の PowerShell の出力は OEM コードページ (日本語版は CP932) なので、日本語が化ける。UTF-8 で見るには先頭に
  `[Console]::OutputEncoding=[Text.Encoding]::UTF8;` を付ける (VM の設定は変えていない。日本語版の素の環境で確かめるため)。
- PowerShell の実行ポリシーは既定 (Restricted) のまま。`npm install -g` で入る `pnpm` / `npm` の `.ps1` は PowerShell から
  動かないので、SSH からは `pnpm.cmd` / `npm.cmd` と書く (SETUP.md の手順を素の環境で確かめるため、ここでも変えていない)。

SSH のセッションは画面の無いサービスのセッションなので、GUI アプリを SSH から起動しても画面には出ない。デスクトップに出すには、
自動ログオンしている `yhtye` の対話セッションで、昇格なしで動かす (`run-gui.ps1`。unattend.iso から `C:\ProgramData\yhtye-vm` に入る):

```sh
scripts/windows-vm/vm.sh ssh 'powershell -NoProfile -File C:\ProgramData\yhtye-vm\run-gui.ps1 -Program C:\Users\yhtye\AppData\Local\Yhtye\yhtye.exe'
```

`schtasks /it /rl LIMITED` でプログラムを直接起動する (間に cmd.exe を挟まないので、それ自身のコンソール窓は出ない)。環境変数はユーザーの
レジストリから読まれるので、先に `[Environment]::SetEnvironmentVariable('NAME', 'value', 'User')` で入れておく
(`Register-ScheduledTask` は鍵認証の SSH セッションのユーザーを解決できず 0x80070534 になるので schtasks を使う)。

## Yhtye の実機確認 (偽エージェントで)

本物のエージェントの代わりに `crates/yhtye-fake-agent` (ACP をしゃべる、JSON の台本どおりに動く偽エージェント) を使う。

1. VM 内でビルドする: `cargo build --release -p yhtye-fake-agent` (Visual Studio Build Tools の C++ と Rust MSVC が要る。SETUP.md の Windows 手順)。
2. 台本をユーザー環境変数 `YHTYE_FAKE_SCRIPT` に入れる (エージェントは Yhtye の環境を受け継ぐ)。Google Antigravity として使うので、
   `models` に `gemini-3.8-flash-low` を含める。例: オーケストレータが `[yhtye:user_message]` で `create_group` / `create_task` を呼び、
   実装役が `[yhtye:step]` で `spawn_child` (自分の `--sleep` の複製を子として起動) してから `wait_cancel`。書き方は
   `crates/yhtye-fake-agent/src/scenario.rs` の先頭、使い方の例は `crates/yhtye-core/tests/orchestration_fake.rs`。
3. 設定 → ハーネス で Google Antigravity のパスに `yhtye-fake-agent.exe` の絶対パスを入れ、全ロールのハーネスを Google Antigravity にする。

画面の操作と読み取りは WebView2 の DevTools プロトコルで行える。ユーザー環境変数
`WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS=--remote-debugging-port=9222` を入れてから Yhtye を起動し、ポートを転送する:

```sh
ssh -f -N -i ~/.ssh/yhtye_win11_vm_ed25519 -p 2222 -L 9222:127.0.0.1:9222 yhtye@127.0.0.1
node scripts/windows-vm/cdp.mjs 'document.body.innerText.slice(0, 300)'
node scripts/windows-vm/cdp.mjs "window.__TAURI_INTERNALS__.invoke('yhtye_command', {host: null, cmd: {type: 'get_harnesses'}})"
```

日本語のユーザー名で確かめるときは、自動ログオンを `日本語ユーザー` に切り替えて再起動し (`set-autologon.ps1`。パスワードは
`~/VMs/win11/README.md`)、`run-gui.ps1 -User "YHTYE-WIN11\日本語ユーザー" -Program ...` でそのデスクトップに起動する。名前は
UTF-8 (BOM 付き) の .ps1 に書いて渡す。終わったら `yhtye` に戻す。`Start-Process -Credential` はタスクスケジューラから動かすと
「アクセスが拒否されました」になるので使わない。

プロセスツリーは VM 内で `Get-CimInstance Win32_Process | ? Name -in 'yhtye.exe','yhtye-fake-agent.exe' | select ProcessId, ParentProcessId, CommandLine`。
コンソール窓のちらつきは `vm.sh screenshot` を 0.2 秒おきに撮って比べる (コンソール窓が出れば数十万画素が変わる)。

### VNC で GUI を触る

VNC は 127.0.0.1:5901 だけで待ち受ける。同じマシンなら TigerVNC などで `vncviewer 127.0.0.1:5901`
(Arch: `pacman -S tigervnc`、ほかに Remmina・KRDC)。別のマシンからは SSH のポート転送を使う:

```sh
ssh -L 5901:127.0.0.1:5901 <ホスト>    # 手元で vncviewer 127.0.0.1:5901
```

自動ログオンしているので、つなぐと `yhtye` のデスクトップが出る。ロックされていたら Ctrl+Alt+Del (ビューアのメニュー、または
`vm.sh key ctrl-alt-delete`) を送ってパスワード (`~/VMs/win11/README.md`) を入れる。マウスは USB タブレットなのでずれない。
