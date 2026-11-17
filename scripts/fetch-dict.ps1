# Downloads Wiktionary data (as extracted by wiktextract, from kaikki.org) and builds Verba's bilingual dictionary: data/words/dict.txt.
# Run scripts/fetch-data.ps1 first (the frequency lists rank the words). The downloads and the dictionary are NOT part of the repository:
# the data is CC BY-SA 4.0 / GFDL (see docs/DATA.md).
#
#   powershell -ExecutionPolicy Bypass -File scripts\fetch-dict.ps1
#   powershell -ExecutionPolicy Bypass -File scripts\fetch-dict.ps1 -Keep          # keep the downloads and the extracts (data\words\src\dict\)
#   powershell -ExecutionPolicy Bypass -File scripts\fetch-dict.ps1 -MaxMB 24      # a larger dictionary (default 14 MB; the commonest words are kept first)
#
# What it does, in order (the whole run takes about 3 to 5 minutes, 2 to 3 of them for the download; see docs/ACCURACY.md):
#   1. downloads three gzip files from https://kaikki.org/dictionary/ (English 534 MB, Arabic 72 MB, French 59 MB). A download that was
#      interrupted is resumed (curl -C -). They go to data\words\src\dict\ and are deleted at the end unless -Keep is given.
#   2. for each file: unpacks it as a STREAM into `build_dict extract` (the 3.4 GB English text is never stored: it goes from the .gz through
#      memory into the program, which keeps a small tab-separated extract of about 130 MB in total).
#   3. `build_dict build` joins the extracts and the frequency lists into data\words\dict.txt, and writes a short report next to the extracts.
# Any failure stops the script with a message and a non-zero exit code: no half-built dictionary is left behind.

param(
    [switch]$Keep,
    [double]$MaxMB = 14
)

$ErrorActionPreference = "Stop"
$root = Split-Path -Parent $PSScriptRoot
$dir = Join-Path $root "data\words"
$tmp = Join-Path $dir "src\dict"
$out = Join-Path $dir "dict.txt"
New-Item -ItemType Directory -Force $tmp | Out-Null
$watch = [System.Diagnostics.Stopwatch]::StartNew()

foreach ($l in "en", "fr", "ar") {
    if (-not (Test-Path (Join-Path $dir "$l.txt"))) { throw "data\words\$l.txt is missing: run scripts\fetch-data.ps1 first" }
}

# 0. the program that does the extracting and building (an example of the verba-local crate)
if (-not (Get-Command cargo -ErrorAction SilentlyContinue)) {
    $cargoBin = Join-Path $env:USERPROFILE ".cargo\bin"
    if (Test-Path (Join-Path $cargoBin "cargo.exe")) { $env:Path = "$cargoBin;$env:Path" }
    else { throw "cargo was not found. Install Rust from https://rustup.rs and try again." }
}
Write-Host "building the dictionary tool ..."
Push-Location $root
try {
    $ErrorActionPreference = "Continue"   # cargo writes its progress to stderr
    cargo build --release -q -p verba-local --example build_dict
    $cargoExit = $LASTEXITCODE
    $ErrorActionPreference = "Stop"
    if ($cargoExit -ne 0) { throw "cargo could not build the dictionary tool" }
} finally {
    Pop-Location
}
$exe = Join-Path $root "target\release\examples\build_dict.exe"
if (-not (Test-Path $exe)) { throw "the dictionary tool was not produced: $exe" }

# the data: kaikki.org, one file per language edition entry set (English Wiktionary's entries of that language)
$sources = @(
    @{ lang = "fr"; name = "French";  }
    @{ lang = "ar"; name = "Arabic";  }
    @{ lang = "en"; name = "English"; }
)

function Get-RemoteSize($url) {
    $curl = Get-Command curl.exe -ErrorAction SilentlyContinue
    if (-not $curl) { return -1 }
    $head = & $curl.Source -sIL $url
    $len = -1
    foreach ($line in $head) { if ($line -match '^(?i)content-length:\s*(\d+)') { $len = [int64]$Matches[1] } }
    return $len
}

