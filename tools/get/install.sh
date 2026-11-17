#!/bin/sh
# Install or update Mimic on macOS, Linux and WSL:
#
#   curl -fsSL https://mimic.codes/install.sh | sh
#
# The same command updates an installed mimic to the latest release (and
# says so when it is already the latest). Options, after `sh -s --`:
#
#   curl -fsSL https://mimic.codes/install.sh | sh -s -- --version 0.3.16
#   curl -fsSL https://mimic.codes/install.sh | sh -s -- --force
#   curl -fsSL https://mimic.codes/install.sh | sh -s -- --uninstall
#   curl -fsSL https://mimic.codes/install.sh | sh -s -- --preview
#   curl -fsSL https://mimic.codes/install.sh | sh -s -- --nightly
#
# Channels: stable releases (the default), previews of the next version
# (--preview: the newest preview or stable release), and nightly builds of
# the development branch (--nightly). The channel chosen is remembered, so
# later updates stay on it; --stable goes back.
#
# It downloads the release for this system from GitHub, checks it against
# the release's SHA256SUMS, and runs the installer inside it, which puts
# mimic in ~/.mimic/bin and on your PATH (no administrator rights needed).
# `mimic upgrade` runs this same script.
#
# Everything is inside main(), called on the last line: a download cut short
# runs nothing, and nothing here reads standard input (with `curl | sh` the
# script itself arrives there).

set -eu

REPO="mimiccodes/mimic"

say() { printf '%s\n' "$*"; }
fail() {
    printf 'error: %s\n' "$*" >&2
    exit 1
}

usage() {
    say "Install or update Mimic: curl -fsSL https://mimic.codes/install.sh | sh"
    say ""
    say "options (after 'sh -s --'):"
    say "  --version X.Y.Z   that release instead of the latest (also MIMIC_VERSION)"
    say "  --force           install even when that version is already installed"
    say "  --check           only say whether an update is available"
    say "  --preview         the newest preview of the next version (or stable, when newer)"
    say "  --nightly         the nightly build of the development branch"
    say "  --stable          stable releases again (the default)"
    say "                    (the channel is remembered for later updates)"
    say "  --uninstall       remove mimic (your projects stay)"
}

# fetch URL FILE: with curl, or wget when there is no curl
fetch() {
    if command -v curl >/dev/null 2>&1; then
        curl -fsSL --retry 3 --proto '=https' --tlsv1.2 -o "$2" "$1"
    elif command -v wget >/dev/null 2>&1; then
        wget -q --https-only -O "$2" "$1"
    else
        fail "curl or wget is needed to download Mimic"
    fi
}

# the latest release's version, from GitHub's redirect to its tag (not the
# API, which limits how often it may be asked)
latest() {
    url="https://github.com/$REPO/releases/latest"
    if command -v curl >/dev/null 2>&1; then
        tag=$(curl -fsSL --retry 3 --proto '=https' -o /dev/null -w '%{url_effective}' "$url")
    else
        tag=$(wget -q --max-redirect=5 --server-response -O /dev/null "$url" 2>&1 | sed -n 's/^ *[Ll]ocation: *//p' | tail -1 | tr -d '\r')
    fi
    v=${tag##*/v}
    case "$v" in
    [0-9]*.[0-9]*.[0-9]*) printf '%s' "$v" ;;
    *) fail "could not find the latest release on GitHub (got '$tag'); check your connection, or pass --version" ;;
    esac
}

# the newest preview or stable release: the first in GitHub's list (newest
# first) that is either; the API is asked once
newest_preview() {
    fetch "https://api.github.com/repos/$REPO/releases?per_page=30" "$1" ||
        fail "could not list the releases on GitHub (try again in a while, or pass --version)"
    v=$(grep -o '"tag_name": *"[^"]*"' "$1" | sed 's/.*"v\([^"]*\)"$/\1/' |
        grep -E '^[0-9]+\.[0-9]+\.[0-9]+(-(preview|fix)\.[0-9]+)?$' | head -1)
    [ -n "$v" ] || fail "found no preview or stable release on GitHub"
    printf '%s' "$v"
}

sha256() {
    if command -v sha256sum >/dev/null 2>&1; then
        sha256sum "$1" | cut -d ' ' -f 1
    elif command -v shasum >/dev/null 2>&1; then
        shasum -a 256 "$1" | cut -d ' ' -f 1
    else
        fail "sha256sum or shasum is needed to check the download"
    fi
}

# which release fits this system
target() {
    os=$(uname -s)
    arch=$(uname -m)
    case "$os" in
    Darwin)
        # a terminal under Rosetta reports x86_64 on Apple silicon
        if [ "$arch" = x86_64 ] && [ "$(sysctl -n hw.optional.arm64 2>/dev/null || echo 0)" = 1 ]; then
            arch=arm64
        fi
        [ "$arch" = arm64 ] || fail "Mimic's macOS release is for Apple silicon; on an Intel Mac, build it from source (https://github.com/$REPO)"
        printf 'macos-arm64'
        ;;
    Linux)
        if ldd --version 2>&1 | grep -qi musl; then
            fail "this Linux uses musl (Alpine and others); the release needs glibc 2.28 or newer. Build from source, or use a glibc distribution"
        fi
        case "$arch" in
        x86_64 | amd64) printf 'linux-x86_64' ;;
        *) fail "there is no Linux release for $arch yet; build it from source (https://github.com/$REPO)" ;;
        esac
        ;;
    MINGW* | MSYS* | CYGWIN*)
        fail "on Windows, run in PowerShell:  irm https://mimic.codes/install.ps1 | iex"
        ;;
    *)
        fail "there is no release for $os yet; build it from source (https://github.com/$REPO)"
        ;;
    esac
}

