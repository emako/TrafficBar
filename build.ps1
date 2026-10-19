#Requires -Version 5.1
<#
.SYNOPSIS
    Build the TrafficBar release binary.
.DESCRIPTION
    Stops this repo's running target\release process and the old hardware helper,
    then runs cargo build --release and deletes a leftover TrafficBar.Hardware.exe.
    Does not start the program.
#>
$ErrorActionPreference = "Stop"
Set-StrictMode -Version Latest
Set-Location -LiteralPath $PSScriptRoot

$exe = Join-Path $PSScriptRoot "target\release\TrafficBar.exe"
$helper = Join-Path $PSScriptRoot "target\release\TrafficBar.Hardware.exe"

function Get-ReleaseProcess {
    @(Get-CimInstance Win32_Process | Where-Object {
        $_.ExecutablePath -and (
            [string]::Equals($_.ExecutablePath, $exe, [StringComparison]::OrdinalIgnoreCase) -or
            [string]::Equals($_.ExecutablePath, $helper, [StringComparison]::OrdinalIgnoreCase)
        )
    })
}

$running = @(Get-ReleaseProcess)
foreach ($proc in $running) {
    Write-Host "结束进程 $($proc.ProcessId): $($proc.ExecutablePath)"
    Stop-Process -Id $proc.ProcessId -Force -ErrorAction SilentlyContinue
}

if ($running.Count -gt 0) {
    $deadline = (Get-Date).AddSeconds(15)
    do {
        $left = @(Get-ReleaseProcess)
        if ($left.Count -eq 0) { break }
        Start-Sleep -Milliseconds 200
    } while ((Get-Date) -lt $deadline)

    if ($left.Count -gt 0) {
        $paths = ($left | ForEach-Object { $_.ExecutablePath }) -join ", "
        throw "进程仍在运行，无法覆盖: $paths"
    }
}

& cargo build --release
if ($LASTEXITCODE -ne 0) {
    throw "cargo build --release 失败，退出码 $LASTEXITCODE"
}

if (Test-Path -LiteralPath $helper) {
    Remove-Item -LiteralPath $helper -Force
}

Write-Host "已生成 $exe"
