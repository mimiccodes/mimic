//! Compilation pipeline: load sources → parse → check → LLVM codegen →
//! compile the runtime with the embedded Clang → link with the embedded LLD
//! (see toolchain.rs).

use crate::targets::Target;
use crate::report::Report;
use mimic_check::{Options, Program, SourceFile};
use mimic_syntax::codes as C;
use mimic_project::{MANIFEST, Project, packages};
use mimic_syntax::parser::parse_file;
use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

pub const RUNTIME_C: &str = include_str!("../../../runtime/mimic_rt.c");
pub const RUNTIME_H: &str = include_str!("../../../runtime/mimic_rt.h");
pub const RUNTIME_NET_C: &str = include_str!("../../../runtime/mimic_net.c");
pub const RUNTIME_STD_C: &str = include_str!("../../../runtime/mimic_std.c");
pub const RUNTIME_SCHED_C: &str = include_str!("../../../runtime/mimic_sched.c");
pub const RUNTIME_IO_C: &str = include_str!("../../../runtime/mimic_io.c");
pub const RUNTIME_UI_C: &str = include_str!("../../../runtime/mimic_ui.c");
pub const RUNTIME_SOCKET_C: &str = include_str!("../../../runtime/mimic_socket.c");
pub const RUNTIME_WINDOW_C: &str = include_str!("../../../runtime/mimic_window.c");
pub const RUNTIME_GPU_C: &str = include_str!("../../../runtime/mimic_gpu.c");
/// wepoll 1.5.8 (BSD-2-Clause): epoll on Windows, for the event loop there
pub const RUNTIME_WEPOLL_C: &str = include_str!("../../../runtime/third_party/wepoll.c");
pub const RUNTIME_JSON_C: &str = include_str!("../../../runtime/mimic_json.c");
pub const RUNTIME_OWN_C: &str = include_str!("../../../runtime/mimic_own.c");

/// What to compile: a whole project, or a single standalone `.mim` file.
pub enum Input {
    Project(Box<Project>),
    File(PathBuf),
}

impl Input {
    pub fn name(&self) -> String {
        match self {
            Input::Project(p) => p.manifest.name.clone(),
            Input::File(f) => {
                let stem = f.file_stem().map(|s| s.to_string_lossy().replace('-', "_")).unwrap_or_default();
                if mimic_project::valid_ident(&stem) { stem } else { "script".into() }
            }
        }
    }
    pub fn is_bin(&self) -> bool {
        match self {
            Input::Project(p) => p.manifest.bin,
            Input::File(_) => true,
        }
    }
}

pub struct Profile {
    pub release: bool,
    /// `--strict-math`: no fused multiply-adds (bit-identical on every CPU).
    pub strict_math: bool,
    /// `--fast-math`: float arithmetic may be regrouped (see Settings).
    pub fast_math: bool,
    /// Compile pure functions to interaction nets (default on).
    pub net: bool,
    pub opt: u8,
    pub leak_check: bool,
    /// `--cpu`: a CPU model to generate code for (`native` = this machine).
    pub cpu: Option<String>,
    /// `--emit-header`: write `<name>.h` declaring the `extern "C" fn`s.
    pub emit_header: bool,
    /// `mimic test`: a binary that runs the tests (with src/test.mim).
    pub tests: bool,
    /// `--shared`: a library as a shared library (.dll, .so, .dylib)
    /// exporting its `extern "C" fn`s, instead of a static archive.
    pub shared: bool,
    /// `mimic profile`: frame pointers everywhere (a release build in
    /// target/profiling/), so the sampler can walk the stack.
    pub frame_pointers: bool,
    /// `mimic test --coverage`: count each line that runs (Settings::coverage).
    pub coverage: bool,
}

/// `MIMIC_DEBUG_DISABLE=optimizer` (a comma-separated list): turn a part of
/// the compiler off, for testing (the speed gate's own check).
pub fn debug_disabled(what: &str) -> bool {
    std::env::var("MIMIC_DEBUG_DISABLE").is_ok_and(|v| v.split(',').any(|x| x.trim() == what))
}

impl Profile {
    pub fn for_input(input: &Input, release: bool) -> Profile {
        let cfg = match input {
            Input::Project(p) => p.build.clone(),
            Input::File(_) => mimic_project::BuildConfig::default(),
        };
        let p = if release { cfg.release } else { cfg.debug };
        // a real slowdown on purpose, to show the speed gate catches one
        // (bench/run.py --gate); part of the build's cache key like any opt
        let opt = if debug_disabled("optimizer") { 0 } else { p.opt };
        Profile { release, net: true, opt, leak_check: p.leak_check, cpu: None, emit_header: false, strict_math: false, fast_math: false, tests: false, shared: false, frame_pointers: false, coverage: false }
    }
    pub fn name(&self) -> &'static str {
        if self.coverage {
            "coverage"
        } else if self.frame_pointers {
            "profiling"
        } else if self.release {
            "release"
        } else {
            "debug"
        }
    }
}

/// The standard prelude: compiled into every program, visible everywhere.
pub const PRELUDE: &str = concat!("pub module prelude

// Optional values: `let x = find(…) else { return … }` unwraps Some.
pub type Option[T] = Some(T) | None

// Results of operations that can fail: `else` unwraps Ok.
pub type Result[T, E] = Ok(T) | Err(E)

// The value inside, or `default` when there is none:
// `parse_int(s) ▷ unwrap_or(300)`, `get(counts, w) ▷ unwrap_or(0)`.
pub fn unwrap_or[T](o: Option[T], default: T) -> T {
    match o {
        Some(x) => x,
        None => default,
    }
}

// Tests: `expect(…)` in a `test \"…\" { }` block becomes one of these (the
// compiler adds the expression's text and line; runtime/mimic_std.c).
pub extern \"mimic\" ordered {
    fn expect_true(ok: bool, text: str, line: i64)
    fn expect_cmp(op: i64, a: any, b: any, text: str, line: i64)
    // property tests (`test \"…\" for x: i64 { … }`): run cases quietly, count
    // their failed expectations, forget them, report the smallest case
    fn expect_quiet(on: bool)
    fn expect_failures() -> i64
    fn expect_forgive(count: i64)
    fn prop_failed(case: str, line: i64)
}

