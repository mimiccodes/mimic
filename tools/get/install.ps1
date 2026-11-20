# Install or update Mimic on Windows (PowerShell):
#
#   irm https://mimic.codes/install.ps1 | iex
#
# The same command updates an installed mimic to the latest release (and
# says so when it is already the latest). Options, through the environment
# (with `irm | iex` there are no arguments):
#
#   $env:MIMIC_VERSION = "0.3.16"; irm https://mimic.codes/install.ps1 | iex
#   $env:MIMIC_FORCE = "1";        irm https://mimic.codes/install.ps1 | iex
#   $env:MIMIC_UNINSTALL = "1";    irm https://mimic.codes/install.ps1 | iex
#   $env:MIMIC_CHANNEL = "preview"; irm https://mimic.codes/install.ps1 | iex
#
# Channels: stable releases (the default), previews of the next version
# ("preview": the newest preview or stable release), and nightly builds of
# the development branch ("nightly"). The channel chosen is remembered, so
# later updates stay on it; MIMIC_CHANNEL = "stable" goes back.
#
# It downloads the Windows release from GitHub, checks it against the
# release's SHA256SUMS, unpacks it with Windows' own tar (Expand-Archive
# does not load where the execution policy forbids scripts) and runs the
# installer inside it, which puts mimic.exe in %USERPROFILE%\.mimic\bin and
# on your PATH (a running mimic.exe, an editor's language server, is moved
# aside first). `mimic upgrade` runs this same script.

