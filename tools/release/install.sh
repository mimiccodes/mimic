#!/bin/sh
# Install mimic from this folder (Linux and macOS):
#
#   ./install.sh              # into ~/.mimic/bin, and onto PATH
#   ./install.sh --uninstall  # remove it again (your projects stay)
#
# MIMIC_BIN=/some/dir ./install.sh installs somewhere else.
set -eu

here=$(cd "$(dirname "$0")" && pwd)
bin=${MIMIC_BIN:-$HOME/.mimic/bin}
line="export PATH=\"$bin:\$PATH\""
os=$(uname -s)

# the shell start-up files that set PATH for new terminals
profiles() {
    case "${SHELL:-}" in
    */zsh) echo "$HOME/.zshrc" ;;
    */bash) if [ "$os" = Darwin ]; then echo "$HOME/.bash_profile"; else echo "$HOME/.bashrc"; fi ;;
    */fish) echo "$HOME/.config/fish/config.fish" ;;
    *) echo "$HOME/.profile" ;;
    esac
}

if [ "${1:-}" = "--uninstall" ]; then
    rm -f "$bin/mimic"
    for p in $(profiles); do
        if [ -f "$p" ]; then
            grep -v "# added by mimic's install.sh" "$p" > "$p.mimic-tmp" || true
            mv "$p.mimic-tmp" "$p"
        fi
    done
    echo "mimic removed from $bin (the build cache stays in ~/.mimic/cache; delete it if you like)"
    exit 0
fi

[ -f "$here/mimic" ] || { echo "error: no mimic next to this script" >&2; exit 1; }

# ---- what the system needs -------------------------------------------------
ok=1
case "$os" in
Darwin)
    [ "$(uname -m)" = arm64 ] || { echo "error: this mimic is for Apple silicon (arm64)" >&2; exit 1; }
    if ! xcode-select -p >/dev/null 2>&1; then
        echo "needed: the Xcode Command Line Tools (the macOS SDK). Install them with:"
        echo "    xcode-select --install"
        ok=0
    fi
    ;;
Linux)
    [ "$(uname -m)" = x86_64 ] || { echo "error: this mimic is for x86_64 Linux" >&2; exit 1; }
    glibc=$(ldd --version 2>/dev/null | head -1 | grep -oE '[0-9]+\.[0-9]+$' || true)
    if [ -n "$glibc" ]; then
        major=${glibc%.*}
        minor=${glibc#*.}
        if [ "$major" -lt 2 ] || { [ "$major" -eq 2 ] && [ "$minor" -lt 28 ]; }; then
            echo "error: this mimic needs glibc 2.28 or newer (Ubuntu 20.04, Debian 10, RHEL 8 or later); this system has $glibc" >&2
            exit 1
        fi
    fi
    if ! command -v cc >/dev/null 2>&1; then
        echo "needed for native Linux programs: a C compiler (cc) to link them. For example:"
        echo "    sudo apt install build-essential      # Ubuntu, Debian"
        echo "    sudo dnf install gcc                  # Fedora"
        echo "(static programs, --target x86_64-linux, need nothing else)"
        ok=0
    fi
    ;;
*)
    echo "error: use install.cmd on Windows" >&2
    exit 1
    ;;
esac

# ---- install ---------------------------------------------------------------
mkdir -p "$bin"
cp "$here/mimic" "$bin/mimic"
chmod +x "$bin/mimic"
# a downloaded program is quarantined on macOS
[ "$os" = Darwin ] && xattr -d com.apple.quarantine "$bin/mimic" 2>/dev/null || true
echo "installed $bin/mimic"

case ":$PATH:" in
*":$bin:"*) ;;
*)
    for p in $(profiles); do
        mkdir -p "$(dirname "$p")"
        if [ "${p##*.}" = fish ]; then
            entry="fish_add_path $bin # added by mimic's install.sh"
        else
            entry="$line # added by mimic's install.sh"
        fi
        if ! grep -qs "# added by mimic's install.sh" "$p"; then
            echo "$entry" >> "$p"
            echo "added $bin to PATH in $p (open a new terminal, or run: $line)"
        fi
    done
    ;;
esac

"$bin/mimic" --version
if [ "$ok" = 1 ]; then
    echo
    echo "ready. Try:  mimic create hello && cd hello && mimic run"
else
    echo
    echo "mimic is installed; install what is listed above, then try:  mimic create hello && cd hello && mimic run"
fi
