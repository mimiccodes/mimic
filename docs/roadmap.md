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
| 4 (next) | Closures that capture, operators as values (`gather(+)`), function types | |
| 5 | Loop words: `gather`, `trail`, `evolve`, `settle`, `arms`/`arm` | |
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
| B | Full C interop: import C headers directly, structs/unions/enums/function pointers/varargs/callbacks, `--emit-header`, compile C sources in a project | |
| D | Language completion (M3 steps 4–5, generics, lists/maps, strings, integer types, richer patterns) | |
| E | Standard library and I/O (M3 step 6, M5, M6) | |
| F | Performance (typed `¨` kernels, fusion through `¨`/`↑`/`↓`/`⌽`/`⌿`, RC elision, LTO, SIMD, scheduling, GPU) | |
| G | `test`, `fmt`, registry, incremental builds, DWARF, benchmark CI including Rust | |
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
| combine everything into one value | `gather` | `/` | yes (tree combine) |
| running results | `trail` | `\` | yes (parallel prefix) |
| step a state *n* times | `evolve` | `⍣` | inside each step |
| step until a condition holds | `settle` | `⍥` | inside each step |
| split work across arms | `arms` / `arm` | `⋔` | yes, one task per `arm` |

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

let a = http::get(url1)                 // both requests are in flight at once
let b = http::get(url2)
print(a.status + b.status)              // waits only here, where the values are used

let user = db::user(id) else { return not_found() }
let page = http::get(url) ▷ timeout(2s) ▷ retry(3) else { return error("upstream down") }
```

## Memory model

Mimic needs no manual memory management, no tracing GC and no borrow checker:

* **Net layer: linear by construction.** Every wire has exactly two ends. Copying is an explicit DUP node and discarding is an explicit ERA node. Each rewrite frees the nodes it consumes, at the moment it consumes them.
* **Array layer: compiler-inserted reference counting.**
  * Values are immutable, so reference cycles, and the leaks they cause, are impossible.
  * Uniquely-owned arrays are updated in place.
  * M3 step 7 removes most count updates at compile time.
