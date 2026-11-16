# Builds Verba and packages it as dist\Verba-Setup-<version>.exe with Inno Setup 6.
#
#   powershell -ExecutionPolicy Bypass -File scripts\build-installer.ps1
#
# Needs: Rust (cargo), Inno Setup 6 (ISCC.exe), and the word lists (downloaded here if missing).
# Inno Setup is looked for in: the ISCC environment variable, PATH, then the usual install folders.

$ErrorActionPreference = "Stop"
$root = Split-Path -Parent $PSScriptRoot
Set-Location $root

function Fail($message) {
    Write-Host ""
    Write-Host "build-installer: $message" -ForegroundColor Red
    exit 1
}

# 1. the version, from the workspace Cargo.toml
$toml = Get-Content (Join-Path $root "Cargo.toml") -Raw
if ($toml -notmatch '(?ms)\[workspace\.package\].*?^\s*version\s*=\s*"([^"]+)"') {
    Fail "could not read the version from [workspace.package] in Cargo.toml"
}
$version = $Matches[1]
Write-Host "Verba $version"

# 2. the program
if (-not (Get-Command cargo -ErrorAction SilentlyContinue)) {
    $cargoBin = Join-Path $env:USERPROFILE ".cargo\bin"
    if (Test-Path (Join-Path $cargoBin "cargo.exe")) { $env:Path = "$cargoBin;$env:Path" }
    else { Fail "cargo was not found. Install Rust from https://rustup.rs and try again." }
}
Write-Host "building the release program ..."
# cargo writes its progress to stderr: that is not an error, so only the exit code counts
$ErrorActionPreference = "Continue"
cargo build --release -p verba-app
$cargoExit = $LASTEXITCODE
$ErrorActionPreference = "Stop"
if ($cargoExit -ne 0) { Fail "cargo build failed" }
$exe = Join-Path $root "target\release\verba.exe"
if (-not (Test-Path $exe)) { Fail "target\release\verba.exe was not produced" }

# 3. the word lists
$words = Join-Path $root "data\words"
if (-not ((Test-Path (Join-Path $words "en.txt")) -and (Test-Path (Join-Path $words "fr.txt")) -and (Test-Path (Join-Path $words "ar.txt")))) {
    Write-Host "downloading the word lists ..."
    & (Join-Path $PSScriptRoot "fetch-data.ps1")
    if (-not (Test-Path (Join-Path $words "ar.txt"))) { Fail "the word lists could not be downloaded (data\words\*.txt)" }
}

# 4. Inno Setup
function Find-Iscc {
    if ($env:ISCC -and (Test-Path $env:ISCC)) { return $env:ISCC }
    $onPath = Get-Command ISCC.exe -ErrorAction SilentlyContinue
    if ($onPath) { return $onPath.Source }
    $candidates = @(
        (Join-Path $env:LOCALAPPDATA "Programs\Inno Setup 6\ISCC.exe"),
        (Join-Path ${env:ProgramFiles(x86)} "Inno Setup 6\ISCC.exe"),
        (Join-Path $env:ProgramFiles "Inno Setup 6\ISCC.exe")
    )
    foreach ($c in $candidates) { if ($c -and (Test-Path $c)) { return $c } }
    return $null
}
$iscc = Find-Iscc
if (-not $iscc) {
    Fail "Inno Setup 6 (ISCC.exe) was not found. Install it from https://jrsoftware.org/isdl.php, or set the ISCC environment variable to the full path of ISCC.exe."
}
Write-Host "Inno Setup: $iscc"

# 5. the installer
New-Item -ItemType Directory -Force (Join-Path $root "dist") | Out-Null
& $iscc "/DAppVersion=$version" "/Qp" (Join-Path $root "installer\verba.iss")
if ($LASTEXITCODE -ne 0) { Fail "Inno Setup failed (see the messages above)" }
$setup = Join-Path $root "dist\Verba-Setup-$version.exe"
if (-not (Test-Path $setup)) { Fail "the installer was not produced: $setup" }

# 6. the checksum, in the usual `sha256sum` format
$hash = (Get-FileHash $setup -Algorithm SHA256).Hash.ToLower()
$name = Split-Path $setup -Leaf
[System.IO.File]::WriteAllText("$setup.sha256", "$hash *$name`n", (New-Object System.Text.UTF8Encoding($false)))

$mb = [math]::Round((Get-Item $setup).Length / 1MB, 2)
Write-Host ""
Write-Host "done: $setup ($mb MB)"
Write-Host "sha256: $hash"
