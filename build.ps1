$ErrorActionPreference = "Stop"

cargo build --locked --release
New-Item -ItemType Directory -Force -Path "bin" | Out-Null

$sourceBinary = Join-Path "target" "release\sr.exe"
$destinationBinary = Join-Path "bin" "sr.exe"
if (-not (Test-Path -LiteralPath $sourceBinary -PathType Leaf)) {
    throw "Release binary was not produced: $sourceBinary"
}

Copy-Item -LiteralPath $sourceBinary -Destination $destinationBinary -Force
if ((Get-Item -LiteralPath $destinationBinary).Length -eq 0) {
    throw "Copied release binary is empty: $destinationBinary"
}

Write-Output "Built $destinationBinary"
