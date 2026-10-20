#Requires -Version 5.1
<#
.SYNOPSIS
    Build TrafficBar installers with Velopack (Setup.exe, MSI, update feed).
.DESCRIPTION
    Runs build.ps1, copies the exe into a clean pack dir, then vpk pack.
.PARAMETER Target
    Rust target triple. Defaults to the host. Supported:
    x86_64-pc-windows-msvc, arm64ec-pc-windows-msvc.
#>
param(
    [Parameter(Position = 0)]
    [string] $Target = "",

    # cargo alias may forward a leading `--` (cargo setup -- -Target …).
    [Parameter(ValueFromRemainingArguments = $true)]
    [string[]] $RemainingArgs = @()
)

# Peel `cargo setup -- -Target …` into -Target.
if ([string]::IsNullOrWhiteSpace($Target) -and $RemainingArgs.Count -gt 0) {
    $argsList = [System.Collections.Generic.List[string]]::new()
    foreach ($a in $RemainingArgs) {
        if ($a -ne "--") { $argsList.Add($a) | Out-Null }
    }
    for ($i = 0; $i -lt $argsList.Count; $i++) {
        if ($argsList[$i] -eq "-Target" -and ($i + 1) -lt $argsList.Count) {
            $Target = $argsList[$i + 1]
            break
        }
        if ($argsList[$i] -like "-Target=*") {
            $Target = $argsList[$i].Substring("-Target=".Length)
            break
        }
    }
}

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

if ([string]::IsNullOrWhiteSpace($Target)) {
    $Target = Get-HostRustTarget
}

switch ($Target) {
    "x86_64-pc-windows-msvc" {
        $archSuffix = "x64"
        $vpkRuntime = "win-x64"
        $vpkChannel = "win"
        $framework = "net472,vcredist143-x64"
    }
    "arm64ec-pc-windows-msvc" {
        $archSuffix = "arm64ec"
        # Velopack has no win-arm64ec RID; gate on machine arch (WoA).
        $vpkRuntime = "win-arm64"
        $vpkChannel = "win-arm64ec"
        $framework = "net472,vcredist143-arm64"
    }
    default {
        throw "Unsupported -Target '$Target'. Use: x86_64-pc-windows-msvc, arm64ec-pc-windows-msvc"
    }
}

$hostTarget = Get-HostRustTarget
if ($Target -eq $hostTarget) {
    $releaseDir = Join-Path $PSScriptRoot "target\release"
} else {
    $releaseDir = Join-Path $PSScriptRoot "target\$Target\release"
}

& "$PSScriptRoot\build.ps1" -Target $Target
if (-not $?) {
    throw "build.ps1 failed"
}

$cargoToml = Get-Content -LiteralPath (Join-Path $PSScriptRoot "Cargo.toml") -Raw
if ($cargoToml -notmatch '(?m)^version\s*=\s*"([^"]+)"') {
    throw "Cannot read version from Cargo.toml"
}
$version = $Matches[1]
$msiVersion = "$version.0"

$packDir = Join-Path $PSScriptRoot "target\pack\$archSuffix"
$outDir = Join-Path $PSScriptRoot "target\publish"
# Per-arch staging (outside publish/) avoids RELEASES / nupkg collisions across arches.
$archOutDir = Join-Path $PSScriptRoot "target\vpk\$archSuffix"
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
New-Item -ItemType Directory -Path $packDir -Force | Out-Null
Copy-Item -LiteralPath $exe -Destination (Join-Path $packDir $exeName)

if (-not (Get-Command dotnet -ErrorAction SilentlyContinue)) {
    throw ".NET SDK 10+ (dotnet) is required"
}
& dotnet tool update -g vpk
if ($LASTEXITCODE -ne 0) {
    throw "dotnet tool update -g vpk failed with exit code $LASTEXITCODE"
}

if (Test-Path -LiteralPath $archOutDir) {
    Remove-Item -LiteralPath $archOutDir -Recurse -Force
}
New-Item -ItemType Directory -Path $archOutDir -Force | Out-Null
New-Item -ItemType Directory -Path $outDir -Force | Out-Null

# -x turns off the interactive overwrite prompt; -y answers yes.
# Re-packing the same Cargo version otherwise stops at [y/n] (default n).
& vpk pack -xy `
    --packId TrafficBar `
    --packVersion $version `
    --packDir $packDir `
    --mainExe $exeName `
    --packTitle TrafficBar `
    --icon $icon `
    --runtime $vpkRuntime `
    --channel $vpkChannel `
    --framework $framework `
    --shortcuts "Desktop,StartMenuRoot" `
    --msi `
    --instLocation PerUser `
    --msiVersion $msiVersion `
    --outputDir $archOutDir

if ($LASTEXITCODE -ne 0) {
    throw "vpk pack failed with exit code $LASTEXITCODE"
}

# vpk names these from packId + channel. Publish names include the Cargo version and arch.
$distBase = "TrafficBar-$version-$archSuffix"
$channelToken = $vpkChannel
$renames = @(
    @{
        From = @(
            "TrafficBar-$channelToken-Setup.exe",
            "TrafficBar-win-Setup.exe",
            "TrafficBar-Setup.exe"
        )
        To = "$distBase.exe"
    },
    @{
        From = @(
            "TrafficBar-$channelToken.msi",
            "TrafficBar-win.msi",
            "TrafficBar.msi"
        )
        To = "$distBase.msi"
    },
    @{
        From = @(
            "TrafficBar-$channelToken-Portable.zip",
            "TrafficBar-win-Portable.zip",
            "TrafficBar-Portable.zip"
        )
        To = "$distBase.zip"
    }
)

