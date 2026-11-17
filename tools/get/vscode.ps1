# Install or update the Mimic extension for VS Code (and VS Code Insiders,
# Cursor, Windsurf and VSCodium) on Windows (PowerShell):
#
#   irm https://mimic.codes/vscode.ps1 | iex
#
# It downloads the latest extension (.vsix) from GitHub and installs it in
# every one of those editors it finds, on PATH or where their installers put
# them; an editor that already has that version is left as it is. Options,
# through the environment:
#
#   $env:MIMIC_EDITOR = "cursor"      only that editor
#   $env:MIMIC_EXT_VERSION = "0.10.5" that release instead of the latest
#   $env:MIMIC_FORCE = "1"            reinstall even when that version is there
#
# `mimic upgrade --editors` runs this same script.

function Install-MimicExtension {
    param([string]$Version, [string]$Only, [switch]$Force)
    $ErrorActionPreference = "Stop"
    $ProgressPreference = "SilentlyContinue"
    [Net.ServicePointManager]::SecurityProtocol = [Net.ServicePointManager]::SecurityProtocol -bor [Net.SecurityProtocolType]::Tls12
    $repo = "mimiccodes/mimic-vscode"
    $id = "mimiccodes.mimic"

    # the editors' command lines: on PATH, or where their installers put them
    $candidates = @()
    foreach ($c in "code", "code-insiders", "cursor", "windsurf", "codium") {
        $cmd = Get-Command "$c.cmd" -ErrorAction SilentlyContinue | Select-Object -First 1
        if ($cmd) { $candidates += $cmd.Source }
    }
    $local = $env:LOCALAPPDATA
    $prog = $env:ProgramFiles
    $candidates += @(
        "$local\Programs\Microsoft VS Code\bin\code.cmd",
        "$prog\Microsoft VS Code\bin\code.cmd",
        "$local\Programs\Microsoft VS Code Insiders\bin\code-insiders.cmd",
        "$local\Programs\cursor\resources\app\bin\cursor.cmd",
        "$local\Programs\Windsurf\bin\windsurf.cmd",
        "$local\Programs\VSCodium\bin\codium.cmd",
        "$prog\VSCodium\bin\codium.cmd"
    )
    $editors = @{}
    foreach ($e in $candidates) {
        if (-not (Test-Path $e)) { continue }
        $n = [System.IO.Path]::GetFileNameWithoutExtension($e)
        if ($Only -and $n -ne $Only) { continue }
        if (-not $editors.ContainsKey($n)) { $editors[$n] = $e }
    }
    if ($editors.Count -eq 0) {
        $what = if ($Only) { " named $Only" } else { "" }
        throw "no editor found$what (VS Code, Cursor, Windsurf, VSCodium); install one, or put its command line on PATH"
    }

    if (-not $Version) {
        $req = [System.Net.HttpWebRequest]::Create("https://github.com/$repo/releases/latest")
        $req.AllowAutoRedirect = $false
        $req.UserAgent = "mimic-install"
        $res = $req.GetResponse()
        $location = $res.Headers["Location"]
        $res.Close()
        if (-not $location -or $location -notmatch "/v(\d+\.\d+\.\d+)$") {
            throw "could not find the latest extension on GitHub (got '$location'); check your connection, or set MIMIC_EXT_VERSION"
        }
        $Version = $Matches[1]
    }
    $Version = $Version.TrimStart("v")

    $tmp = Join-Path ([System.IO.Path]::GetTempPath()) ("mimic-vscode-" + [guid]::NewGuid().ToString("N"))
    New-Item -ItemType Directory -Force $tmp | Out-Null
    $vsix = Join-Path $tmp "mimic-$Version.vsix"
    $changed = $false
    try {
        foreach ($n in $editors.Keys) {
            $e = $editors[$n]
            $have = (& $e --list-extensions --show-versions 2>$null | Where-Object { $_ -like "$id@*" } | ForEach-Object { ($_ -split "@")[1] } | Select-Object -First 1)
            if ($have -eq $Version -and -not $Force) {
                Write-Host "${n}: the Mimic extension $Version is already installed"
                continue
            }
            if (-not (Test-Path $vsix)) {
                Invoke-WebRequest -UseBasicParsing -Uri "https://github.com/$repo/releases/download/v$Version/mimic-$Version.vsix" -OutFile $vsix
            }
            if ($have -eq $Version) { Write-Host "${n}: reinstalling the Mimic extension $Version" }
            elseif ($have) { Write-Host "${n}: updating the Mimic extension $have to $Version" }
            else { Write-Host "${n}: installing the Mimic extension $Version" }
            $out = (& $e --install-extension $vsix --force 2>&1)
            if ($LASTEXITCODE -ne 0) { $out | Write-Host; throw "$n could not install the extension (exit status $LASTEXITCODE)" }
            $changed = $true
        }
        if ($changed) { Write-Host "done: reload the editor's window (or restart it) to use the new version" }
    } finally {
        Remove-Item -Recurse -Force -ErrorAction SilentlyContinue $tmp
    }
}

Install-MimicExtension -Version $env:MIMIC_EXT_VERSION -Only $env:MIMIC_EDITOR -Force:($env:MIMIC_FORCE -eq "1")
