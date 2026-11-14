#!/usr/bin/env bash
# Check everyday features one by one against the compiler and print which
# are missing (the list and the versions that close them: docs/gaps.md).
#
#   tools/gaps.sh                 # uses target/release/mimic
#   MIMIC=path/to/mimic tools/gaps.sh
#
# Each check is a tiny program; "missing" means it does not compile. When a
# gap closes, its check moves into the golden tests.
set -u
mimic=${MIMIC:-target/release/mimic}
dir=$(mktemp -d)
trap 'rm -rf "$dir"' EXIT
total=0
missing=0

check() { # name, body of main (Mimic), extra declarations before main
    local name=$1 body=$2 decl=${3:-}
    total=$((total + 1))
    # (files named gap_*: a program takes its file's name, which must not
    # be the name being checked)
    printf 'module main\n\n%s\n\nfn main() {\n%s\n}\n' "$decl" "$body" > "$dir/gap_$name.mim"
    local out
    out=$("$mimic" check "$dir/gap_$name.mim" 2>&1)
    if printf '%s' "$out" | grep -q '^error'; then
        missing=$((missing + 1))
        printf 'missing  %-22s %s\n' "$name" "$(printf '%s' "$out" | grep -m1 '^error' | cut -c1-90)"
    else
        printf 'ok       %s\n' "$name"
    fi
}

# a C header for the C-interop checks
cat > "$dir/cx.h" <<'EOF'
#define ANSWER 42
#define GREETING "hi"
enum color { RED, GREEN = 5 };
extern int counter;
struct buf { char data[8]; int len; };
union num { int i; float f; };
int use_buf(struct buf b);
EOF
c='trusted extern "C" include "cx.h"
trusted extern "C" include "stdio.h"'

echo "== found by first users"
check escape_hex      '    print("\x1b[0m")'
check stdlib_h        '    print(c::div(7, 2).quot)' 'trusted extern "C" include "stdlib.h"'
echo "== terminal and console"
check io_write        '    io::write("no newline")'
check io_flush        '    io::flush()'
check io_read_char    '    print(io::read_char())'
check io_read_bytes   '    print(io::read_bytes(4))'
check term_raw        '    term::raw(true)'
check term_size       '    print(term::size())'
check term_is_tty     '    print(term::is_terminal(1))'
check term_read_key   '    print(term::read_key())'
check term_cursor     '    term::move_to(0, 0)
    term::clear()'
check on_interrupt    '    os::on_interrupt(() => print("bye"))'
echo "== text and numbers"
check char_code       '    print(char_code("A"))'
check char_of         '    print(char_of(65))'
check fmt_spec        '    let x = 3.14159
    print("{x:.2}")'
check int_hex         '    print(format(255, "x"))'
check parse_int_base  '    print(parse_int("ff", 16))'
check pad_start       '    print(pad_start("7", 3, "0"))'
check words           '    print(words("a  b c"))'
check slice_text      '    print(slice("hello", 1, 3))'
echo "== collections"
check unique          '    print(unique([1 1 2]))'
check sort_by         '    print(sort_by([3 1 2], x => 0 - x))'
check zip             '    print(zip([1 2], [3 4]))'
check enumerate       '    print(enumerate(["a" "b"]))'
check any_all         '    print(any([1 2 3] > 2))'
check count_by        '    print(count([1 2 3], x => x > 1))'
check group_by        '    print(group_by([1 2 3], x => x % 2))'
check flatten         '    print(flatten([[1 2] [3]]))'
check clamp           '    print(clamp(15, 0, 10))'
check sets            '    print(set::of([1 2 2]))'
check to_map          '    print(to_map([("a", 1)]))'
check min_max_by      '    print(min_by([3 1 2], x => x))'
echo "== programs and the system"
check panic           '    panic("stop")'
check assert          '    assert(1 == 1)'
check env_all         '    print(os::env_all())'
check fs_walk         '    print(fs::walk("."))'
check fs_modified     '    print(fs::modified("."))'
check time_pattern    '    print(time::format(time::now(), "%Y-%m-%d"))'
check random_choice   '    print(random::choice([1 2 3]))'
check random_shuffle  '    print(random::shuffle([1 2 3]))'
echo "== C interop"
check c_define_int    '    print(c::ANSWER)' "$c"
check c_define_str    '    print(c::GREETING)' "$c"
check c_enum          '    print(c::GREEN)' "$c"
check c_global        '    print(c::counter)' "$c"
check c_stdout        '    c::fflush(c::stdout)' "$c"
check c_array_field   '    print(c::use_buf(c::Buf { data: [0 0 0 0 0 0 0 0], len: 0 }))' "$c"
echo "== later (unions: version 0.10)"
check c_union         '    let n = c::Num { i: 1 }' "$c"
echo
echo "$missing of $total checks missing (docs/gaps.md)"
