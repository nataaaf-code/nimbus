param(
    [string]$RepoRoot = (Split-Path -Parent $PSScriptRoot)
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

# Resolve paths
$RepoRoot      = (Resolve-Path $RepoRoot).Path
$ScriptsDir    = Join-Path $RepoRoot 'scripts'
$LauncherVbs   = Join-Path $ScriptsDir 'start-nimbus.vbs'
$AutostartCmd  = "wscript.exe `"$LauncherVbs`""
$CargoManifest = Join-Path $RepoRoot 'src-tauri\Cargo.toml'
$ReleaseExe    = Join-Path $RepoRoot 'src-tauri\target\release\nimbus.exe'

$AppLocalDir   = Join-Path $env:LOCALAPPDATA 'Nimbus'
$LiveDir       = Join-Path $AppLocalDir 'live'
$LiveExe       = Join-Path $LiveDir 'nimbus.exe'
$LogFile       = Join-Path $AppLocalDir 'watcher.log'

New-Item -ItemType Directory -Force -Path $LiveDir | Out-Null

# Ensure Cargo bin is on PATH
$CargoBin = Join-Path $env:USERPROFILE '.cargo\bin'
if ((Test-Path $CargoBin) -and ($env:PATH -notlike "*$CargoBin*")) {
    $env:PATH = "$CargoBin;$env:PATH"
}

function Write-Log {
    param([string]$Message)
    try {
        if ((Test-Path $LogFile) -and ((Get-Item $LogFile).Length -gt 524288)) {
            $tail = Get-Content $LogFile -Tail 200 -ErrorAction SilentlyContinue
            Set-Content -Path $LogFile -Value $tail -Encoding UTF8
        }
        $ts = Get-Date -Format 'yyyy-MM-dd HH:mm:ss'
        Add-Content -Path $LogFile -Value "[$ts] $Message" -Encoding UTF8
    } catch {
        # Best-effort logging
    }
}

function Get-LiveNimbusProcesses {
    $procs = @()
    foreach ($p in (Get-Process -Name 'nimbus' -ErrorAction SilentlyContinue)) {
        try {
            if ($p.Path -and ($p.Path -ieq $LiveExe)) {
                $procs += $p
            }
        } catch {
            # Ignore inaccessible process
        }
    }
    return $procs
}

function Test-ManualDevRunning {
    foreach ($p in (Get-Process -Name 'nimbus' -ErrorAction SilentlyContinue)) {
        try {
            if ($p.Path -and ($p.Path -like "*\target\debug\nimbus.exe")) {
                return $true
            }
        } catch {}
    }
    return $false
}

function Stop-LiveNimbus {
    $procs = @(Get-LiveNimbusProcesses)
    if ($procs.Count -eq 0) { return }
    foreach ($p in $procs) {
        try {
            Stop-Process -Id $p.Id -Force -ErrorAction SilentlyContinue
        } catch {}
    }
    $deadline = (Get-Date).AddSeconds(4)
    while ((Get-Date) -lt $deadline) {
        if (@(Get-LiveNimbusProcesses).Count -eq 0) { break }
        Start-Sleep -Milliseconds 150
    }
}

function Copy-ReleaseToLive {
    if (-not (Test-Path $ReleaseExe)) { return $false }

    $needsCopy = $true
    if (Test-Path $LiveExe) {
        $src = Get-Item $ReleaseExe
        $dst = Get-Item $LiveExe
        if (($src.Length -eq $dst.Length) -and ($src.LastWriteTimeUtc -le $dst.LastWriteTimeUtc)) {
            $needsCopy = $false
        }
    }
    if (-not $needsCopy) { return $true }

    for ($i = 0; $i -lt 10; $i++) {
        try {
            Copy-Item -Path $ReleaseExe -Destination $LiveExe -Force
            return $true
        } catch {
            Start-Sleep -Milliseconds 250
        }
    }
    Write-Log "ERROR: Gagal menyalin $ReleaseExe ke $LiveExe"
    return $false
}

function Start-LiveNimbus {
    if (-not (Test-Path $LiveExe)) {
        if (-not (Copy-ReleaseToLive)) { return $false }
    }
    if (@(Get-LiveNimbusProcesses).Count -gt 0) {
        return $true
    }

    try {
        $psi = New-Object System.Diagnostics.ProcessStartInfo
        $psi.FileName = $LiveExe
        $psi.WorkingDirectory = $RepoRoot
        $psi.UseShellExecute = $false
        $psi.CreateNoWindow = $false
        $psi.WindowStyle = [System.Diagnostics.ProcessWindowStyle]::Normal
        $psi.EnvironmentVariables['NIMBUS_AUTOSTART_CMD'] = $AutostartCmd
        $psi.EnvironmentVariables['PATH'] = $env:PATH
        $proc = [System.Diagnostics.Process]::Start($psi)
        Write-Log "Nimbus dijalankan (PID $($proc.Id)) dari $LiveExe"
        return $true
    } catch {
        Write-Log "ERROR: Gagal menjalankan Nimbus: $_"
        return $false
    }
}

function Get-LatestSourceTimeUtc {
    $latest = [DateTime]::MinValue
    $targets = @(
        (Join-Path $RepoRoot 'ui'),
        (Join-Path $RepoRoot 'src-tauri\src'),
        (Join-Path $RepoRoot 'src-tauri\icons')
    )
    foreach ($dir in $targets) {
        if (Test-Path $dir) {
            foreach ($f in (Get-ChildItem -Path $dir -File -Recurse -ErrorAction SilentlyContinue)) {
                if ($f.LastWriteTimeUtc -gt $latest) { $latest = $f.LastWriteTimeUtc }
            }
        }
    }
    $singleFiles = @(
        (Join-Path $RepoRoot 'src-tauri\Cargo.toml'),
        (Join-Path $RepoRoot 'src-tauri\Cargo.lock'),
        (Join-Path $RepoRoot 'src-tauri\tauri.conf.json'),
        (Join-Path $RepoRoot 'src-tauri\build.rs')
    )
    foreach ($sf in $singleFiles) {
        if (Test-Path $sf) {
            $item = Get-Item $sf -ErrorAction SilentlyContinue
            if ($item -and ($item.LastWriteTimeUtc -gt $latest)) { $latest = $item.LastWriteTimeUtc }
        }
    }
    return $latest
}

function Invoke-RebuildAndSwap {
    param([string]$Reason)

    if (Test-ManualDevRunning) {
        Write-Log "Lewati auto-rebuild ($Reason): sesi manual target\debug\nimbus.exe sedang aktif."
        return
    }

    Write-Log "Mulai rebuild release ($Reason)..."
    $sw = [System.Diagnostics.Stopwatch]::StartNew()

    $psi = New-Object System.Diagnostics.ProcessStartInfo
    $psi.FileName = 'cargo'
    $psi.Arguments = "build --release --manifest-path `"$CargoManifest`""
    $psi.WorkingDirectory = $RepoRoot
    $psi.UseShellExecute = $false
    $psi.CreateNoWindow = $true
    $psi.RedirectStandardOutput = $true
    $psi.RedirectStandardError = $true
    $psi.EnvironmentVariables['PATH'] = $env:PATH

    try {
        $proc = [System.Diagnostics.Process]::Start($psi)
        $outTask = $proc.StandardOutput.ReadToEndAsync()
        $errTask = $proc.StandardError.ReadToEndAsync()
        $proc.WaitForExit()
        $sw.Stop()
        $exitCode = $proc.ExitCode
        $stderr = $errTask.Result
    } catch {
        $sw.Stop()
        Write-Log "ERROR: Gagal menjalankan cargo: $_"
        return
    }

    if ($exitCode -ne 0) {
        $lines = @($stderr -split "`r?`n" | Where-Object { $_.Trim().Length -gt 0 })
        $tail = ($lines | Select-Object -Last 12) -join ' | '
        Write-Log "BUILD GAGAL (exit $exitCode, $([math]::Round($sw.Elapsed.TotalSeconds, 1))s) - versi lama tetap jalan. Detail: $tail"
        return
    }

    Write-Log "Build selesai ($([math]::Round($sw.Elapsed.TotalSeconds, 1))s). Memperbarui Nimbus..."
    Stop-LiveNimbus
    if (Copy-ReleaseToLive) {
        Start-LiveNimbus | Out-Null
    }
}

