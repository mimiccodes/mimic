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
  | `struct vec2` by value (in a `trusted` import) | the record `Vec2`, see below |

* **Name clashes.** Names your module already defines win over imported ones: your functions, types, modules and hand-written externs. Each name is imported once, even if several headers declare it.
* **What is skipped, for now.** These functions are left out. They are not an error, and the reason is recorded:

  | Skipped | Reason |
  |---|---|
  | Functions that take unions by value, or structs with array, union or function-pointer fields | Rest of B2 |
  | Function pointers | B4 |
  | `long double` | No Mimic type |
  | `static inline` functions | No symbol to link |
  | Names starting with an uppercase letter | Mimic reserves those for types and variants |
  | Reserved `_names` | |

* **Caching.** Results are cached per header, target and modification time in `~/.mimic/cache/cimport`, so only the first build pays the roughly 1 s that Clang takes.
* **Errors.** A header that cannot be read is error E0805, with Clang's message.

### Varargs

Variadic C functions (`printf`, `snprintf`, `open`) are imported, and can be declared by hand with `...`:

```mimic
extern "C" {
    fn snprintf(buf: ptr[u8], n: usize, fmt: cstr, ...) -> i32
}

unsafe { printf("%s has %lld cores, pi ~ %.3f\n", "mimic", 12, 3.14159) }
```

The extra arguments get C's default promotions:

| Mimic | Passed to C as | Format |
|---|---|---|
| Integers (64-bit) | `long long` | `%lld` |
| `f64` | `double` | `%f` |
| `bool` | `int` | `%d` |
| `str` | `char *` | `%s` |
| Arrays | a pointer to their data | |

As in C, the format must match the argument. `%d` for a Mimic integer may *appear* to work on arm64 and x86-64, where variadic arguments travel in registers, but it reads the wrong bytes on wasm32 and on other platforms. Use `%lld`.

### Structs by value

In a `trusted` import, every C struct that functions pass or return by value becomes a Mimic record, and those functions take and return records:

```c
// geo.h
struct vec2 { double x, y; };
typedef struct { int w; int h; } size2;
typedef struct rect { struct vec2 origin; size2 size; unsigned char flags; } rect_t;
struct vec2 vadd(struct vec2 a, struct vec2 b);
rect_t mkrect(double x, double y, int w, int h);
```

```mimic
trusted extern "C" include "geo.h"

fn main() {
    print(vadd(Vec2 { x: 3.0, y: 4.0 }, Vec2 { x: 1.0, y: 1.0 }))  // Vec2 { x: 4.0, y: 5.0 }
    let r = mkrect(1.5, 2.5, 640, 480)
    print(r.size.w * r.size.h)                                    // 307200
}
```

**How the names work:**
* Struct names become Mimic type names: `vec2` → `Vec2`, `size2` → `Size2`, `rect` → `Rect`.
* Fields keep their C names.
* Integer fields are `i64`, floating-point fields `f64`, `_Bool` fields `bool`, and pointer fields hold a `cptr` address.

**How a call works:**
* `mimic` generates a small C wrapper for each such function. The wrapper takes and returns plain numbers, and calls the real function with real structs.
* The Clang inside `mimic` compiles the wrapper for the target. So each platform's rules for passing structs (registers, stack, hidden return pointers) are applied by Clang itself.
* A generated Mimic function with the C function's name packs and unpacks the records. It has been tested natively, under WASI, and under Rosetta on x86-64. Linking has been checked for aarch64, x86, riscv64 and arm Linux.

**Why only in `trusted` imports:** the generated Mimic function hides the `unsafe` C call. In an untrusted import, these functions are not generated.

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

## Calling Mimic from C

A function declared `extern "C" fn` is a normal Mimic function that is also exported to C under its plain name, with C types and the C calling convention:

```mimic
module lib

pub extern "C" fn mm_hypot(a: f64, b: f64) -> f64 { sqrt(a * a + b * b) }
pub extern "C" fn mm_sum_to(n: i64) -> i64 { ⍳ n ▷ +/ }          // parallel inside
pub extern "C" fn mm_greet(name: cstr) -> cstr { "hello, {name}" }
```

Build it and write the header:

```sh
mimic build --emit-header        # target/release/libmm.a + target/release/mm.h
```

```c
#include "mm.h"   // double mm_hypot(double a, double b); char * mm_greet(const char * name); …
int main(void) {
    printf("%.1f\n", mm_hypot(3, 4));
    char *g = mm_greet("C"); puts(g); free(g);   // strings returned to C are malloc'd copies
}
```

```sh
mimic cc main.c -Itarget/release target/release/libmm.a -o main
```

**What the exported signature may use:**
* Parameters and the result use C types: integers, `f32`/`f64`, `bool`, `c_char`, `cstr` (a string, copied in) and `cptr`.
* Arrays cannot be received from C yet.

**How C sees it:**
* Mimic's runtime starts itself on the first call, from any thread, so the C program needs no setup. The work inside still runs on all cores.
* The header maps the types to C: `int64_t`, `double`, `bool`, `const char *`, `char *` (returned strings, which the caller frees), and `void *` (for `cptr`).

## C sources in a project

```mimic
// mim.mim
ffi {
    sources = ["c/vec.c", "vendor/sqlite3.c"]
    include = ["c", "vendor"]
    cflags = ["-DSQLITE_OMIT_LOAD_EXTENSION"]
}
```

* **How they are built.** The C files are compiled by the Clang inside `mimic`, for the target and profile being built:
  * `-O2` for release builds and `-O0` for debug builds, both with debug info;
  * the project root and `include` directories are on the include path;
  * then the files are linked into the program, or added to `lib<name>.a` for libraries.
* **Cross-compiling.** The same project cross-compiles with no change. The C files are built for the target, against its C library.
* **Rebuilding.** An object is rebuilt when its source is newer, or when the flags or target change.

Together with header import, vendoring a C library is two lines: add its `.c` files to `sources`, then `include "its_header.h"`.

## Planned work (Phase B of the roadmap)

* **B2 (rest):** unions; array fields; reading and writing structs through pointers.
* **B4: callbacks.** A Mimic function is passed as a C function pointer through a generated trampoline, including closures with a context pointer.
* **B5 (rest):** C memory adopted by Mimic (arrays over C buffers), reading through pointers, and stricter `unsafe` rules for `cptr`.
* **B6 (rest):** receiving arrays from C (`ptr[T]` with a length), shared libraries (`.so`/`.dylib`/`.dll`).
* **B8: ABI conformance tests** on every stable target.
