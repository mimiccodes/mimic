#!/usr/bin/env bash
# Run prebuilt golden-fixture programs and compare their output with the
# expected `.out` files: for targets whose programs are built on one machine
# and run on another (Windows programs cross-built on Linux, for example).
#
#   tools/build-fixtures.sh TARGET DIR   # on the building machine
#   tools/run-fixtures.sh DIR            # on the target machine
#
# DIR holds <name>.out and <profile>/<name><suffix> for profile debug and
# release. Every program runs like in the golden tests: debug, release,
# debug on 8 threads one element per chunk (leak check on), release on one
# thread with the GPU forced on.
set -u
dir=$(cd "$1" && pwd) # absolute: each program runs in a temporary directory
fail=0
total=0
# a program that hangs fails on its own instead of stopping the whole run
limit=""
command -v timeout >/dev/null && limit="timeout 120"
for want in "$dir"/*.out; do
    name=$(basename "$want" .out)
    for run in "debug|" "release|" "debug|MIMIC_THREADS=8 MIMIC_PAR_GRAIN=1" "release|MIMIC_THREADS=1 MIMIC_GPU=1"; do
        profile=${run%%|*}
        envs=${run#*|}
        exe=""
        for e in "$dir/$profile/$name" "$dir/$profile/$name.exe"; do
            [ -f "$e" ] && exe=$e
        done
        total=$((total + 1))
        if [ -z "$exe" ]; then
            echo "FAIL $name ($profile): not built"
            fail=$((fail + 1))
            continue
        fi
        tmp=$(mktemp -d)
        got=$(cd "$tmp" && env $envs TMPDIR="$tmp" TMP="$tmp" TEMP="$tmp" $limit "$exe" </dev/null 2>"$tmp.err"; echo "exit $?")
        rm -rf "$tmp"
        expected=$(cat "$want"; echo "exit 0")
        if [ "$got" != "$expected" ] || grep -q leak "$tmp.err"; then
            echo "FAIL $name ($profile${envs:+, $envs})"
            diff <(echo "$expected") <(echo "$got") | head -20
            head -c 2000 "$tmp.err"
            fail=$((fail + 1))
        fi
        rm -f "$tmp.err"
    done
done
echo "$((total - fail)) of $total runs passed"
[ "$fail" -eq 0 ]
