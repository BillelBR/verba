# Downloads the OPUS-MT translation models for sentence translation and converts them for CTranslate2 (int8).
#
#   powershell -ExecutionPolicy Bypass -File scripts\fetch-models.ps1                 # Arabic <-> English
#   powershell -ExecutionPolicy Bypass -File scripts\fetch-models.ps1 -Pairs ar-en,en-ar,en-fr,fr-en
#   powershell -ExecutionPolicy Bypass -File scripts\fetch-models.ps1 -PythonExe C:\Python312\python.exe    # a Python 3.10 to 3.13 that is not the first one found
#
# Output: models\<pair>\ with model.bin, shared_vocabulary.json, config.json, source.spm, target.spm and SOURCE.txt (where it came from).
# Which model each direction uses (the table $models below):
#   ar-en   Helsinki-NLP/opus-mt-tc-big-ar-en, the big model (245 MB), CC-BY-4.0. The checkpoint on the Hub's main branch is broken (it gives
#           "IDIDID..."); the corrected one is in an unmerged pull request (#7) and is pinned here by its commit.
#   others  Helsinki-NLP/opus-mt-<pair>, the base model (78 MB), Apache-2.0.
# Both are trained on OPUS parallel text; see THIRD-PARTY.md and docs/DATA.md.
#
# Needs: Python 3.10 to 3.13 and an internet connection. It makes a temporary Python environment (about 1.5 GB: PyTorch is only needed for
# the conversion), downloads the original weights (300 to 480 MB per direction), converts, and deletes both again.
# A direction that is already converted from the same repository is skipped (a folder made from another one is made again).

param(
    [string[]]$Pairs = @("ar-en", "en-ar"),
    [string]$OutDir = "",
    # a Python 3.10 to 3.13 to use, when the one found on its own is not the right one
    [string]$PythonExe = "",
    [switch]$KeepEnvironment
)

$ErrorActionPreference = "Stop"
$root = Split-Path -Parent $PSScriptRoot
if (-not $OutDir) { $OutDir = Join-Path $root "models" }

function Fail($message) {
    Write-Host ""
    Write-Host "fetch-models: $message" -ForegroundColor Red
    exit 1
}

foreach ($p in $Pairs) { if ($p -notmatch '^[a-z]{2}-[a-z]{2}$') { Fail "not a direction: $p (use names like ar-en)" } }

# repository, revision (empty: the latest), compute type of the converted model, license, and a note for SOURCE.txt, per direction.
# The compute type is int8_float32 for the big model because its checkpoint is stored in half precision (with plain int8 CTranslate2
# prints a warning at every load); the base models are stored in full precision.
$models = @{
    "ar-en" = @{
        Repo = "Helsinki-NLP/opus-mt-tc-big-ar-en"; Revision = "b59ec62dbea19d7b18d71008e0a9c452aa4362b5"; Quant = "int8_float32"; License = "CC-BY-4.0"
        Note = "The revision is the corrected checkpoint of pull request 7 (the one on the main branch gives garbage); converted to int8 with float32 compute."
    }
}
function Get-ModelSpec([string]$pair) {
    if ($models.ContainsKey($pair)) { return $models[$pair] }
    return @{ Repo = "Helsinki-NLP/opus-mt-$pair"; Revision = ""; Quant = "int8"; License = "Apache-2.0"; Note = "Converted to int8 for CTranslate2." }
}
# a direction is done when its model is there and was made from the repository the table names now
function Test-Done([string]$pair) {
    $dir = Join-Path $OutDir $pair
    if (-not (Test-Path (Join-Path $dir "model.bin"))) { return $false }
    $source = Join-Path $dir "SOURCE.txt"
    if (-not (Test-Path $source)) { return $false }
    return ((Get-Content $source -Raw) -like "*$((Get-ModelSpec $pair).Repo) at revision*")
}
$todo = @($Pairs | Where-Object { -not (Test-Done $_) })
if ($todo.Count -eq 0) { Write-Host "all models are there already ($($Pairs -join ', '))"; exit 0 }

$pyCmd = $null
if ($PythonExe) { $pyCmd = @($PythonExe) }
$candidates = if ($PythonExe) { @() } else { @("py -3.12", "py -3.13", "py -3.11", "py -3.10", "python") }
foreach ($candidate in $candidates) {
    $parts = $candidate -split " "
    $cmd = Get-Command $parts[0] -ErrorAction SilentlyContinue
    if (-not $cmd) { continue }
    $probe = @($parts | Select-Object -Skip 1) + @("-c", "import sys; print('%d.%d' % sys.version_info[:2])")
    $version = $null
    # (the Windows launcher writes to stderr when the version asked for is not installed: that only means "not this one")
    $ErrorActionPreference = "Continue"
    try { $version = & $cmd.Source @probe 2>$null } catch { }
    $ErrorActionPreference = "Stop"
    if ($LASTEXITCODE -eq 0 -and $version -match '^3\.(1[0-3])$') { $pyCmd = @($cmd.Source) + @($parts | Select-Object -Skip 1); break }
}
if (-not $pyCmd) { Fail "Python 3.10 to 3.13 was not found (CTranslate2's converter does not run on newer versions yet). Install it from https://www.python.org/downloads/ and try again." }

