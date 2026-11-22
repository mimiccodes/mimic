# Ownership in the Mimic IR (0.4.2)

Mimic's values are reference counted and immutable to the program: a
change (`{ r with … }`, `put`, `push`) makes a new value. It is fast only
when the change can be made in place, which needs the value to be held
once. Until 0.4.1 the code generator found such places with syntactic
rules (`owned.rs`): a local moves at its only use, a record parameter that
`{ p with … }` consumes is taken over. Each slow program in
[gaps.md](../gaps.md) for 0.4.2 is a shape those rules miss: a `settle`
step written as a lambda (`huffman`, 37 s instead of 2.1 s), a value read
and then changed (`diff`), a change through a helper or a branch (`topo`,
union-find), a bare list changed by a function that calls itself (a heap's
sift-up), a list grown in a state (`lz77`).

0.4.2 replaces the rules with an analysis over the IR, as Perceus does for
Koka: every heap value has an owner, the compiler places every
reference-count operation, and a value is changed in place wherever
nothing else holds it. No annotation: the language does not change.

## The model

* **Owned and borrowed values.** A heap value (a list, a record on the
  heap, text) is *owned* by the region that made it (a call's result, a
  map, a record built, a change) or *borrowed* (a parameter the function
  does not consume, a field of a borrowed value). Borrowed values cost no
  reference-count operation; owned ones are released once.
* **Last uses.** For each owned value the pass finds its last use on every
  path. A last use that can take the value over *consumes* it: the
  base of `{ r with … }`, the list of `put` and `push`, an argument to a
  parameter the callee consumes, a region's result. A value consumed is
  not released by its region; one never consumed is released right after
  its last use (`drop`), not at the end of the region.
* **Uses inside loops.** A value defined outside a loop's step and read
  inside it is not consumed there (the step runs again); the loop's state
  is consumed by each step and handed to the next.
* **Consumed parameters.** A parameter whose every path consumes it is
  taken owned: callers hand over a reference (their own at their last
  use, else a new one). Found by a fixpoint over the call graph, so it
  goes through helpers: `next(s)` that calls `cross(s)` that changes `s`
  consumes `s` too. This replaces `owned.rs`'s consumed parameters.
* **In place.** A change to a consumed value checks its count (one load,
  one compare) and writes in place when it is one, else copies; with the
  analysis the count is one wherever the program holds the value once.
  Where the analysis proves it (a value made in this function and never
  shared), the check goes.
* **Taking a field.** `{ s with f: put(s.f, i, x) }`, `s` consumed: the
  field's list is taken out of `s` first (its slot cleared), so the list
  is held once and changed in place, then stored back. This generalises
  the code generator's "steal" of a field.
* **Reads before a change.** `let x = s.v[i]` then `{ s with v: put(s.v,
  j, x) }`: a borrowed read ends where its value is last used, so the
  change still finds the list held once (`diff`).

## Order of work

1. The ownership core on what the IR holds now (lists made by calls and
   maps): last uses, `drop` placed after them, consumption by calls, a
   region's result, consumed parameters by fixpoint. Reference-count
   operations counted by the runtime when asked (`MIMIC_COUNT_RC=1`), to
   measure each step.
2. Records on the heap in the IR (built, a field read, `{ r with … }`),
   `put` and `push` of typed lists, list literals: each consuming its
   operand where the analysis says so, in place when held once; taking a
   field.
3. The shapes of the gaps: a `settle`/`evolve` step written as a lambda,
   reads before a change, helpers and branches, a bare list in a function
   that calls itself, a small fixed list as state, `put` without a call.
4. A result built where its caller keeps it (a list grown in a state).
5. `mimic check --copies`: every copy left, where, and why (the analysis's
   own reasons).

**Done when:** the steps of `qoi` and `paths` copy nothing, and the
reference-count operations of the corpus are at least halved (counted
with `MIMIC_COUNT_RC=1`).
