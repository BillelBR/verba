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
foreach ($lang in "en", "fr", "ar") {
    $out = Join-Path $dir "$lang.txt"
    Write-Host "downloading $lang ..."
    Get-FileWithRetry "$base/$lang/${lang}_50k.txt" $out
    $lines = (Get-Content $out -Encoding UTF8 | Measure-Object -Line).Lines
    Write-Host "  $lang.txt: $lines words"
}
Write-Host "done: $dir"
