$ErrorActionPreference = 'SilentlyContinue'

# Stop watcher PowerShell processes running nimbus-watch.ps1
$watchers = Get-CimInstance Win32_Process -Filter "Name='powershell.exe' OR Name='pwsh.exe'" |
    Where-Object { $_.ProcessId -ne $PID -and $_.CommandLine -like '*-File*nimbus-watch.ps1*' }
foreach ($w in $watchers) {
    Stop-Process -Id $w.ProcessId -Force
}

# Stop running live nimbus.exe
$liveExe = Join-Path $env:LOCALAPPDATA 'Nimbus\live\nimbus.exe'
foreach ($p in (Get-Process -Name 'nimbus')) {
    if ($p.Path -ieq $liveExe) {
        Stop-Process -Id $p.Id -Force
    }
}

Write-Host "Nimbus dan live watcher telah dihentikan."
