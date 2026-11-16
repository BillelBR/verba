# OPTIONAL: builds data/words/dz-ar.txt, a list of real Algerian words written in Arabic letters, from the Mendeley Data set
# "Algerian Dialect" (45 000 YouTube comments from Algerian press channels; Benmounah and Boulesnane; doi 10.17632/zzwg3nnhsz.2;
# license CC BY 4.0, https://creativecommons.org/licenses/by/4.0/).
# Darija in Arabic letters works without it (the program has its own seed list); this only adds the words and spellings people really write.
# It is NOT part of the repository and keeps its own license (see docs/DATA.md). Needs data/words/ar.txt (scripts/fetch-data.ps1) and Rust.
#
#   powershell -ExecutionPolicy Bypass -File scripts\fetch-dz-ar.ps1

$ErrorActionPreference = "Stop"
$root = Split-Path -Parent $PSScriptRoot
$raw = Join-Path $root "data\dz\ar-youtube"
$out = Join-Path $root "data\words\dz-ar.txt"
New-Item -ItemType Directory -Force $raw | Out-Null
New-Item -ItemType Directory -Force (Split-Path $out) | Out-Null
if (-not (Test-Path (Join-Path $root "data\words\ar.txt"))) { throw "data\words\ar.txt is missing: run scripts\fetch-data.ps1 first" }

$csv = Join-Path $raw "comments.csv"
Write-Host "downloading the comments (12 MB) ..."
Invoke-WebRequest -Uri "https://data.mendeley.com/public-files/datasets/zzwg3nnhsz/files/2c9cb953-635e-4b82-9d28-01828a1254b9/file_downloaded" -OutFile $csv -UseBasicParsing
$sha = (Get-FileHash $csv -Algorithm SHA256).Hash.ToLowerInvariant()
if ($sha -ne "281276ce76bea67d9cd5d32bd5f63fa223605bc59bf71b0b9a9631c29d4f4ac5") { Write-Warning "the file is not the one this script was written for (sha256 $sha)" }
@"
Algerian Dialect. Zakaria Benmounah, Abdennour Boulesnane. Mendeley Data, V2, doi: 10.17632/zzwg3nnhsz.2
License: Creative Commons Attribution 4.0 International (CC BY 4.0), http://creativecommons.org/licenses/by/4.0
"You can share, copy and modify this dataset so long as you give appropriate credit, provide a link to the CC BY license, and indicate if changes
were made, but you may not do so in a way that suggests the rights holder has endorsed you or your use of the dataset. Note that further permission
may be required for any content within the dataset that is identified as belonging to a third party."
"@ | Set-Content -Path (Join-Path $raw "LICENSE-and-source.txt") -Encoding UTF8

# the words the Modern Standard Arabic list lacks and that occur at least 4 times in the comments
if (-not (Get-Command cargo -ErrorAction SilentlyContinue)) {
    $cargoBin = Join-Path $env:USERPROFILE ".cargo\bin"
    if (Test-Path (Join-Path $cargoBin "cargo.exe")) { $env:Path = "$cargoBin;$env:Path" } else { throw "cargo was not found: install Rust from https://rustup.rs" }
}
Push-Location $root
try {
    cargo run --release -q -p verba-local --example build_dz_ar -- --csv $csv --out $out
    if ($LASTEXITCODE -ne 0) { throw "build_dz_ar failed" }
} finally { Pop-Location }
Write-Host "done: $out"
