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
* **Step 2:** values kept in registers: short lists (1 … 16 numbers or truth values) and tuples and records of numbers, truth values and such lists (64 numbers at most, the code generator's rule). They are made (`(a, b)`, `[x y z]`, `R { … }`), read (`.field`, `.0`, `xs[2]`), changed (`{ r with … }` of a variable) and taken apart (`let (a, b) = …`), and passed to and returned from functions. Operators stay on single values: on lists they work element by element, which comes with the array operations.
* **Step 3:** `evolve` and `settle` as loops: one structured operation with the state as a value the loop defines (a number or a value kept in registers), the step's and `done`'s lambdas as its regions (what they use from outside is simply in scope), `done` asked before each step and a negative count stopping the program, as before. Loops nest.
* **`match`** on numbers, truth values and values kept in registers: literals (negative numbers, floats, truth values), alternatives that bind nothing, ranges, tuple and record patterns, `as`; each arm's test computed without effects, then the arm or the next as `if` regions. Guards, and patterns on lists, variants and text, take the old path; so does a `match` whose last arm may not match.
* **Step 4a:** lists read: a list of i64, f64 or bytes passed in (a parameter, or one handed on) is an IR value; its length and its elements (one bounds check, the same message) are IR operations. The IR makes and frees no list yet: a call giving a list keeps the old path. A binary search over a list (`lower` in `topo`, `bfs`, `pagerank`) goes through the IR.
* **Step 4b:** lists the IR owns: a call giving a list hands it over, and an `if` yielding lists yields an owned one (a borrowed branch retained); an owned list is released at the end of the region that made it, unless that region yields it (it moves out). Lists passed in stay borrowed. A list as a loop's state keeps the old path. This is the first reference counting in the IR, deliberately simple (released at the region's end, not at the last use: that is 0.4.2's analysis).
* **Embedding:** an expression the IR does not cover yet (a map, a reduction, a call of the runtime, `print` …) no longer keeps its whole function off the IR: it becomes one `embed` operation, compiled by the old path where it stands, the variables it reads given as the IR's values (lent, in slots of their own). Each kind of expression then moves into the IR as an operation of its own. A list the IR owns is lent to a call (a callee that takes it over gets its own reference), and released by the IR as before. `MIMIC_IR_ONLY=text` sends only the functions whose path holds `text` through the IR, to find where a difference comes from.
* **Sinking:** an instruction that cannot fail and has no effect, which only one branch of a later `if` uses, moves into that branch (the old path compiles a `let` where it is used; LLVM may otherwise turn the branch into selects and compute both sides: pip was twice as slow). `and` and `or` are emitted as a branch past the right side, as the old path does. A function with a `let` of a list the old path fuses into later array expressions keeps the old path until the IR fuses (sum_of_squares would otherwise build a list of a hundred million numbers).
* **On by default:** with embedding, most functions go through the IR, with the same output for every fixture and the same speed (a dozen benchmarks within ±2% on one core); the IR is on by default, `MIMIC_IR=0` turns it off, and CI runs every fixture both ways while the old path remains.
* **Next:** the array operations (map, reduce, scan, filter) and fusion as IR passes, replacing their embedding (a `let` of a list used only inside array expressions is fused by the old path and built by an embedding: to settle before the IR is on by default); then inlining, lambdas compiled per use and constants built once, and the IR on by default.

## Risks

* **Size:** today's code generator is over 10,000 lines; the move is weeks of work. Moving one function at a time keeps value arriving early.
* **Regressions:** each special case today encodes a lesson; each must reappear as a general pass or a test. The 0.4.0 corpus and the golden tests are the gate.
* **Compile time:** more passes cost time; the IR caches per function, and incremental compilation (0.12.3) builds on that.
