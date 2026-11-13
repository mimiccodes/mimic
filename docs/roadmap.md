# Mimic roadmap

Mimic's goals:

* **Fastest language:** measured, not claimed. See [BENCHMARK.md](../BENCHMARK.md).
* **Parallel and concurrent by default:** no `parallel`, `spawn`, `async` or `await`.
* **No loops:** a small set of words (and glyphs) replaces them.
* **Simple syntax:** any programmer can read it. Words are the default spelling and glyphs are optional.
* **Natural for interfaces and networking:** the syntax is shaped so that writing UIs and network code feels easy.
* **Memory safe:** without manual memory management, a tracing GC, or a borrow checker.

## Done

| Milestone | What it delivered |
|---|---|
| M1 | Toolchain (`create`/`init`, `run`/`build`, `check`, `mod`, `install`), LLVM codegen, array runtime, C FFI, cross-compilation |
| M2.1 | Parallel array layer: thread pool, deterministic parallel reductions, purity-driven parallel `¨` |
| M2.2 | Interaction-net core: lock-free parallel reducer, unboxed numbers, lazy branches |
| M2.3 | Pure functions compile to nets; adaptive native fallback; deep recursion moves to the heap |
| — | `mimic build` produces the fastest binary by default |

## M3: a fast, usable core (in progress)

| Step | Contents | Status |
|---|---|---|
| 0 | This roadmap; the syntax decisions below | ✅ |
| 1 | `div`/`⫽` (floored integer division), `range`/`‥`, `max`/`⌈`, `min`/`⌊`, `abs`/`∣` | ✅ |
| 2 | Static types and unboxed scalars. Inference, compile-time type errors, per-call-site specialisation. **Result:** `fib` runs at 1.42× of C on one core and 5.4× faster than C on 12 threads. | ✅ |
| 3a | Records with `with`, tagged unions, exhaustive `match`, field access, string interpolation and equality, named and default arguments, lists of strings/records/lists | ✅ |
| 3b | Tuples (typed) and tuple patterns, `Option`/`Result` prelude with `let … else`, duration literals, route patterns with typed captures | ✅ |
| perf 1 | Kernel fusion, single-division floored modulo, power-of-two shifts/masks, real loops for tail recursion. **Result:** the showcase compute benchmark matches C on one core (4.61 s vs 4.81 s) and is 4.6× faster on all cores. | ✅ |
| 4 | Closures that capture (by value), multi-parameter lambdas, functions and operators as values (`(+)`), calling any function value, `fn(…) -> T` types, parallel `¨` over pure function values | ✅ |
| 5 | Loop words: `gather`, `trail`, `evolve`, `settle`, `arms`/`arm` (see below) | ✅ |
| 6 | Concurrent I/O: values that wait automatically when used, streams, event loop | |
| 7 | Performance: kernel fusion, compile-time reference-count elision, wider net subset, benchmark CI | |
| 8 | Tooling: `mimic test`, `mimic fmt` | |
| 9 | Proof libraries: minimal `ui` (terminal renderer) and `net::http` | |

## Toolchain phases (the plan of 2026-11-10)

These run alongside the language milestones. Bootstrapping, a compiler written in Mimic, comes only after everything else is done.

