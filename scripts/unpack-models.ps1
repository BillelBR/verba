# Unpacks the translation models that were downloaded from a models release (made by scripts\pack-models.ps1): checks every zip against
# SHA256SUMS.txt, then expands it into models\<pair>\.
#
#   powershell -ExecutionPolicy Bypass -File scripts\unpack-models.ps1 -From models-dl            # -> models\
#
# Exit code 0 when every model of the checksum file is there, 1 otherwise (nothing half-done is left in the output folder: a folder is only
# put in place once its zip matched the checksum and held a model.bin).

param(
    [Parameter(Mandatory = $true)][string]$From,
    [string]$Out = ""
)

$ErrorActionPreference = "Stop"
$root = Split-Path -Parent $PSScriptRoot
if (-not $Out) { $Out = Join-Path $root "models" }

function Fail($message) {
    Write-Host "unpack-models: $message" -ForegroundColor Red
    exit 1
}

$sums = Join-Path $From "SHA256SUMS.txt"
if (-not (Test-Path $sums)) { Fail "no SHA256SUMS.txt in $From" }
$entries = @(Get-Content $sums | Where-Object { $_.Trim() -ne "" })
if ($entries.Count -eq 0) { Fail "SHA256SUMS.txt is empty" }

New-Item -ItemType Directory -Force $Out | Out-Null
foreach ($line in $entries) {
    if ($line -notmatch '^([0-9a-fA-F]{64}) \*?([A-Za-z0-9_.-]+\.zip)$') { Fail "cannot read this line of SHA256SUMS.txt: $line" }
    $expected = $Matches[1].ToLower()
    $name = $Matches[2]
    $zip = Join-Path $From $name
    if (-not (Test-Path $zip)) { Fail "$name is listed but was not downloaded" }
    $actual = (Get-FileHash $zip -Algorithm SHA256).Hash.ToLower()
    if ($actual -ne $expected) { Fail "$name does not match its checksum (expected $expected, got $actual)" }
    $pair = [System.IO.Path]::GetFileNameWithoutExtension($name)
    if ($pair -notmatch '^[a-z]{2}-[a-z]{2}$') { Fail "$name is not named after a direction (ar-en.zip)" }
    $tmp = Join-Path $Out "$pair.partial"
    Remove-Item -Recurse -Force $tmp -ErrorAction SilentlyContinue
    Expand-Archive -Path $zip -DestinationPath $tmp -Force
    if (-not (Test-Path (Join-Path $tmp "model.bin"))) { Remove-Item -Recurse -Force $tmp; Fail "$name holds no model.bin" }
    $final = Join-Path $Out $pair
    Remove-Item -Recurse -Force $final -ErrorAction SilentlyContinue
    Rename-Item $tmp $pair
    Write-Host "${pair}: unpacked"
}
Write-Host "done: $Out"