function Test-RelevantPath {
    param([string]$FullPath)
    if ([string]::IsNullOrWhiteSpace($FullPath)) { return $false }
    if ($FullPath -match '\\(target|\.git|node_modules)(\\|$)') { return $false }
    if ($FullPath -match '\\Cargo\.lock$') { return $false }
    if ($FullPath -match '(\.tmp|\.swp|\.bak|~|\.crdownload)$') { return $false }
    # Ignore directories if they still exist as directories
    if (Test-Path $FullPath -PathType Container) { return $false }
    return $true
}

# Single-instance mutex so autostart / re-launch never spawns duplicate watchers
$createdNew = $false
$mutex = New-Object System.Threading.Mutex($true, 'Local\NimbusLiveWatcherMutex', [ref]$createdNew)
if (-not $createdNew) {
    # Watcher already active: make sure Nimbus itself is running, then exit
    if (@(Get-LiveNimbusProcesses).Count -eq 0 -and (Test-Path $ReleaseExe)) {
        Copy-ReleaseToLive | Out-Null
        Start-LiveNimbus | Out-Null
    }
    exit 0
}

$watchers = @()
$subs = @()

try {
    Write-Log "=== Nimbus Live Watcher dimulai (repo: $RepoRoot) ==="

    # 1. Start existing release immediately so Nimbus appears instantly on boot
    $needsInitialBuild = $false
    if (Test-Path $ReleaseExe) {
        Copy-ReleaseToLive | Out-Null
        Start-LiveNimbus | Out-Null
        $binTime = (Get-Item $ReleaseExe).LastWriteTimeUtc
        $srcTime = Get-LatestSourceTimeUtc
        if ($srcTime -gt $binTime.AddSeconds(1)) {
            $needsInitialBuild = $true
        }
    } else {
        $needsInitialBuild = $true
    }

    if ($needsInitialBuild) {
        Invoke-RebuildAndSwap -Reason 'source lebih baru saat startup'
    }

    # 2. Setup FileSystemWatchers (excluding src-tauri\target completely)
    $watchSpecs = @(
        @{ Path = (Join-Path $RepoRoot 'ui');               Subdirs = $true;  Filter = '*.*' },
        @{ Path = (Join-Path $RepoRoot 'src-tauri\src');    Subdirs = $true;  Filter = '*.*' },
        @{ Path = (Join-Path $RepoRoot 'src-tauri\icons');  Subdirs = $true;  Filter = '*.*' },
        @{ Path = (Join-Path $RepoRoot 'src-tauri');        Subdirs = $false; Filter = '*.*' }
    )

    foreach ($spec in $watchSpecs) {
        if (-not (Test-Path $spec.Path)) { continue }
        $fsw = New-Object System.IO.FileSystemWatcher
        $fsw.Path = $spec.Path
        $fsw.Filter = $spec.Filter
        $fsw.IncludeSubdirectories = $spec.Subdirs
        $fsw.InternalBufferSize = 65536
        $fsw.NotifyFilter = [System.IO.NotifyFilters]'FileName, LastWrite, Size'
        $fsw.EnableRaisingEvents = $true
        $watchers += $fsw

        foreach ($evtName in @('Changed', 'Created', 'Deleted', 'Renamed')) {
            $subs += Register-ObjectEvent -InputObject $fsw -EventName $evtName -SourceIdentifier ("NimbusWatch_" + [guid]::NewGuid().ToString('N'))
        }
    }

    Write-Log "Memantau perubahan di ui/, src-tauri/src/, dan config (debounce 1.5s)."

    # 3. Idle-friendly event loop with debounce
    $dirty = $false
    $lastEventAt = [DateTime]::MinValue
    $lastReason = ''
    $debounceMs = 1500

    while ($true) {
        $waitSec = if ($dirty) { 1 } else { 5 }
        $ev = Wait-Event -Timeout $waitSec

        # Drain all pending events
        $allEvents = @()
        if ($null -ne $ev) { $allEvents += $ev }
        $more = @(Get-Event -ErrorAction SilentlyContinue)
        if ($more.Count -gt 0) { $allEvents += $more }

        foreach ($e in $allEvents) {
            try {
                $fp = $e.SourceEventArgs.FullPath
                if (Test-RelevantPath $fp) {
                    $dirty = $true
                    $lastEventAt = [DateTime]::UtcNow
                    $rel = $fp.Replace($RepoRoot + '\', '')
                    $lastReason = $rel
                }
            } catch {}
            Remove-Event -EventIdentifier $e.EventIdentifier -ErrorAction SilentlyContinue
        }

        if ($dirty -and (([DateTime]::UtcNow - $lastEventAt).TotalMilliseconds -ge $debounceMs)) {
            $dirty = $false
            Invoke-RebuildAndSwap -Reason "perubahan $lastReason"
        }
    }
} finally {
    foreach ($s in $subs) {
        try { Unregister-Event -SubscriptionId $s.Id -ErrorAction SilentlyContinue } catch {}
    }
    foreach ($w in $watchers) {
        try { $w.EnableRaisingEvents = $false; $w.Dispose() } catch {}
    }
    try { $mutex.ReleaseMutex() } catch {}
    try { $mutex.Dispose() } catch {}
    Write-Log "=== Nimbus Live Watcher dihentikan ==="
}
