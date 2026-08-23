param(
    [Parameter(Mandatory = $true)]
    [string]$SourceDirectory,
    [string]$PreparedDirectory = "evaluation/div2k/local/prepared",
    [ValidateSet("Validation", "Train", "All")]
    [string]$Split = "Validation"
)

$ErrorActionPreference = "Stop"
$repositoryRoot = $PSScriptRoot

function Get-AbsolutePath([string]$Path) {
    if ([System.IO.Path]::IsPathRooted($Path)) {
        return [System.IO.Path]::GetFullPath($Path)
    }
    return [System.IO.Path]::GetFullPath((Join-Path $repositoryRoot $Path))
}

$sourcePath = Get-AbsolutePath $SourceDirectory
$preparedPath = Get-AbsolutePath $PreparedDirectory
$splitArgument = $Split.ToLowerInvariant()

Push-Location $repositoryRoot
try {
    python scripts/div2k_pairs.py prepare $sourcePath $preparedPath --split $splitArgument
    if ($LASTEXITCODE -ne 0) {
        throw "DIV2K pair preparation failed with exit code $LASTEXITCODE."
    }
    python scripts/div2k_pairs.py verify $sourcePath $preparedPath --split $splitArgument
    if ($LASTEXITCODE -ne 0) {
        throw "DIV2K pair verification failed with exit code $LASTEXITCODE."
    }
} finally {
    Pop-Location
}

Write-Output "Prepared and verified offline DIV2K $splitArgument pairs at $preparedPath"
