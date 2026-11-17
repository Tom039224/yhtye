# Install the developer tools the Yhtye checks need in the Windows 11 test VM: Git for Windows,
# Node.js 22 LTS (npx) and pnpm. Run as the admin user over SSH:
#   vm.sh ssh 'powershell -NoProfile -ExecutionPolicy Bypass -File C:\ProgramData\yhtye-vm\install-tools.ps1'
# Downloads the official installers directly (winget needs the Store-updated App Installer, which a fresh
# evaluation install over SSH does not reliably have). Re-running skips what is already installed.
# Keep this file ASCII (Windows PowerShell 5.1 reads BOM-less scripts in the ANSI code page).
$ErrorActionPreference = 'Stop'
$ProgressPreference = 'SilentlyContinue'
[Net.ServicePointManager]::SecurityProtocol = [Net.SecurityProtocolType]::Tls12
$work = 'C:\ProgramData\yhtye-vm\downloads'
New-Item -ItemType Directory -Force -Path $work | Out-Null

function Refresh-Path {
  $env:Path = [Environment]::GetEnvironmentVariable('Path', 'Machine') + ';' + [Environment]::GetEnvironmentVariable('Path', 'User')
}

function Download($url, $name) {
  $out = Join-Path $work $name
  if (-not (Test-Path $out)) {
    Write-Host "downloading $url"
    Invoke-WebRequest -UseBasicParsing -Uri $url -OutFile "$out.part"
    Move-Item -Force "$out.part" $out
  }
  $out
}

function Run($file, [string[]]$arguments) {
  $p = Start-Process -FilePath $file -ArgumentList $arguments -Wait -PassThru
  if ($p.ExitCode -ne 0) { throw "$file exited with $($p.ExitCode)" }
}

Refresh-Path

if (-not (Get-Command git -ErrorAction SilentlyContinue)) {
  $rel = Invoke-RestMethod -UseBasicParsing 'https://api.github.com/repos/git-for-windows/git/releases/latest'
  $asset = $rel.assets | Where-Object { $_.name -match '^Git-[\d.]+-64-bit\.exe$' } | Select-Object -First 1
  $exe = Download $asset.browser_download_url $asset.name
  Run $exe @('/VERYSILENT', '/NORESTART', '/NOCANCEL', '/SP-', '/SUPPRESSMSGBOXES')
}

$node = Get-Command node -ErrorAction SilentlyContinue
if (-not $node -or -not ((& node --version) -like 'v22.*')) {
  $index = Invoke-RestMethod -UseBasicParsing 'https://nodejs.org/dist/index.json'
  $v = ($index | Where-Object { $_.version -like 'v22.*' } | Select-Object -First 1).version
  $msi = Download "https://nodejs.org/dist/$v/node-$v-x64.msi" "node-$v-x64.msi"
  Run msiexec.exe @('/i', $msi, '/qn', '/norestart')
}

Refresh-Path
if (-not (Get-Command pnpm -ErrorAction SilentlyContinue)) {
  & npm.cmd install --global pnpm@latest
  if ($LASTEXITCODE -ne 0) { throw "npm install -g pnpm failed ($LASTEXITCODE)" }
}

Refresh-Path
Write-Host ('git  ' + (& git --version))
Write-Host ('node ' + (& node --version))
Write-Host ('npm  ' + (& npm.cmd --version))
Write-Host ('npx  ' + (Get-Command npx.cmd).Source)
Write-Host ('pnpm ' + (& pnpm.cmd --version))
