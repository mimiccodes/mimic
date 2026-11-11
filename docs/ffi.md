# Foreign function interface

Mimic's dtypes *are* C's dtypes, so calling C needs no wrapper types and passing arrays needs no copies.

## Type mapping

| Mimic | C | Notes |
|---|---|---|
| `i8 i16 i32 i64` | `int8_t … int64_t` | |
| `u8 u16 u32 u64` | `uint8_t … uint64_t` | |
| `isize usize` | `intptr_t size_t` | |
| `f32 f64` | `float double` | |
| `bool` | `bool` | |
| `c_char` | `char` | |
| `cstr` | `const char *` | Mimic strings are always NUL-terminated, so they are passed without a copy. |
| `cptr` | any other pointer: `sqlite3 *`, `FILE *`, `void *` | An opaque C handle. Mimic keeps it as its address and gives it back to C unchanged. It is only meaningful to `extern` functions. |
| `ptr[T]` | `T *` | Points at an array's buffer. Argument position only in M1. |
| `struct` with C layout | `struct` | Planned for M4. |

## Importing C functions (M1)

```mimic
module main

// Functions known to be memory safe can be marked `trusted`.
trusted extern "C" {
    fn sqrt(x: f64) -> f64
}

extern "C" {
    pub fn strlen(s: cstr) -> usize
    fn memset(p: ptr[u8], v: i32, n: usize)
}

fn main() {
    print(sqrt(2.0))                    // trusted: callable anywhere
    print(unsafe { strlen("mimic") })   // untrusted: requires unsafe
}
```

* Calls into an untrusted `extern` block must sit inside `unsafe { … }`. This is the **only** use of `unsafe` in Mimic.
* Link libraries through `mim.mim`: `ffi { link = ["portaudio"] }`.
* Extern functions are items like any other. They are private by default and can be made `pub` and re-used by path (`audio::pa_open`).
* An array passed as `ptr[T]` is borrowed for the duration of the call. The caller holds a reference, so the buffer cannot be freed underneath C.
  * C must not keep the pointer after the call returns.
  * C must not write through it unless the array is uniquely owned (`mut` and not shared).

## Importing C headers directly

No binding code is needed: name the header, and every C function it makes visible becomes an extern declaration. This works the same way C's `#include` does.

```mimic
trusted extern "C" include "math.h"      // the whole header, trusted

extern "C" {
    include "sqlite3.h"                  // inside a block (untrusted: calls need unsafe)
    fn my_extra(x: i32) -> i32           // hand-written declarations still work
}

pub extern "C" include "mylib.h"         // `pub`: the imported functions are public
```

How the import works:

* **Which Clang and which target.** The header is read by the Clang inside `mimic`, for **the target being built**. So `mimic build --target aarch64-linux` sees musl's headers, `--target wasm32-wasi` sees wasi-libc's, and native macOS builds see the macOS SDK's.
* **Where headers are found.** First next to the source file, then in the project root and in `ffi { include = ["include", "vendor/sqlite"] }` in `mim.mim`, then among the target's system headers.
* **How C types map.** Types are mapped through the target's real data model. For example, `long` is 64-bit on Linux and macOS but 32-bit on wasm32 and Windows, and `size_t` is `usize`.

  | C | Mimic |
  |---|---|
  | `char *`, `const char *` | `cstr` |
  | Returned `const unsigned char *` | `cstr` |
  | Pointers to numbers, as arguments | `ptr[T]` |
  | `T **` out-parameters | `ptr[cptr]`: pass a one-element array, then read element 0 |
  | Every other pointer | `cptr` (opaque handle) |
  | `enum` | `i32` |
  | Typedefs | Resolved to what they stand for |

* **Name clashes.** Names your module already defines win over imported ones: your functions, types, modules and hand-written externs. Each name is imported once, even if several headers declare it.
* **What is skipped, for now.** These functions are left out. They are not an error, and the reason is recorded:

  | Skipped | Reason |
  |---|---|
  | Functions that take structs or unions by value | B2 |
  | Varargs (`printf`) | B3 |
  | Function pointers | B4 |
  | `long double` | No Mimic type |
  | `static inline` functions | No symbol to link |
  | Names starting with an uppercase letter | Mimic reserves those for types and variants |
  | Reserved `_names` | |

* **Caching.** Results are cached per header, target and modification time in `~/.mimic/cache/cimport`, so only the first build pays the roughly 1 s that Clang takes.
* **Errors.** A header that cannot be read is error E0805, with Clang's message.

### Example: SQLite with no binding code

```mimic
extern "C" include "sqlite3.h"            // mim.mim: ffi { link = ["sqlite3"] }

fn main() {
    let mut db_out = [0]                  // sqlite3 **: C writes the handle here
    unsafe { sqlite3_open(":memory:", db_out) }
    let db = db_out[0]                    // a cptr
    let mut stmt_out = [0]
    let mut tail = [0]
    unsafe { sqlite3_prepare_v2(db, "select 'mimic', 40 + 2", ¯1, stmt_out, tail) }
    let stmt = stmt_out[0]
    unsafe { sqlite3_step(stmt) }
    print(unsafe { sqlite3_column_text(stmt, 0) })   // mimic
    print(unsafe { sqlite3_column_int64(stmt, 1) })  // 42
}
```

Out-parameters (`T **`) use 64-bit array elements. They work on every 64-bit target, and on little-endian 32-bit targets.

## Planned work (Phase B of the roadmap)

* **B2: structs and unions** with exact C layout, passed by value and by pointer. C wrappers are generated by the embedded Clang, so every target's calling convention is handled by Clang itself.
* **B3: varargs** (`printf`-style).
* **B4: callbacks.** A Mimic function is passed as a C function pointer through a generated trampoline, including closures with a context pointer.
* **B5 (rest):** C memory adopted by Mimic (arrays over C buffers), reading through pointers, and stricter `unsafe` rules for `cptr`.
* **B6: exporting Mimic to C.** `pub extern fn mm_sum(xs: ptr[f64], n: usize) -> f64 { … }` plus `mimic build --emit-header`, which writes `target/<profile>/<name>.h`.
* **B7: C sources in a project.** `c_sources = ["vendor/sqlite3.c"]` in `build.mim` is compiled by the embedded Clang.
* **B8: ABI conformance tests** on every stable target.
