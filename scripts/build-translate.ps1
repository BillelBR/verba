# Builds verba-translate.exe, the sentence translator (CTranslate2 + oneDNN, compiled from source with CMake).
#
#   powershell -ExecutionPolicy Bypass -File scripts\build-translate.ps1
#
# Output: target\mt\release\verba-translate.exe (and vcomp140.dll next to it, copied from the Visual C++ redistributable files).
#
# Needs: Rust (cargo), the Visual Studio C++ build tools, CMake (winget install Kitware.CMake), and about 8 GB of free disk space.
# First build: about 7 to 10 minutes and about 7 GB of build files in target\mt (delete that folder afterwards to get the space back;
# the program itself is 35 MB and needs nothing from it). Later builds: seconds.
#
# It has its own target folder (target\mt) on purpose: the build leaves about 5 GB of CTranslate2 and oneDNN files, which should stay out
# of target\release and its caches. CTranslate2 is compiled with the static C runtime (/MT); the Rust side uses it too, through
# `+crt-static` in .cargo/config.toml.

$ErrorActionPreference = "Stop"
$root = Split-Path -Parent $PSScriptRoot
Set-Location $root

function Fail($message) {
    Write-Host ""
    Write-Host "build-translate: $message" -ForegroundColor Red
    exit 1
}

if (-not (Get-Command cargo -ErrorAction SilentlyContinue)) {
    $cargoBin = Join-Path $env:USERPROFILE ".cargo\bin"
    if (Test-Path (Join-Path $cargoBin "cargo.exe")) { $env:Path = "$cargoBin;$env:Path" }
    else { Fail "cargo was not found. Install Rust from https://rustup.rs and try again." }
}
if (-not (Get-Command cmake -ErrorAction SilentlyContinue)) {
    $cmakeBin = Join-Path $env:ProgramFiles "CMake\bin"
    if (Test-Path (Join-Path $cmakeBin "cmake.exe")) { $env:Path = "$cmakeBin;$env:Path" }
    else { Fail "CMake was not found. Install it with: winget install Kitware.CMake" }
}

$free = (Get-PSDrive -Name ((Get-Item $root).PSDrive.Name)).Free
if ($free -lt 8GB -and -not (Test-Path (Join-Path $root "target\mt\release\build"))) {
    Write-Host ("warning: only {0:N1} GB free on this drive; the first build of CTranslate2 and oneDNN needs about 8 GB." -f ($free / 1GB)) -ForegroundColor Yellow
}

Write-Host "building verba-translate (the first time takes about 10 minutes) ..."
$ErrorActionPreference = "Continue"
cargo build --release -p verba-translate --target-dir (Join-Path $root "target\mt")
$cargoExit = $LASTEXITCODE
$ErrorActionPreference = "Stop"
if ($cargoExit -ne 0) { Fail "cargo build failed" }
$exe = Join-Path $root "target\mt\release\verba-translate.exe"
if (-not (Test-Path $exe)) { Fail "target\mt\release\verba-translate.exe was not produced" }

# oneDNN uses the Visual C++ OpenMP runtime, which is not part of Windows: ship it next to the program
$outDir = Split-Path $exe
$vcomp = Join-Path $outDir "vcomp140.dll"
if (-not (Test-Path $vcomp)) {
    $found = $null
    # the redistributable files of the installed Visual Studio
    $vswhere = Join-Path ${env:ProgramFiles(x86)} "Microsoft Visual Studio\Installer\vswhere.exe"
    if (Test-Path $vswhere) {
        $vs = & $vswhere -latest -products * -property installationPath
        if ($vs) {
            $found = Get-ChildItem (Join-Path $vs "VC\Redist\MSVC") -Recurse -Filter vcomp140.dll -ErrorAction SilentlyContinue |
                Where-Object { $_.FullName -match "\\x64\\Microsoft\.VC\d+\.OpenMP\\" -and $_.FullName -notmatch "\\onecore\\|debug_nonredist" } |
                Sort-Object FullName -Descending | Select-Object -First 1
        }
    }
    if (-not $found) {
        $sys = Join-Path $env:WINDIR "System32\vcomp140.dll"
        if (Test-Path $sys) { $found = Get-Item $sys }
    }
    if (-not $found) { Fail "vcomp140.dll was not found (it comes with the Visual C++ build tools: Redist\MSVC\...\Microsoft.VC143.OpenMP)" }
    Copy-Item $found.FullName $vcomp
    Write-Host "vcomp140.dll: $($found.FullName)"
}

$mb = [math]::Round((Get-Item $exe).Length / 1MB, 1)
Write-Host ""
Write-Host "done: $exe ($mb MB)"
