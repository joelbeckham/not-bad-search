$env:RUSTUP_HOME = Join-Path $PSScriptRoot '..\.tools\rustup'
$env:CARGO_HOME = Join-Path $PSScriptRoot '..\.tools\cargo'
$env:PATH = "$env:CARGO_HOME\bin;$(Join-Path $PSScriptRoot '..\.tools\w64devkit\bin');$env:PATH"
$env:RUSTFLAGS = '-C link-self-contained=yes'
