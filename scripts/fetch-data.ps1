# Downloads the word lists Verba's local engine uses into data/words/.
# They are NOT part of the repository: each list keeps its own license (see docs/DATA.md).
#
#   powershell -ExecutionPolicy Bypass -File scripts\fetch-data.ps1

$ErrorActionPreference = "Stop"
$root = Split-Path -Parent $PSScriptRoot
$dir = Join-Path $root "data\words"
New-Item -ItemType Directory -Force $dir | Out-Null

# hermitdave/FrequencyWords, 2018 lists built from OpenSubtitles. Code MIT, content CC-BY-SA-4.0.
$base = "https://raw.githubusercontent.com/hermitdave/FrequencyWords/master/content/2018"
foreach ($lang in "en", "fr", "ar") {
    $out = Join-Path $dir "$lang.txt"
    Write-Host "downloading $lang ..."
    Invoke-WebRequest -Uri "$base/$lang/${lang}_50k.txt" -OutFile $out -UseBasicParsing
    $lines = (Get-Content $out -Encoding UTF8 | Measure-Object -Line).Lines
    Write-Host "  $lang.txt: $lines words"
}
Write-Host "done: $dir"
