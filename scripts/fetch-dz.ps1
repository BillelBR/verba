# OPTIONAL: builds data/words/dz.txt, a list of real Algerian Arabizi words, from the NArabizi treebank
# (Universal Dependencies, UD_Maghrebi_Arabic_French-Arabizi, license CC BY-SA 4.0).
# The Algerian Darija suggestions work without it (the program has its own seed list); this only adds the words
# and spellings people really wrote. It is NOT part of the repository and keeps its own license (see docs/DATA.md).
#
#   powershell -ExecutionPolicy Bypass -File scripts\fetch-dz.ps1

$ErrorActionPreference = "Stop"
$root = Split-Path -Parent $PSScriptRoot
$raw = Join-Path $root "data\dz\narabizi"
$out = Join-Path $root "data\words\dz.txt"
New-Item -ItemType Directory -Force $raw | Out-Null
New-Item -ItemType Directory -Force (Split-Path $out) | Out-Null

$base = "https://raw.githubusercontent.com/UniversalDependencies/UD_Maghrebi_Arabic_French-Arabizi/master"
foreach ($f in "LICENSE.txt", "qaf_arabizi-ud-train.conllu", "qaf_arabizi-ud-dev.conllu", "qaf_arabizi-ud-test.conllu") {
    Write-Host "downloading $f ..."
    Invoke-WebRequest -Uri "$base/$f" -OutFile (Join-Path $raw $f) -UseBasicParsing
}

# one word per token that is labelled Algerian (LangO=ar_dz) and is not a name; digits only as Arabizi letters (2 3 5 6 7 8 9)
$counts = @{}
foreach ($f in "qaf_arabizi-ud-train.conllu", "qaf_arabizi-ud-dev.conllu", "qaf_arabizi-ud-test.conllu") {
    foreach ($line in [System.IO.File]::ReadLines((Join-Path $raw $f), [System.Text.Encoding]::UTF8)) {
        if ($line.Length -eq 0 -or $line[0] -eq '#') { continue }
        $c = $line.Split("`t")
        if ($c.Length -lt 10 -or $c[0].Contains("-")) { continue }
        if (-not $c[9].Contains("LangO=ar_dz") -or $c[3] -eq "PROPN") { continue }
        $w = $c[1].ToLowerInvariant()
        if ($w -notmatch '^[a-z2356789]{2,24}$' -or $w -notmatch '[a-z]' -or $w -match '(.)\1\1' -or $w -match '\d\d') { continue }
        $counts[$w] = 1 + [int]$counts[$w]
    }
}
$lines = $counts.GetEnumerator() | Where-Object { $_.Value -ge 2 } | Sort-Object -Property @{Expression = "Value"; Descending = $true}, @{Expression = "Name"} |
    ForEach-Object { "$($_.Name) $($_.Value)" }
[System.IO.File]::WriteAllLines($out, $lines, (New-Object System.Text.UTF8Encoding($false)))
Write-Host "done: $out ($(@($lines).Count) words seen at least twice)"