// Inputs for property tests: mostly small values, sometimes edges and large
// ones; `k` (0 to 99) grows them case by case.
pub fn prop_int(k: i64) -> i64 {
    let r = random::int(0, 10)
    if r == 0 { 0 } else if r < 8 { random::int(0 - k - 3, k + 4) } else { random::int(¯1000000, 1000001) }
}
pub fn prop_float(k: i64) -> f64 {
    if random::int(0, 10) == 0 { 0.0 } else { (random::float() - 0.5) × 2.0 × (1.0 + k × 10.0) }
}
pub fn prop_bool(k: i64) -> bool { random::int(0, 2) == 1 }
pub fn prop_str(k: i64) -> str {
    let letters = [\"a\" \"b\" \"z\" \"A\" \" \" \"1\" \"é\" \"-\"]
    join(⍳ random::int(0, k div 4 + 3) ¨ (i => letters[random::int(0, 8)]), \"\")
}
pub fn prop_list(k: i64, gen) { ⍳ random::int(0, k div 5 + 3) ¨ (i => gen(k)) }
// (an empty list keeps its element type: `⍳ 0 ¨ f` would not know it)
pub fn prop_ints(k: i64) -> [i64] {
    let n = random::int(0, k div 5 + 3)
    if n == 0 { [0] ↓ 1 } else { ⍳ n ¨ (i => prop_int(k)) }
}
pub fn prop_floats(k: i64) -> [f64] {
    let n = random::int(0, k div 5 + 3)
    if n == 0 { [0.0] ↓ 1 } else { ⍳ n ¨ (i => prop_float(k)) }
}
pub fn prop_bools(k: i64) -> [bool] {
    let n = random::int(0, k div 5 + 3)
    if n == 0 { [true] ↓ 1 } else { ⍳ n ¨ (i => prop_bool(k)) }
}
pub fn prop_strs(k: i64) -> [str] {
    let n = random::int(0, k div 5 + 3)
    if n == 0 { [\"\"] ↓ 1 } else { ⍳ n ¨ (i => prop_str(k)) }
}
// Smaller values, nearest the simplest first.
pub fn prop_shrink_int(x: i64) -> [i64] {
    let cands = [0 (x div 2) (x - (if x > 0 { 1 } else { ¯1 }))]
    unique(cands ⌿ ((∣ cands) < ∣ x))
}
pub fn prop_shrink_float(x: f64) -> [f64] {
    let toward_zero = if x < 0.0 { math::ceil(x) } else { math::floor(x) }
    let cands = [0.0 toward_zero (x / 2.0)]
    unique(cands ⌿ ((∣ cands) < ∣ x))
}
pub fn prop_shrink_bool(x: bool) -> [bool] { if x { [false] } else { [] ⌿ [] } }
pub fn prop_shrink_str(s: str) -> [str] {
    let n = ≢ chars(s)
    if n == 0 { [] ⌿ [] } else { unique([\"\" (join(chars(s) ↑ (n div 2), \"\")) (join(chars(s) ↓ 1, \"\"))]) }
}
pub fn prop_shrink_list_int(xs: [i64]) -> [[i64]] {
    let halved = xs ¨ (v => v div 2)
    if any(halved != xs) { push(prop_shrink_list(xs), halved) } else { prop_shrink_list(xs) }
}
pub fn prop_shrink_list_float(xs: [f64]) -> [[f64]] {
    let halved = xs ¨ (v => if v < 0.0 { math::ceil(v / 2.0) } else { math::floor(v / 2.0) })
    if any(halved != xs) { push(prop_shrink_list(xs), halved) } else { prop_shrink_list(xs) }
}
pub fn prop_shrink_list(xs) {
    let n = ≢ xs
    if n == 0 { [xs] ↓ 1 } else { [xs ↑ 0, xs ↑ (n div 2), xs ↓ (n div 2), xs ↓ 1, xs ↓ ¯1] ⌿ ([0 (n div 2) (n - n div 2) (n - 1) (n - 1)] < n) }
}

// Strings, lists and maps: implemented in Mimic's runtime (runtime/mimic_std.c).
pub extern \"mimic\" {
    fn split(s: str, sep: str) -> [str]
    fn join(parts: any, sep: str) -> str
    fn lines(s: str) -> [str]
    fn chars(s: str) -> [str]
    fn trim(s: str) -> str
    fn trim_start(s: str) -> str
    fn trim_end(s: str) -> str
    fn contains(within: any, part: any) -> bool
    fn starts_with(s: str, prefix: str) -> bool
    fn ends_with(s: str, suffix: str) -> bool
    fn find_index(s: str, part: str) -> i64
    fn replace(s: str, from: str, to: str) -> str
    fn upper(s: str) -> str
    fn lower(s: str) -> str
    fn repeat(s: str, n: i64) -> str
    fn string(v: any) -> str
    fn is_int(s: str) -> bool
    fn to_int(s: str) -> i64
    fn is_float(s: str) -> bool
    fn to_float(s: str) -> f64
    fn push(xs: any, x: any) -> any
    // `e else { … }` (the parser writes these): whether an Option or Result
    // holds a value (Some, Ok), and that value
    fn __has_value(o: any) -> bool
    fn __value_of(o: any) -> any
    fn concat(xs: any, ys: any) -> any
    fn sort(xs: any) -> any
    fn has(m: any, key: any) -> bool
    fn put(m: any, key: any, value: any) -> any
    fn map_of(keys: any, values: any) -> any
    fn remove(m: any, key: any) -> any
    fn keys(m: any) -> any
    fn values(m: any) -> any
    fn char_code(s: str) -> i64
    fn char_of(code: i64) -> str
    fn words(s: str) -> [str]
    fn slice(s: str, from: i64, to: i64) -> str
    fn format(value: any, spec: str) -> str
    fn text_pad_start(s: str, width: i64, fill: str) -> str
    fn text_pad_end(s: str, width: i64, fill: str) -> str
    fn is_int_base(s: str, base: i64) -> bool
    fn to_int_base(s: str, base: i64) -> i64
    fn sort_index(keys: any) -> [i64]
    fn group(keys: any, xs: any) -> any
}

// A broken `requires` or `ensures` (debug builds and tests): stops the
// program with what was promised. Pure, like a bounds check, so functions
// with contracts still run in parallel.
// flatten's fast way (see runtime/mimic_std.c): private to the prelude
extern \"mimic\" pure {
    fn flat_plain(xss: any) -> bool
    fn concat_plain(xss: any) -> [any]
}

pub extern \"mimic\" pure {
    fn contract_failed(message: str)
}

// Stopping: `panic(\"why\")` ends the program with a message (status 101).
pub extern \"mimic\" sync {
    fn panic(message: str)
}

// Stop with `message` unless `ok` holds.
pub fn assert(ok: bool, message: str = \"assertion failed\") {
    if not ok { panic(message) }
}

// ---- collections -------------------------------------------------------------

// The distinct elements of `xs`, in order of first appearance.
pub fn unique(xs) { keys(map_of(xs, xs)) }

// `xs` ordered by `key(x)`; equal keys keep their order. `descending: true`
// for the largest first.
pub fn sort_by(xs, key, descending: bool = false) {
    let picked = sort_index(xs ¨ key) ¨ (i => xs[i])
    if descending { ⌽ picked } else { picked }
}

// Pairs of elements at the same position, as long as the shorter list.
pub fn zip(a, b) {
    let n = (≢ a) ⌊ (≢ b)
    ⍳ n ¨ (i => (a[i], b[i]))
}

// Each element with its position: [(0, x0) (1, x1) …].
pub fn enumerate(xs) { ⍳ (≢ xs) ¨ (i => (i, xs[i])) }

// Is any (or every) element of a list of bools true? `any(xs > 2)`.
pub fn any(mask) { (+/ mask) > 0 }
pub fn all(mask) { (+/ mask) == (≢ mask) }

// How many elements `f` says yes to.
pub fn count(xs, f) { +/ (xs ¨ f) }

// A map from each `f(x)` to the elements with that value, in order.
pub fn group_by(xs, f) { group(xs ¨ f, xs) }

// One list from a list of lists. Linear: each element is placed once, in
// parallel (each list's start from a running sum of the lengths, and each
// position's list by replicating the list numbers), where joining the lists
// two at a time copied the growing result again for every list.
pub fn flatten(xss) {
    let m = ≢ xss
    if m == 0 { [] } else if flat_plain(xss) { concat_plain(xss) } else {
        let lens = xss ¨ (x => ≢ x)
        let ends = lens ▷ trail(+)
        let owner = ⍳ m ⌿ lens
        ⍳ ends[m - 1] ¨ (k => {
            let o = owner[k]
            xss[o][k - ends[o] + lens[o]]
        })
    }
}

// The element with the smallest (largest) `f(x)`, or `None` for no elements.
pub fn min_by(xs, f) {
    if (≢ xs) == 0 { None } else { Some(xs[sort_index(xs ¨ f)[0]]) }
}
pub fn max_by(xs, f) {
    if (≢ xs) == 0 { None } else {
        let order = sort_index(xs ¨ f)
        Some(xs[order[(≢ order) - 1]])
    }
}

// `x` kept between `lo` and `hi` (elementwise on lists).
pub fn clamp(x, lo, hi) { (lo ⌈ x) ⌊ hi }

// A map from a list of (key, value) pairs; a later key wins.
pub fn to_map(pairs) { map_of(pairs ¨ (p => p.0), pairs ¨ (p => p.1)) }

// Sets: a map from each element to true, so `has` is fast and order is kept.
pub module set {
    pub fn of(xs) { map_of(xs, xs ¨ (x => true)) }
    pub fn has(s, x) { super::has(s, x) }
    pub fn add(s, x) { put(s, x, true) }
    pub fn remove(s, x) { super::remove(s, x) }
    pub fn items(s) { keys(s) }
    pub fn size(s) { ≢ keys(s) }
    pub fn union(a, b) { of(concat(keys(a), keys(b))) }
    pub fn intersection(a, b) {
        let xs = keys(a)
        of(xs ⌿ (xs ¨ (x => super::has(b, x))))
    }
    pub fn difference(a, b) {
        let xs = keys(a)
        of(xs ⌿ (xs ¨ (x => not super::has(b, x))))
    }
}

// `s` widened to `width` characters with `fill` in front (or after).
pub fn pad_start(s: str, width: i64, fill: str = \" \") -> str { text_pad_start(s, width, fill) }
pub fn pad_end(s: str, width: i64, fill: str = \" \") -> str { text_pad_end(s, width, fill) }

// `Some(byte index)` of the first occurrence of `part`, or `None`.
pub fn find(s: str, part: str) {
    let i = find_index(s, part)
    if i < 0 { None } else { Some(i) }
}

// `Some(n)` when `s` is an integer (in `base`, 2 to 36), else `None`.
pub fn parse_int(s: str, base: i64 = 10) {
    if base == 10 {
        if is_int(s) { Some(to_int(s)) } else { None }
    } else {
        if is_int_base(s, base) { Some(to_int_base(s, base)) } else { None }
    }
}

// `Some(x)` when `s` is a number, else `None`.
pub fn parse_float(s: str) {
    if is_float(s) { Some(to_float(s)) } else { None }
}

// `Some(m[key])` when the map has the key, else `None`.
pub fn get(m, key) {
    if has(m, key) { Some(m[key]) } else { None }
}

// ---- standard modules ------------------------------------------------------
// Used by path from any file, no import needed (`fs::read(p)`); `use fs::read`
// gives a short name. A module of the program with the same name comes first.
// Each extern block says what its functions do (docs/ffi.md):
//   pure        no effects
//   concurrent  may overlap (reads, waiting): implicit futures, parallel ¨
//   ordered     in program order, changes nothing others read (printing)
//   sync        changes shared state: pending futures finish first
// (runtime/mimic_io.c, runtime/mimic_socket.c)

// Files and directories.
pub module fs {
    pub extern \"mimic\" concurrent {
        fn read(path: str) -> Result[str, str]
        fn size(path: str) -> Result[i64, str]
        fn list_dir(path: str) -> Result[[str], str]
        fn exists(path: str) -> bool
        fn is_file(path: str) -> bool
        fn is_dir(path: str) -> bool
        fn current_dir() -> str
        fn temp_dir() -> str
    }

    pub extern \"mimic\" concurrent {
        fn read_bytes(path: str) -> Result[[u8], str]
        // every file below `path`, as paths relative to it, sorted
        fn walk(path: str) -> Result[[str], str]
        // when the file last changed (ns since 1970, like time::now)
        fn modified(path: str) -> Result[i64, str]
    }

    pub extern \"mimic\" sync {
        fn write_bytes(path: str, data: [u8]) -> Result[i64, str]
        fn write(path: str, text: str) -> Result[i64, str]
        fn append(path: str, text: str) -> Result[i64, str]
        fn copy(from: str, to: str) -> Result[i64, str]
        fn rename(from: str, to: str) -> Result[bool, str]
        fn remove(path: str) -> Result[bool, str]
        fn make_dir(path: str) -> Result[bool, str]
    }
}

// Bytes: `[u8]` arrays, one byte per element (each reads as an i64).
pub module bytes {
    pub extern \"mimic\" pure {
        fn from(values: [i64]) -> [u8]
        fn of_text(text: str) -> [u8]
        fn to_text(data: [u8]) -> Result[str, str]
        fn hex(data: [u8]) -> str
        fn from_hex(text: str) -> Result[[u8], str]
        fn base64(data: [u8]) -> str
        fn from_base64(text: str) -> Result[[u8], str]
        // floats as 8 bytes each, little-endian, and back (exact)
        fn of_f64s(values: [f64]) -> [u8]
        fn to_f64s(data: [u8]) -> [f64]
    }
}

// Paths as text: pure.
pub module path {
    pub extern \"mimic\" pure {
        fn join(a: str, b: str) -> str
        fn file_name(path: str) -> str
        fn parent(path: str) -> str
        fn extension(path: str) -> str
        fn stem(path: str) -> str
    }
}

// Standard input and standard error (`print` writes standard output).
pub module io {
    pub extern \"mimic\" ordered {
        fn read_line() -> Option[str]
        fn read_input() -> str
        fn eprint(value: any)
        // like print, without the newline (shown at once on a terminal)
        fn write(value: any)
        fn flush()
        fn read_char() -> Option[str]
        fn read_bytes(n: i64) -> [u8]
    }
}

// The terminal: raw mode, size, keys, the cursor (ANSI codes; on Windows too).
pub module term {
    pub type Size = { columns: i64, rows: i64 }

    pub extern \"mimic\" ordered {
        // no echo, no line editing: keys arrive one at a time (Ctrl-C still works)
        fn raw(on: bool) -> Result[bool, str]
        fn size() -> Option[Size]
        // 0 standard input, 1 standard output, 2 standard error
        fn is_terminal(stream: i64) -> bool
        // \"up\", \"enter\", \"ctrl-c\", … or the character typed
        fn read_key() -> Option[str]
        fn clear()
        // from 0, the top left
        fn move_to(column: i64, row: i64)
        fn show_cursor(on: bool)
        fn alternate_screen(on: bool)
    }
}

// The program and its process: arguments, environment, other programs.
pub module os {
    // What a finished program left behind.
    pub type Output = { status: i64, stdout: str, stderr: str }

    pub extern \"mimic\" concurrent {
        fn args() -> [str]
        fn env(name: str) -> Option[str]
        // every environment variable: a map from name to value
        fn env_all() -> any
        fn pid() -> i64
    }

    pub extern \"mimic\" ordered {
        fn exit(code: i64)
    }

    pub extern \"mimic\" sync {
        fn set_env(name: str, value: str) -> Result[bool, str]
        fn run(program: str, arguments: [str]) -> Result[Output, str]
        // the same, in the working directory `dir` (a relative program
        // path, like ./tool, is found from there)
        fn run_in(dir: str, program: str, arguments: [str]) -> Result[Output, str]
        // Run a command line through the system shell: /bin/sh -c, or
        // cmd.exe /c on Windows.
        fn shell(command: str) -> Result[Output, str]
        // Ctrl-C calls `handler` (on a thread of its own) instead of stopping
        fn on_interrupt(handler: fn())
    }
}

// Clocks and dates. Instants and durations are i64 nanoseconds.
pub module time {
    // weekday: 1 = Monday … 7 = Sunday; utc_offset in seconds
    pub type DateTime = {
        year: i64, month: i64, day: i64,
        hour: i64, minute: i64, second: i64, nanosecond: i64,
        weekday: i64, utc_offset: i64,
    }

    pub extern \"mimic\" concurrent {
        fn now() -> i64
        fn clock() -> i64
        fn sleep(duration: i64)
        fn local(t: i64) -> DateTime
    }

    pub extern \"mimic\" pure {
        fn utc(t: i64) -> DateTime
        fn format_iso(t: i64) -> str
        fn format_pattern(t: i64, pattern: str) -> str
        fn timestamp(year: i64, month: i64, day: i64, hour: i64, minute: i64, second: i64) -> i64
    }

    // ISO 8601 in UTC, or by a pattern: %Y %m %d %H %M %S %f (ms) %j %a %A %b %B %y %I %p %%
    pub fn format(t: i64, pattern: str = \"\") -> str {
        if pattern == \"\" { format_iso(t) } else { format_pattern(t, pattern) }
    }
}

