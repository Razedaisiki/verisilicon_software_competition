param(
    [string]$PreparedDirectory = "evaluation/eval30/local/prepared",
    [string]$Report = "target/quality-sweep.csv",
    [ValidateRange(2, 10)]
    [int]$Folds = 5
)

$ErrorActionPreference = "Stop"
$sourceRoot = $PSScriptRoot

function Get-AbsolutePath([string]$Path) {
    if ([System.IO.Path]::IsPathRooted($Path)) {
        return [System.IO.Path]::GetFullPath($Path)
    }
    return [System.IO.Path]::GetFullPath((Join-Path $sourceRoot $Path))
}

$preparedPath = Get-AbsolutePath $PreparedDirectory
$reportPath = Get-AbsolutePath $Report
$catalogPath = Join-Path $sourceRoot "evaluation/eval30/sources"
$manifestPath = Join-Path $preparedPath "pairs.tsv"
$binaryPath = Join-Path $sourceRoot "target/release/examples/quality_sweep.exe"
$reportParent = Split-Path -Parent $reportPath

Push-Location $sourceRoot
try {
    python scripts/eval_dataset.py verify $catalogPath $preparedPath
    if ($LASTEXITCODE -ne 0) {
        throw "Eval30 verification failed with exit code $LASTEXITCODE."
    }

    cargo build --offline --locked --release --example quality_sweep
    if ($LASTEXITCODE -ne 0) {
        throw "Quality sweep build failed with exit code $LASTEXITCODE."
    }

    New-Item -ItemType Directory -Force -Path $reportParent | Out-Null
    & $binaryPath $manifestPath $reportPath $Folds
    if ($LASTEXITCODE -ne 0) {
        throw "Quality sweep failed with exit code $LASTEXITCODE."
    }
} finally {
    Pop-Location
}

Write-Output "Wrote quality sweep report to $reportPath"
