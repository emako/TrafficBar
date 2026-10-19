#Requires -Version 5.1
<#
.SYNOPSIS
    Pack TrafficMonitorBar with Velopack (Setup.exe, MSI, update feed).
.DESCRIPTION
    Runs build.ps1, copies exe + hardware dll into a clean pack dir, then vpk pack.
#>
$ErrorActionPreference = "Stop"
Set-StrictMode -Version Latest
Set-Location -LiteralPath $PSScriptRoot

& "$PSScriptRoot\build.ps1"
if (-not $?) {
    throw "build.ps1 failed"
}

$cargoToml = Get-Content -LiteralPath (Join-Path $PSScriptRoot "Cargo.toml") -Raw
if ($cargoToml -notmatch '(?m)^version\s*=\s*"([^"]+)"') {
    throw "Cannot read version from Cargo.toml"
}
$version = $Matches[1]
$msiVersion = "$version.0"

$releaseDir = Join-Path $PSScriptRoot "target\release"
$packDir = Join-Path $PSScriptRoot "target\pack"
$outDir = Join-Path $PSScriptRoot "target\setup"
$exeName = "TrafficMonitorBar.exe"
$dllName = "TrafficMonitorBar.Hardware.dll"
$exe = Join-Path $releaseDir $exeName
$dll = Join-Path $releaseDir $dllName
$icon = Join-Path $PSScriptRoot "src\assets\icons\app.ico"

foreach ($path in @($exe, $dll, $icon)) {
    if (-not (Test-Path -LiteralPath $path)) {
        throw "Missing pack input: $path"
    }
}

if (Test-Path -LiteralPath $packDir) {
    Remove-Item -LiteralPath $packDir -Recurse -Force
}
New-Item -ItemType Directory -Path $packDir | Out-Null
Copy-Item -LiteralPath $exe -Destination (Join-Path $packDir $exeName)
Copy-Item -LiteralPath $dll -Destination (Join-Path $packDir $dllName)

if (-not (Get-Command dotnet -ErrorAction SilentlyContinue)) {
    throw ".NET SDK 8+ (dotnet) is required"
}
& dotnet tool update -g vpk
if ($LASTEXITCODE -ne 0) {
    throw "dotnet tool update -g vpk failed with exit code $LASTEXITCODE"
}

# VC++ 2022 x64; use vcredist145-x64 when linking against VS 2026 toolsets.
$framework = "net472,vcredist143-x64"

& vpk pack `
    --packId TrafficMonitorBar `
    --packVersion $version `
    --packDir $packDir `
    --mainExe $exeName `
    --packTitle TrafficMonitorBar `
    --icon $icon `
    --runtime win-x64 `
    --framework $framework `
    --shortcuts "Desktop,StartMenuRoot" `
    --msi `
    --instLocation PerUser `
    --msiVersion $msiVersion `
    --outputDir $outDir

if ($LASTEXITCODE -ne 0) {
    throw "vpk pack failed with exit code $LASTEXITCODE (bump Cargo.toml version or remove the existing release under target\setup\ to re-pack the same version)"
}

Write-Host "Output: $outDir"
Get-ChildItem -LiteralPath $outDir | ForEach-Object { Write-Host ("  " + $_.Name) }