// Random numbers (xoshiro256**, seeded from the system, one per thread).
pub module random {
    pub extern \"mimic\" ordered {
        fn float() -> f64
        fn int(lo: i64, hi: i64) -> i64
        fn seed(n: i64)
        fn permutation(n: i64) -> [i64]
    }

    // One element at random, or `None` for no elements.
    pub fn choice(xs) {
        if (≢ xs) == 0 { None } else { Some(xs[int(0, ≢ xs)]) }
    }

    // The elements in a random order.
    pub fn shuffle(xs) { permutation(≢ xs) ¨ (i => xs[i]) }
}

// JSON text ↔ values: `json::encode(user)`, `json::decode[User](text)`,
// `json::parse(text)` for an untyped tree to match on.
pub module json {
    // An untyped JSON value.
    pub type Value =
        | Null
        | Bool(bool)
        | Number(f64)
        | Text(str)
        | List([Value])
        | Object([str: Value])

    pub extern \"mimic\" pure {
        fn encode(value: any) -> str
        fn encode_pretty(value: any) -> str
        fn parse(text: str) -> Result[Value, str]
        fn decode[T](text: str) -> Result[T, str]
    }
}

// Channels between tasks: `let (tx, rx) = chan::new[str]()`. `send` never
// waits; `receive` waits for a value, or returns None once the channel is
// closed and empty. Tasks are implicit futures (`let t = worker(rx)`) and
// server handlers.
pub module chan {
    pub type Sender[T] = { id: i64 }
    pub type Receiver[T] = { id: i64 }

    pub extern \"mimic\" concurrent {
        fn new[T]() -> (Sender[T], Receiver[T])
        fn bounded[T](capacity: i64) -> (Sender[T], Receiver[T])
        fn send[T](tx: Sender[T], value: T) -> bool
        fn receive[T](rx: Receiver[T]) -> Option[T]
        fn try_receive[T](rx: Receiver[T]) -> Option[T]
        fn close[T](tx: Sender[T])
    }
}

// WebSockets: `ws::connect(url)` (ws:// and wss://), `ws::serve(port, handler)`.
// Sends on one socket are never interleaved; pings are answered for you.
pub module ws {
    pub type Socket = { id: i64 }

    // A received message. `Closed` when the other side has closed.
    pub type Message = Text(str) | Binary([u8]) | Closed

    pub extern \"mimic\" concurrent {
        fn connect(url: str) -> Result[Socket, str]
        fn send(socket: Socket, text: str) -> Result[i64, str]
        fn send_bytes(socket: Socket, data: [u8]) -> Result[i64, str]
        fn receive(socket: Socket) -> Result[Message, str]
        fn close(socket: Socket)
        // one call of `handler` per connection (in parallel when the
        // handler's effects may overlap); the socket closes when it returns
        fn serve(port: i64, handler: fn(Socket)) -> Result[i64, str]
        fn serve_connections(port: i64, handler: fn(Socket), count: i64) -> Result[i64, str]
        fn serve_tls(port: i64, handler: fn(Socket), identity: tls::Identity) -> Result[i64, str]
        fn serve_tls_connections(port: i64, handler: fn(Socket), identity: tls::Identity, count: i64) -> Result[i64, str]
    }
}

// ---- networking: the `tcp` and `http` modules ------------------------------

// TCP sockets: `tcp::connect(host, port)`, … (in program order).
pub module tcp {
    pub type Socket = { fd: i64 }
    pub type Listener = { fd: i64, port: i64 }

    pub extern \"mimic\" ordered {
        fn connect(host: str, port: i64) -> Result[Socket, str]
        fn listen(port: i64) -> Result[Listener, str]
        fn accept(listener: Listener) -> Result[Socket, str]
        fn send(socket: Socket, text: str) -> Result[i64, str]
        fn receive(socket: Socket, limit: i64) -> Result[str, str]
        fn send_bytes(socket: Socket, data: [u8]) -> Result[i64, str]
        fn receive_bytes(socket: Socket, limit: i64) -> Result[[u8], str]
        fn close(socket: any)
    }
}

// TLS: `tls::identity(cert_pem, key_pem)` for servers (`http::serve_tls`,
// `ws::serve_tls`, `tls::accept`), and encrypted TCP streams with
// `tls::connect(host, port)`.
pub module tls {
    // A certificate (chain) and its private key, loaded once.
    pub type Identity = { id: i64 }
    // An encrypted connection.
    pub type Stream = { id: i64 }

    pub extern \"mimic\" concurrent {
        fn self_signed(host: str) -> Result[(str, str), str]
        fn identity(cert: str, key: str) -> Result[Identity, str]
        fn connect(host: str, port: i64) -> Result[Stream, str]
        fn connect_trusting(host: str, port: i64, cert: str) -> Result[Stream, str]
        fn accept(listener: tcp::Listener, identity: Identity) -> Result[Stream, str]
        fn send(stream: Stream, text: str) -> Result[i64, str]
        fn send_bytes(stream: Stream, data: [u8]) -> Result[i64, str]
        fn receive(stream: Stream, limit: i64) -> Result[str, str]
        fn receive_bytes(stream: Stream, limit: i64) -> Result[[u8], str]
        fn close(stream: Stream)
    }
}

// UDP: `udp::bind(port)`, `udp::send(socket, host, port, text)`, `udp::receive(socket)`.
// Datagrams over IPv4; packets may be lost or arrive out of order.
pub module udp {
    pub type Socket = { fd: i64, port: i64 }
    // One received packet: its bytes (also as text) and who sent it.
    pub type Packet = { data: [u8], text: str, host: str, port: i64 }

    pub extern \"mimic\" concurrent {
        fn bind(port: i64) -> Result[Socket, str]
        fn send(socket: Socket, host: str, port: i64, text: str) -> Result[i64, str]
        fn send_bytes(socket: Socket, host: str, port: i64, data: [u8]) -> Result[i64, str]
        fn receive(socket: Socket) -> Result[Packet, str]
        fn receive_within(socket: Socket, timeout: i64) -> Result[Packet, str]
        fn close(socket: Socket)
    }
}

// HTTP: `http::fetch(url)`, `http::serve(port, handler)`, …
pub module http {
    // A request as a server receives it. Header names are lower case.
    pub type Request = { method: str, path: str, query: str, headers: [str: str], body: str }

    // A response (what a server sends and what a client receives).
    pub type Response = { status: i64, headers: [str: str], body: str }

