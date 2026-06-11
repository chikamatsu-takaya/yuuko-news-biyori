<#
.SYNOPSIS
Windows上のゆうこアプリについて、起動時間と常駐中のCPU・メモリ・TCP接続を記録する。

.DESCRIPTION
実行ファイルを起動して測定するか、既存プロセスへ接続して一定間隔でサンプリングする。
測定結果はCSVとJSONへ保存し、アプリ本体やapp-dataは変更しない。
#>

[CmdletBinding(DefaultParameterSetName = "AttachByName")]
param(
    [Parameter(Mandatory = $true, ParameterSetName = "Launch")]
    [string]$ExecutablePath,

    [Parameter(Mandatory = $true, ParameterSetName = "AttachById")]
    [ValidateRange(1, [int]::MaxValue)]
    [int]$TargetProcessId,

    [Parameter(ParameterSetName = "AttachByName")]
    [ValidateNotNullOrEmpty()]
    [string]$ProcessName = "app",

    [ValidateRange(5, 86400)]
    [int]$DurationSeconds = 300,

    [ValidateRange(1, 3600)]
    [int]$SampleIntervalSeconds = 5,

    [ValidateRange(1, 300)]
    [int]$StartupTimeoutSeconds = 60,

    [ValidatePattern("^[A-Za-z0-9_-]+$")]
    [string]$Label = "baseline",

    [string]$OutputDirectory = "performance-results",

    [string]$AppDataDirectory = (Join-Path $env:APPDATA "jp.star-system.yuuko-news"),

    [Parameter(ParameterSetName = "Launch")]
    [switch]$KeepProcessRunning
)

Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

function Get-Percentile {
    param(
        [double[]]$Values,
        [ValidateRange(0, 100)]
        [double]$Percentile
    )

    if ($Values.Count -eq 0) {
        return $null
    }

    $sorted = @($Values | Sort-Object)
    $index = [Math]::Ceiling(($Percentile / 100) * $sorted.Count) - 1
    $index = [Math]::Max(0, [Math]::Min($index, $sorted.Count - 1))
    return [Math]::Round([double]$sorted[$index], 3)
}

function Get-MetricSummary {
    param([double[]]$Values)

    if ($Values.Count -eq 0) {
        return $null
    }

    $measurement = $Values | Measure-Object -Minimum -Maximum -Average
    return [ordered]@{
        min = [Math]::Round([double]$measurement.Minimum, 3)
        average = [Math]::Round([double]$measurement.Average, 3)
        p95 = Get-Percentile -Values $Values -Percentile 95
        max = [Math]::Round([double]$measurement.Maximum, 3)
    }
}

function Get-RefreshStateSnapshot {
    param([string]$StatePath)

    if (-not (Test-Path -LiteralPath $StatePath -PathType Leaf)) {
        return [ordered]@{
            exists = $false
            lastWriteTimeUtc = $null
            lastNewsRefreshDate = $null
            parseError = $null
        }
    }

    $item = Get-Item -LiteralPath $StatePath
    $lastRefreshDate = $null
    $parseError = $null
    try {
        $payload = Get-Content -LiteralPath $StatePath -Raw -Encoding UTF8 | ConvertFrom-Json
        $lastRefreshDate = $payload.lastNewsRefreshDate
    }
    catch {
        $parseError = $_.Exception.Message
    }

    return [ordered]@{
        exists = $true
        lastWriteTimeUtc = $item.LastWriteTimeUtc.ToString("o")
        lastNewsRefreshDate = $lastRefreshDate
        parseError = $parseError
    }
}

function Get-NewsMarkdownCount {
    param([string]$NewsDirectory)

    if (-not (Test-Path -LiteralPath $NewsDirectory -PathType Container)) {
        return 0
    }

    return @(Get-ChildItem -LiteralPath $NewsDirectory -Filter "*.md" -File -Recurse -ErrorAction SilentlyContinue).Count
}

