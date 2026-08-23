$ErrorActionPreference = "Stop"

$sourceRoot = $PSScriptRoot
$targetTriple = "x86_64-pc-windows-msvc"
$targetDir = Join-Path $sourceRoot "target"
$sourceBinary = Join-Path $targetDir "$targetTriple\release\sr.exe"
$destinationDirectory = Join-Path $sourceRoot "bin"
$destinationBinary = Join-Path $destinationDirectory "sr.exe"
$separator = [char]0x1f
$previousOffline = $env:CARGO_NET_OFFLINE
$previousIncremental = $env:CARGO_INCREMENTAL
$previousRustFlags = $env:CARGO_ENCODED_RUSTFLAGS

try {
    $env:CARGO_NET_OFFLINE = "true"
    $env:CARGO_INCREMENTAL = "0"
    $env:CARGO_ENCODED_RUSTFLAGS = @(
        "-C",
        "link-arg=/Brepro",
        "--remap-path-prefix=$sourceRoot=."
    ) -join $separator
    Push-Location $sourceRoot
    try {
        cargo build --offline --locked --release --target $targetTriple --target-dir $targetDir
        if ($LASTEXITCODE -ne 0) {
            throw "Cargo release build failed with exit code $LASTEXITCODE."
        }
    } finally {
        Pop-Location
    }
} finally {
    $env:CARGO_NET_OFFLINE = $previousOffline
    $env:CARGO_INCREMENTAL = $previousIncremental
    $env:CARGO_ENCODED_RUSTFLAGS = $previousRustFlags
}
if (-not (Test-Path -LiteralPath $sourceBinary -PathType Leaf)) {
    throw "Release binary was not produced: $sourceBinary"
}

New-Item -ItemType Directory -Force -Path $destinationDirectory | Out-Null
Copy-Item -LiteralPath $sourceBinary -Destination $destinationBinary -Force
if ((Get-Item -LiteralPath $destinationBinary).Length -eq 0) {
    throw "Copied release binary is empty: $destinationBinary"
}

Write-Output "Built reproducible $destinationBinary for $targetTriple"
