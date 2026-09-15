param(
    [Parameter(Mandatory=$true)][string]$PhaseLimiterDirectory,
    [string]$FfmpegPath
)
$ErrorActionPreference = 'Stop'
$projectRoot = Split-Path $PSScriptRoot -Parent
$source = (Resolve-Path -LiteralPath $PhaseLimiterDirectory).Path
$sourceBin = Join-Path $source 'bin'
$sourceResources = Join-Path $source 'resource'
foreach ($required in @((Join-Path $sourceBin 'phase_limiter.exe'), (Join-Path $sourceResources 'sound_quality2_cache'))) {
    if (-not (Test-Path -LiteralPath $required -PathType Leaf)) { throw "Missing release file: $required. Pass the extracted phaselimiter directory (containing bin and resource)." }
}
$destinationBin = Join-Path $projectRoot 'bin'
$destinationResources = Join-Path $projectRoot 'resources\phaselimiter'
# Copy only the engine and matching DLLs, not the old GTK GUI.
Get-ChildItem -LiteralPath $sourceBin -File | Where-Object { $_.Name -eq 'phase_limiter.exe' -or $_.Extension -eq '.dll' } | ForEach-Object {
    Copy-Item -LiteralPath $_.FullName -Destination $destinationBin
}
Copy-Item -LiteralPath (Join-Path $sourceResources 'sound_quality2_cache') -Destination $destinationResources
foreach ($notice in @('LICENSE','licenses')) {
    $noticePath = Join-Path $source $notice
    if (Test-Path -LiteralPath $noticePath) { Copy-Item -LiteralPath $noticePath -Destination $destinationResources -Recurse -Force }
}
if ($FfmpegPath) {
    $ff = (Resolve-Path -LiteralPath $FfmpegPath).Path
    if ([IO.Path]::GetFileName($ff) -ne 'ffmpeg.exe') { throw 'FfmpegPath must point to ffmpeg.exe.' }
    Copy-Item -LiteralPath $ff -Destination $destinationBin
    Write-Host 'FFmpeg copied. Also preserve the license and matching source/build information from its distribution.'
}
& (Join-Path $PSScriptRoot 'check-runtime.ps1')
