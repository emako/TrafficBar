#Requires -Version 5.1
<#
.SYNOPSIS
    Pack TrafficBar with Velopack (Setup.exe, MSI, update feed).
.DESCRIPTION
    Runs build.ps1, copies the exe into a clean pack dir, then vpk pack.
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
$exeName = "TrafficBar.exe"
$exe = Join-Path $releaseDir $exeName
$icon = Join-Path $PSScriptRoot "src\assets\icons\app.ico"

foreach ($path in @($exe, $icon)) {
    if (-not (Test-Path -LiteralPath $path)) {
        throw "Missing pack input: $path"
    }
}

if (Test-Path -LiteralPath $packDir) {
    Remove-Item -LiteralPath $packDir -Recurse -Force
}
New-Item -ItemType Directory -Path $packDir | Out-Null
Copy-Item -LiteralPath $exe -Destination (Join-Path $packDir $exeName)

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
    --packId TrafficBar `
    --packVersion $version `
    --packDir $packDir `
    --mainExe $exeName `
    --packTitle TrafficBar `
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

# vpk names these from packId + channel. Publish names include the Cargo version and arch.
$distBase = "TrafficBar-$version-x64"
$renames = @(
    @{ From = @("TrafficBar-win-Setup.exe", "TrafficBar-Setup.exe"); To = "$distBase.exe" },
    @{ From = @("TrafficBar-win.msi", "TrafficBar.msi"); To = "$distBase.msi" },
    @{ From = @("TrafficBar-win-Portable.zip", "TrafficBar-Portable.zip"); To = "$distBase.zip" }
)

$applied = New-Object System.Collections.Generic.List[object]
foreach ($rename in $renames) {
    $sourceName = $null
    foreach ($candidate in $rename.From) {
        if (Test-Path -LiteralPath (Join-Path $outDir $candidate)) {
            $sourceName = $candidate
            break
        }
    }
    if (-not $sourceName) {
        $listing = (Get-ChildItem -LiteralPath $outDir -File | ForEach-Object { $_.Name }) -join ", "
        throw "Missing pack output (tried: $($rename.From -join ', ')). Found: $listing"
    }
    $dest = Join-Path $outDir $rename.To
    if ((Join-Path $outDir $sourceName) -ne $dest -and (Test-Path -LiteralPath $dest)) {
        Remove-Item -LiteralPath $dest -Force
    }
    Rename-Item -LiteralPath (Join-Path $outDir $sourceName) -NewName $rename.To
    $applied.Add([pscustomobject]@{ From = $sourceName; To = $rename.To }) | Out-Null
}

$utf8 = New-Object System.Text.UTF8Encoding $false
Get-ChildItem -LiteralPath $outDir -Filter "assets*.json" -File | ForEach-Object {
    $json = [System.IO.File]::ReadAllText($_.FullName)
    foreach ($rename in $applied) {
        $json = $json.Replace([string]$rename.From, [string]$rename.To)
    }
    [System.IO.File]::WriteAllText($_.FullName, $json, $utf8)
}

Write-Host "Output: $outDir"
Get-ChildItem -LiteralPath $outDir | ForEach-Object { Write-Host ("  " + $_.Name) }