function Get-ProcessTreeSnapshot {
    param([int]$RootProcessId)

    $processRows = @(Get-CimInstance Win32_Process -Property ProcessId, ParentProcessId, Name)
    $processIds = New-Object System.Collections.Generic.HashSet[int]
    [void]$processIds.Add($RootProcessId)

    do {
        $added = $false
        foreach ($row in $processRows) {
            if ($processIds.Contains([int]$row.ParentProcessId) -and $processIds.Add([int]$row.ProcessId)) {
                $added = $true
            }
        }
    } while ($added)

    $liveProcesses = New-Object System.Collections.Generic.List[object]
    foreach ($currentProcessId in $processIds) {
        $currentProcess = Get-Process -Id $currentProcessId -ErrorAction SilentlyContinue
        if ($null -ne $currentProcess) {
            $currentProcess.Refresh()
            $liveProcesses.Add($currentProcess)
        }
    }

    $totalCpuSeconds = 0.0
    $workingSetBytes = [int64]0
    $privateMemoryBytes = [int64]0
    $threadCount = 0
    $handleCount = 0
    foreach ($currentProcess in $liveProcesses) {
        $totalCpuSeconds += $currentProcess.TotalProcessorTime.TotalSeconds
        $workingSetBytes += $currentProcess.WorkingSet64
        $privateMemoryBytes += $currentProcess.PrivateMemorySize64
        $threadCount += $currentProcess.Threads.Count
        $handleCount += $currentProcess.HandleCount
    }

    return [ordered]@{
        processIds = @($liveProcesses | ForEach-Object Id)
        processNames = @($liveProcesses | ForEach-Object ProcessName | Sort-Object -Unique)
        processCount = $liveProcesses.Count
        totalCpuSeconds = $totalCpuSeconds
        workingSetBytes = $workingSetBytes
        privateMemoryBytes = $privateMemoryBytes
        threadCount = $threadCount
        handleCount = $handleCount
    }
}

function Get-NetworkSnapshot {
    param([int[]]$OwnerProcessIds)

    if (-not (Get-Command Get-NetTCPConnection -ErrorAction SilentlyContinue)) {
        return [ordered]@{
            available = $false
            activeConnectionCount = $null
            establishedConnectionCount = $null
            remoteEndpoints = @()
            error = "Get-NetTCPConnection is unavailable"
        }
    }

    try {
        $ownerSet = New-Object System.Collections.Generic.HashSet[int]
        foreach ($ownerProcessId in $OwnerProcessIds) {
            [void]$ownerSet.Add($ownerProcessId)
        }
        $connections = @(Get-NetTCPConnection -ErrorAction Stop | Where-Object {
            $ownerSet.Contains([int]$_.OwningProcess)
        })
        $activeConnections = @($connections | Where-Object {
            $_.State -ne "Listen" -and
            $_.RemoteAddress -notin @("0.0.0.0", "::")
        })
        $remoteEndpoints = @($activeConnections |
            ForEach-Object { "{0}:{1}" -f $_.RemoteAddress, $_.RemotePort } |
            Sort-Object -Unique)

        return [ordered]@{
            available = $true
            activeConnectionCount = $activeConnections.Count
            establishedConnectionCount = @($activeConnections | Where-Object State -eq "Established").Count
            remoteEndpoints = $remoteEndpoints
            error = $null
        }
    }
    catch {
        return [ordered]@{
            available = $false
            activeConnectionCount = $null
            establishedConnectionCount = $null
            remoteEndpoints = @()
            error = $_.Exception.Message
        }
    }
}

function Get-TargetProcess {
    if ($PSCmdlet.ParameterSetName -eq "AttachById") {
        return Get-Process -Id $TargetProcessId -ErrorAction Stop
    }

    $processes = @(Get-Process -Name $ProcessName -ErrorAction SilentlyContinue |
        Sort-Object StartTime -Descending)
    if ($processes.Count -eq 0) {
        throw "Process '$ProcessName' was not found. Start the app first or specify -ExecutablePath."
    }
    if ($processes.Count -gt 1) {
        $processIds = $processes.Id -join ", "
        throw "Multiple '$ProcessName' processes were found (PIDs: $processIds). Specify the intended process with -TargetProcessId."
    }

    return $processes[0]
}

if ([System.IO.Path]::IsPathRooted($OutputDirectory)) {
    $resolvedOutputDirectory = [System.IO.Path]::GetFullPath($OutputDirectory)
}
else {
    $resolvedOutputDirectory = [System.IO.Path]::GetFullPath((Join-Path (Get-Location) $OutputDirectory))
}
New-Item -ItemType Directory -Path $resolvedOutputDirectory -Force | Out-Null

$startedByScript = $PSCmdlet.ParameterSetName -eq "Launch"
$startupSeconds = $null
$startupTimedOut = $false
$process = $null
$measurementStartedAt = [DateTime]::UtcNow