| Phase | Contents | Status |
|---|---|---|
| C1–C4 | Stable error codes, `mimic explain`, `check/build/run --emit-json` ([diagnostics.md](diagnostics.md)), fix suggestions with exact edits, `check --fix`, type errors reported at the call site | ✅ |
| C5–C7 | `--json` for `mod list` and `targets`, inferred types per span (`--types`), LSP server | |
| A1, A2, A5 | Embedded LLD and Clang (`mimic cc`, `mimic ld.lld`), in-process archive writer; native macOS builds | ✅ |
| A3 (musl), A4, A6 | 30 musl Linux targets self-contained (bundled musl 1.2.5 and compiler-rt, built on first use); targets are LLVM triples, and every one of the 39 CPU architectures with an LLVM backend compiles Mimic programs (objects, or PTX/assembly); GPU architectures wait for the GPU backend (F8); `mimic targets` lists LLVM's architectures, OSes, environments and backends; Mimic code uses Clang's CPU/ABI per target | ✅ |
| A3 (WASI) | `wasm32-wasi`: bundled wasi-libc, `wasm-ld`; all golden tests pass in Node's WASI; `mimic cc` fills in the C library for every bundled target | ✅ |
| A3 (Apple) | Every Apple platform with an installed SDK: macOS, Mac Catalyst, iOS, tvOS, watchOS, visionOS, devices and simulators; ad-hoc signed. Catalyst runs every golden test here; an iOS-simulator build ran in the simulator | ✅ |
| A (rest) | More C libraries for complete executables (glibc stubs, mingw-w64, BSD, Android, Apple SDK stubs); `--cpu`; our own static LLVM build with every backend enabled, including the experimental ones (no Homebrew z3/zstd); one static `mimic` binary | |
| B1 | `extern "C" include "header.h"`: C headers imported directly, read by the embedded Clang for the build's target, types mapped by the target's data model, cached; `--cpu portable` | ✅ |
| B5 (handles) | `cptr` opaque C pointers (returned and passed), `T **` out-parameters, typedefs resolved; SQLite works with no binding code | ✅ |
| B7 | C sources in a project (`ffi { sources, include, cflags }`), compiled per target by the embedded Clang, cached, linked or archived | ✅ |
| B2 (structs) | C structs by value, both ways, nested and anonymous-typedef'd, as Mimic records, through generated C wrappers compiled by the embedded Clang (each target's struct ABI handled by Clang) | ✅ |
| B3 | Varargs: imported variadic functions and `...` in extern declarations; arguments get C's default promotions; true variadic calls | ✅ |
| B6 | `extern "C" fn` exports Mimic functions to C (C types, C ABI, runtime starts itself), `mimic build --emit-header` | ✅ |
| B8 | ABI conformance suite (`tests/abi`): 1-byte, HFA, mixed-class, 64-byte, padded, nested structs and stack spill, checked on native arm64, Catalyst, x86-64 (Rosetta), WebAssembly; linked for 6 Linux ABIs | ✅ |
| B4 | Callbacks: Mimic functions, lambdas and closures passed to C as function pointers through generated trampolines (synchronous; nested) | ✅ |
| B (rest) | Callbacks C keeps for later, unions, array fields, arrays from C, shared libraries | |
| B | Full C interop: import C headers directly, structs/unions/enums/function pointers/varargs/callbacks, `--emit-header`, compile C sources in a project | |
| D1–D3 | Closures, functions/operators as values, loop words | ✅ |
| D5, D6 (first part) | Maps (`["a": 1]`, `m[k]`, copy-on-write `m[k] = v`, `get`/`has`/`keys`/`values`/`put`/`remove`), list functions (`push`, `concat`, `sort`), the string library (`split`, `join`, `trim`, `replace`, `find`, `parse_int`, …), `+` and `==` on strings; built on `extern "mimic"` runtime intrinsics ([std.md](std.md)) | ✅ |
| D4, D7–D10 | Generics (`Option[T]`, `fn f[T]`), bitwise operators and hex/binary literals, guards/alternatives/ranges/record/array patterns and `if let`, structural `==`, matrices (`⥊ ⍉ ⊠`, `m[i, j]`) | ✅ |
| D (bytes) | `[u8]` byte arrays (compact; elements read as i64), the `bytes` module (text, hex, base64), `fs::read_bytes`/`write_bytes`, `tcp::send_bytes`/`receive_bytes` | ✅ |
| D (sized, part 1) | `f32` and `i32` as real types: scalars and lists (`[f32]`, `[i32]`, 4 bytes each), f32 arithmetic rounded to f32, i32 wrapping; promotion rules shared by the type checker and the runtime; boxed for now (compiled registers and fused loops next), for the GPU (F8) | ✅ |
| D (rest) | `f32`/`i32` in registers and fused loops; other sized types (`i8`, `i16`, `u16`, `u32`, `u64`) | |
| E1, E2 | Arguments, environment, stdin, files, directories, paths; clocks, dates, random numbers, running programs ([std.md](std.md)) | ✅ |
| E3 (first part) | Implicit futures for overlapping effects; `¨` runs I/O-only functions in parallel ([syntax.md](syntax.md#concurrency-implicit-futures)) | ✅ |
| E4 (first part) | `http` module (client, server with route patterns, parallel pure handlers) and `tcp` module; effect annotations on `extern` blocks (`pure`, `concurrent`, `ordered`, `sync`) for C libraries and the runtime | ✅ |
| E4 (JSON) | `json` module: `encode`/`encode_pretty` for any value, `decode[T]` into records (schemas emitted by the compiler; explicit type arguments), `parse` into `json::Value`; raw strings `'…'`; `http::json` | ✅ |
| E4 (TLS) | `https://` in the HTTP client through the system's TLS (Security framework / OpenSSL 3 via dlopen), certificate and host-name checks; redirects | ✅ |
| E4 (WebSockets) | `ws` module: client (`ws://`, `wss://`) and server, RFC 6455 framing, pings, fragments | ✅ |
| E3 (channels) | `chan` module: typed channels between tasks, bounded channels with backpressure; deferred tasks without threads (WASI); `¨` with effect-only functions; nested `match` exhaustiveness | ✅ |
| E3 (event loop) | Connections and their tasks run as coroutines on a few scheduler threads; one poller (kqueue / epoll) for sockets and timers; channels, sleeps and awaits park instead of blocking ([std.md](std.md#the-event-loop)) | ✅ |
| E4 (UDP) | `udp` module: bind, send/send_bytes, receive/receive_within with sender address; parks on the event loop | ✅ |
| E4 (TLS servers) | `tls` module: identities from PEM (PKCS#8/EC/RSA keys), `tls::self_signed` (P-256 certificates built in the runtime), `http::serve_tls`, `ws::serve_tls`, `tls::accept`, encrypted TCP streams with `tls::connect` / `connect_trusting` | ✅ |
| E5 (first part) | `ui` module: `view(state)` / `update(state, message)`, elements as functions with named arguments, `ui::run` serving the browser (one task per tab, minimal patches over a WebSocket), constructors as functions, nested `with`, purity polymorphic in function parameters, E0605 ([ui.md](ui.md)) | ✅ |
| E3 (rest), E4 (rest), E5 | Event loop on Windows (IOCP), io_uring, decoding unions, interfaces (`view(state)`, design in [ui.md](ui.md)) | |
| F (first part) | Typed `¨` kernels: `¨` with pure numeric functions fused into the parallel loops (lambdas may capture numbers and numeric arrays); direct indexing of numeric vectors ([BENCHMARK.md](../BENCHMARK.md)) | ✅ |
| F (benchmarks) | `bench/` suite in Mimic, C, Rust and Go (7 benchmarks); radix sort; typed scans; `math` module; direct field access; adaptive `¨` ([BENCHMARK.md](../BENCHMARK.md)) | ✅ |
| F (static lengths) | Lists of known length (≤ 8) in registers, shaped and flat records, `evolve` compiled as a loop, short loops unrolled; inline allocator; storage follows the static type. nbody 8.6× → 1.09× C ([BENCHMARK.md](../BENCHMARK.md)) | ✅ |
| F6 (fork-join) | Independent calls to pure recursive functions run in parallel (`f(a) + f(b)`, constructors, runs of `let`s): fork, sequential and ordinary versions of each function, a work-stealing pool, parallel freeing of large values; small blocks carved from per-thread chunks ([BENCHMARK.md](../BENCHMARK.md)) | ✅ |
| F2 (part) | Fusion through `⌽`, `↑` and `↓`: index remaps inside the fused loop, no intermediate arrays | ✅ |
| F (json) | `json` benchmark (C, serde, Go): direct typed decoding, fast number formatting and parsing; 2.9× → 1.10× C. LTO with the runtime measured: no gain, not adopted ([BENCHMARK.md](../BENCHMARK.md)) | ✅ |
| F (http) | `http` benchmark (C floor, hyper, net/http, Go load generator in run.py); HTTP/1.1 keep-alive and pipelining in `http::serve`; Mimic 1.34 s vs hyper 1.52 s on one core ([BENCHMARK.md](../BENCHMARK.md)) | ✅ |
| F (records) | Numbers stored raw in records and variants; `with` inline (in place when unshared); `¨` lends elements and moves results; `records` benchmark (1.94 s → 0.93 s CPU; 27× C, reuse is next) ([BENCHMARK.md](../BENCHMARK.md)) | ✅ |
| F3 (reuse) | Reuse in place: moving locals (one use), consumed parameters (`{ p with … }`), `¨` handing elements over (`mim_each_own`); `¨` over numbers without boxes, over records fused; records 26.7× → 5.9× C ([BENCHMARK.md](../BENCHMARK.md)) | ✅ |
| F (keep) | `+/ ×/ ≢` over `xs ⌿ mask` fused into one pass; non-atomic reference counts measured: no gain, not adopted ([BENCHMARK.md](../BENCHMARK.md)) | ✅ |
| F (ui) | `ui` benchmark (10k rows, view + diff per frame: 250 → 174 ms on one core, 39 ms on all); inline `≢`, handle indexing and list literals; idle fork-join workers back off; 32-byte headers; inline `with` uniqueness check ([BENCHMARK.md](../BENCHMARK.md)) | ✅ |
| F (finish) | Rows from capturing lambdas reused while their captures stay the same; `xs ⌿ mask` of a fused expression counted then filled; chains `xs ¨ f ¨ g` fused (records 4.6× → 3.9× C, 1.6× on all cores); performance-core scheduling measured: nothing to change ([BENCHMARK.md](../BENCHMARK.md)) | ✅ |
| F8 (GPU) | Metal kernels: fused maps and `xs ¨ f` with pure numeric functions (self tail calls as loops), reductions taking their elements from the GPU; results exactly those of `--strict-math`; unboxed f32/i32 scalars; f32 mandelbrot kernel 18 ms vs ~200 ms on all CPU cores (NVPTX/AMDGPU/SPIR-V wait for hardware to test on) | ✅ |
| F5, F7, F9 | SIMD across elements (`xs ¨ f` 8 at a time: mandelbrot 0.71× C on one core); compile-time net pre-reduction (nets ~20% faster); `--fast-math` | ✅ |
| F (records 2) | `¨` into records fused; release builds don't free `main`'s values at exit; records 2.2× C on one core, 1.0× on all ([BENCHMARK.md](../BENCHMARK.md)) | ✅ |
| F (deferred) | Lists of records stored flat (an array of structs): would take records from 2.2× to about 1× C on one core, but every runtime path over lists would need a new kind of list; deferred (2026-11-13) | ⏸ |
| G1 | `test "name" { }` blocks and `expect` (failures show both sides), `mimic test` (a process per test, in parallel, filters); `mimic fmt` (words or `--glyphs`, indentation, `--check`; checked to keep every token) | ✅ |
| G4 | Debug information (DWARF) in debug builds: breakpoints on `.mim` lines, stepping, numeric parameters and `let`s, backtraces under Mimic names (lldb) | ✅ |
| G | Registry, incremental builds, benchmark CI including Rust, CI hosts, `mimic doc`; lists, strings and records shown as values in the debugger | |
| H | Bootstrap: the compiler in Mimic, stage 2 reproduces itself | |

## Later milestones

| Milestone | Contents |
|---|---|
| M4: compute | explicit SIMD, multi-dimensional arrays and rank polymorphism, GPU backend |
| M5: networking | full TCP/HTTP/websocket/TLS stack, JSON ↔ records, HTTP throughput benchmark |
| M6: interfaces | reactive `view(state)` model; backends: terminal → web (WASM) → native GPU |
| Ecosystem | registry, versioned resolution, workspaces, incremental compilation, LSP, `mimic doc`, C header export and import |

## Syntax decisions

### Loops are replaced by words

| You want to… | Word | Glyph | Parallel |
|---|---|---|---|
| transform every element | `each` | `¨` | yes |
| keep some elements | `keep` | `⌿` | yes |
| combine everything into one value | `gather` | `∕` (and `+/` `×/` `⌈/` `⌊/`) | yes (tree combine, when the function is pure) |
| running results | `trail` | `⍀` | yes (parallel prefix, when pure) |
| step a state *n* times | `evolve` | `⍣` | inside each step |
| step until a condition holds | `settle` | `⍥` | inside each step |
| split work across arms | `arms` / `arm` | `⋔` | yes, one task per `arm` (when the body compiles to nets; see below) |

**Implemented (M3 step 5)**, with these details:
* **Glyphs for `gather` and `trail`.** These are `∕` (U+2215) and `⍀`. Plain `/` is division and `\` starts digraphs, so neither is free. Type them as `\g` and `\t`.
* **The function must be associative.** `gather` and `trail` combine chunks in order. With an associative function (`+`, `⌈`, string join) the parallel result equals a left fold.
* **How `arms` runs.** `arms` is lifted to a hidden named function, with captured variables passed as extra parameters. When that function is pure and uses only numbers, recursion and the reductions, it compiles to interaction nets and every `arm` runs in parallel. A 3-million-element Collatz sum took 3.39 s on one core and 0.32 s on all cores. A body that uses `¨`, strings or closures runs its recursion on one core for now. Since F6 any pure `arms` body runs in parallel by fork-join, closures, `¨` and strings included (the same Collatz sum with a `¨` in the body: 3.74 s on one core, 0.39 s on all).

```mimic
let total   = prices ▷ gather(+)
let balance = deposits ▷ trail(+)
let world   = start ▷ evolve(60, physics)
let root    = 1.0 ▷ settle(newton, x => abs(x × x - 2) < 1e-12)
let sum     = arms (lo = 0, hi = 1_000_000) {
    if hi - lo < 1000 { lo ‥ hi ▷ gather(+) }
    else {
        let mid = (lo + hi) div 2
        arm(lo, mid) + arm(mid, hi)
    }
}
```

### Interfaces: a UI is data

```mimic
type State = { count: i64 }
type Msg = Inc | Dec | Reset

fn update(s: State, msg: Msg) -> State {
    match msg {
        Inc   => { s with count: s.count + 1 }
        Dec   => { s with count: s.count - 1 }
        Reset => State { count: 0 }
    }
}

fn view(s: State) -> View {
    column [
        text("Count: {s.count}", size: 24)
        row [ button("−", on: Dec)  button("+", on: Inc) ]
    ]
}
```

### Networking: routes are patterns, and I/O is concurrent

```mimic
fn handle(req: Request) -> Response {
    match req {
        GET  "/"                => ok("hello")
        GET  "/users/{id: i64}" => db::user(id) ▷ json
        POST "/users"           => req.body ▷ decode(User) ▷ db::insert ▷ created
        _                       => not_found()
    }
}

let a = http::fetch(url1)                // both requests are in flight at once
let b = http::fetch(url2)
print(a.status + b.status)              // waits only here, where the values are used

let user = db::user(id) else { return not_found() }
let page = http::fetch(url) ▷ timeout(2s) ▷ retry(3) else { return error("upstream down") }
```

## Memory model

Mimic needs no manual memory management, no tracing GC and no borrow checker:

* **Net layer: linear by construction.** Every wire has exactly two ends. Copying is an explicit DUP node and discarding is an explicit ERA node. Each rewrite frees the nodes it consumes, at the moment it consumes them.
* **Array layer: compiler-inserted reference counting.**
  * Values are immutable, so reference cycles, and the leaks they cause, are impossible.
  * Uniquely-owned arrays are updated in place.
  * M3 step 7 removes most count updates at compile time.
