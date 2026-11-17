# Downloads Tatoeba sentences and builds Verba's next-word tables: data/words/{en,fr,ar}.bigrams.txt.
# Run scripts/fetch-data.ps1 first (the word lists are used to leave out words that are a trait of the corpus).
# The sentences and the tables are NOT part of the repository: they keep their own license (see docs/DATA.md).
#
#   powershell -ExecutionPolicy Bypass -File scripts\fetch-bigrams.ps1
#   powershell -ExecutionPolicy Bypass -File scripts\fetch-bigrams.ps1 -Lang en,fr    # only some languages
#   powershell -ExecutionPolicy Bypass -File scripts\fetch-bigrams.ps1 -Clean         # also delete the downloaded sentences afterwards
#
# Sentences: Tatoeba (https://tatoeba.org), per-language exports, CC BY 2.0 FR, (c) Tatoeba contributors.
# The exports are .bz2 files: Windows cannot open them by itself, so this script uses Python 3, bzip2 or 7-Zip, whichever it finds.
# The sentences stay in data/words/src/ (the accuracy tool `nextword_accuracy` measures on them); -Clean removes them.

param(
    [string[]]$Lang = @("en", "fr", "ar"),
    [switch]$Clean
)

$ErrorActionPreference = "Stop"

# Downloads a file and tries again when the server does not answer (GitHub's runners sometimes cannot reach a host for a minute).
function Get-FileWithRetry([string]$Uri, [string]$OutFile) {
    for ($try = 1; $try -le 6; $try++) {
        try {
            Invoke-WebRequest -Uri $Uri -OutFile $OutFile -UseBasicParsing -TimeoutSec 600
            return
        } catch {
            if (Test-Path $OutFile) { Remove-Item $OutFile -Force -ErrorAction SilentlyContinue }
            if ($try -eq 6) { throw }
            Write-Host "download failed ($($_.Exception.Message)); trying again in $($try * 10) seconds ($try of 5 retries) ..."
            Start-Sleep -Seconds ($try * 10)
        }
    }
}

$root = Split-Path -Parent $PSScriptRoot
$dir = Join-Path $root "data\words"
$src = Join-Path $dir "src"
New-Item -ItemType Directory -Force $src | Out-Null

# the Tatoeba code of each language (Arabic is `ara`, a single macro-language export)
$codes = @{ en = "eng"; fr = "fra"; ar = "ara" }

function Expand-Bzip2($from, $to) {
    $python = Get-Command python -ErrorAction SilentlyContinue
    if (-not $python) { $python = Get-Command py -ErrorAction SilentlyContinue }
    $bzip2 = Get-Command bzip2 -ErrorAction SilentlyContinue
    $sevenZip = Get-Command 7z -ErrorAction SilentlyContinue
    if ($python) {
        # -I: do not load anything from the current folder
        & $python.Source -I -c "import bz2, shutil, sys; shutil.copyfileobj(bz2.open(sys.argv[1], 'rb'), open(sys.argv[2], 'wb'))" $from $to
        if ($LASTEXITCODE -ne 0) { throw "python could not unpack $from" }
    } elseif ($bzip2) {
        & cmd /c "`"$($bzip2.Source)`" -dc `"$from`" > `"$to`""
        if ($LASTEXITCODE -ne 0) { throw "bzip2 could not unpack $from" }
    } elseif ($sevenZip) {
        & $sevenZip.Source e -y "-o$(Split-Path -Parent $to)" $from | Out-Null
        if ($LASTEXITCODE -ne 0) { throw "7z could not unpack $from" }
    } else {
        throw "Cannot unpack .bz2 files: install Python 3, bzip2 or 7-Zip, then run this script again."
    }
}

foreach ($l in $Lang) {
    if (-not $codes.ContainsKey($l)) { throw "unknown language '$l' (use en, fr or ar)" }
    $t = $codes[$l]
    $bz = Join-Path $src "${t}_sentences.tsv.bz2"
    $tsv = Join-Path $src "${t}_sentences.tsv"
    if (-not (Test-Path (Join-Path $dir "$l.txt"))) { throw "data\words\$l.txt is missing: run scripts\fetch-data.ps1 first" }
    Write-Host "downloading Tatoeba $t sentences ..."
    Get-FileWithRetry "https://downloads.tatoeba.org/exports/per_language/$t/${t}_sentences.tsv.bz2" $bz
    Write-Host "unpacking ..."
    Expand-Bzip2 $bz $tsv
    Remove-Item $bz
    Write-Host "building $l.bigrams.txt ..."
    Push-Location $root
    try {
        cargo run --release -q -p verba-local --example build_bigrams -- --lang $l --tsv $tsv --out (Join-Path $dir "$l.bigrams.txt")
        if ($LASTEXITCODE -ne 0) { throw "build_bigrams failed for $l" }
    } finally {
        Pop-Location
    }
    if ($Clean) { Remove-Item $tsv }
}
Write-Host "done: $dir"
