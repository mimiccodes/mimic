#!/bin/sh
# Install or update the Mimic extension for VS Code (and VS Code Insiders,
# Cursor, Windsurf and VSCodium) on macOS, Linux and WSL:
#
#   curl -fsSL https://mimic.codes/vscode.sh | sh
#
# It downloads the latest extension (.vsix) from GitHub and installs it in
# every one of those editors it finds, on PATH or where they are usually
# installed; an editor that already has that version is left as it is.
# Options, after `sh -s --`:
#
#   --editor cursor     only that editor (code, code-insiders, cursor, windsurf, codium)
#   --version 0.10.5    that release instead of the latest
#   --force             reinstall even when that version is there
#
# `mimic upgrade --editors` runs this same script.

set -eu

REPO="mimiccodes/mimic-vscode"
ID="mimiccodes.mimic"

say() { printf '%s\n' "$*"; }
fail() {
    printf 'error: %s\n' "$*" >&2
    exit 1
}

fetch() {
    if command -v curl >/dev/null 2>&1; then
        curl -fsSL --retry 3 --proto '=https' --tlsv1.2 -o "$2" "$1"
    elif command -v wget >/dev/null 2>&1; then
        wget -q --https-only -O "$2" "$1"
    else
        fail "curl or wget is needed to download the extension"
    fi
}

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
    *) fail "could not find the latest extension on GitHub (got '$tag'); check your connection, or pass --version" ;;
    esac
}

# the command lines of the editors on this machine: on PATH, or inside the
# usual install folders (a Mac app started from the Dock puts nothing on PATH)
editors() {
    for c in code code-insiders cursor windsurf codium; do
        command -v "$c" 2>/dev/null || true
    done
    for app in "Visual Studio Code" "Visual Studio Code - Insiders" "Cursor" "Windsurf" "VSCodium"; do
        for root in /Applications "$HOME/Applications"; do
            d="$root/$app.app/Contents/Resources/app/bin"
            for c in code code-insiders cursor windsurf codium; do
                [ -x "$d/$c" ] && printf '%s\n' "$d/$c"
            done
        done
    done
    for d in /usr/share/code/bin /usr/share/code-insiders/bin /usr/share/codium/bin /opt/visual-studio-code/bin /snap/bin "$HOME/.local/share/cursor/bin"; do
        for c in code code-insiders cursor windsurf codium; do
            [ -x "$d/$c" ] && printf '%s\n' "$d/$c"
        done
    done
    return 0
}

main() {
    version=""
    only=""
    force=0
    while [ $# -gt 0 ]; do
        case "$1" in
        --version)
            [ $# -ge 2 ] || fail "--version needs a version"
            version=${2#v}
            shift 2
            ;;
        --editor)
            [ $# -ge 2 ] || fail "--editor needs a name (code, code-insiders, cursor, windsurf, codium)"
            only=$2
            shift 2
            ;;
        --force) force=1 && shift ;;
        -h | --help)
            say "Install or update the Mimic extension: curl -fsSL https://mimic.codes/vscode.sh | sh"
            say "options (after 'sh -s --'): --editor NAME, --version X.Y.Z, --force"
            return 0
            ;;
        *) fail "unknown option '$1' (see --help)" ;;
        esac
    done

    # each editor once (the same one can be on PATH and in its folder)
    found=""
    seen=""
    for e in $(editors | tr ' ' '\001'); do
        e=$(printf '%s' "$e" | tr '\001' ' ')
        n=$(basename "$e")
        [ -z "$only" ] || [ "$n" = "$only" ] || continue
        case " $seen " in *" $n "*) continue ;; esac
        seen="$seen $n"
        found="$found
$e"
    done
    [ -n "$(printf '%s' "$found" | tr -d '\n')" ] || fail "no editor found${only:+ named $only} (VS Code, Cursor, Windsurf, VSCodium); install one, or put its command line on PATH"

    [ -n "$version" ] || version=$(latest)
    tmp=$(mktemp -d 2>/dev/null || mktemp -d -t mimic-vscode)
    trap 'rm -rf "$tmp"' EXIT INT TERM
    vsix="$tmp/mimic-$version.vsix"

    printf '%s\n' "$found" | while IFS= read -r e; do
        [ -n "$e" ] || continue
        n=$(basename "$e")
        have=$("$e" --list-extensions --show-versions </dev/null 2>/dev/null | grep -i "^$ID@" | cut -d @ -f 2 || true)
        if [ "$have" = "$version" ] && [ "$force" = 0 ]; then
            say "$n: the Mimic extension $version is already installed"
            continue
        fi
        if [ ! -f "$vsix" ]; then
            fetch "https://github.com/$REPO/releases/download/v$version/mimic-$version.vsix" "$vsix" || fail "could not download the extension $version"
        fi
        if [ "$have" = "$version" ]; then
            say "$n: reinstalling the Mimic extension $version"
        elif [ -n "$have" ]; then
            say "$n: updating the Mimic extension $have to $version"
        else
            say "$n: installing the Mimic extension $version"
        fi
        # (the editor's own warnings only when it fails)
        if ! "$e" --install-extension "$vsix" --force </dev/null >/dev/null 2>"$tmp/err"; then
            cat "$tmp/err" >&2
            fail "$n could not install the extension"
        fi
        : >"$tmp/changed"
    done
    if [ -f "$tmp/changed" ]; then
        say "done: reload the editor's window (or restart it) to use the new version"
    fi
}

main "$@"