function Install-Mimic {
    param([string]$Version, [string]$Channel, [switch]$Force, [switch]$Check, [switch]$Uninstall)
    $ErrorActionPreference = "Stop"
    $ProgressPreference = "SilentlyContinue" # (the progress bar makes downloads many times slower)
    # Windows PowerShell 5.1 may still default to old TLS versions
    [Net.ServicePointManager]::SecurityProtocol = [Net.ServicePointManager]::SecurityProtocol -bor [Net.SecurityProtocolType]::Tls12
    $repo = "mimiccodes/mimic"
    $mimicHome = if ($env:MIMIC_HOME) { $env:MIMIC_HOME } else { Join-Path $env:USERPROFILE ".mimic" }
    $bin = if ($env:MIMIC_BIN) { $env:MIMIC_BIN } else { Join-Path $mimicHome "bin" }
    $exe = Join-Path $bin "mimic.exe"

    if ($Uninstall) {
        Remove-Item -Force -ErrorAction SilentlyContinue $exe, (Join-Path $mimicHome "installed-version"), (Join-Path $mimicHome "channel")
        $userPath = [Environment]::GetEnvironmentVariable("Path", "User")
        $kept = @($userPath -split ";" | Where-Object { $_ -ne "" -and $_.TrimEnd("\") -ne $bin.TrimEnd("\") })
        [Environment]::SetEnvironmentVariable("Path", ($kept -join ";"), "User")
        Write-Host "mimic removed from $bin and from PATH (the build cache stays in $mimicHome\cache)"
        return
    }

    $arch = if ($env:PROCESSOR_ARCHITEW6432) { $env:PROCESSOR_ARCHITEW6432 } else { $env:PROCESSOR_ARCHITECTURE }
    if ($arch -eq "ARM64") {
        Write-Host "note: this is Windows on Arm; Mimic's Windows release is x86_64 and runs under Windows' x64 emulation"
    } elseif ($arch -ne "AMD64") {
        throw "there is no Windows release for $arch; Mimic needs 64-bit Windows"
    }

    # the channel: as asked, else the one last installed from, else stable
    $channelFile = Join-Path $mimicHome "channel"
    if (-not $Channel -and (Test-Path $channelFile)) { $Channel = (Get-Content $channelFile -TotalCount 1).Trim() }
    if (-not $Channel) { $Channel = "stable" }
    if ($Channel -notin @("stable", "preview", "nightly")) { throw "unknown channel '$Channel' (stable, preview or nightly)" }
    $latestIs = "the latest"
    $tag = $null

    if (-not $Version -and $Channel -eq "preview") {
        # the newest preview or stable release: the first in the releases'
        # feed (newest first) that is either (the feed, not the API, which
        # limits how often it may be asked)
        $feed = (Invoke-WebRequest -UseBasicParsing -Uri "https://github.com/$repo/releases.atom").Content
        if ($feed -is [byte[]]) { $feed = [System.Text.Encoding]::UTF8.GetString($feed) }
        # (the feed also lists a tag whose release is still being built: the
        # first one with its files)
        $candidates = @([regex]::Matches($feed, 'releases/tag/(v[^"]+)"') | ForEach-Object { $_.Groups[1].Value } | Where-Object { $_ -match "^v\d+\.\d+\.\d+(-(preview|fix)\.\d+)?$" } | Select-Object -First 5)
        $Version = $null
        foreach ($c in $candidates) {
            try {
                $null = Invoke-WebRequest -UseBasicParsing -Method Head -Uri "https://github.com/$repo/releases/download/$c/SHA256SUMS"
                $Version = $c
                break
            } catch { }
        }
        if (-not $Version) { throw "found no preview or stable release with files on GitHub" }
        $latestIs = "the latest preview"
    } elseif (-not $Version -and $Channel -eq "nightly") {
        $text = (Invoke-WebRequest -UseBasicParsing -Uri "https://github.com/$repo/releases/download/nightly/version.txt").Content
        if ($text -is [byte[]]) { $text = [System.Text.Encoding]::UTF8.GetString($text) }
        $Version = ($text -split "`n")[0].Trim()
        $tag = "nightly"
        $latestIs = "the latest nightly"
    }

    # the latest release's version: GitHub's redirect to its tag (not the
    # API, which limits how often it may be asked)
    if (-not $Version) {
        $req = [System.Net.HttpWebRequest]::Create("https://github.com/$repo/releases/latest")
        $req.AllowAutoRedirect = $false
        $req.UserAgent = "mimic-install"
        $res = $req.GetResponse()
        $location = $res.Headers["Location"]
        $res.Close()
        if (-not $location -or $location -notmatch "/v(\d+\.\d+\.\d+(-fix\.\d+)?)$") {
            throw "could not find the latest release on GitHub (got '$location'); check your connection, or set MIMIC_VERSION"
        }
        $Version = $Matches[1]
    }
    $Version = $Version.TrimStart("v")
    if (-not $tag) { $tag = "v$Version" }

    $current = $null
    if (Test-Path $exe) {
        $line = (& $exe --version 2>$null | Select-Object -First 1)
        if ($line) { $current = ($line -split " ")[1] }
    }
    if ($Check) {
        if (-not $current) { Write-Host "Mimic is not installed in $bin; the latest is $Version" }
        elseif ($current -eq $Version) { Write-Host "mimic $current is $latestIs" }
        else { Write-Host "mimic $current is installed; $Version is available (run this again, or mimic upgrade)" }
        return
    }
    if ($current -eq $Version -and -not $Force) {
        Write-Host "mimic $Version is already installed in $bin, and is $latestIs (MIMIC_FORCE=1 reinstalls it)"
        Set-Content -Path $channelFile -Value $Channel
        return
    }

    $name = "mimic-$Version-windows-x86_64.zip"
    $base = "https://github.com/$repo/releases/download/$tag"
    $tmp = Join-Path ([System.IO.Path]::GetTempPath()) ("mimic-install-" + [guid]::NewGuid().ToString("N"))
    New-Item -ItemType Directory -Force $tmp | Out-Null
    try {
        if ($current) { Write-Host "updating mimic $current to $Version" } else { Write-Host "installing mimic $Version" }
        $zip = Join-Path $tmp $name
        Invoke-WebRequest -UseBasicParsing -Uri "$base/$name" -OutFile $zip
        $sums = (Invoke-WebRequest -UseBasicParsing -Uri "$base/SHA256SUMS").Content
        if ($sums -is [byte[]]) { $sums = [System.Text.Encoding]::UTF8.GetString($sums) }
        $want = ($sums -split "`n" | Where-Object { $_ -match "^\s*([0-9a-fA-F]{64})\s+\*?$([regex]::Escape($name))\s*$" } | ForEach-Object { $Matches[1] } | Select-Object -First 1)
        if (-not $want) { throw "$name is not listed in SHA256SUMS" }
        $have = (Get-FileHash -Algorithm SHA256 $zip).Hash
        if ($want.ToLower() -ne $have.ToLower()) { throw "the download does not match SHA256SUMS (got $have); nothing was installed" }

        # Windows' own tar (Windows 10 and later) reads zip files
        $tar = Join-Path $env:SystemRoot "System32\tar.exe"
        $unpacked = $false
        if (Test-Path $tar) {
            & $tar -xf $zip -C $tmp
            $unpacked = ($LASTEXITCODE -eq 0)
        }
        if (-not $unpacked) {
            powershell -NoProfile -NonInteractive -ExecutionPolicy Bypass -Command "`$ErrorActionPreference='Stop'; Expand-Archive -Force -LiteralPath '$($zip -replace "'", "''")' -DestinationPath '$($tmp -replace "'", "''")'"
        }
        $pkg = Join-Path $tmp "mimic-$Version-windows-x86_64"
        $script = Join-Path $pkg "install.ps1"
        if (-not (Test-Path $script)) { throw "unpacking $name did not give install.ps1" }

        # the release's own installer, whatever the execution policy
        $env:MIMIC_HOME = $mimicHome
        $env:MIMIC_BIN = $bin
        # (interactive: it may ask whether to remove another mimic found first on PATH)
        powershell -NoProfile -ExecutionPolicy Bypass -File $script
        if ($LASTEXITCODE -ne 0) { throw "the installer failed (exit status $LASTEXITCODE)" }
        # this terminal sees mimic too, not only new ones
        if (($env:Path -split ";") -notcontains $bin) { $env:Path = "$bin;$env:Path" }
        Set-Content -Path $channelFile -Value $Channel
        if ($Channel -ne "stable") { Write-Host "on the $Channel channel: later updates stay on it (MIMIC_CHANNEL=stable goes back)" }
    } finally {
        Remove-Item -Recurse -Force -ErrorAction SilentlyContinue $tmp
    }
}

Install-Mimic -Version $env:MIMIC_VERSION -Channel $env:MIMIC_CHANNEL -Force:($env:MIMIC_FORCE -eq "1") -Check:($env:MIMIC_CHECK -eq "1") -Uninstall:($env:MIMIC_UNINSTALL -eq "1")
