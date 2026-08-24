$ErrorActionPreference = "Stop"

$sourceRoot = $PSScriptRoot
$toolchainVersion = "1.85.0"
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
    $actualRustc = & rustc "+$toolchainVersion" --version
    if ($LASTEXITCODE -ne 0 -or $actualRustc -ne "rustc $toolchainVersion (4d91de4e4 2025-02-17)") {
        throw "Rust $toolchainVersion is required; received: $actualRustc"
    }
    $env:CARGO_NET_OFFLINE = "true"
    $env:CARGO_INCREMENTAL = "0"
    $env:CARGO_ENCODED_RUSTFLAGS = @(
        "-C",
        "link-arg=/Brepro",
        "-C",
        "target-feature=+crt-static",
        "--remap-path-prefix=$sourceRoot=."
    ) -join $separator
    Push-Location $sourceRoot
    try {
        cargo "+$toolchainVersion" build --offline --locked --release --target $targetTriple --target-dir $targetDir
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