$applied = New-Object System.Collections.Generic.List[object]
foreach ($rename in $renames) {
    $sourceName = $null
    foreach ($candidate in $rename.From) {
        if (Test-Path -LiteralPath (Join-Path $archOutDir $candidate)) {
            $sourceName = $candidate
            break
        }
    }
    if (-not $sourceName) {
        $listing = (Get-ChildItem -LiteralPath $archOutDir -File | ForEach-Object { $_.Name }) -join ", "
        throw "Missing pack output (tried: $($rename.From -join ', ')). Found: $listing"
    }
    $dest = Join-Path $archOutDir $rename.To
    if ((Join-Path $archOutDir $sourceName) -ne $dest -and (Test-Path -LiteralPath $dest)) {
        Remove-Item -LiteralPath $dest -Force
    }
    Rename-Item -LiteralPath (Join-Path $archOutDir $sourceName) -NewName $rename.To
    $applied.Add([pscustomobject]@{ From = $sourceName; To = $rename.To }) | Out-Null
}

$utf8 = New-Object System.Text.UTF8Encoding $false
Get-ChildItem -LiteralPath $archOutDir -Filter "assets*.json" -File | ForEach-Object {
    $json = [System.IO.File]::ReadAllText($_.FullName)
    foreach ($rename in $applied) {
        $json = $json.Replace([string]$rename.From, [string]$rename.To)
    }
    [System.IO.File]::WriteAllText($_.FullName, $json, $utf8)
}
# Channel-specific feed (releases.win.json / releases.win-arm64ec.json) already uses the channel name.
Get-ChildItem -LiteralPath $archOutDir -Filter "releases*.json" -File | ForEach-Object {
    $json = [System.IO.File]::ReadAllText($_.FullName)
    $changed = $false
    foreach ($rename in $applied) {
        $next = $json.Replace([string]$rename.From, [string]$rename.To)
        if ($next -ne $json) {
            $json = $next
            $changed = $true
        }
    }
    if ($changed) {
        [System.IO.File]::WriteAllText($_.FullName, $json, $utf8)
    }
}

# Avoid clobbering the other arch's legacy RELEASES / default-named nupkg in the flat publish dir.
$releasesPath = Join-Path $archOutDir "RELEASES"
if (Test-Path -LiteralPath $releasesPath) {
    Rename-Item -LiteralPath $releasesPath -NewName "RELEASES.$channelToken"
}
Get-ChildItem -LiteralPath $archOutDir -Filter "*-full.nupkg" -File | ForEach-Object {
    if ($_.Name -notmatch [regex]::Escape($archSuffix) -and $_.Name -notmatch [regex]::Escape($channelToken)) {
        $newName = $_.Name -replace "-full\.nupkg$", "-$archSuffix-full.nupkg"
        $fromName = $_.Name
        Rename-Item -LiteralPath $_.FullName -NewName $newName
        $applied.Add([pscustomobject]@{ From = $fromName; To = $newName }) | Out-Null
    }
}
# Keep JSON + RELEASES.* feeds consistent if the nupkg was renamed.
Get-ChildItem -LiteralPath $archOutDir -File | Where-Object {
    $_.Extension -eq ".json" -or $_.Name -like "RELEASES*"
} | ForEach-Object {
    $text = [System.IO.File]::ReadAllText($_.FullName)
    $changed = $false
    foreach ($rename in $applied) {
        $next = $text.Replace([string]$rename.From, [string]$rename.To)
        if ($next -ne $text) {
            $text = $next
            $changed = $true
        }
    }
    if ($changed) {
        [System.IO.File]::WriteAllText($_.FullName, $text, $utf8)
    }
}

# Flatten into target\publish for CI / release upload. Drop stale unsuffixed names from older packs.
foreach ($stale in @("RELEASES", "TrafficBar-$version-full.nupkg")) {
    $path = Join-Path $outDir $stale
    if (Test-Path -LiteralPath $path) {
        Remove-Item -LiteralPath $path -Force
    }
}
Get-ChildItem -LiteralPath $archOutDir -File | ForEach-Object {
    $dest = Join-Path $outDir $_.Name
    if (Test-Path -LiteralPath $dest) {
        Remove-Item -LiteralPath $dest -Force
    }
    Copy-Item -LiteralPath $_.FullName -Destination $dest -Force
}

# True single-file portable: just TrafficBar.exe (no Update.exe). Distinct from the Velopack
# Portable.zip which still ships the updater layout.
$singleZip = Join-Path $outDir "TrafficBar-$version-single-$archSuffix.zip"
if (Test-Path -LiteralPath $singleZip) {
    Remove-Item -LiteralPath $singleZip -Force
}
Compress-Archive -LiteralPath $exe -DestinationPath $singleZip -CompressionLevel Optimal

Write-Host "Output: $outDir (channel=$vpkChannel, runtime=$vpkRuntime)"
Get-ChildItem -LiteralPath $outDir -File | ForEach-Object { Write-Host ("  " + $_.Name) }