    // Requests overlap (`let a = http::fetch(u1)` and `let b = http::fetch(u2)`
    // are in flight together); a server runs its handler in parallel when the
    // handler's effects may overlap, else one request at a time, in order.
    pub extern \"mimic\" concurrent {
        fn fetch(url: str) -> Result[Response, str]
        fn request(method: str, url: str, headers: [str: str], body: str) -> Result[Response, str]
        fn serve(port: i64, handler: fn(Request) -> Response) -> Result[i64, str]
        fn serve_requests(port: i64, handler: fn(Request) -> Response, count: i64) -> Result[i64, str]
        fn serve_tls(port: i64, handler: fn(Request) -> Response, identity: tls::Identity) -> Result[i64, str]
        fn serve_tls_requests(port: i64, handler: fn(Request) -> Response, identity: tls::Identity, count: i64) -> Result[i64, str]
    }

    pub fn respond(status: i64, body: str) -> Response {
        Response { status: status, headers: [\"content-type\": \"text/plain; charset=utf-8\"], body: body }
    }

    pub fn ok(body: str) -> Response { respond(200, body) }

    pub fn not_found() -> Response { respond(404, \"not found\") }

    // A 200 response with `value` as JSON.
    pub fn json(value: any) -> Response {
        Response { status: 200, headers: [\"content-type\": \"application/json\"], body: super::json::encode(value) }
    }
}

", include_str!("../../../runtime/math.mim"), include_str!("../../../runtime/hash.mim"), include_str!("../../../runtime/text.mim"), include_str!("../../../runtime/graph.mim"), include_str!("../../../runtime/order.mim"), include_str!("../../../runtime/parse.mim"), include_str!("../../../runtime/compress.mim"), include_str!("../../../runtime/ui.mim"));

pub fn prelude_file() -> SourceFile {
    let key = PathBuf::from("<prelude>");
    if let Some(sf) = sources::parsed(&key, PRELUDE, "std", "<prelude>") {
        return sf;
    }
    let ast = parse_file(PRELUDE).expect("the prelude parses");
    let sf = SourceFile { path: key, display: "<prelude>".into(), src: PRELUDE.into(), ast, package: "std".into() };
    sources::remember(&sf);
    sf
}

/// Where source text comes from, and parses worth keeping. A command run
/// once reads every file from disk and keeps nothing; a long-running process
/// (the Workbench engine) turns the cache on, so only changed files are
/// parsed again, and gives the text of files open in an editor, which wins
/// over the file on disk.
pub mod sources {
    use mimic_check::SourceFile;
    use std::collections::HashMap;
    use std::path::{Path, PathBuf};
    use std::sync::Mutex;
    use std::sync::atomic::{AtomicBool, Ordering};

    static KEEP: AtomicBool = AtomicBool::new(false);
    static CONTRACTS: AtomicBool = AtomicBool::new(true);

    /// Compile `requires` and `ensures` checks in (debug builds, tests,
    /// checking) or leave them out (release builds).
    pub fn set_contracts(on: bool) {
        CONTRACTS.store(on, Ordering::Relaxed);
    }

    pub fn contracts() -> bool {
        CONTRACTS.load(Ordering::Relaxed)
    }
    static OPEN: Mutex<Option<HashMap<PathBuf, String>>> = Mutex::new(None);
    /// path → (hash of text, package, display, the parse)
    static PARSED: Mutex<Option<HashMap<PathBuf, (u64, SourceFile)>>> = Mutex::new(None);

    /// Keep parsed files for the life of the process.
    pub fn keep_parses() {
        KEEP.store(true, Ordering::Relaxed);
    }

    /// The text of a file open in an editor (`None`: use the file on disk again).
    pub fn set_open(path: &Path, text: Option<String>) {
        let mut open = OPEN.lock().unwrap();
        let open = open.get_or_insert_with(HashMap::new);
        let real = super::canonical(path);
        match text {
            Some(t) => open.insert(real, t),
            None => open.remove(&real),
        };
    }

    /// The text of `path`: the editor's, or the file's.
    pub fn read(path: &Path) -> std::io::Result<String> {
        if let Some(open) = OPEN.lock().unwrap().as_ref()
            && !open.is_empty()
        {
            // the same file may be named two ways (`/var` and `/private/var`)
            let real = super::canonical(path);
            if let Some(t) = open.get(path).or_else(|| open.get(&real)) {
                return Ok(t.clone());
            }
        }
        std::fs::read_to_string(path)
    }

    /// A parse kept from before, if the text is the same.
    pub fn parsed(path: &Path, src: &str, package: &str, display: &str) -> Option<SourceFile> {
        if !KEEP.load(Ordering::Relaxed) {
            return None;
        }
        let hash = mimic_syntax::fnv1a(src.as_bytes());
        let kept = PARSED.lock().unwrap();
        let (h, sf) = kept.as_ref()?.get(path)?;
        // (the parse depends on whether contracts are compiled in)
        (*h == hash ^ contracts() as u64 && sf.package == package && sf.display == display && sf.src == src).then(|| sf.clone())
    }

    /// Keep a fresh parse (before C headers are imported into it).
    pub fn remember(sf: &SourceFile) {
        if KEEP.load(Ordering::Relaxed) {
            let hash = mimic_syntax::fnv1a(sf.src.as_bytes()) ^ contracts() as u64;
            PARSED.lock().unwrap().get_or_insert_with(HashMap::new).insert(sf.path.clone(), (hash, sf.clone()));
        }
    }
}

fn display_path(root: Option<&Path>, p: &Path) -> String {
    root.and_then(|r| p.strip_prefix(r).ok()).unwrap_or(p).display().to_string()
}

fn read_parse(path: &Path, display: String, package: &str, report: &mut Report, tests: bool) -> Option<SourceFile> {
    let src = match sources::read(path) {
        Ok(s) if tests => with_doc_tests(&s),
        Ok(s) => s,
        Err(e) => {
            report.push_plain(&C::IO, format!("cannot read {}: {e}", path.display()));
            return None;
        }
    };
    if let Some(sf) = sources::parsed(path, &src, package, &display) {
        return Some(sf);
    }
    match parse_file(&src).and_then(|mut ast| mimic_syntax::desugar::lift_arms(&mut ast).map(|_| ast)) {
        Ok(mut ast) => {
            mimic_syntax::desugar::contracts(&mut ast, &src, sources::contracts());
            expect_with_text(&mut ast.module.items, &src);
            // (after the `expect` rewrite: a property's body moves out of its test)
            if let Err(d) = mimic_syntax::desugar::properties(&mut ast) {
                report.push_in(Some(path), &display, &src, d);
                return None;
            }
            let sf = SourceFile { path: path.to_path_buf(), display, src, ast, package: package.into() };
            sources::remember(&sf);
            Some(sf)
        }
        Err(d) => {
            report.push_in(Some(path), &display, &src, d);
            None
        }
    }
}

/// Examples in comments, for `mimic test`: a `// > expression` line and the
/// comment line after it, which is what `print` would show for the value.
///
/// ```text
/// // The larger of two numbers.
/// // > larger(3, 7)
/// // 7
/// ```
///
/// Each becomes a test at the end of the file (so line numbers stay), in the
/// file's own module.
pub fn with_doc_tests(src: &str) -> String {
    let lines: Vec<&str> = src.lines().collect();
    let mut tests = String::new();
    for (i, line) in lines.iter().enumerate() {
        let Some(expr) = line.trim_start().strip_prefix("// > ") else { continue };
        let Some(want) = lines.get(i + 1).and_then(|l| l.trim_start().strip_prefix("//")) else { continue };
        let want = want.strip_prefix(' ').unwrap_or(want);
        if want.starts_with("> ") {
            continue;
        }
        let quoted: String = want
            .chars()
            .flat_map(|c| match c {
                '"' | '\\' | '{' | '}' => vec!['\\', c],
                _ => vec![c],
            })
            .collect();
        tests += &format!("\ntest \"example on line {}\" {{ expect(string({}) == \"{quoted}\") }}\n", i + 1, expr.trim());
    }
    if tests.is_empty() { src.to_string() } else { format!("{src}\n{tests}") }
}

/// In tests (`test "…" { }`, functions named `test …`): `expect(a == b)`
/// becomes `expect_cmp(op, a, b, "a == b", line)` and `expect(c)` becomes
/// `expect_true(c, "c", line)`, so a failure shows what was expected.
fn expect_with_text(items: &mut [mimic_syntax::ast::Item], src: &str) {
    use mimic_syntax::ast::{BinOp, Expr, ExprKind, Item};
    for it in items {
        match it {
            Item::Module(m) => expect_with_text(&mut m.items, src),
            Item::Fn(f) if f.name.starts_with("test ") => {
                f.body.walk_mut(&mut |e: &mut Expr| {
                    let ExprKind::Call { callee, args } = &mut e.kind else { return };
                    if args.len() != 1 || !matches!(&callee.kind, ExprKind::Path(p) if p.len() == 1 && p[0] == "expect") {
                        return;
                    }
                    let arg = args.pop().unwrap();
                    let text = src.get(arg.span.start..arg.span.end).unwrap_or("").trim().to_string();
                    let span = e.span;
                    let lit = |k: ExprKind| Expr { kind: k, span };
                    let line = lit(ExprKind::Int(span.line as i64));
                    let (name, new_args) = match arg.kind {
                        ExprKind::Binary { op: op @ (BinOp::Eq | BinOp::Ne | BinOp::Lt | BinOp::Le | BinOp::Gt | BinOp::Ge), lhs, rhs } => {
                            ("expect_cmp", vec![lit(ExprKind::Int(op as i64)), *lhs, *rhs, lit(ExprKind::Str(text)), line])
                        }
                        kind => ("expect_true", vec![Expr { kind, span: arg.span }, lit(ExprKind::Str(text)), line]),
                    };
                    **callee = lit(ExprKind::Path(vec![name.into()]));
                    *args = new_args;
                });
            }
            _ => {}
        }
    }
}

/// Load, parse, resolve and check for the host. Errors come back as a structured report.
pub fn load(input: &Input) -> Result<Program, Report> {
    load_for(input, &Target::host())
}

/// Like [`load`], for a target: C headers (`extern "C" include`) are read as
/// that target sees them.
pub fn load_for(input: &Input, target: &Target) -> Result<Program, Report> {
    load_with(input, target, false)
}

/// `tests`: also src/test.mim (for `mimic test`).
pub fn load_with(input: &Input, target: &Target, tests: bool) -> Result<Program, Report> {
    let mut clock = Phases::new();
    let mut report = Report::default();
    let mut files = Vec::new();
    let pkg = input.name();
    match input {
        Input::File(f) => {
            if let Some(sf) = read_parse(f, f.display().to_string(), &pkg, &mut report, tests) {
                if sf.ast.module.name != "main" {
                    let d = mimic_syntax::Diag::new(
                        &C::ENTRY_MODULE,
                        format!("a file used on its own must declare `module main` (found `module {}`)", sf.ast.module.name),
                        sf.ast.module.span,
                    )
                    .help("a module of a project is checked and built with its project: run `mimic check` or `mimic build` in the project's folder (the one with mim.mim)");
                    report.push_in(Some(&sf.path), &sf.display, &sf.src, d);
                }
                files.push(sf);
            }
        }
        Input::Project(p) => {
            for path in p.sources()? {
                // test.mim is only compiled by `mimic test`
                if !tests && path.file_name().is_some_and(|n| n == "test.mim") && path.parent() == Some(&p.root.join("src")) {
                    continue;
                }
                if let Some(sf) = read_parse(&path, display_path(Some(&p.root), &path), &pkg, &mut report, tests) {
                    files.push(sf);
                }
            }
            if !p.manifest.bin && !files.iter().any(|f| f.ast.module.name == "lib") {
                report.push_plain(&C::ENTRY_MODULE, "library projects need src/lib.mim with `module lib`");
            }
            // dependencies (transitively), compiled from their library sources
            let mut seen = BTreeSet::from([pkg.clone()]);
            let mut queue: Vec<(PathBuf, mimic_project::Dependency)> =
                p.manifest.dependencies.iter().map(|d| (p.root.clone(), d.clone())).collect();
            let dep_err = |s: String| Report::from_text(&C::DEPENDENCY, &s);
            while let Some((from_root, dep)) = queue.pop() {
                if !seen.insert(dep.name.clone()) {
                    continue;
                }
                let holder = Project::load(&from_root)?;
                let dir = holder.dep_dir(&dep).map_err(dep_err)?;
                let dp = Project::load(&dir)?;
                if dp.manifest.name != dep.name {
                    return Err(Report::plain(
                        &C::DEPENDENCY,
                        format!("dependency `{}` resolves to {}, whose mim.mim names it `{}`", dep.name, dir.display(), dp.manifest.name),
                    ));
                }
                if !dp.manifest.lib {
                    return Err(Report::plain(&C::DEPENDENCY, format!("dependency `{}` is not a library", dep.name)));
                }
                for path in dp.sources()? {
                    let fname = path.file_name().unwrap().to_string_lossy().to_string();
                    if path.parent() == Some(&dir.join("src")) && (fname == "main.mim" || fname == "test.mim") {
                        continue;
                    }
                    let shown = format!("{}/{}", dep.name, display_path(Some(&dir), &path));
                    if let Some(sf) = read_parse(&path, shown, &dep.name, &mut report, false) {
                        files.push(sf);
                    }
                }
                for d in &dp.manifest.dependencies {
                    queue.push((dir.clone(), d.clone()));
                }
            }
        }
    }
    clock.mark("read and parse");
    if !report.entries.is_empty() {
        return Err(report);
    }
    // C headers named by `extern "C" include "…"`
    let mut wrappers: Vec<String> = Vec::new();
    for sf in &mut files {
        let dirs = header_dirs(input, Some(&sf.path));
        let mut errs = Vec::new();
        import_headers(&mut sf.ast.module.items, &dirs, target, &mut errs, &mut wrappers);
        for (msg, span) in errs {
            report.push_in(Some(&sf.path), &sf.display, &sf.src, mimic_syntax::Diag::new(&C::CIMPORT, msg, span));
        }
    }
    if !report.entries.is_empty() {
        return Err(report);
    }
    clock.mark("C headers");
    files.push(prelude_file());
    clock.mark("prelude");
    let (mut program, diags) = Program::build(files, &pkg);
    clock.mark("resolve");
    if !wrappers.is_empty() {
        // one C file with every struct wrapper, named by its content
        let text = wrappers.join("\n");
        let dir = packages::cache_dir().join("cimport");
        let path = dir.join(format!("wrap-{:016x}.c", mimic_syntax::fnv1a(text.as_bytes())));
        if !path.is_file() {
            let _ = fs::create_dir_all(&dir);
            mimic_toolchain::jobs::write_atomic(&path, text.as_bytes()).map_err(|e| Report::plain(&C::CIMPORT, e))?;
        }
        program.c_sources.push(path);
    }
    if !diags.is_empty() {
        return Err(Report::from_program(&program, diags));
    }
    // named and default arguments become plain positional calls
    let ndiags = mimic_check::normalize(&mut program);
    clock.mark("normalize");
    if !ndiags.is_empty() {
        return Err(Report::from_program(&program, ndiags));
    }
    let diags = mimic_check::check(&program, &Options { require_main: input.is_bin() && !tests, check_std: std::env::var_os("MIMIC_CHECK_STD").is_some() });
    clock.mark("check");
    if !diags.is_empty() {
        return Err(Report::from_program(&program, diags));
    }
    // static type errors (inference over every reachable specialisation)
    let tdiags = mimic_check::types::check_types(&program, !input.is_bin());
    clock.mark("types");
    if !tdiags.is_empty() {
        return Err(Report::from_program(&program, tdiags));
    }
    Ok(program)
}

/// Where each piece of the program runs, as a release build for this
/// machine decides it (`mimic why`, the labels in editors).
pub fn plan(input: &Input, program: &Program) -> Result<Vec<mimic_codegen::Decision>, String> {
    let profile = Profile::for_input(input, true);
    let is_exe = input.is_bin();
    mimic_codegen::plan(
        program,
        &mimic_codegen::Settings {
            triple: None,
            native: true,
            opt_level: profile.opt,
            leak_check: false,
            memcheck: !profile.release,
            emit_main: is_exe,
            export_pub: !is_exe,
            use_net: profile.net,
            cpu: None,
            features: None,
            abi: None,
            strict_math: profile.strict_math,
            fast_math: profile.fast_math,
            debug_info: false,
            frame_pointers: false,
            coverage: false,
            tests: false,
            split: None,
        },
    )
}

/// `MIMIC_DEBUG_PHASES=1`: how long each step of loading a program takes.
struct Phases {
    on: bool,
    last: std::time::Instant,
}

impl Phases {
    fn new() -> Self {
        Phases { on: std::env::var_os("MIMIC_DEBUG_PHASES").is_some(), last: std::time::Instant::now() }
    }

    fn mark(&mut self, what: &str) {
        if self.on {
            let now = std::time::Instant::now();
            eprintln!("mimic: {what:<16} {:>8.2} ms", (now - self.last).as_secs_f64() * 1e3);
            self.last = now;
        }
    }
}

// ---- toolchain --------------------------------------------------------------

pub use crate::toolchain::run_tool;
use crate::toolchain::{self, LinkJob, Toolchain};

/// Whether `MIMIC_COUNT_RC=1` asks for a build that counts reference-count
/// operations (the runtime counts them; the code calls it for every one).
pub fn counting_rc() -> bool {
    std::env::var("MIMIC_COUNT_RC").is_ok_and(|v| v == "1")
}

/// Compile (and cache) the C runtime for a target.
pub fn runtime_object(tc: &Toolchain, target: &Target, release: bool, cpu: Option<&str>, frame_pointers: bool) -> Result<PathBuf, String> {
    let portable = cpu == Some("portable");
    let resolved = if portable { portable_cpu(target) } else { cpu.map(String::from) };
    let cpu = resolved.as_deref().or(if portable { Some("") } else { None });
    // the compiler flags (target, sysroot, minimum OS, CPU) are part of the key
    let mut flags: Vec<String> = vec!["-c".into(), if release { "-O3" } else { "-O1" }.into(), "-fPIC".into()];
    if frame_pointers {
        flags.push("-fno-omit-frame-pointer".into());
    }
    // every function of the runtime at a 64-byte boundary: a change to one
    // then moves the functions after it by whole cache lines, never within
    // one, so a benchmark's speed does not change with where its runtime
    // code happens to fall (unchanged programs measured 5-20% slower on one
    // machine after runtime changes they never ran: fib, vm, qoi)
    if !target.name().starts_with("wasm") {
        flags.push("-falign-functions=64".into());
    }
    if !target.is_apple() {
        // a section per function, so the linker drops what a program does not
        // use (Mach-O linkers do this by symbol without being asked)
        flags.extend(["-ffunction-sections".into(), "-fdata-sections".into()]);
    }
    match cpu {
        Some("native") | None if release && target.name.is_none() => {
            // native builds use every feature of this CPU (x86 names it with -march)
            flags.push(if cfg!(target_arch = "x86_64") { "-march=native" } else { "-mcpu=native" }.into());
        }
        Some("") => {} // portable without a family baseline: the target default
        Some(cpu) if cpu != "native" => {
            // x86 names a CPU with -march; every other architecture with -mcpu
            let x86 = matches!(target.arch.as_str(), "x86_64" | "x86" | "i386" | "i686");
            flags.push(if x86 { format!("-march={cpu}") } else { format!("-mcpu={cpu}") });
        }
        _ => {}
    }
    // debug builds: the memory check (a freed value marked, and a use of it
    // stops the program; mimic_own.c); release builds compile none of it
    if !release {
        flags.push("-DMIM_MEMCHECK=1".into());
    }
    // MIMIC_COUNT_RC=1: a runtime that counts its reference-count operations
    // (docs/design/ownership.md)
    if counting_rc() {
        flags.push("-DMIM_COUNT_RC=1".into());
    }
    // static Linux: TLS from the bundled Mbed TLS (OpenSSL cannot be loaded)
    if let Some(tls) = toolchain::static_tls(tc) {
        flags.push("-DMIM_MBEDTLS=1".into());
        flags.extend(tls.cflags);
    }
    let base = toolchain::cc(tc, target);
    let base_args: Vec<String> = base.get_args().map(|a| a.to_string_lossy().into_owned()).collect();
    let key = mimic_syntax::fnv1a(format!("{RUNTIME_C}{RUNTIME_H}{RUNTIME_NET_C}{RUNTIME_STD_C}{RUNTIME_SCHED_C}{RUNTIME_IO_C}{RUNTIME_UI_C}{RUNTIME_SOCKET_C}{RUNTIME_JSON_C}{RUNTIME_WINDOW_C}{RUNTIME_GPU_C}{RUNTIME_OWN_C}{RUNTIME_WEPOLL_C}{}{base_args:?}{flags:?}", target.name()).as_bytes());
    let dir = packages::cache_dir().join("runtime");
    fs::create_dir_all(&dir).map_err(|e| format!("cannot create {}: {e}", dir.display()))?;
    let obj = dir.join(format!("mimic_rt-{}-{key:016x}.o", target.name()));
    if obj.is_file() {
        return Ok(obj);
    }
    // each runtime version gets its own directory: mimic_rt.c #includes mimic_net.c
    let src_dir = runtime_sources(&dir, key)?;
    let src = src_dir.join("mimic_rt.c");
    let tmp = mimic_toolchain::jobs::temp_beside(&obj);
    let mut c = toolchain::cc(tc, target);
    c.args(&flags).arg(&src).arg("-o").arg(&tmp);
    run_tool(c, "compiling the mimic runtime")?;
    fs::rename(&tmp, &obj).map_err(|e| e.to_string())?;
    Ok(obj)
}

/// The runtime's sources for one version, written once. A finished
/// directory is never written again: replacing a file other builds are
/// compiling from frees its inode, the next file written can get the same
/// number, and the C compiler, which knows files by inode, then takes one
/// file for another (`redefinition of 'MIM_UI_JS'`: mimic_json.c read as
/// mimic_ui.c). So the files go into a private directory, which is moved
/// into place whole; a build that finds the shared one complete uses it,
/// and one that cannot move its own there (another build was first, or the
/// shared one lost files to a cleaner) compiles from its own.
fn runtime_sources(dir: &Path, key: u64) -> Result<PathBuf, String> {
    const MARKER: &str = ".complete";
    let shared = dir.join(format!("sources-{key:016x}"));
    if mimic_toolchain::jobs::intact(&shared, MARKER) {
        return Ok(shared);
    }
    let own = mimic_toolchain::jobs::temp_beside(&shared);
    fs::create_dir_all(&own).map_err(|e| format!("cannot create {}: {e}", own.display()))?;
    for (name, text) in [
        ("mimic_rt.c", RUNTIME_C),
        ("mimic_net.c", RUNTIME_NET_C),
        ("mimic_std.c", RUNTIME_STD_C),
        ("mimic_sched.c", RUNTIME_SCHED_C),
        ("mimic_io.c", RUNTIME_IO_C),
        ("mimic_ui.c", RUNTIME_UI_C),
        ("mimic_socket.c", RUNTIME_SOCKET_C),
        ("mimic_json.c", RUNTIME_JSON_C),
        ("mimic_window.c", RUNTIME_WINDOW_C),
        ("mimic_gpu.c", RUNTIME_GPU_C),
        ("mimic_own.c", RUNTIME_OWN_C),
        ("wepoll.c", RUNTIME_WEPOLL_C),
        ("mimic_rt.h", RUNTIME_H),
    ] {
        let path = own.join(name);
        fs::write(&path, text).map_err(|e| format!("cannot write {}: {e}", path.display()))?;
    }
    mimic_toolchain::jobs::seal(&own, MARKER)?;
    if fs::rename(&own, &shared).is_ok() {
        return Ok(shared);
    }
    if mimic_toolchain::jobs::intact(&shared, MARKER) {
        let _ = fs::remove_dir_all(&own);
        return Ok(shared);
    }
    Ok(own)
}

pub struct Artifact {
    pub path: PathBuf,
    pub kind: ArtifactKind,
    /// Why only an object was produced, when that happens.
    pub note: Option<String>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ArtifactKind {
    Executable,
    StaticLibrary,
    /// A `.dll`, `.so` or `.dylib` exporting the `extern "C" fn`s (`--shared`).
    SharedLibrary,
    /// LLVM can generate code for the target, but `mimic` has no C library
    /// for it yet: the program's object file, without the runtime.
    Object,
    /// Like `Object`, for backends that only emit assembly (NVPTX: `.ptx`).
    Assembly,
}

impl ArtifactKind {
    pub fn describe(self) -> &'static str {
        match self {
            ArtifactKind::Executable => "executable",
            ArtifactKind::StaticLibrary => "static library",
            ArtifactKind::SharedLibrary => "shared library",
            ArtifactKind::Object => "object file",
            ArtifactKind::Assembly => "assembly",
        }
    }
    pub fn json(self) -> &'static str {
        match self {
            ArtifactKind::Executable => "executable",
            ArtifactKind::StaticLibrary => "static-library",
            ArtifactKind::SharedLibrary => "shared-library",
            ArtifactKind::Object => "object",
            ArtifactKind::Assembly => "assembly",
        }
    }
}

/// The real path of a file, as tools compare and show it: links resolved,
/// and on Windows without the `\\?\` prefix that `canonicalize` adds
/// (editors and `file:` URIs never use it).
pub fn canonical(p: &Path) -> PathBuf {
    let c = p.canonicalize().unwrap_or_else(|_| p.to_path_buf());
    if cfg!(windows)
        && let Some(s) = c.to_str()
        && let Some(rest) = s.strip_prefix(r"\\?\")
        && !rest.starts_with("UNC\\")
    {
        return PathBuf::from(rest);
    }
    c
}

pub fn out_dir(input: &Input, profile: &Profile, target: &Target) -> PathBuf {
    let base = match input {
        Input::Project(p) => p.target_dir(),
        Input::File(f) => {
            let abs = fs::canonicalize(f).unwrap_or_else(|_| f.clone());
            let h = mimic_syntax::fnv1a(abs.to_string_lossy().as_bytes());
            packages::cache_dir().join("run").join(format!("{}-{h:08x}", input.name()))
        }
    };
    let mut d = base.join(profile.name());
    if target.name.is_some() {
        d = d.join(target.name());
    }
    d
}

/// What generated callback trampolines need from Mimic's runtime.
const CALLBACK_PRELUDE: &str = "typedef struct mim_val mim_val;\n\
mim_val *mim_call(mim_val *f, int f_argc, mim_val **args);\n\
mim_val *mim_int(long long x);\nmim_val *mim_float(double x);\nmim_val *mim_bool(long long x);\n\
mim_val *mim_from_cstr(const char *s);\nlong long mim_as_i64(mim_val *v);\ndouble mim_as_f64(mim_val *v);\n\
void mim_release(mim_val *v);\n";

/// Where C headers are looked for: next to `file`, the project root, `ffi.include`.
fn header_dirs(input: &Input, file: Option<&Path>) -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    if let Some(f) = file {
        dirs.push(f.parent().filter(|d| !d.as_os_str().is_empty()).map(Path::to_path_buf).unwrap_or_else(|| PathBuf::from(".")));
    }
    if let Input::Project(p) = input {
        dirs.push(p.root.clone());
        dirs.extend(p.manifest.ffi_include.iter().map(|d| p.root.join(d)));
    }
    dirs
}

/// Fill `extern "C" include` blocks (in every module, nested ones too) with the
/// functions the header makes visible, and put everything that comes from C
/// in its own namespace: each module's `extern "C"` blocks (imported and
/// hand-written functions alike), the records of C structs and their
/// wrappers move into a nested module `c` (or the block's `as` name), so C
/// names are always written `c::printf`, `c::Vec2`, and never collide with
/// Mimic's own. Functions taking or returning structs by value get generated
/// Mimic wrappers and C wrappers (collected in `wrappers`); they are only
/// generated for `trusted` imports, since the wrapper hides the C call.
fn import_headers(
    items: &mut Vec<mimic_syntax::ast::Item>,
    dirs: &[PathBuf],
    target: &Target,
    errs: &mut Vec<(String, mimic_syntax::Span)>,
    wrappers: &mut Vec<String>,
) {
    use mimic_syntax::ast::{Item, ModuleDecl};
    use std::collections::{BTreeMap, HashSet};
    let ns_of = |b: &mimic_syntax::ast::ExternBlock| b.alias.clone().unwrap_or_else(|| "c".to_string());
    // per namespace: the names it has (hand-written declarations win over
    // imported ones; each name is imported once) and its generated Mimic code
    let mut taken: BTreeMap<String, HashSet<String>> = BTreeMap::new();
    let mut generated: BTreeMap<String, String> = BTreeMap::new();
    // per namespace: C constants (`c::SEEK_SET` becomes its value) and
    // getters of globals and expression macros (`c::errno` becomes a call)
    let mut constants: BTreeMap<String, std::collections::HashMap<String, crate::cimport::CConst>> = BTreeMap::new();
    let mut getters: BTreeMap<String, HashSet<String>> = BTreeMap::new();
    // their C, emitted only for the ones the program uses (a getter may name
    // a function of a library the program does not link)
    let mut getter_c: std::collections::HashMap<String, (String, String)> = std::collections::HashMap::new();
    // per namespace: C record → its fields' zero values (`c::T { … }` fills the rest)
    let mut zeros: BTreeMap<String, std::collections::HashMap<String, Vec<(String, String)>>> = BTreeMap::new();
    for item in items.iter() {
        if let Item::Extern(b) = item
            && b.abi == "C"
        {
            taken.entry(ns_of(b)).or_default().extend(b.fns.iter().map(|f| f.name.clone()));
        }
    }
    // the C names this scope's code mentions (`c::name`): only their C
    // wrappers are generated, since a wrapper refers to its C function, and a
    // header can declare functions of libraries the program does not link
    let mut referenced: HashSet<(String, String)> = HashSet::new();
    // names used without a namespace (a C constant written `SEEK_END` gets a
    // declaration below, only so the checker can say to write `c::SEEK_END`)
    let mut bare: HashSet<String> = HashSet::new();
    for item in items.iter() {
        if let Item::Fn(f) = item {
            f.body.walk(&mut |e| match &e.kind {
                mimic_syntax::ast::ExprKind::Path(p) | mimic_syntax::ast::ExprKind::Record { path: p, .. } if p.len() == 2 => {
                    referenced.insert((p[0].clone(), p[1].clone()));
                }
                mimic_syntax::ast::ExprKind::Path(p) if p.len() == 1 => {
                    bare.insert(p[0].clone());
                }
                _ => {}
            });
        }
    }
    for item in items.iter_mut() {
        match item {
            Item::Module(m) => import_headers(&mut m.items, dirs, target, errs, wrappers),
            Item::Extern(b) if !b.includes.is_empty() => {
                let ns = ns_of(b);
                let taken = taken.entry(ns.clone()).or_default();
                for (header, span) in b.includes.clone() {
                    match crate::cimport::import(&header, dirs, target) {
                        Ok(imp) => {
                            // trusted: a function that takes plain structs by
                            // pointer is the record-taking wrapper, not the
                            // opaque-pointer extern
                            let wrapped: HashSet<&str> = if b.trusted { imp.struct_fns.iter().map(|f| f.name.as_str()).collect() } else { HashSet::new() };
                            for f in imp.extern_fns(true, b.effect, span) {
                                if wrapped.contains(f.name.as_str()) && referenced.contains(&(ns.clone(), f.name.clone())) {
                                    continue;
                                }
                                if taken.insert(f.name.clone()) {
                                    b.fns.push(f);
                                }
                            }
                            zeros.entry(ns.clone()).or_default().extend(imp.zeros.iter().map(|(k, v)| (k.clone(), v.clone())));
                            // the C records the code names (`c::Termios { … }`), with the
                            // records nested in them
                            {
                                let by_name: std::collections::HashMap<String, &String> =
                                    imp.records.values().map(|d| (d.split_whitespace().nth(2).unwrap_or("").to_string(), d)).collect();
                                let mut want: Vec<String> =
                                    referenced.iter().filter(|(n, t)| n == &ns && by_name.contains_key(t)).map(|(_, t)| t.clone()).collect();
                                while let Some(t) = want.pop() {
                                    if !taken.insert(t.clone()) {
                                        continue;
                                    }
                                    let decl = by_name[&t];
                                    generated.entry(ns.clone()).or_default().push_str(&format!("{}\n", decl.replacen("PUB ", "pub ", 1)));
                                    for word in decl.split(|c: char| !(c.is_alphanumeric() || c == '_')) {
                                        if word != t && by_name.contains_key(word) {
                                            want.push(word.to_string());
                                        }
                                    }
                                }
                            }
                            let consts = constants.entry(ns.clone()).or_default();
                            for (name, value) in &imp.constants {
                                if !taken.contains(name) {
                                    consts.entry(name.clone()).or_insert_with(|| value.clone());
                                }
                            }
                            getters.entry(ns.clone()).or_default().extend(imp.globals.iter().cloned());
                            for l in imp.globals_c.lines() {
                                if let Some(sym) = l.split('(').next().and_then(|x| x.rsplit(' ').next()) {
                                    getter_c.entry(sym.to_string()).or_insert_with(|| (imp.include_line.clone(), l.to_string()));
                                }
                            }
                            // callback trampolines (the externs keep their unsafe
                            // rule), for the functions used
                            let used_callbacks: String = imp
                                .callback_c
                                .split("/* mimic:fn ")
                                .filter(|chunk| chunk.split(" */").next().is_some_and(|n| referenced.contains(&(ns.clone(), n.to_string()))))
                                .map(|chunk| chunk.split_once(" */\n").map(|x| x.1).unwrap_or(""))
                                .collect();
                            if !used_callbacks.is_empty() {
                                wrappers.push(format!("{}\n{CALLBACK_PRELUDE}{used_callbacks}", imp.include_line));
                            }
                            if !b.trusted {
                                continue; // struct wrappers hide the unsafe call: trusted imports only
                            }
                            let generated = generated.entry(ns.clone()).or_default();
                            let mut c = String::new();
                            for f in &imp.struct_fns {
                                if !referenced.contains(&(ns.clone(), f.name.clone())) {
                                    continue; // not used here
                                }
                                // a wrapper that does not parse is left out, not the header
                                let alone: String = f.uses.iter().map(|r| format!("{}\n", imp.records[r])).collect::<String>() + &f.mimic;
                                if mimic_syntax::parser::parse_file(&format!("module generated\n{}", alone.replace("PUB ", ""))).is_err() {
                                    continue;
                                }
                                if !taken.insert(f.name.clone()) {
                                    continue;
                                }
                                for r in &f.uses {
                                    let decl = &imp.records[r];
                                    let tname = decl.split_whitespace().nth(2).unwrap_or("").to_string();
                                    if taken.insert(tname) {
                                        generated.push_str(&decl.replacen("PUB ", "pub ", 1));
                                        generated.push('\n');
                                    }
                                }
                                generated.push_str(&f.mimic.replace("PUB ", "pub "));
                                c.push_str(&f.c);
                            }
                            if !c.is_empty() {
                                wrappers.push(format!("{}\n{c}", imp.include_line));
                            }
                        }
                        Err(e) => errs.push((e, span)),
                    }
                }
            }
            _ => {}
        }
    }
    // the namespaces: every `extern "C"` block of this scope moves into one
    let mut spaces: BTreeMap<String, ModuleDecl> = BTreeMap::new();
    let mut kept = Vec::with_capacity(items.len());
    for item in items.drain(..) {
        match item {
            Item::Extern(mut b) if b.abi == "C" => {
                let ns = ns_of(&b);
                let m = spaces.entry(ns.clone()).or_insert_with(|| ModuleDecl { name: ns, is_pub: false, items: Vec::new(), span: b.span, full: b.span });
                m.is_pub |= b.is_pub; // `pub extern`: `lib::c::…` from other modules
                for f in &mut b.fns {
                    f.is_pub = true; // visible to the module that imported it
                }
                m.items.push(Item::Extern(b));
            }
            other => kept.push(other),
        }
    }
    *items = kept;
    // `c::NAME` in this module's functions: constants become their value,
    // globals a call of their getter (`c::errno` reads errno each time)
    let used: std::cell::RefCell<HashSet<String>> = Default::default();
    let rewrite = |e: &mut mimic_syntax::ast::Expr| {
        use mimic_syntax::ast::{Expr, ExprKind};
        use crate::cimport::CConst;
        match &mut e.kind {
            ExprKind::Path(p) if p.len() == 2 => {
                if let Some(c) = constants.get(&p[0]).and_then(|m| m.get(&p[1])) {
                    e.kind = match c {
                        CConst::Int(v) => ExprKind::Int(*v),
                        CConst::Float(v) => ExprKind::Float(*v),
                        CConst::Str(v) => ExprKind::Str(v.clone()),
                    };
                } else if getters.get(&p[0]).is_some_and(|g| g.contains(&p[1])) {
                    used.borrow_mut().insert(format!("mimicw_g_{}", p[1]));
                    let callee = Expr { kind: ExprKind::Path(p.clone()), span: e.span };
                    e.kind = ExprKind::Call { callee: Box::new(callee), args: Vec::new() };
                }
            }
            // `c::Termios { … }`: the fields left out are zero, as `= {0}` in C
            ExprKind::Record { path, fields } if path.len() == 2 => {
                if let Some(zs) = zeros.get(&path[0]).and_then(|m| m.get(&path[1])) {
                    for (f, z) in zs {
                        if !fields.iter().any(|(g, _, _)| g == f)
                            && let Ok(mut v) = mimic_syntax::parser::parse_expr(&z.replace("@NS@", &path[0]))
                        {
                            v.walk_mut(&mut |x| x.span = e.span);
                            fields.push((f.clone(), v, e.span));
                        }
                    }
                }
            }
            // `c::errno()` written out: undo the call added to its callee
            ExprKind::Call { callee, args } if args.is_empty() => {
                if let ExprKind::Call { callee: inner, args: a } = &callee.kind
                    && a.is_empty()
                    && matches!(&inner.kind, ExprKind::Path(p) if p.len() == 2 && getters.get(&p[0]).is_some_and(|g| g.contains(&p[1])))
                {
                    let inner = (**inner).clone();
                    **callee = inner;
                }
            }
            _ => {}
        }
    };
    if !constants.is_empty() || !getters.is_empty() || !zeros.is_empty() {
        for item in items.iter_mut() {
            if let Item::Fn(f) = item {
                f.body.walk_mut(&mut |e| rewrite(e));
                for p in &mut f.params {
                    if let Some(d) = &mut p.default {
                        d.walk_mut(&mut |e| rewrite(e));
                    }
                }
            }
        }
    }
    // the getters used, once each in the whole program (several headers
    // define NULL, EOF, …)
    for sym in used.into_inner() {
        if let Some((include, line)) = getter_c.get(&sym)
            && !wrappers.iter().any(|w| w.contains(&format!(" {sym}(void)")))
        {
            wrappers.push(format!("{include}\n{line}\n"));
        }
    }
    // C constants written without their namespace: declared (never used)
    // so the error for them carries the note and the fix `c::NAME`
    for (ns, consts) in &constants {
        for name in &bare {
            if let Some(c) = consts.get(name)
                && !taken.get(ns).is_some_and(|t| t.contains(name))
            {
                let ty = match c {
                    crate::cimport::CConst::Int(_) => "i64",
                    crate::cimport::CConst::Float(_) => "f64",
                    crate::cimport::CConst::Str(_) => "str",
                };
                let lit = match c {
                    crate::cimport::CConst::Int(_) => "0",
                    crate::cimport::CConst::Float(_) => "0.0",
                    crate::cimport::CConst::Str(_) => "\"\"",
                };
                generated.entry(ns.clone()).or_default().push_str(&format!("pub fn {name}() -> {ty} {{ {lit} }}\n"));
            }
        }
    }
    for (ns, code) in generated {
        if code.is_empty() {
            continue;
        }
        if std::env::var_os("MIMIC_DEBUG_CIMPORT").is_some() {
            eprintln!("== generated for {ns}::\n{code}");
        }
        match mimic_syntax::parser::parse_file(&format!("module generated\n{code}")) {
            Ok(f) => {
                if let Some(m) = spaces.get_mut(&ns) {
                    m.items.extend(f.module.items);
                }
            }
            Err(d) => errs.push((format!("internal error in generated C bindings: {}\n{code}", d.msg), d.span)),
        }
    }
    for (ns, m) in spaces {
        if let Some(Item::Module(own)) = items.iter().find(|i| matches!(i, Item::Module(o) if o.name == ns)) {
            errs.push((format!("`{ns}` is the namespace of the C functions imported here (`{ns}::…`): rename this module, or import with `as` another name"), own.span));
            continue;
        }
        items.push(Item::Module(m));
    }
}

pub fn build(input: &Input, profile: &Profile, target: &Target, emit_ir: bool, emit_net: bool) -> Result<Artifact, Report> {
    // contracts are checked in debug builds and tests, and cost nothing in release builds
    sources::set_contracts(!profile.release || profile.tests);
    let program = load_with(input, target, profile.tests);
    sources::set_contracts(true);
    let program = program?;
    if let Input::Project(p) = input
        && !profile.tests
    {
        crate::modfile::sync(p, &program)?;
    }
    codegen_and_link(input, &program, profile, target, emit_ir, emit_net).map_err(|e| Report::from_text(&C::BUILD, &e))
}

fn codegen_and_link(
    input: &Input,
    program: &Program,
    profile: &Profile,
    target: &Target,
    emit_ir: bool,
    emit_net: bool,
) -> Result<Artifact, String> {
    if target.is_device_only() {
        return Err(format!(
            "`{}` is a GPU target: a whole program cannot run there; compiling Mimic's array kernels for GPUs is the GPU backend on the roadmap (F8)",
            target.name()
        ));
    }
    if let Some(why) = target.backend_gap() {
        return Err(format!("`{}`: {why}", target.name()));
    }
    let dir = out_dir(input, profile, target);
    fs::create_dir_all(&dir).map_err(|e| format!("cannot create {}: {e}", dir.display()))?;
    // a test build is a program of its own (also for a library)
    let name = if profile.tests { format!("{}-tests", input.name()) } else { input.name() };
    let obj = dir.join(format!("{name}.o"));
    let ir = dir.join(format!("{name}.ll"));
    let net = dir.join(format!("{name}.mnet"));
    let is_exe = input.is_bin() || profile.tests;
    // cross targets: the same CPU/features/ABI as Clang uses for the C side.
    // Not the host: on Windows it has a triple too (the MinGW environment),
    // and Clang's default for that, plain x86-64, would hide this machine's
    // CPU (no AVX2, no FMA: Windows programs ran at 2003 speed)
    let cross = target.name.is_some();
    let (mut cpu, mut features, abi) = match &target.llvm {
        Some(t) => match toolchain::clang_defaults(t) {
            Some((c, f, a)) if cross => (Some(c), Some(f), a),
            Some((_, _, a)) => (None, None, a), /* the host: its own CPU (Settings::native) */
            None => (None, None, None),
        },
        None => (None, None, None),
    };
    // --cpu: a named model replaces the defaults (its features follow from it)
    let portable = profile.cpu.as_deref() == Some("portable");
    let cpu_choice = if portable { portable_cpu(target) } else { profile.cpu.clone() };
    match cpu_choice.as_deref() {
        None if portable => {
            // no family baseline known: the target's default CPU is already portable
            if target.name.is_none() {
                cpu = Some("generic".into());
                features = Some(String::new());
            }
        }
        None => {}
        Some("native") if target.name.is_none() => {}
        Some("native") => return Err(format!("--cpu native only applies to native builds, not `{}`", target.name())),
        Some(c) => {
            check_cpu(target, c)?;
            cpu = Some(c.to_string());
            features = None;
        }
    }
    // eBPF: v4 (Linux 6.6+) is the first generation with signed division,
    // which floored `div` and `%` need
    if target.arch.starts_with("bpf") && cpu.as_deref().is_none_or(|c| c == "generic") {
        cpu = Some("v4".into());
    }
    // unchanged since the last build (sources, the C declarations they
    // import, C sources, flags, target, this compiler): the program is there
    let out = if is_exe {
        dir.join(format!("{name}{}", target.exe_suffix()))
    } else if profile.shared {
        dir.join(toolchain::shared_name(target, &name))
    } else {
        dir.join(format!("lib{name}.a"))
    };
    let stamp = dir.join(format!("{name}.stamp"));
    let key = build_key(input, program, profile, target, &cpu, &features, &abi);
    let header_ok = !profile.emit_header || dir.join(format!("{name}.h")).exists();
    if !emit_ir && !emit_net && header_ok && out.exists() && obj.exists() && fs::read_to_string(&stamp).is_ok_and(|k| k == key) {
        let kind = if is_exe { ArtifactKind::Executable } else if profile.shared { ArtifactKind::SharedLibrary } else { ArtifactKind::StaticLibrary };
        return Ok(Artifact { path: out, kind, note: None });
    }
    let _ = fs::remove_file(&stamp);
    let mut clock = Phases::new();
    let objs = mimic_codegen::compile(
        program,
        &mimic_codegen::Settings {
            triple: target.llvm.clone(),
            native: target.name.is_none(),
            opt_level: profile.opt,
            leak_check: profile.leak_check,
            memcheck: !profile.release,
            emit_main: is_exe,
            export_pub: !is_exe,
            use_net: profile.net,
            cpu: cpu.clone(),
            features: features.clone(),
            abi: abi.clone(),
            strict_math: profile.strict_math,
            fast_math: profile.fast_math,
            debug_info: !profile.release,
            frame_pointers: profile.frame_pointers,
            coverage: profile.coverage,
            tests: profile.tests,
            // machine code in parts at once (native builds: linked here)
            split: if target.name.is_none() { Some(mimic_toolchain::split_codegen) } else { None },
        },
        &obj,
        emit_ir.then_some(ir.as_path()),
        emit_net.then_some(net.as_path()),
    )?;
    clock.mark("code generation");
    let obj = objs[0].clone();
    let parts: Vec<PathBuf> = objs[1..].to_vec();
    if profile.emit_header {
        let h = dir.join(format!("{name}.h"));
        fs::write(&h, c_header(program, &name)).map_err(|e| format!("cannot write {}: {e}", h.display()))?;
    }
    let tc = match toolchain::select(target) {
        Ok(tc) => tc,
        // every target LLVM supports gets at least its object file
        Err(why) if target.name.is_some() => {
            let kind = if obj.extension().is_some_and(|e| e == "o") { ArtifactKind::Object } else { ArtifactKind::Assembly };
            return Ok(Artifact { path: obj, kind, note: Some(why) });
        }
        Err(why) => return Err(why),
    };
    clock.mark("toolchain");
    let rt = runtime_object(&tc, target, profile.release, profile.cpu.as_deref(), profile.frame_pointers)?;
    clock.mark("runtime");
    let c_objects = compile_c_sources(input, &program.c_sources, &header_dirs(input, None), &tc, target, profile, &dir)?;
    clock.mark("C sources");
    if is_exe {
        let exe = dir.join(format!("{name}{}", target.exe_suffix()));
        let mut objects = vec![obj, rt];
        objects.extend(parts);
        objects.extend(c_objects);
        objects.extend(toolchain::static_tls(&tc).map(|t| t.lib));
        let mut job = LinkJob { objects: &objects, output: &exe, search: Vec::new(), libs: Vec::new(), debug: !profile.release };
        if let Input::Project(p) = input {
            job.search = p.manifest.ffi_search.iter().map(|s| p.root.join(s)).collect();
            job.libs = p.manifest.ffi_link.clone();
        }
        run_tool(toolchain::link(&tc, target, &job), "linking")?;
        toolchain::mac_main_stack(&tc, &exe)?;
        clock.mark("link");
        let _ = mimic_toolchain::jobs::write_atomic(&stamp, key.as_bytes());
        Ok(Artifact { path: exe, kind: ArtifactKind::Executable, note: None })
    } else {
        let mut members = vec![obj, rt];
        members.extend(parts);
        members.extend(c_objects);
        if profile.shared {
            // a shared library exporting the `extern "C" fn`s, and nothing else
            let exports: Vec<String> = program
                .fns
                .iter()
                .filter_map(|f| match &f.kind {
                    mimic_check::FnKind::User(d) if d.export_c && program.mods[f.module].package == program.root_package => Some(d.name.clone()),
                    _ => None,
                })
                .collect();
            if exports.is_empty() {
                return Err("a shared library exports the project's `extern \"C\" fn`s, and it has none".into());
            }
            members.extend(toolchain::static_tls(&tc).map(|t| t.lib));
            let mut job = LinkJob { objects: &members, output: &out, search: Vec::new(), libs: Vec::new(), debug: !profile.release };
            if let Input::Project(p) = input {
                job.search = p.manifest.ffi_search.iter().map(|s| p.root.join(s)).collect();
                job.libs = p.manifest.ffi_link.clone();
            }
            let _ = fs::remove_file(&out);
            run_tool(toolchain::link_shared(&tc, target, &job, &exports, &dir)?, "linking the shared library")?;
            let _ = mimic_toolchain::jobs::write_atomic(&stamp, key.as_bytes());
            return Ok(Artifact { path: out, kind: ArtifactKind::SharedLibrary, note: None });
        }
        // library: static archive of the program object + runtime
        let lib = dir.join(format!("lib{name}.a"));
        let _ = fs::remove_file(&lib);
        toolchain::archive(&tc, target, &lib, &members)?;
        let _ = mimic_toolchain::jobs::write_atomic(&stamp, key.as_bytes());
        Ok(Artifact { path: lib, kind: ArtifactKind::StaticLibrary, note: None })
    }
}

/// What a build's output depends on, as one key: every source file, the C
/// declarations imported from headers, the C sources compiled with it, the
/// profile, the target and CPU, the project's link settings, and this
/// compiler (its runtime is inside it).
fn build_key(input: &Input, p: &Program, profile: &Profile, target: &Target, cpu: &Option<String>, features: &Option<String>, abi: &Option<String>) -> String {
    let mut text = String::new();
    for f in &p.files {
        text.push_str(&f.path.to_string_lossy());
        text.push('\0');
        text.push_str(&f.src);
        text.push('\0');
    }
    for f in &p.fns {
        if let mimic_check::FnKind::Extern { decl, trusted } = &f.kind {
            text.push_str(&format!("{decl:?}{trusted}\0"));
        }
    }
    for c in &p.c_sources {
        text.push_str(&c.to_string_lossy());
        text.push_str(&fs::read_to_string(c).unwrap_or_default());
    }
    if let Input::Project(pr) = input {
        text.push_str(&format!("{:?}{:?}", pr.manifest.ffi_link, pr.manifest.ffi_search));
    }
    // switches that change the code made (the IR, parts turned off for tests)
    for v in ["MIMIC_IR", "MIMIC_IR_ONLY", "MIMIC_DEBUG_DISABLE", "MIMIC_COUNT_RC"] {
        text.push_str(&format!("|{v}={}", std::env::var(v).unwrap_or_default()));
    }
    let exe = std::env::current_exe().ok().and_then(|e| fs::metadata(e).ok());
    let compiler = exe.map(|m| format!("{}:{:?}", m.len(), m.modified().ok())).unwrap_or_default();
    text.push_str(&format!(
        "{}|{}|{}|{}|{}|{}|{}|{}|{}|{}|{}|{}|{:?}|{:?}|{:?}|{:?}|{}|{}",
        profile.release,
        profile.net,
        profile.opt,
        profile.leak_check,
        profile.strict_math,
        profile.fast_math,
        profile.tests,
        profile.shared,
        profile.emit_header,
        profile.frame_pointers,
        profile.coverage,
        target.name(),
        target.llvm,
        cpu,
        features,
        abi,
        compiler,
        env!("CARGO_PKG_VERSION")
    ));
    format!("{:016x}-{}", mimic_syntax::fnv1a(text.as_bytes()), text.len())
}

/// A C header declaring every `extern "C" fn` of the program.
pub fn c_header(p: &Program, name: &str) -> String {
    use mimic_syntax::ast::Type;
    let cty = |t: &Type, ret: bool| -> String {
        match t {
            Type::Named(n, _) => match n.as_str() {
                "i8" | "i16" | "i32" | "i64" => format!("int{}_t", &n[1..]),
                "u8" | "u16" | "u32" | "u64" => format!("uint{}_t", &n[1..]),
                "isize" => "intptr_t".into(),
                "usize" => "size_t".into(),
                "f32" => "float".into(),
                "f64" => "double".into(),
                "bool" => "bool".into(),
                "c_char" => "char".into(),
                "cstr" if ret => "char *".into(),
                "cstr" => "const char *".into(),
                _ => "void *".into(),
            },
            _ => "void *".into(),
        }
    };
    let guard = format!("{}_H", name.to_uppercase().replace(|c: char| !c.is_ascii_alphanumeric(), "_"));
    let mut out = format!(
        "/* {name}.h: generated by mimic. Functions exported with `extern \"C\" fn`.\n * Strings returned to C are malloc'd copies: free() them. */\n\
         #ifndef {guard}\n#define {guard}\n\n#include <stdbool.h>\n#include <stddef.h>\n#include <stdint.h>\n\n\
         #ifdef __cplusplus\nextern \"C\" {{\n#endif\n\n"
    );
    for f in &p.fns {
        let mimic_check::FnKind::User(d) = &f.kind else { continue };
        if !d.export_c || p.mods[f.module].package != p.root_package {
            continue;
        }
        let ret = d.ret.as_ref().map(|t| cty(t, true)).unwrap_or_else(|| "void".into());
        let params: Vec<String> = d.params.iter().map(|prm| format!("{} {}", prm.ty.as_ref().map(|t| cty(t, false)).unwrap_or_default(), prm.name)).collect();
        out.push_str(&format!("{ret} {}({});\n", d.name, if params.is_empty() { "void".into() } else { params.join(", ") }));
    }
    out.push_str("\n#ifdef __cplusplus\n}\n#endif\n\n#endif\n");
    out
}

/// Compile the project's C sources (`ffi { sources = [...] }`) for the target,
/// in parallel; an object is reused while it is newer than its source and was
/// built with the same flags.
fn compile_c_sources(
    input: &Input,
    extra: &[PathBuf],
    include_dirs: &[PathBuf],
    tc: &Toolchain,
    target: &Target,
    profile: &Profile,
    dir: &Path,
) -> Result<Vec<PathBuf>, String> {
    let (mut sources, mut cflags): (Vec<(String, PathBuf)>, Vec<String>) = (Vec::new(), Vec::new());
    if let Input::Project(p) = input {
        sources.extend(p.manifest.ffi_sources.iter().map(|s| (s.clone(), p.root.join(s))));
        cflags.extend(p.manifest.ffi_cflags.iter().cloned());
    }
    sources.extend(extra.iter().map(|e| (e.file_name().unwrap_or_default().to_string_lossy().into_owned(), e.clone())));
    if sources.is_empty() {
        return Ok(Vec::new());
    }
    let out_dir = dir.join("c");
    fs::create_dir_all(&out_dir).map_err(|e| format!("cannot create {}: {e}", out_dir.display()))?;
    let mut flags: Vec<String> = vec![if profile.release { "-O2".into() } else { "-O0".into() }, "-g".into()];
    flags.extend(include_dirs.iter().filter(|d| !d.as_os_str().is_empty()).map(|d| format!("-I{}", d.display())));
    flags.extend(cflags);
    let stamp = format!("{:016x}", mimic_syntax::fnv1a(format!("{flags:?}{:?}", target.llvm).as_bytes()));
    let jobs: Vec<(PathBuf, PathBuf)> = sources
        .into_iter()
        .map(|(s, src)| {
            let obj = out_dir.join(format!("{}-{stamp}.o", s.replace(['/', '\\', '.'], "_")));
            (src, obj)
        })
        .collect();
    let errors = std::sync::Mutex::new(Vec::new());
    std::thread::scope(|scope| {
        for (src, obj) in &jobs {
            let (flags, errors) = (&flags, &errors);
            scope.spawn(move || {
                let fresh = match (fs::metadata(src).and_then(|m| m.modified()), fs::metadata(obj).and_then(|m| m.modified())) {
                    (Ok(s), Ok(o)) => o >= s,
                    (Err(e), _) => {
                        errors.lock().unwrap().push(format!("cannot read C source {}: {e}", src.display()));
                        return;
                    }
                    _ => false,
                };
                if fresh {
                    return;
                }
                let mut c = toolchain::cc(tc, target);
                c.args(flags).arg("-c").arg(src).arg("-o").arg(obj);
                if let Err(e) = run_tool(c, &format!("compiling {}", src.display())) {
                    errors.lock().unwrap().push(e);
                }
            });
        }
    });
    let errors = errors.into_inner().unwrap();
    if !errors.is_empty() {
        return Err(errors.join("\n"));
    }
    Ok(jobs.into_iter().map(|(_, o)| o).collect())
}

/// `--cpu portable`: the oldest model of the target's CPU family, so one binary
/// runs on every machine of that family.
fn portable_cpu(target: &Target) -> Option<String> {
    let arch = if target.name.is_none() { std::env::consts::ARCH } else { target.arch.as_str() };
    let apple = target.name.is_none() && cfg!(target_os = "macos") || target.is_apple();
    match arch {
        "aarch64" | "arm64" if apple => Some("apple-m1".into()),
        "x86_64" => Some("x86-64-v2".into()),
        _ => None,
    }
}

/// `--cpu` must name a model LLVM knows for the target.
fn check_cpu(target: &Target, cpu: &str) -> Result<(), String> {
    let triple = target.llvm.clone().unwrap_or_else(|| format!("{}-{}", std::env::consts::ARCH, std::env::consts::OS));
    let triple = mimic_toolchain::llvm_triple(&triple).triple;
    // aliases count (apple-m3 is an alias LLVM accepts)
    if mimic_toolchain::cpu_valid(&triple, cpu) != Some(false) {
        return Ok(());
    }
    let Some(list) = mimic_toolchain::cpus(&triple, false) else { return Ok(()) };
    let names: Vec<&str> = list.iter().map(|(n, _)| n.as_str()).collect();
    let hint = mimic_syntax::closest(cpu, names.iter().copied()).map(|b| format!("; did you mean `{b}`?")).unwrap_or_default();
    Err(format!("unknown CPU `{cpu}` for `{}`{hint} (run `mimic targets --cpus {}`)", target.name(), target.arch))
}

/// Resolve the CLI's notion of "current project" or a given file.
pub fn input_from(file: Option<&Path>) -> Result<Input, String> {
    if let Some(f) = file {
        if !f.is_file() {
            return Err(format!("error: no such file: {}", f.display()));
        }
        if f.extension().is_none_or(|e| e != "mim") {
            return Err(format!("error: {} is not a .mim file", f.display()));
        }
        return Ok(Input::File(f.to_path_buf()));
    }
    let cwd = std::env::current_dir().map_err(|e| e.to_string())?;
    let root = mimic_project::find_root(&cwd).ok_or_else(|| {
        format!("error: could not find `{MANIFEST}` in {} or any parent directory\n  = help: create a project with `mimic create <name>` or `mimic init`, or pass a file: `mimic run file.mim`", cwd.display())
    })?;
    Ok(Input::Project(Box::new(Project::load(&root)?)))
}

/// For `mimic check <file>`: a file inside a project is checked with its
/// project (a library module means nothing alone, and editors check the
/// file they show); anything else as `input_from`.
pub fn input_for_check(file: Option<&Path>) -> Result<Input, String> {
    if let Some(f) = file
        && f.is_file()
        && let Ok(abs) = fs::canonicalize(f)
        && let Some(root) = abs.parent().and_then(mimic_project::find_root)
        && abs.starts_with(root.join("src"))
    {
        return Ok(Input::Project(Box::new(Project::load(&root)?)));
    }
    input_from(file)
}

/// Compile a raw `.mnet` interaction-net file into an executable that reduces `@main`.
pub fn build_net(file: &Path, release: bool) -> Result<PathBuf, String> {
    let src = fs::read_to_string(file).map_err(|e| format!("cannot read {}: {e}", file.display()))?;
    let book = mimic_net::parse(&src).map_err(|e| format!("error: {}:{e}", file.display()))?;
    let enc = mimic_net::encode(&book).map_err(|e| format!("error: {}: {e}", file.display()))?;
    let entry = enc.id("main").ok_or_else(|| format!("error: {}: no @main definition", file.display()))?;
    let target = Target::host();
    let abs = fs::canonicalize(file).unwrap_or_else(|_| file.to_path_buf());
    let h = mimic_syntax::fnv1a(abs.to_string_lossy().as_bytes());
    let stem = file.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_else(|| "net".into());
    let dir = packages::cache_dir().join("net").join(format!("{stem}-{h:08x}")).join(if release { "release" } else { "debug" });
    fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let c_src = dir.join("main.c");
    fs::write(&c_src, mimic_net::emit_c_main(&enc, entry, !release)).map_err(|e| e.to_string())?;
    let tc = toolchain::select(&target)?;
    let rt = runtime_object(&tc, &target, release, None, false)?;
    let main_obj = dir.join("main.o");
    let mut c = toolchain::cc(&tc, &target);
    c.args(["-c", if release { "-O2" } else { "-O0" }]).arg(&c_src).arg("-o").arg(&main_obj);
    run_tool(c, "compiling the net")?;
    let exe = dir.join(&stem);
    let mut objects = vec![main_obj, rt];
    objects.extend(toolchain::static_tls(&tc).map(|t| t.lib));
    let job = LinkJob { objects: &objects, output: &exe, search: Vec::new(), libs: Vec::new(), debug: !release };
    run_tool(toolchain::link(&tc, &target, &job), "linking the net")?;
    Ok(exe)
}

#[cfg(test)]
mod std_tests {
    use super::*;

    /// Programs skip checking the standard library's own function bodies;
    /// this checks all of them (names, types of every annotated function).
    #[test]
    fn the_standard_library_checks() {
        let (p, diags) = Program::build(vec![prelude_file()], "std");
        assert!(diags.is_empty(), "{:?}", diags.iter().map(|d| &d.diag.msg).collect::<Vec<_>>());
        let mut p = p;
        let n = mimic_check::normalize(&mut p);
        assert!(n.is_empty(), "{:?}", n.iter().map(|d| &d.diag.msg).collect::<Vec<_>>());
        let d = mimic_check::check(&p, &Options { require_main: false, check_std: true });
        assert!(d.is_empty(), "{:?}", d.iter().map(|d| (&d.diag.msg, d.diag.span.line)).collect::<Vec<_>>());
        let t = mimic_check::types::check_types(&p, true);
        assert!(t.is_empty(), "{:?}", t.iter().map(|d| (&d.diag.msg, d.diag.span.line)).collect::<Vec<_>>());
    }
}