function Get-Download($url, $file) {
    $curl = Get-Command curl.exe -ErrorAction SilentlyContinue
    $want = Get-RemoteSize $url
    if ((Test-Path $file) -and $want -gt 0 -and (Get-Item $file).Length -eq $want) { Write-Host "  already downloaded: $file"; return }
    if ($curl) {
        # -C - resumes a partial file; --retry covers a dropped connection
        & $curl.Source -L --fail --silent --show-error -C - --retry 8 --retry-delay 5 --retry-all-errors -o $file $url
        if ($LASTEXITCODE -ne 0) { throw "download failed ($LASTEXITCODE): $url" }
    } else {
        Invoke-WebRequest -Uri $url -OutFile $file -UseBasicParsing   # no resume without curl
    }
    if ($want -gt 0 -and (Get-Item $file).Length -ne $want) { throw "the download is incomplete: $file is $((Get-Item $file).Length) bytes, the server says $want" }
}

# unpacks a .gz as a stream into `build_dict extract` (standard input)
function Invoke-Extract($gz, $lang, $extract) {
    $psi = New-Object System.Diagnostics.ProcessStartInfo
    $psi.FileName = $exe
    $psi.Arguments = "extract --lang $lang --out `"$extract`""
    $psi.UseShellExecute = $false
    $psi.RedirectStandardInput = $true
    $proc = [System.Diagnostics.Process]::Start($psi)
    $fs = [System.IO.File]::OpenRead($gz)
    try {
        $gzs = New-Object System.IO.Compression.GZipStream($fs, [System.IO.Compression.CompressionMode]::Decompress)
        try {
            $gzs.CopyTo($proc.StandardInput.BaseStream, 1MB)
            $proc.StandardInput.BaseStream.Flush()
        } catch [System.IO.IOException] {
            # the tool closed its input: its own message and exit code say why
        } finally {
            $gzs.Dispose()
        }
    } finally {
        $fs.Dispose()
        try { $proc.StandardInput.Close() } catch { }
    }
    $proc.WaitForExit()
    if ($proc.ExitCode -ne 0) { throw "extracting $gz failed (exit code $($proc.ExitCode))" }
}

foreach ($s in $sources) {
    $url = "https://kaikki.org/dictionary/$($s.name)/kaikki.org-dictionary-$($s.name).jsonl.gz"
    $gz = Join-Path $tmp "$($s.name).jsonl.gz"
    $t = $watch.Elapsed.TotalSeconds
    Write-Host "downloading $($s.name) ..."
    Get-Download $url $gz
    Write-Host ("  {0:N0} MB in {1:N0} s" -f ((Get-Item $gz).Length / 1MB), ($watch.Elapsed.TotalSeconds - $t))
    $t = $watch.Elapsed.TotalSeconds
    Write-Host "extracting $($s.name) ..."
    Invoke-Extract $gz $s.lang (Join-Path $tmp "dict-$($s.lang).tsv")
    Write-Host ("  {0:N0} s" -f ($watch.Elapsed.TotalSeconds - $t))
    if (-not $Keep) { Remove-Item $gz }
}

Write-Host "building dict.txt ..."
$t = $watch.Elapsed.TotalSeconds
$maxmb = $MaxMB.ToString([System.Globalization.CultureInfo]::InvariantCulture)
& $exe build --src $tmp --words $dir --out $out --max-mb $maxmb --report (Join-Path $tmp "dict-report.txt")
if ($LASTEXITCODE -ne 0) { throw "build_dict failed" }
if (-not (Test-Path $out) -or (Get-Item $out).Length -lt 1MB) { throw "dict.txt was not produced or is too small: $out" }
Write-Host ("  {0:N0} s" -f ($watch.Elapsed.TotalSeconds - $t))

if (-not $Keep) {
    Remove-Item (Join-Path $tmp "dict-*.tsv")
    Remove-Item (Join-Path $tmp "dict-report.txt") -ErrorAction SilentlyContinue
}
Write-Host ("done: {0} ({1:N1} MB) in {2:N0} s" -f $out, ((Get-Item $out).Length / 1MB), $watch.Elapsed.TotalSeconds)
