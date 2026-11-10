# Mimic — design and runtime model

Mimic is named after the mimic octopus. It is a compiled, array-based language that aims to be massively parallel and memory safe. It has two equivalent spellings for everything: APL/BQN/Uiua-style glyphs and modern functional words. The toolchain is a single binary, `mimic`. Source files use the `.mim` extension.

The language targets three problems:

| Problem | Mimic's answer |
|---|---|
| Massively parallel compute | Programs are lowered to an **interaction net**, whose rewrites are local and confluent, so any number of cores can reduce it at once without locks on program state. |
| Memory safety | Interaction nets are **linear**: every wire has exactly one consumer. Duplication and erasure are explicit nodes, so the compiler knows exactly when data is shared or freed. There is no GC, no use-after-free and no double free. |
| The memory wall | **Data stays out of the net.** Bulk data lives in a separate array layer of contiguous, typed, SIMD/GPU-friendly buffers. The net only orchestrates: it decides *what* gets computed (lazily, so only demanded results are produced) and fuses chained array operations into single passes over memory. |

## Two layers

```
            ┌───────────────────────────── net layer (orchestration) ───────────────┐
 program ─▶ │  CON / DUP / ERA + NUM, OP, REF, SWITCH, ARR-handle nodes            │
            │  parallel work-stealing reducer · lazy: only demanded redexes fire    │
            └───────────────┬───────────────────────────────────────────────────────┘
                            │ ARR handles (opaque)        DUP ⇒ retain, ERA ⇒ release
            ┌───────────────▼───────────── array layer (data) ──────────────────────┐
            │  refcounted, immutable-by-default typed buffers (C dtypes)            │
            │  copy-on-write `mut` · fused SIMD/GPU kernels for primitives          │
            └───────────────────────────────────────────────────────────────────────┘
```

* **Net layer.** It uses the interaction combinators (CON, DUP, ERA), plus HVM2-style extension nodes for numbers, operators, references to global definitions, and branching. A **node never contains array elements**. It holds an `ARR` handle instead.
* **Array layer.** A value is a refcounted header (`dtype`, `rank`, `len`, `data`) pointing at a contiguous buffer.
  * The dtypes are the C dtypes, which keeps FFI zero-copy.
  * When a net node holding an `ARR` handle is **duplicated**, the handle is retained. When it is **erased**, the handle is released.
  * A `mut` write mutates in place when the buffer is uniquely owned (`rc == 1`) and copies otherwise.
* **Kernels.** Primitives (`+/`, `⌽`, `↑`, `¨` …) run as kernels over whole buffers. They are not reduced element-by-element in the net, which is what makes interaction nets viable for numeric code. Adjacent kernels in a train fuse: `xs ¨ f ▷ +/` becomes one pass.

## Milestone 1 (implemented)

M1 is a vertical slice: source → parse → check → **LLVM** → native object, then `zig cc` links it. The pieces:

* **Front end** (`crates/mimic-syntax`). It has a lexer in which glyphs and words produce identical tokens. The glyph table is the single source of truth, and the parser is a precedence-climbing parser with left-to-right trains.
* **Checker** (`crates/mimic-check`) covers:
  * the module tree and path resolution;
  * Rust-style privacy and immutability, plus a no-shadowing rule;
  * call arity;
  * `unsafe` for FFI;
  * lambda capture rules.
* **Codegen** (`crates/mimic-codegen`):
  * Every value is a `mim_val*` handle into the array layer.
  * Codegen inserts retain/release according to one contract. Runtime calls and Mimic functions **borrow** their arguments and return an **owned** value. `let` bindings own their value until scope exit.
  * In debug builds the program checks at exit that no array is still alive. The golden tests run every fixture under that leak check.
* **Runtime** (`runtime/mimic_rt.c`). This is the array layer in portable C. It is embedded in the `mimic` binary, then compiled and cached per target with `zig cc`, which is why every Zig target is reachable.
* **Type annotations** (`x: i64`, `xs: [f64]`, `-> f64`) are optional. When present they are enforced at function boundaries at runtime. Static inference is planned for M4.