uninstall() {
    home=${MIMIC_HOME:-$HOME/.mimic}
    bin=${MIMIC_BIN:-$home/bin}
    rm -f "$bin/mimic" "$home/installed-version" "$home/channel"
    for p in "$HOME/.zshrc" "$HOME/.bashrc" "$HOME/.bash_profile" "$HOME/.profile" "$HOME/.config/fish/config.fish"; do
        if [ -f "$p" ] && grep -q "# added by mimic's install.sh" "$p"; then
            grep -v "# added by mimic's install.sh" "$p" >"$p.mimic-tmp" || true
            mv "$p.mimic-tmp" "$p"
        fi
    done
    say "mimic removed from $bin (the build cache stays in $home/cache; delete it if you like)"
}

main() {
    version=${MIMIC_VERSION:-}
    force=0
    check=0
    channel=${MIMIC_CHANNEL:-}
    while [ $# -gt 0 ]; do
        case "$1" in
        --version)
            [ $# -ge 2 ] || fail "--version needs a version, e.g. --version 0.3.16"
            version=${2#v}
            shift 2
            ;;
        --force) force=1 && shift ;;
        --check) check=1 && shift ;;
        --stable | --preview | --nightly) channel=${1#--} && shift ;;
        --uninstall) uninstall && return 0 ;;
        -h | --help) usage && return 0 ;;
        *) fail "unknown option '$1' (see --help)" ;;
        esac
    done

    home=${MIMIC_HOME:-$HOME/.mimic}
    bin=${MIMIC_BIN:-$home/bin}
    t=$(target)
    # the channel: as asked, else the one last installed from, else stable
    if [ -z "$channel" ] && [ -f "$home/channel" ]; then channel=$(head -1 "$home/channel"); fi
    case "${channel:-stable}" in
    stable | preview | nightly) channel=${channel:-stable} ;;
    *) fail "unknown channel '$channel' (stable, preview or nightly)" ;;
    esac

    tmp=$(mktemp -d 2>/dev/null || mktemp -d -t mimic)
    trap 'rm -rf "$tmp"' EXIT INT TERM
    latest_is="the latest"
    tag=""
    if [ -z "$version" ]; then
        case "$channel" in
        stable) version=$(latest) ;;
        preview)
            version=$(newest_preview "$tmp/releases.json")
            latest_is="the latest preview"
            ;;
        nightly)
            fetch "https://github.com/$REPO/releases/download/nightly/version.txt" "$tmp/version.txt" ||
                fail "could not find the nightly build on GitHub"
            version=$(head -1 "$tmp/version.txt" | tr -d '\r')
            tag=nightly
            latest_is="the latest nightly"
            ;;
        esac
    fi
    [ -n "$tag" ] || tag="v$version"

    current=""
    if [ -x "$bin/mimic" ]; then
        current=$("$bin/mimic" --version 2>/dev/null </dev/null | head -1 | cut -d ' ' -f 2 || true)
    fi
    if [ "$check" = 1 ]; then
        if [ -z "$current" ]; then
            say "Mimic is not installed in $bin; the latest is $version"
        elif [ "$current" = "$version" ]; then
            say "mimic $current is $latest_is"
        else
            say "mimic $current is installed; $version is available (run this script again, or mimic upgrade)"
        fi
        return 0
    fi
    if [ "$current" = "$version" ] && [ "$force" = 0 ]; then
        say "mimic $version is already installed in $bin, and is $latest_is (--force reinstalls it)"
        printf '%s\n' "$channel" >"$home/channel"
        return 0
    fi

    name="mimic-$version-$t.tar.gz"
    base="https://github.com/$REPO/releases/download/$tag"
    if [ -n "$current" ]; then
        say "updating mimic $current to $version ($t)"
    else
        say "installing mimic $version ($t)"
    fi
    fetch "$base/$name" "$tmp/$name" || fail "could not download $base/$name (is $version a release?)"
    fetch "$base/SHA256SUMS" "$tmp/SHA256SUMS" || fail "could not download the release's SHA256SUMS"
    want=$(grep " \*\{0,1\}$name\$" "$tmp/SHA256SUMS" | cut -d ' ' -f 1 | head -1)
    [ -n "$want" ] || fail "$name is not listed in SHA256SUMS"
    have=$(sha256 "$tmp/$name")
    [ "$want" = "$have" ] || fail "the download does not match SHA256SUMS (got $have); nothing was installed"
    tar -xzf "$tmp/$name" -C "$tmp"
    pkg="$tmp/mimic-$version-$t"
    [ -f "$pkg/install.sh" ] || fail "the release has no install.sh"
    # the release's own installer: it replaces mimic safely while an editor
    # runs it, clears an old version's build cache and sets PATH
    MIMIC_HOME=$home MIMIC_BIN=$bin sh "$pkg/install.sh" </dev/null
    printf '%s\n' "$channel" >"$home/channel"
    [ "$channel" = stable ] || say "on the $channel channel: later updates stay on it (--stable goes back)"
}

main "$@"
