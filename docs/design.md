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

M1 is a vertical slice: source → parse → check → **LLVM** → native object, then a linker turns it into an executable (today: the LLD built into `mimic`). The pieces:

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
  * Debug builds also run the memory check (`-DMIM_MEMCHECK=1` for the runtime; compiled code then sends every release to it). A freed value's header is marked (its count `MIM_FREED_RC`, its type `MIM_FREED_DT`, so no type check in compiled code passes and every use reaches the runtime), its elements are filled with `0xDB`, and it is kept in a queue (up to 65,536 values and 64 MB) before its memory is used again. A retain, a release or a read of a marked value stops the program in `mim_memcheck_failed`, where `mimic lldb` sets a breakpoint. Release builds compile none of it: their runtime object is the same.
* **Runtime** (`runtime/mimic_rt.c`). This is the array layer in portable C. It is embedded in the `mimic` binary, then compiled and cached per target by the Clang built into `mimic`.
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
* **At exit.** In release builds (no leak check) the values `main` still holds when it ends are not freed one by one: the process exits right after, and the system takes the memory back at once. Nothing is lost: releasing a value only ever frees memory. Debug builds free everything and check for leaks.

## SIMD across elements (F5)

`xs ¨ f`, where `f` (a named function or a lambda, and what it calls) is pure and works on `f64`, `f32`, `i64`, `i32` and truth values, also gets a *vector version* (`crates/mimic-codegen/src/simd.rs`) that computes 8 elements at once, in the lanes of vectors, as ISPC does:
* An `if` computes both sides for every lane and picks per lane; nothing in such code can fail, so a side a lane does not take is harmless. Calls see the condition: a call gets the lanes that take it as a mask, and the other lanes do not run it.
* A function calling itself last is a loop: each lane either finishes, keeping its result, or goes round again with new parameters; the loop runs while any lane goes round.
* The kernel runs groups of 8 through the vector version and the remaining elements one by one through the scalar function. Arithmetic is the scalar code's, lane by lane, so the results are the same. Integer `div` and `%` qualify by a number written in the program, or by a value the lambda captures (checked once: 0 or -1 runs everything one by one, and the scalar code reports a division by zero as always).

With 8 lanes (several vector registers each, so their chains of operations overlap), mandelbrot on one core goes from 2.65 s to 1.75 s, about 0.7× C.

## GPU kernels

A fused map (see kernel fusion) that the GPU computes *exactly* as the CPU does is also compiled to a Metal kernel (`crates/mimic-codegen/src/gpu.rs`); the runtime (`runtime/mimic_gpu.c`) runs it on the GPU when that pays off. No annotation: like threads, it is the compiler's and runtime's call.

* **What qualifies.** Elementwise arithmetic, comparisons and logic over `[f32]`, `[i32]`, `[i64]` and `[bool]`, with `⌽ ↑ ↓` and `⍳`; and `xs ¨ f` where `f` (a named function or a lambda, and every function it calls) is pure and works on `f32`, `i32`, `i64` and truth values: `let`, `if`/`else`, operators, calls (`gpufn.rs`). A function calling itself last becomes a loop; other recursion stays on the CPU. Apple GPUs have no `f64`, so `f32` code is made only where each float operation takes f32 operands and rounds to f32, exactly what the CPU computes. Integer `div` and `mod` qualify by a number written in the program, or by a value the lambda captures (checked once before the GPU starts: 0 or -1 keeps the map on the CPU, which reports a division by zero as always). Calls to other kinds of functions, records and `f64` stay on the CPU.
* **When.** Metal is loaded on first use (`dlopen`, no link-time dependency; on other systems everything runs on the CPU). Lists are read and written in place (unified memory: no copies). Starting Metal takes about 0.1 s once, and the CPU's cores do simple arithmetic about as fast, so by default a kernel goes to the GPU only with a lot of work (about 4·10⁹ estimated operations, a quarter of that once the GPU is running). `MIMIC_GPU=1` sends every kernel there, `MIMIC_GPU=0` none.
* **Results.** The GPU never fuses a multiply and an add, so its results are exactly those of the CPU with `--strict-math` (and with `--strict-math` both agree bit for bit). By default the CPU may fuse `a * b + c` (see fused multiply-add), so a float result can differ in its last bits between the two, as between CPUs with and without the instruction. The golden tests run every fixture with `MIMIC_GPU=1` too.

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

**Pre-reduction (F7).** Before a definition is encoded, every interaction that needs no definition expanded is done at compile time (`Def::pre_reduce`): constructor and duplicator pairs annihilate, erasers and copies of numbers propagate, integer `+ - ×` on known numbers fold, a switch on a known number picks its branch, and a wire between a variable and a tree becomes the tree itself. References are never expanded, so laziness and recursion are unchanged. Most of the redexes a compiled function starts with were such wires: `fib` on nets runs about 20% faster (4.2 s → 3.4 s for `fib(30)` on one thread). Functions on `f32` or `i32` stay native: nets carry 64-bit numbers.

Adaptive native fallback runs each core's share of the work at native speed. Bounded native recursion hands deep recursion back to the heap-allocated net. `--no-net` compiles everything natively, for comparison.

## Milestone 3, step 2 (implemented): static types and unboxed scalars

`mimic_check::types` infers a type for every expression, once per **specialisation**: a function together with the argument types it is called with, as with C++ templates and Rust generics.
* Recursion is solved by fixpoint iteration.
* Values whose type depends on run time (for example, an `if` whose branches return `i64` and `f64`) get the type `Dyn` and stay boxed, so behaviour is identical to the dynamic semantics.
* Annotations coerce (`i64` → `f64`) and are checked at compile time.

Codegen keeps `i64`, `f64` and `bool` in registers, boxing only at array, runtime and net boundaries. Each specialisation can be emitted in three kinds:
* `Plain`: typed native code.
* `NetWrap`: the function runs as an interaction net.
* `Native`: depth-counted native code for the net's fallback. The depth travels as a hidden argument, so counting it costs nothing; past 2048 frames the call moves to the heap-allocated net.

Boxed-ABI thunks connect typed code to `¨`, the net's native table (which dispatches to the typed specialisation when the arguments fit) and library exports. Results are in [BENCHMARK.md](../BENCHMARK.md).

## Roadmap

| Milestone | Scope |
|---|---|
| **M1** ✅ | CLI, project system, mod.mim, packages from git, lexer/parser/checker, LLVM codegen, array runtime, C FFI (import), cross-compilation. |
| **M2** | Net runtime in C with a parallel reducer and `ARR` handle nodes. Lazy `let`. Parallel `¨`. Kernel fusion. Atomic refcounts for shared handles. |
| **M3** | LLVM auto-vectorised kernels with explicit SIMD widths. A GPU backend (NVPTX / AMDGPU / SPIR-V) for array kernels, with the net deciding placement. |
| **M4** | Static types with rank polymorphism and generics. Nested arrays (`⊂`). Higher ranks. Closures. `pub extern fn` exports with `--emit-header`. Registry, `mimic fmt`, `mimic test`, LSP. |
