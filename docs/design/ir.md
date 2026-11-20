---
title: Mimic IR design (stage 0.4)
description: The intermediate representation between the syntax tree and LLVM, the passes over it, and how the compiler moves onto it.
---
# Mimic IR design (stage 0.4)

**Status: draft for review.** The plan for step 0.4.1 in [versions.md](../versions.md), and the base of steps 0.4.2 to 0.4.13.

## Why

Today the compiler goes from the syntax tree straight to LLVM, with a special case for each shape it knows how to make fast. Every slowdown found while building the 0.3.16 benchmarks was a shape it did not know: a `match` with a `panic` arm typed as dynamic, a tuple built per pixel, a list copied every step of a state machine, a small helper sent to the interaction nets. Each fix added another special case; the next program found the next one.

An intermediate representation (IR) is where general analyses run instead: ownership, escape analysis, loop forming, fusion, range analysis and the choice of where code runs, each written once and applied to every program shape. Rust (MIR), Swift (SIL), Lean (λRC) and Koka are built this way.

## Shape

**Three levels, lowered in order, one data structure:**

1. **Array level:** what the program says, with types. Array operations are single operations with a function region: `map`, `reduce`, `scan`, `keep`, `gather`, `stencil`, `windows`, `outer`, `inner`, `sort`, `grade`. Fusion, parallelism and the choice of CPU, SIMD, GPU or nets happen here.
2. **Loop level:** array operations become loops and kernels; records, tuples and small lists become their fields; ownership operations are explicit (`dup`, `drop`, `reuse`).
3. **LLVM IR:** from the loop level, almost one to one.

**Structured control flow:** `if`, `match`, loops and array operations are operations with regions (blocks inside them), not jumps between blocks. That is what lets nets (switches) and GPU kernels (no arbitrary jumps) be made from the same IR, and keeps loops easy to find.

**SSA values:** every value is defined once; a `let mut` becomes a new value at each assignment.

## Types

The checker's types, made exact (no `Dyn` unless the program is dynamic, and then the IR says where and why, for the 0.4.4 warnings):

* Scalars: `i8 … i64`, `u8 … u64`, `f32`, `f64`, `c64`, `c128`, `bool`, `char`.
* Lists of a known small length (in registers), lists of any length, matrices and higher ranks.
* Records, tuples and variants, with their layout chosen per use (in registers, on the stack, packed, flat in a list).
* Text, maps, sets, closures, C pointers.
* `dyn`: a value whose type is known only at run time.

## Effects

Each operation and function carries one of today's effect levels: **pure**, **concurrent** (may overlap with other concurrent work), **ordered** (keeps its place among effects), **sync**. Passes may reorder, merge, duplicate or run in parallel only what the levels allow; `mimic why` reads its reasons from them.

## Ownership (0.4.2)

* Values are reference counted, as today; the IR makes it explicit with `dup` (retain) and `drop` (release).
* A pass in the style of Perceus inserts them from liveness: a value is dropped right after its last use, and its last use **moves** it.
* **Borrowing** is inferred per parameter: a function that only reads a value borrows it (no `dup` at the call).
* **Reuse:** a `drop` of a value whose memory fits what is built next becomes a `reuse` token; with the only reference, `{ s with … }`, `push`, `put` and a map over a list write in place. This replaces the pattern rules in `owned.rs` and also applies to debug builds.
* Interaction nets keep their meaning: DUP and ERA on an array handle are `dup` and `drop`.

## Passes, in order

1. **Lowering** from the checked syntax tree (one function at a time).
2. **Inlining** of small and single-use functions and lambdas.
3. **Specialisation** on types (as today) and on small constants where it pays (sizes known at compile time).
4. **Ownership:** liveness, `dup`/`drop`, borrowing, reuse.
5. **Fusion** of array operations (today's chains, plus the new vocabulary), across function boundaries once inlined.
6. **Placement** (0.4.10): one cost model picks a fused kernel with SIMD, fork-join, interaction nets, a task, or (from 0.5) the GPU, from sizes, work per element and profiles.
7. **Loop forming:** `evolve`, `settle` and tail recursion become loops; array operations become kernels.
8. **Scalar replacement and escape analysis** (0.4.3): records, tuples and small lists that do not escape become registers or stack slots.
9. **Range analysis** (0.4.4): bounds checks and floored-division corrections proved unneeded are removed; integers narrowed in machine code where results do not change.
10. **Emission:** LLVM IR for the CPU; kernels for the GPU backends; nets for the net runtime.

Each pass can be printed (`mimic build --emit-mir`, per level) and the IR verified after each pass in debug builds of the compiler.

## Interaction nets

Pure functions are translated to nets from the array level (today from the syntax tree); structured `if`/`match` map to net switches. The net runtime, `.mnet` files and `mimic net show`/`trace` do not change. Whether a function runs on nets is the placement pass's choice: recursive, irregular or lazy work, where nets are strongest.

## GPU kernels

Kernels for Metal today, and for Vulkan, CUDA and ROCm in the stages after 0.4, are emitted from fused array operations at the array level: one source for every backend, already typed and fused, with flat layouts.

## Moving the compiler onto it

* **One function at a time:** a function the IR can lower goes through it; anything else uses today's code generator, until every construct is covered. Each step stays releasable.
* **The tests are the gate:** every golden test, the WASI and net fixtures, and every benchmark within ±3% (and faster where the IR's analyses apply), on macOS, Linux and Windows.
* **At the end of 0.4.1** today's code generator is removed: `fuse.rs`, `forkjoin.rs`, `simd.rs`, `owned.rs` and the codegen in `lib.rs` become passes or emission over the IR.

## Progress

* **Step 1 (0.4.1-dev):** the `mimic-ir` crate: types, values and regions, a printer and a verifier, and lowering for functions of plain numbers and truth values (literals, parameters, `let`, operators, `and`/`or` as `if`, `if`, calls). The code generator emits such a function's body from its IR, with its own operators and calls, so results do not change; anything else, and any function where calls run in parallel, takes the old path. `MIMIC_IR=1` turns it on (off by default for now), `MIMIC_IR=print` also prints each function's IR, and CI runs every fixture through it on Linux.
* **Next:** records and tuples, `match`, `evolve` and `settle` as loops, lists and the array operations; then inlining, lambdas compiled per use and constants built once, and the IR on by default.

## Risks

* **Size:** today's code generator is over 10,000 lines; the move is weeks of work. Moving one function at a time keeps value arriving early.
* **Regressions:** each special case today encodes a lesson; each must reappear as a general pass or a test. The 0.4.0 corpus and the golden tests are the gate.
* **Compile time:** more passes cost time; the IR caches per function, and incremental compilation (0.12.3) builds on that.