There is no net layer yet in M1: code runs strictly and sequentially, with the array layer handling memory.

## Parallel by default

Mimic has **no parallelism annotations**: no `parallel`, `spawn` or `async`. Whatever can safely run in parallel does, and the compiler and runtime make that call:

* **Purity analysis** (`mimic_check::Purity`). A function is *pure* unless it transitively calls `print` or FFI. Pure code may run in any order, on any thread, or not at all. Impure code keeps program order.
* The **net layer** (M2 phases 2–3) extends this to every independent subexpression, so the two sides of `fib(n-1) + fib(n-2)`, independent `let`s, and function arguments all reduce in parallel.

## Milestone 2, phase 1 (implemented): parallel array layer

* **Thread pool.** It is sized to the online CPUs (`MIMIC_THREADS` overrides). Nested parallel work runs inline on the worker that encounters it.
* **Data-parallel kernels.** Elementwise operators, `⍳` and the reductions are split into **fixed-size chunks** (`MIMIC_PAR_GRAIN`, default 16384).
  * Chunk boundaries never depend on the thread count, and partial results are combined in chunk order.
  * As a result, floating-point reductions give bit-identical results with 1 thread or 100.
* **`¨` (each).** Calls with a pure function run in parallel, using fine-grained chunks (about 16 per thread) to balance the load. Calls with an impure function run in order.
* **Memory:**
  * Refcounts are atomic.
  * Each value is a single allocation, with the header and elements together.
  * Scalar-sized blocks are recycled through per-thread free lists.
  * The live count used by the leak check is kept per thread, padded to 128-byte cache lines.
  * `true`, `false`, small integers and every literal in the program are **immortal**: they are created once and never refcounted or freed.
* **Panics.** If several threads panic, only the first one reports.

## Milestone 2, phase 2 (implemented): interaction-net core

The parallel reducer for interaction nets is described in [net.md](net.md). It has:
* nodes CON, DUP, ERA, REF, NUM, OPR, SWI and ARR;
* lock-free linking;
* per-worker private stacks with work stealing;
* unboxed numbers;
* array handles whose kernels run on the array layer;
* lazy `REF` expansion, so untaken branches are never computed.

Nets are written in a text format (`.mnet`) and run with `mimic net run`. Phase 3 compiles pure Mimic functions to this form.

## Milestone 2, phase 3 (implemented): compiling Mimic to nets

Pure functions compile to interaction nets ([net.md](net.md#compiling-mimic-to-nets-phase-3)), so ordinary recursive code runs in parallel with no annotations:
* Independent calls such as `fib(n-1) + fib(n-2)` reduce on different cores.
* `if` is lazy.
* Type annotations are enforced in nets exactly as in native code.

Adaptive native fallback runs each core's share of the work at native speed. Bounded native recursion hands deep recursion back to the heap-allocated net. `--no-net` compiles everything natively, for comparison.

**Known bottleneck:** scalars are still boxed (one allocation per scalar result). Unboxed scalars arrive with NUM nodes in the net layer and static types in M4.

## Roadmap

| Milestone | Scope |
|---|---|
| **M1** ✅ | CLI, project system, mod.mim, packages from git, lexer/parser/checker, LLVM codegen, array runtime, C FFI (import), cross-compilation via Zig. |
| **M2** | Net runtime in C with a parallel reducer and `ARR` handle nodes. Lazy `let`. Parallel `¨`. Kernel fusion. Atomic refcounts for shared handles. |
| **M3** | LLVM auto-vectorised kernels with explicit SIMD widths. A GPU backend (NVPTX / AMDGPU / SPIR-V) for array kernels, with the net deciding placement. |
| **M4** | Static types with rank polymorphism and generics. Nested arrays (`⊂`). Higher ranks. Closures. `pub extern fn` exports with `--emit-header`. Registry, `mimic fmt`, `mimic test`, LSP. |
