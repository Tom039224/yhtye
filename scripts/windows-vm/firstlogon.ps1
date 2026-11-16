# First-logon setup of the Windows 11 test VM (run by autounattend.xml as the auto-logged-on admin).
# Installs and enables OpenSSH Server with public-key auth and PowerShell as the default shell,
# keeps the VM awake, and adds the Japanese-named administrator.
# Safe to re-run: C:\ProgramData\yhtye-vm\firstlogon.ps1 -SourceDir C:\ProgramData\yhtye-vm
# Keep this file ASCII: Windows PowerShell 5.1 reads BOM-less scripts in the ANSI code page.
param([Parameter(Mandatory = $true)][string]$SourceDir)

$ErrorActionPreference = 'Stop'
$work = 'C:\ProgramData\yhtye-vm'
New-Item -ItemType Directory -Force -Path $work | Out-Null
Start-Transcript -Path "$work\firstlogon.log" -Append | Out-Null

function Step($name, [scriptblock]$body) {
  Write-Host "== $name"
  try { & $body } catch { Write-Host "!! $name failed: $_" }
}

if ((Resolve-Path $SourceDir).Path -ne (Resolve-Path $work).Path) {
  Copy-Item -Force -Path "$SourceDir\*" -Destination $work
}

Step 'power: never sleep, never turn the display off, no lock screen' {
  powercfg /change standby-timeout-ac 0
  powercfg /change monitor-timeout-ac 0
  powercfg /change hibernate-timeout-ac 0
  New-Item -Force -Path 'HKLM:\SOFTWARE\Policies\Microsoft\Windows\Personalization' | Out-Null
  Set-ItemProperty -Path 'HKLM:\SOFTWARE\Policies\Microsoft\Windows\Personalization' -Name NoLockScreen -Type DWord -Value 1
}

Step 'Windows Update: no automatic restart while logged on' {
  $au = 'HKLM:\SOFTWARE\Policies\Microsoft\Windows\WindowsUpdate\AU'
  New-Item -Force -Path $au | Out-Null
  Set-ItemProperty -Path $au -Name NoAutoRebootWithLoggedOnUsers -Type DWord -Value 1
}

Step 'OpenSSH Server' {
  $cap = Get-WindowsCapability -Online -Name 'OpenSSH.Server*' | Select-Object -First 1
  if ($cap -and $cap.State -ne 'Installed') {
    # The capability comes from Windows Update; the network may still be coming up right after OOBE.
    for ($i = 1; $i -le 10; $i++) {
      try { Add-WindowsCapability -Online -Name $cap.Name | Out-Null; break }
      catch { Write-Host "Add-WindowsCapability attempt $i failed: $_"; Start-Sleep -Seconds 30 }
    }
  }
  if (-not (Get-Service sshd -ErrorAction SilentlyContinue)) {
    Write-Host 'capability unavailable; installing Win32-OpenSSH from GitHub'
    $rel = Invoke-RestMethod -UseBasicParsing 'https://api.github.com/repos/PowerShell/Win32-OpenSSH/releases/latest'
    $asset = $rel.assets | Where-Object { $_.name -like 'OpenSSH-Win64-v*.msi' } | Select-Object -First 1
    $msi = "$work\$($asset.name)"
    Invoke-WebRequest -UseBasicParsing -Uri $asset.browser_download_url -OutFile $msi
    Start-Process msiexec.exe -Wait -ArgumentList '/i', $msi, '/qn', 'ADDLOCAL=Server'
  }
  # Starting once generates host keys and C:\ProgramData\ssh\sshd_config.
  Start-Service sshd
  Stop-Service sshd
}

Step 'sshd: key auth only, PowerShell as the default shell' {
  $keys = 'C:\ProgramData\ssh\administrators_authorized_keys'
  Copy-Item -Force "$work\authorized_keys" $keys
  # sshd ignores the file unless only Administrators (S-1-5-32-544) and SYSTEM (S-1-5-18) can access it.
  icacls.exe $keys /inheritance:r /grant '*S-1-5-32-544:F' /grant '*S-1-5-18:F' | Out-Null

  $conf = 'C:\ProgramData\ssh\sshd_config'
  $text = Get-Content -Raw $conf
  $text = $text -replace '(?m)^#?\s*PubkeyAuthentication\s.*$', 'PubkeyAuthentication yes'
  $text = $text -replace '(?m)^#?\s*PasswordAuthentication\s.*$', 'PasswordAuthentication no'
  Set-Content -Path $conf -Value $text -Encoding ascii

  New-Item -Force -Path 'HKLM:\SOFTWARE\OpenSSH' | Out-Null
  Set-ItemProperty -Path 'HKLM:\SOFTWARE\OpenSSH' -Name DefaultShell `
    -Value 'C:\Windows\System32\WindowsPowerShell\v1.0\powershell.exe'

  if (-not (Get-NetFirewallRule -Name 'yhtye-vm-sshd' -ErrorAction SilentlyContinue)) {
    New-NetFirewallRule -Name 'yhtye-vm-sshd' -DisplayName 'OpenSSH Server (yhtye-vm)' -Profile Any `
      -Direction Inbound -Protocol TCP -LocalPort 22 -Action Allow | Out-Null
  }
  Set-Service sshd -StartupType Automatic
  Start-Service sshd
}

Step 'second administrator with a Japanese user name (not possible in autounattend.xml)' {
  $lines = Get-Content -Encoding UTF8 "$work\ja-user.txt"
  $name = $lines[0].Trim()
  if (-not (Get-LocalUser -Name $name -ErrorAction SilentlyContinue)) {
    $password = ConvertTo-SecureString $lines[1].Trim() -AsPlainText -Force
    New-LocalUser -Name $name -Password $password -PasswordNeverExpires -AccountNeverExpires | Out-Null
    Add-LocalGroupMember -SID 'S-1-5-32-544' -Member $name
  }
  Remove-Item -Force "$work\ja-user.txt"
}

Set-Content -Path "$work\firstlogon.done" -Value (Get-Date -Format o)
Stop-Transcript | Out-Null
