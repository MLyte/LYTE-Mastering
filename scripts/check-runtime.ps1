param([switch]$RequireBundled)
$projectRoot = Split-Path $PSScriptRoot -Parent
$missing = @()
$engine = Join-Path $projectRoot 'bin\phase_limiter.exe'
$cache = Join-Path $projectRoot 'resources\phaselimiter\sound_quality2_cache'
if (Test-Path -LiteralPath $engine -PathType Leaf) { Write-Host "OK PhaseLimiter: $engine" } else { $missing += 'bin/phase_limiter.exe (and its matching Windows DLLs)' }
if ((Test-Path -LiteralPath $cache -PathType Leaf) -and (Get-Item -LiteralPath $cache).Length -gt 0) { Write-Host "OK cache: $cache" } else { $missing += 'resources/phaselimiter/sound_quality2_cache (nonempty file from the same release)' }
$ff = Join-Path $projectRoot 'bin\ffmpeg.exe'
if (-not $RequireBundled -and -not (Test-Path -LiteralPath $ff -PathType Leaf)) { $command = Get-Command ffmpeg.exe -ErrorAction SilentlyContinue; $ff = if ($command) { $command.Source } else { $null } }
if ($ff -and (Test-Path -LiteralPath $ff -PathType Leaf)) { Write-Host "OK FFmpeg: $ff" } else { $missing += 'bin/ffmpeg.exe (or ffmpeg.exe in PATH)' }
$compiler = Get-Command cargo -ErrorAction SilentlyContinue
if ($compiler) { Write-Host "OK Cargo: $($compiler.Source)" } elseif (Test-Path (Join-Path $projectRoot '.tools\cargo\bin\cargo.exe')) { Write-Host 'OK local Cargo: run . ./scripts/use-local-rust.ps1 first' } else { Write-Host 'BUILD prerequisite missing: Rust MSVC toolchain (Cargo).' }
if ($RequireBundled) {
    foreach ($dll in @('sndfile.dll','boost_system.dll','boost_filesystem.dll','boost_serialization.dll','boost_math_tr1.dll','boost_iostreams.dll','tbb.dll','tbbmalloc.dll','libbz2.dll','zlib.dll','zstd.dll')) {
        if (-not (Test-Path -LiteralPath (Join-Path $projectRoot "bin\$dll") -PathType Leaf)) { $missing += "bin/$dll (PhaseLimiter v0.2.0 dependency)" }
    }
}
if ($missing.Count) { foreach ($item in $missing) { Write-Host "MISSING: $item" }; exit 1 }
Write-Host 'Audio files are present. This presence check does not validate DLL compatibility or a mastering run.'
