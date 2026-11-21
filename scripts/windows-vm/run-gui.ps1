# Start a program in the logged-on user's desktop session, non-elevated (as a double-click would),
# from an SSH session (which has no desktop). Returns right away; the program keeps running.
#   powershell -File C:\ProgramData\yhtye-vm\run-gui.ps1 -Program 'C:\path\app.exe' [-Arguments '--flag']
# The program is started directly by Task Scheduler (no cmd.exe in between, so no console window flashes).
# It gets the user's environment from the registry: set extra variables with
# [Environment]::SetEnvironmentVariable('NAME', 'value', 'User') beforehand.
# Keep this file ASCII (Windows PowerShell 5.1 reads BOM-less scripts in the ANSI code page).
param(
  [Parameter(Mandatory = $true)][string]$Program,
  [string]$Arguments = '',
  [string]$User = [Security.Principal.WindowsIdentity]::GetCurrent().Name
)
$ErrorActionPreference = 'Stop'
$name = 'yhtye-vm-gui-' + [guid]::NewGuid().ToString('N').Substring(0, 8)
$run = "`"$Program`""
if ($Arguments) { $run += " $Arguments" }
# Register-ScheduledTask cannot map the user of a key-authenticated SSH session (0x80070534); schtasks can.
# /it: only in the interactive session, /rl LIMITED: without elevation. The start time is irrelevant (/run).
schtasks.exe /create /tn $name /tr $run /sc once /st 23:59 /it /rl LIMITED /ru $User /f | Out-Null
if ($LASTEXITCODE -ne 0) { throw "schtasks /create failed ($LASTEXITCODE)" }
schtasks.exe /run /tn $name | Out-Null
Start-Sleep -Seconds 2
schtasks.exe /delete /tn $name /f | Out-Null
Write-Host "started in the desktop session of ${User}: $run"
