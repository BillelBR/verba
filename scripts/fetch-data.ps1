# Downloads the word lists Verba's local engine uses into data/words/.
# They are NOT part of the repository: each list keeps its own license (see docs/DATA.md).
#
#   powershell -ExecutionPolicy Bypass -File scripts\fetch-data.ps1

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
New-Item -ItemType Directory -Force $dir | Out-Null

# hermitdave/FrequencyWords, 2018 lists built from OpenSubtitles. Code MIT, content CC-BY-SA-4.0.
$base = "https://raw.githubusercontent.com/hermitdave/FrequencyWords/master/content/2018"
# How many lines of each list are kept (the most frequent ones). English and Arabic are longer than French: measured on real text, a longer
# list finds more rare words (docs/ACCURACY.md, "Size of the lists"). The engine cuts each list again to `list_cap` (crates/verba-local/src/lib.rs).
$keep = @{ en = 150000; fr = 150000; ar = 300000 }
$utf8 = New-Object System.Text.UTF8Encoding($false)
foreach ($lang in "en", "fr", "ar") {
    $out = Join-Path $dir "$lang.txt"
    $n = $keep[$lang]
    Write-Host "downloading $lang ..."
    if ($n -le 50000) {
        # the 50k file of the project is exactly the size wanted
        Get-FileWithRetry "$base/$lang/${lang}_50k.txt" $out
    } else {
        # the full list (millions of lines, sorted by frequency): only the first lines are kept
        $full = Join-Path $dir "$lang.full.tmp"
        Get-FileWithRetry "$base/$lang/${lang}_full.txt" $full
        $top = Get-Content $full -Encoding UTF8 -TotalCount $n
        [System.IO.File]::WriteAllLines($out, $top, $utf8)
        Remove-Item $full -Force
    }
    $lines = (Get-Content $out -Encoding UTF8 | Measure-Object -Line).Lines
    Write-Host "  $lang.txt: $lines words"
}
Write-Host "done: $dir"
