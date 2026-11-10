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
| 3b (next) | `else` for errors, tuples, durations, route patterns in `match`, more string operations | |
| 4 | Closures that capture, operators as values (`gather(+)`), function types | |
| 5 | Loop words: `gather`, `trail`, `evolve`, `settle`, `arms`/`arm` | |
| 6 | Concurrent I/O: values that wait automatically when used, streams, event loop | |
| 7 | Performance: kernel fusion, compile-time reference-count elision, wider net subset, benchmark CI | |
| 8 | Tooling: `mimic test`, `mimic fmt` | |
| 9 | Proof libraries: minimal `ui` (terminal renderer) and `net::http` | |

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
