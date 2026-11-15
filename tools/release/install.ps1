# Install mimic from this folder (Windows): copies mimic.exe to
# %USERPROFILE%\.mimic\bin and puts that folder on your PATH. An earlier
# mimic is replaced, and the build cache it left is cleared.
#
#   powershell -NoProfile -ExecutionPolicy Bypass -File install.ps1             (or double-click install.cmd)
#   powershell -NoProfile -ExecutionPolicy Bypass -File install.ps1 -Uninstall
param([switch]$Uninstall)
$ErrorActionPreference = "Stop"

$here = Split-Path -Parent $MyInvocation.MyCommand.Path
$mimicHome = if ($env:MIMIC_HOME) { $env:MIMIC_HOME } else { Join-Path $env:USERPROFILE ".mimic" }
$bin = if ($env:MIMIC_BIN) { $env:MIMIC_BIN } else { Join-Path $mimicHome "bin" }
$target = Join-Path $bin "mimic.exe"
$mark = Join-Path $mimicHome "installed-version"
$userPath = [Environment]::GetEnvironmentVariable("Path", "User")
$parts = @($userPath -split ";" | Where-Object { $_ -ne "" })

if ($Uninstall) {
    Remove-Item -Force -ErrorAction SilentlyContinue $target, $mark
    $kept = $parts | Where-Object { $_.TrimEnd("\") -ne $bin.TrimEnd("\") }
    [Environment]::SetEnvironmentVariable("Path", ($kept -join ";"), "User")
    Write-Host "mimic removed from $bin and from PATH (the build cache stays in $mimicHome\cache)"
    exit 0
}

# files from a download are marked as coming from the internet: all of them
Get-ChildItem -Path $here -Recurse -File | Unblock-File

$exe = Join-Path $here "mimic.exe"
if (-not (Test-Path $exe)) { throw "no mimic.exe next to this script" }
if ($env:PROCESSOR_ARCHITECTURE -ne "AMD64" -and $env:PROCESSOR_ARCHITEW6432 -ne "AMD64") {
    Write-Host "note: this mimic is for x86_64; on Arm it runs under Windows' x64 emulation"
}

# ---- an earlier mimic ------------------------------------------------------
$new = (& $exe --version 2>$null | Select-Object -First 1)
$old = if (Test-Path $mark) { (Get-Content $mark -TotalCount 1) } else { $null }
if (-not $old -and (Test-Path $target)) { $old = (& $target --version 2>$null | Select-Object -First 1) }

# another mimic.exe found first on PATH would hide this one
$others = @(Get-Command mimic.exe -All -ErrorAction SilentlyContinue | Where-Object { $_.Source -and ($_.Source -ne $target) })
foreach ($o in $others) {
    $v = (& $o.Source --version 2>$null | Select-Object -First 1)
    Write-Host "another mimic is on PATH: $($o.Source) ($v)"
    $answer = "n"
    if ([Environment]::UserInteractive) { $answer = Read-Host "remove it, so the new one is used? [y/N]" }
    if ($answer -match "^(y|yes)$") {
        try {
            Remove-Item -Force $o.Source
            Write-Host "removed $($o.Source)"
        } catch {
            Write-Host "could not remove it ($($_.Exception.Message)); delete it by hand, or remove its folder from PATH"
        }
    } else {
        Write-Host "kept it. If it comes first on PATH, remove it to use the new mimic."
    }
}

# ---- install ---------------------------------------------------------------
New-Item -ItemType Directory -Force $bin | Out-Null
Copy-Item -Force $exe $target
Unblock-File $target
if ($old) { Write-Host "installed $target (replacing $old)" } else { Write-Host "installed $target" }
# what an older mimic built (its runtime, C libraries, imported headers) is
# rebuilt by the new one; projects and installed libraries are not touched
$cache = Join-Path $mimicHome "cache"
if ($old -and ($old -ne $new) -and (Test-Path $cache)) {
    Remove-Item -Recurse -Force $cache
    Write-Host "cleared the build cache of the old version ($cache)"
}
Set-Content -Path $mark -Value $new

if (-not ($parts | Where-Object { $_.TrimEnd("\") -eq $bin.TrimEnd("\") })) {
    [Environment]::SetEnvironmentVariable("Path", (($parts + $bin) -join ";"), "User")
    Write-Host "added $bin to your PATH (open a new terminal to use it)"
}
$env:Path = "$bin;$env:Path"

& $target --version
Write-Host ""
Write-Host "ready. In a new terminal, try:  mimic create hello; cd hello; mimic run"
Write-Host "(nothing else is needed: Windows programs are linked with the MinGW-w64 inside mimic)"
