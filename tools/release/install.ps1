# Install mimic from this folder (Windows): copies mimic.exe to
# %USERPROFILE%\.mimic\bin and puts that folder on your PATH.
#
#   powershell -ExecutionPolicy Bypass -File install.ps1             (or double-click install.cmd)
#   powershell -ExecutionPolicy Bypass -File install.ps1 -Uninstall
param([switch]$Uninstall)
$ErrorActionPreference = "Stop"

$here = Split-Path -Parent $MyInvocation.MyCommand.Path
$bin = if ($env:MIMIC_BIN) { $env:MIMIC_BIN } else { Join-Path $env:USERPROFILE ".mimic\bin" }
$userPath = [Environment]::GetEnvironmentVariable("Path", "User")
$parts = @($userPath -split ";" | Where-Object { $_ -ne "" })

if ($Uninstall) {
    Remove-Item -Force -ErrorAction SilentlyContinue (Join-Path $bin "mimic.exe")
    $kept = $parts | Where-Object { $_.TrimEnd("\") -ne $bin.TrimEnd("\") }
    [Environment]::SetEnvironmentVariable("Path", ($kept -join ";"), "User")
    Write-Host "mimic removed from $bin and from PATH (the build cache stays in $env:USERPROFILE\.mimic\cache)"
    exit 0
}

$exe = Join-Path $here "mimic.exe"
if (-not (Test-Path $exe)) { throw "no mimic.exe next to this script" }
if ($env:PROCESSOR_ARCHITECTURE -ne "AMD64" -and $env:PROCESSOR_ARCHITEW6432 -ne "AMD64") {
    Write-Host "note: this mimic is for x86_64; on Arm it runs under Windows' x64 emulation"
}

New-Item -ItemType Directory -Force $bin | Out-Null
Copy-Item -Force $exe (Join-Path $bin "mimic.exe")
# downloaded files are marked as coming from the internet
Unblock-File (Join-Path $bin "mimic.exe")
Write-Host "installed $bin\mimic.exe"

if (-not ($parts | Where-Object { $_.TrimEnd("\") -eq $bin.TrimEnd("\") })) {
    [Environment]::SetEnvironmentVariable("Path", (($parts + $bin) -join ";"), "User")
    Write-Host "added $bin to your PATH (open a new terminal to use it)"
}
$env:Path = "$bin;$env:Path"

& (Join-Path $bin "mimic.exe") --version
Write-Host ""
Write-Host "ready. In a new terminal, try:  mimic create hello; cd hello; mimic run"
Write-Host "(nothing else is needed: Windows programs are linked with the MinGW-w64 inside mimic)"
