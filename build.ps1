#Requires -Version 5.1
<#
.SYNOPSIS
    Build the TrafficBar release binary.
.DESCRIPTION
    Stops this repo's running target release process and the old hardware helper,
    enters the matching MSVC environment when needed, then runs cargo build --release.
    Does not start the program.
.PARAMETER Target
    Rust target triple. Defaults to the host (no --target). Use
    x86_64-pc-windows-msvc or arm64ec-pc-windows-msvc for an explicit arch.
#>
param(
    [string] $Target = ""
)

$ErrorActionPreference = "Stop"
Set-StrictMode -Version Latest
Set-Location -LiteralPath $PSScriptRoot

function Get-HostRustTarget {
    $raw = & rustc -vV 2>$null
    if ($LASTEXITCODE -ne 0) {
        throw "rustc -vV failed"
    }
    foreach ($line in $raw) {
        if ($line -match '^host:\s*(.+)$') {
            return $Matches[1].Trim()
        }
    }
    throw "Could not parse host from rustc -vV"
}

function Get-VsInstallPath {
    $vswhere = Join-Path ${env:ProgramFiles(x86)} "Microsoft Visual Studio\Installer\vswhere.exe"
    if (-not (Test-Path -LiteralPath $vswhere)) {
        return $null
    }
    $path = & $vswhere -latest -products * -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath
    if ($LASTEXITCODE -ne 0 -or [string]::IsNullOrWhiteSpace($path)) {
        $path = & $vswhere -latest -products * -property installationPath
    }
    if ([string]::IsNullOrWhiteSpace($path)) {
        return $null
    }
    return $path.Trim()
}

function Test-Arm64CrossClInstalled {
    param([string] $VsInstallPath)
    $msvcRoot = Join-Path $VsInstallPath "VC\Tools\MSVC"
    if (-not (Test-Path -LiteralPath $msvcRoot)) {
        return $false
    }
    $found = Get-ChildItem -LiteralPath $msvcRoot -Directory -ErrorAction SilentlyContinue |
        ForEach-Object { Join-Path $_.FullName "bin\Hostx64\arm64\cl.exe" } |
        Where-Object { Test-Path -LiteralPath $_ }
    return ($null -ne $found -and @($found).Count -gt 0)
}

function Import-VcVars {
    param(
        [Parameter(Mandatory = $true)]
        [string] $VcArch
    )

    $vs = Get-VsInstallPath
    if (-not $vs) {
        throw "Visual Studio with C++ tools was not found (vswhere)."
    }
    $vcvars = Join-Path $vs "VC\Auxiliary\Build\vcvarsall.bat"
    if (-not (Test-Path -LiteralPath $vcvars)) {
        throw "vcvarsall.bat not found at $vcvars"
    }

    if ($VcArch -in @("x64_arm64", "amd64_arm64", "arm64") -and -not (Test-Arm64CrossClInstalled -VsInstallPath $vs)) {
        throw @"
MSVC ARM64/ARM64EC build tools are missing (no Hostx64\arm64\cl.exe).
In Visual Studio Installer, add individual component:
  MSVC v*** - VS *** C++ ARM64/ARM64EC build tools
  (Microsoft.VisualStudio.Component.VC.Tools.ARM64)
Then re-run this script.
"@
    }

    # Capture env after vcvarsall; ARM64EC uses the ARM64 cross tools (cl /arm64EC).
    $cmd = "`"$vcvars`" $VcArch >nul 2>&1 && set"
    $output = & cmd /c $cmd
    if ($LASTEXITCODE -ne 0) {
        throw "vcvarsall.bat $VcArch failed (exit $LASTEXITCODE). Install MSVC ARM64/ARM64EC build tools for arm64ec targets."
    }
    foreach ($line in $output) {
        if ($line -match '^(.*?)=(.*)$') {
            $name = $Matches[1]
            $value = $Matches[2]
            [System.Environment]::SetEnvironmentVariable($name, $value, "Process")
        }
    }
    Write-Host "MSVC env: vcvarsall $VcArch"
}

function Test-ClAvailable {
    $null -ne (Get-Command cl.exe -ErrorAction SilentlyContinue)
}

if ([string]::IsNullOrWhiteSpace($Target)) {
    $Target = Get-HostRustTarget
}

$supported = @(
    "x86_64-pc-windows-msvc",
    "arm64ec-pc-windows-msvc"
)
if ($Target -notin $supported) {
    throw "Unsupported -Target '$Target'. Use: $($supported -join ', ')"
}

$hostTarget = Get-HostRustTarget
$useExplicitTarget = $Target -ne $hostTarget
if ($useExplicitTarget) {
    $releaseDir = Join-Path $PSScriptRoot "target\$Target\release"
} else {
    # Host build without --target keeps the conventional target\release path.
    $releaseDir = Join-Path $PSScriptRoot "target\release"
}

switch ($Target) {
    "x86_64-pc-windows-msvc" { $vcArch = "x64" }
    "arm64ec-pc-windows-msvc" {
        # ARM64EC is built with the ARM64 toolset (see Microsoft Arm64EC docs).
        if ($hostTarget -eq "aarch64-pc-windows-msvc") {
            $vcArch = "arm64"
        } else {
            $vcArch = "x64_arm64"
        }
    }
}

if ($Target -eq "arm64ec-pc-windows-msvc" -or -not (Test-ClAvailable)) {
    Import-VcVars -VcArch $vcArch
}

if (-not (Test-ClAvailable)) {
    throw "cl.exe not on PATH after vcvarsall $vcArch"
}

$exe = Join-Path $releaseDir "TrafficBar.exe"
$helper = Join-Path $releaseDir "TrafficBar.Hardware.exe"

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

$cargoArgs = @("build", "--release")
if ($useExplicitTarget) {
    $cargoArgs += @("--target", $Target)
}
Write-Host "cargo $($cargoArgs -join ' ')"
& cargo @cargoArgs
if ($LASTEXITCODE -ne 0) {
    throw "cargo build --release 失败，退出码 $LASTEXITCODE"
}

if (Test-Path -LiteralPath $helper) {
    Remove-Item -LiteralPath $helper -Force
}

Write-Host "已生成 $exe"
