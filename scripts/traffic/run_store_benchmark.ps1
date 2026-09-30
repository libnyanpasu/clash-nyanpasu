param(
    [Parameter(Mandatory = $true)][string]$Executable,
    [Parameter(Mandatory = $true)][string]$OutputRoot,
    [Parameter(Mandatory = $true)][string]$TempRoot,
    [int]$Rounds = 3
)
$ErrorActionPreference = 'Stop'
$benchmarkExecutable = (Resolve-Path -LiteralPath $Executable).Path
$benchmarkOutput = [System.IO.Path]::GetFullPath($OutputRoot)
$benchmarkTemp = [System.IO.Path]::GetFullPath($TempRoot)
New-Item -ItemType Directory -Force -Path $benchmarkOutput, $benchmarkTemp | Out-Null
$env:NYANPASU_TRAFFIC_PERF_TEMP_DIR = $benchmarkTemp
for ($round = 1; $round -le $Rounds; $round++) {
    $caseIndex = 0
    foreach ($case in @('1000-active', '10000-active', '100000-closed', '1000000-closed')) {
        $stores = if (($round + $caseIndex) % 2 -eq 1) { @('redb', 'turso') } else { @('turso', 'redb') }
        foreach ($store in $stores) {
            $runName = "r$round-$store-$case"
            $resultPath = Join-Path $benchmarkOutput "$runName.json"
            if (Test-Path -LiteralPath $resultPath) { throw "Refusing to overwrite $resultPath" }
            $env:NYANPASU_TRAFFIC_PERF_CASE = $case
            $env:NYANPASU_TRAFFIC_PERF_STORE = $store
            $env:NYANPASU_TRAFFIC_PERF_OUTPUT = $resultPath
            $start = [DateTimeOffset]::UtcNow
            Write-Output "BENCHMARK_START $runName $($start.ToString('o'))"
            $process = Start-Process -FilePath $benchmarkExecutable -ArgumentList @('--ignored', '--exact', 'benchmark::measured_session_workloads', '--nocapture') -WindowStyle Hidden -PassThru -Wait -RedirectStandardOutput (Join-Path $benchmarkOutput "$runName.stdout.log") -RedirectStandardError (Join-Path $benchmarkOutput "$runName.stderr.log")
            if ($process.ExitCode -ne 0) { throw "$runName failed with exit code $($process.ExitCode); inspect logs" }
            $result = Get-Content -LiteralPath $resultPath -Raw | ConvertFrom-Json
            Write-Output "BENCHMARK_DONE $runName elapsed_s=$($result.elapsed_s) observation_mean_ms=$($result.observation_distribution.mean_ms) store_mean_ms=$($result.store_commit_distribution.mean_ms)"
            # tempfile owns cleanup inside the fresh child process. Any residual
            # directories are preserved for diagnosis; this runner deletes nothing.
        }
        $caseIndex++
    }
}
