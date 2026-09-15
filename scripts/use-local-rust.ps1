# Dot-source this file: . .\scripts\use-local-rust.ps1
$projectRoot = Split-Path $PSScriptRoot -Parent
$env:RUSTUP_HOME = Join-Path $projectRoot '.tools\rustup'
$env:CARGO_HOME = Join-Path $projectRoot '.tools\cargo'
$env:PATH = (Join-Path $env:CARGO_HOME 'bin') + ';' + $env:PATH