$script = $null
$env1 = Join-Path $env:TEMP "verba-models-env"
$raw = Join-Path $env:TEMP "verba-models-raw"
try {
    if (-not (Test-Path (Join-Path $env1 "Scripts\python.exe"))) {
        Write-Host "making a temporary Python environment (about 1.5 GB, deleted at the end) ..."
        & $pyCmd[0] @($pyCmd | Select-Object -Skip 1) -m venv $env1
        if ($LASTEXITCODE -ne 0) { Fail "could not make the Python environment" }
        $py = Join-Path $env1 "Scripts\python.exe"
        & $py -m pip install --quiet --disable-pip-version-check --upgrade pip
        # PyTorch from its CPU-only index (the default wheel is several times bigger); the others from PyPI
        & $py -m pip install --quiet --disable-pip-version-check torch --index-url https://download.pytorch.org/whl/cpu
        if ($LASTEXITCODE -ne 0) { Fail "could not install PyTorch" }
        # (the versions the conversion was checked with: CTranslate2's converter needs transformers 5)
        & $py -m pip install --quiet --disable-pip-version-check "ctranslate2==4.9.0" "transformers==5.23.0" sentencepiece huggingface_hub safetensors numpy pyyaml sacremoses protobuf
        if ($LASTEXITCODE -ne 0) { Fail "could not install CTranslate2 and transformers" }
    }
    $py = Join-Path $env1 "Scripts\python.exe"

    $script = Join-Path $env:TEMP "verba-convert-model.py"
    @'
import os, subprocess, sys, shutil
from huggingface_hub import HfApi, snapshot_download

pair, raw_root, out_root, repo, revision, quant, license_name, note = sys.argv[1:9]
revision = revision or None
info = HfApi().model_info(repo, revision=revision)
files = [s.rfilename for s in info.siblings]
weights = "model.safetensors" if "model.safetensors" in files else "pytorch_model.bin"
raw = os.path.join(raw_root, pair)
snapshot_download(repo, revision=revision, local_dir=raw, allow_patterns=[weights, "*.json", "*.spm", "vocab*"])
out = os.path.join(out_root, pair)
tmp = out + ".partial"
shutil.rmtree(tmp, ignore_errors=True)
r = subprocess.run([sys.executable, "-c", "from ctranslate2.converters.transformers import main; main()",
                    "--model", raw, "--output_dir", tmp, "--quantization", quant,
                    "--copy_files", "source.spm", "target.spm"])
if r.returncode != 0:
    sys.exit("conversion of %s failed" % pair)
for needed in ("model.bin", "shared_vocabulary.json", "config.json", "source.spm", "target.spm"):
    if not os.path.isfile(os.path.join(tmp, needed)):
        sys.exit("%s is missing from the converted %s" % (needed, pair))
with open(os.path.join(tmp, "SOURCE.txt"), "w", encoding="utf-8") as f:
    f.write("%s at revision %s (license: %s). %s\n" % (repo, info.sha, license_name, note))
shutil.rmtree(out, ignore_errors=True)
os.replace(tmp, out)
print("%s: done (%.0f MB)" % (pair, os.path.getsize(os.path.join(out, "model.bin")) / 1e6))
'@ | Set-Content -Path $script -Encoding utf8

    New-Item -ItemType Directory -Force $OutDir | Out-Null
    foreach ($pair in $todo) {
        Write-Host "${pair}: downloading and converting ..."
        $spec = Get-ModelSpec $pair
        & $py -I $script $pair $raw $OutDir $spec.Repo $spec.Revision $spec.Quant $spec.License $spec.Note
        if ($LASTEXITCODE -ne 0) { Fail "could not prepare ${pair}" }
    }
}
finally {
    if ($script) { Remove-Item -Force $script -ErrorAction SilentlyContinue }
    Remove-Item -Recurse -Force $raw -ErrorAction SilentlyContinue
    if (-not $KeepEnvironment) { Remove-Item -Recurse -Force $env1 -ErrorAction SilentlyContinue }
}
Write-Host ""
Write-Host "done: $OutDir"
