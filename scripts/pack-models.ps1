# Packs the converted translation models (models\<pair>\) into one zip per direction and a checksum file, to be published as the assets of
# a GitHub release (see .github/workflows/models.yml). A Verba release then downloads these instead of converting from Hugging Face.
#
#   powershell -ExecutionPolicy Bypass -File scripts\pack-models.ps1                    # models\ -> dist-models\
#   powershell -ExecutionPolicy Bypass -File scripts\pack-models.ps1 -Models D:\m -Out D:\out
#
# Output: <Out>\<pair>.zip for every folder of <Models> that has a model.bin, and <Out>\SHA256SUMS.txt in the usual `sha256sum` format.

param(
    [string]$Models = "",
    [string]$Out = ""
)

$ErrorActionPreference = "Stop"
$root = Split-Path -Parent $PSScriptRoot
if (-not $Models) { $Models = Join-Path $root "models" }
if (-not $Out) { $Out = Join-Path $root "dist-models" }

$dirs = @(Get-ChildItem $Models -Directory -ErrorAction SilentlyContinue | Where-Object { Test-Path (Join-Path $_.FullName "model.bin") })
if ($dirs.Count -eq 0) { Write-Host "pack-models: no converted model in $Models (run scripts\fetch-models.ps1 first)" -ForegroundColor Red; exit 1 }

New-Item -ItemType Directory -Force $Out | Out-Null
Get-ChildItem $Out -Filter *.zip -ErrorAction SilentlyContinue | Remove-Item -Force
$lines = @()
foreach ($d in ($dirs | Sort-Object Name)) {
    $zip = Join-Path $Out "$($d.Name).zip"
    Compress-Archive -Path (Join-Path $d.FullName "*") -DestinationPath $zip -CompressionLevel Optimal
    $hash = (Get-FileHash $zip -Algorithm SHA256).Hash.ToLower()
    $lines += "$hash *$($d.Name).zip"
    Write-Host ("{0}: {1:N1} MB" -f $d.Name, ((Get-Item $zip).Length / 1MB))
}
[System.IO.File]::WriteAllText((Join-Path $Out "SHA256SUMS.txt"), (($lines -join "`n") + "`n"), (New-Object System.Text.UTF8Encoding($false)))
Write-Host "done: $Out"
