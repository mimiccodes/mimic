# The interaction-net layer

Mimic's parallel runtime reduces **interaction nets**. A net is a graph of nodes connected port to port. Computation is the rewriting of *active pairs*: two nodes whose principal ports face each other.

Every rewrite touches only its own pair, and the rewrite system is confluent, so any number of threads can reduce different pairs at the same time with no locks on program state. A program does the same total work, and gets the same result, on 1 thread or 100. The interaction count is identical across thread counts.

The implementation is `runtime/mimic_net.c`, which is compiled into the same unit as the array runtime. The IR, parser and encoder are in `crates/mimic-net`.

## Nodes

| Text | Node | Rule summary |
|---|---|---|
| `x` | wire | each name connects exactly two ports |
| `*` | ERA | erases what it meets: frees boxes and releases arrays |
| `#5` `#-2` `#2.5` `#true` | NUM | integers within ±2⁵⁷ and bools are **unboxed in the port**; floats and larger ints are boxed in net memory, linear, with no refcount |
| (array) | ARR | holds a `mim_val*`; DUP retains it, ERA releases it |
| `@name` | REF | a global definition, **expanded only when it meets a node that needs it** |
| `(a b)` | CON | function or pair: (argument, result) |
| `{a b}`, `{3 a b}` | DUP | duplicator, with an optional label |
| `$op(b r)` | OPR | `principal ⊕ b → r`. Scalars are computed inline; arrays go to the parallel kernels. |
| `?(sel r)` | SWI | non-zero → `sel ~ (r *)`, zero → `sel ~ (* r)`: the untaken branch is erased **unevaluated** |

**Operators:** `+ - * / % == != < <= > >= and or neg not`, plus the array primitives by their word names: `iota shape reverse sum product maximum minimum len take drop keep`.

### Interaction rules

| Pair | Rule |
|---|---|
| CON~CON, or DUP~DUP with the same label | **annihilate:** connect the auxiliary ports pairwise |
| CON~DUP, DUP~DUP with different labels, or any other pair of binary nodes | **commute:** 4 new nodes |
| ERA~node | **erase:** the eraser propagates to both auxiliary ports |
| NUM/ARR~DUP | **copy** |
| REF~DUP | **copy the reference**, so recursion stays lazy |
| REF~other | **call:** expand the definition |
| NUM/ARR~OPR | **operate**, or wait for the other operand |
| NUM~SWI | **switch** |
| two nullary nodes | **void** |

## Text format (`.mnet`)

```text
// fib(n) = n < 2 ? n : fib(n-1) + fib(n-2)
@fib    = ({n0 n1} r) & n0 ~ $<(#2 c) & c ~ ?((@fib_lt @fib_ge) (n1 r))
@fib_lt = (n n)
@fib_ge = ({a b} r)
        & a ~ $-(#1 a1) & @fib ~ (a1 ra)    // these two calls are independent
        & b ~ $-(#2 b1) & @fib ~ (b1 rb)    // redexes: they run in parallel
        & ra ~ $+(rb r)
@main   = r & @fib ~ (#20 r)
```

A definition is written `@name = root & a ~ b & …`, where each `&` adds an active pair. The interface of a function is `(arg1 (arg2 … result))`.

```sh
mimic net run tests/nets/fib.mnet --stats --threads 12
mimic net fmt file.mnet
```

## Runtime design

* **Ports** are 64 bits: a 4-bit tag plus a 60-bit payload. A binary node's payload is a 32-bit location plus a 28-bit aux field, which holds the DUP label or the OPR operator.
* **Linking** is lock-free and follows HVM2. The two ends of a wire race to an atomic exchange on the wire's slot, and whichever arrives second connects the two sides.
* **Scheduling:**
  * Each worker reduces from a *private stack* with no synchronisation.
  * Only while another worker is idle does it move the *oldest* half of that stack (the biggest pending subproblems) into a shared bag. Idle workers steal from those bags.
  * A run is finished when every participating worker is idle.
* **Memory:**
  * Nodes and wires live in two global tables, sized by `MIMIC_NET_NODES` (default 16M) and committed lazily.
  * Each thread allocates from private chunks and keeps private free lists.
* **Leak check:** debug builds verify at exit that no net node (and no array) is still alive.
* **Verification:**
  * ThreadSanitizer reports no races on the recursion, higher-order and array nets.
  * The test suite runs every net sequentially, on 8 threads, and optimised.
  * It also runs repeated parallel stress runs.

### Performance (M3 Pro, 6P+6E cores, under load)

fib(32), which takes 81M interactions:

| Threads | Time | M interactions/s |
|---|---|---|
| 1 | 9.0 s | 9.0 |
| 4 | 3.0 s | 27.3 |
| 12 | 1.7 s | 47.7 |

Single-thread throughput still has room to improve. The known costs are the atomic exchange on every wire and the per-call expansion of small definitions. A later phase will add HVM-style optimisations: eager pre-reduction of definitions, and skipping wire allocation for direct links.

## Phase 3: compiling Mimic to nets

* Pure functions, as decided by `mimic_check::Purity`, compile to definitions. Everything else stays in LLVM code, which calls `mim_net_call`.
* `if` compiles to SWI, so only the taken branch is ever expanded.
* Independent subexpressions become independent redexes. This is what makes "parallel by default" cover ordinary code, not only array primitives.