try {
    if ($startedByScript) {
        $resolvedExecutablePath = (Resolve-Path -LiteralPath $ExecutablePath).Path
        if (-not [System.IO.Path]::GetExtension($resolvedExecutablePath).Equals(
            ".exe",
            [System.StringComparison]::OrdinalIgnoreCase
        )) {
            throw "ExecutablePath must point to a Windows .exe file."
        }

        $startupStopwatch = [System.Diagnostics.Stopwatch]::StartNew()
        $process = Start-Process -FilePath $resolvedExecutablePath -PassThru
        while ($startupStopwatch.Elapsed.TotalSeconds -lt $StartupTimeoutSeconds) {
            Start-Sleep -Milliseconds 100
            if ($process.HasExited) {
                throw "The launched process exited before its window became ready."
            }
            $process.Refresh()
            if ($process.MainWindowHandle -ne 0 -and $process.Responding) {
                $startupSeconds = [Math]::Round($startupStopwatch.Elapsed.TotalSeconds, 3)
                break
            }
        }
        $startupStopwatch.Stop()
        if ($null -eq $startupSeconds) {
            $startupTimedOut = $true
        }
    }
    else {
        $process = Get-TargetProcess
    }

    $process.Refresh()
    $processId = $process.Id
    $processStartTimeUtc = $process.StartTime.ToUniversalTime().ToString("o")
    $refreshStatePath = Join-Path $AppDataDirectory "state\news_refresh_state.json"
    $newsDirectory = Join-Path $AppDataDirectory "news"
    $initialRefreshState = Get-RefreshStateSnapshot -StatePath $refreshStatePath
    $initialNewsMarkdownCount = Get-NewsMarkdownCount -NewsDirectory $newsDirectory
    $samples = New-Object System.Collections.Generic.List[object]
    $observedRemoteEndpoints = New-Object System.Collections.Generic.HashSet[string]
    $observedProcessNames = New-Object System.Collections.Generic.HashSet[string]
    $refreshStateChangeCount = 0
    $previousRefreshWriteTime = $initialRefreshState.lastWriteTimeUtc
    $initialProcessTree = Get-ProcessTreeSnapshot -RootProcessId $processId
    $previousCpuSeconds = $initialProcessTree.totalCpuSeconds
    $previousSampleAt = [DateTime]::UtcNow
    $measurementStopwatch = [System.Diagnostics.Stopwatch]::StartNew()
    $nextSampleSeconds = [double]$SampleIntervalSeconds
    $expectedSampleCount = [int][Math]::Ceiling($DurationSeconds / [double]$SampleIntervalSeconds)

    Write-Host "Measuring process $($process.ProcessName) (PID $processId) for $DurationSeconds seconds..."
    while ($samples.Count -lt $expectedSampleCount) {
        $targetSampleSeconds = [Math]::Min($nextSampleSeconds, [double]$DurationSeconds)
        $sleepSeconds = [Math]::Max(0, $targetSampleSeconds - $measurementStopwatch.Elapsed.TotalSeconds)
        if ($sleepSeconds -gt 0) {
            Start-Sleep -Milliseconds ([int][Math]::Ceiling($sleepSeconds * 1000))
        }

        if ($process.HasExited) {
            break
        }

        $process.Refresh()
        $processTree = Get-ProcessTreeSnapshot -RootProcessId $processId
        if ($processTree.processCount -eq 0) {
            break
        }
        $sampledAt = [DateTime]::UtcNow
        $elapsedSeconds = ($sampledAt - $previousSampleAt).TotalSeconds
        $currentCpuSeconds = $processTree.totalCpuSeconds
        $cpuDeltaSeconds = [Math]::Max(0, $currentCpuSeconds - $previousCpuSeconds)
        $cpuPercent = 0
        if ($elapsedSeconds -gt 0) {
            $cpuPercent = ($cpuDeltaSeconds / $elapsedSeconds / [Environment]::ProcessorCount) * 100
        }

        $network = Get-NetworkSnapshot -OwnerProcessIds $processTree.processIds
        foreach ($endpoint in $network.remoteEndpoints) {
            [void]$observedRemoteEndpoints.Add($endpoint)
        }
        foreach ($currentProcessName in $processTree.processNames) {
            [void]$observedProcessNames.Add($currentProcessName)
        }

        $refreshState = Get-RefreshStateSnapshot -StatePath $refreshStatePath
        if ($refreshState.lastWriteTimeUtc -ne $previousRefreshWriteTime) {
            $refreshStateChangeCount += 1
            $previousRefreshWriteTime = $refreshState.lastWriteTimeUtc
        }

        $samples.Add([pscustomobject][ordered]@{
            sampledAtUtc = $sampledAt.ToString("o")
            elapsedSeconds = [Math]::Round($measurementStopwatch.Elapsed.TotalSeconds, 3)
            cpuPercent = [Math]::Round($cpuPercent, 3)
            processCount = $processTree.processCount
            processNames = ($processTree.processNames -join ";")
            workingSetMb = [Math]::Round($processTree.workingSetBytes / 1MB, 3)
            privateMemoryMb = [Math]::Round($processTree.privateMemoryBytes / 1MB, 3)
            threadCount = $processTree.threadCount
            handleCount = $processTree.handleCount
            activeTcpConnections = $network.activeConnectionCount
            establishedTcpConnections = $network.establishedConnectionCount
            remoteEndpoints = ($network.remoteEndpoints -join ";")
            refreshStateLastWriteUtc = $refreshState.lastWriteTimeUtc
            lastNewsRefreshDate = $refreshState.lastNewsRefreshDate
        })

        $previousCpuSeconds = $currentCpuSeconds
        $previousSampleAt = $sampledAt
        $nextSampleSeconds += $SampleIntervalSeconds
    }
    $measurementStopwatch.Stop()

    $finalRefreshState = Get-RefreshStateSnapshot -StatePath $refreshStatePath
    $finalNewsMarkdownCount = Get-NewsMarkdownCount -NewsDirectory $newsDirectory
    $endedEarly = $process.HasExited -and $measurementStopwatch.Elapsed.TotalSeconds -lt $DurationSeconds
    $timestamp = $measurementStartedAt.ToString("yyyyMMdd-HHmmss")
    $baseName = "yuuko-performance-$Label-$timestamp"
    $csvPath = Join-Path $resolvedOutputDirectory "$baseName.csv"
    $jsonPath = Join-Path $resolvedOutputDirectory "$baseName.json"

    $samples | Export-Csv -LiteralPath $csvPath -NoTypeInformation -Encoding UTF8
    $summary = [ordered]@{
        schemaVersion = 1
        label = $Label
        measuredAtUtc = $measurementStartedAt.ToString("o")
        environment = [ordered]@{
            machineName = $env:COMPUTERNAME
            osVersion = [Environment]::OSVersion.VersionString
            logicalProcessorCount = [Environment]::ProcessorCount
            powershellVersion = $PSVersionTable.PSVersion.ToString()
        }
        process = [ordered]@{
            name = $process.ProcessName
            id = $processId
            startTimeUtc = $processStartTimeUtc
            startedByScript = $startedByScript
            exitedDuringMeasurement = $endedEarly
            scope = "root process and descendants"
            observedProcessNames = @($observedProcessNames | Sort-Object)
        }
        measurement = [ordered]@{
            requestedDurationSeconds = $DurationSeconds
            actualDurationSeconds = [Math]::Round($measurementStopwatch.Elapsed.TotalSeconds, 3)
            sampleIntervalSeconds = $SampleIntervalSeconds
            sampleCount = $samples.Count
            startupReadySeconds = $startupSeconds
            startupTimedOut = $startupTimedOut
            cpuPercent = Get-MetricSummary -Values @($samples | ForEach-Object { [double]$_.cpuPercent })
            workingSetMb = Get-MetricSummary -Values @($samples | ForEach-Object { [double]$_.workingSetMb })
            privateMemoryMb = Get-MetricSummary -Values @($samples | ForEach-Object { [double]$_.privateMemoryMb })
            processCount = Get-MetricSummary -Values @($samples | ForEach-Object { [double]$_.processCount })
            threadCount = Get-MetricSummary -Values @($samples | ForEach-Object { [double]$_.threadCount })
            activeTcpConnections = Get-MetricSummary -Values @($samples |
                Where-Object { $null -ne $_.activeTcpConnections } |
                ForEach-Object { [double]$_.activeTcpConnections })
        }
        networkObservation = [ordered]@{
            remoteEndpoints = @($observedRemoteEndpoints | Sort-Object)
            note = "TCP connections are sampled observations, not an HTTP request counter."
        }
        newsRefreshObservation = [ordered]@{
            statePath = $refreshStatePath
            initialState = $initialRefreshState
            finalState = $finalRefreshState
            stateFileChangeCount = $refreshStateChangeCount
            initialMarkdownCount = $initialNewsMarkdownCount
            finalMarkdownCount = $finalNewsMarkdownCount
            markdownCountDelta = $finalNewsMarkdownCount - $initialNewsMarkdownCount
        }
        outputs = [ordered]@{
            samplesCsv = $csvPath
            summaryJson = $jsonPath
        }
    }

    $summary | ConvertTo-Json -Depth 8 | Set-Content -LiteralPath $jsonPath -Encoding UTF8
    Write-Host "Measurement complete."
    Write-Host "Summary: $jsonPath"
    Write-Host "Samples: $csvPath"
    $summary | ConvertTo-Json -Depth 8
}
finally {
    if ($startedByScript -and $null -ne $process -and -not $KeepProcessRunning) {
        if (-not $process.HasExited) {
            [void]$process.CloseMainWindow()
            if (-not $process.WaitForExit(5000)) {
                Stop-Process -Id $process.Id -Force -ErrorAction SilentlyContinue
            }
        }
    }
}
