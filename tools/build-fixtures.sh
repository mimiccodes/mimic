#!/usr/bin/env bash
# Build every golden run-fixture for TARGET into DIR (debug and release),
# with the expected outputs, for tools/run-fixtures.sh on the target machine.
#
#   tools/build-fixtures.sh x86_64-windows winfix
set -eu
target=$1
dir=$2
mimic=${MIMIC:-target/release/mimic}
mkdir -p "$dir/debug" "$dir/release"
# the target's OS word (windows, linux, …): <name>.<os>.out is the expected
# output where it differs, and not-yet-<os>.txt lists what cannot run yet
what=$target
[ "$target" = native ] && what=$(uname -s | tr '[:upper:]' '[:lower:]')
case "$what" in
*windows* | mingw* | msys* | cygwin*) os=windows ;;
*linux*) os=linux ;;
*apple* | *macos* | darwin*) os=macos ;;
*) os=other ;;
esac
skip=" $(grep -v '^#' "tests/fixtures/run/not-yet-$os.txt" 2>/dev/null | tr '\n' ' ') "
skipped=0
for f in tests/fixtures/run/*.mim; do
    name=$(basename "$f" .mim)
    case "$skip" in *" $name "*) skipped=$((skipped + 1)); continue ;; esac
    for profile in debug release; do
        exe=$("$mimic" build --$profile --target "$target" --emit-json "$f" |
            python3 -c 'import json, sys; print(json.load(sys.stdin)["artifacts"][0]["path"])')
        cp "$exe" "$dir/$profile/"
    done
    if [ -f "tests/fixtures/run/$name.$os.out" ]; then
        cp "tests/fixtures/run/$name.$os.out" "$dir/$name.out"
    else
        cp "tests/fixtures/run/$name.out" "$dir/"
    fi
done
echo "built $(ls "$dir"/*.out | wc -l | tr -d ' ') fixtures for $target into $dir ($skipped not yet for $os)"
