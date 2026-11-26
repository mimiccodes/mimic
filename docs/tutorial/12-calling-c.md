---
title: Calling C
description: Use any C library — declare functions, import headers directly, pass arrays, structs and callbacks, and export Mimic functions to C.
order: 12
---

# Calling C

Mimic's number types are C's number types, so calling C needs no wrappers and arrays cross the boundary without copying.

Everything that comes from C lives in the namespace `c::`: you write `c::sqrt(2.0)`, `c::printf(…)`, `c::Vec2 { … }`. So C code is always easy to spot, and C names never collide with Mimic's own: `c::abs` and Mimic's `abs`, `c::div` and the `div` operator, all work side by side.

## Declaring C functions

```mimic run
module main

// functions you know are memory safe can be `trusted`
trusted extern "C" {
    fn sqrt(x: f64) -> f64
    fn abs(x: i32) -> i32
}

extern "C" {
    fn strlen(s: cstr) -> usize
}

fn main() {
    print(c::sqrt(2.0))
    print(c::abs(-7))
    print(unsafe { c::strlen("mimic") })   // untrusted: needs `unsafe`
}
```

```output
1.4142135623730951
7
5
```

Calls into an untrusted `extern` block must sit inside `unsafe { … }`. That is the **only** use of `unsafe` in Mimic.

## Importing a header

You don't have to copy declarations by hand. Name the header and every function in it becomes available:

```mimic run
module main

trusted extern "C" include "math.h"

fn main() {
    print(c::floor(2.7))
    print(c::pow(2.0, 10.0))
    print(c::hypot(3.0, 4.0))
}
```

```output
2.0
1024.0
5.0
```

How the import works:
* **Which headers are read.** Headers are read for the target you are building for. A Linux build sees Linux's headers, a WebAssembly build sees WASI's.
* **Your own headers.** Put them next to your source, or list their folders in `mim.mim` under `ffi { include = [...] }`.
* **Your own C code.** List C files under `ffi { sources = [...] }`. They are compiled into your program for every target.
* **Libraries to link.** Name them under `ffi { link = ["sqlite3"] }`.
* **Every name keeps its C spelling.** After `c::` and `.` any name works, even ones Mimic uses itself: `c::div(17, 5)` from `stdlib.h`, a struct field `t.type`.
* **Another namespace.** `include "sqlite3.h" as sql` puts the header's names under `sql::` instead: `sql::sqlite3_open(…)`.
* **Constants and globals.** `#define` constants and `enum` values come too (`c::SEEK_END`, `c::M_PI`), and so do globals such as `c::stdout` and `c::errno`, read when used.

## Type mapping

| Mimic | C |
|---|---|
| `i8` … `i64`, `u8` … `u64` | `int8_t` … `uint64_t` |
| `isize`, `usize` | `intptr_t`, `size_t` |
| `f32`, `f64` | `float`, `double` |
| `bool`, `c_char` | `bool`, `char` |
| `cstr` | `const char *`. Mimic strings are always zero-terminated, so no copy is made. |
| `ptr[T]` | `T *`, pointing straight at a Mimic array's elements |
| `cptr` | any other pointer: an opaque handle such as `sqlite3 *` or `FILE *` |

## Structs, varargs and callbacks

When the header is imported with `trusted`:
* **Structs by value.** A C `struct vec2` becomes a Mimic record `c::Vec2`, and you pass and receive it as one. Array fields are lists. `c::Vec2 {}` is all zeros, like `= {0}` in C, and you can set only some fields.
* **Structs through pointers.** A plain struct passed by pointer is passed as a record. If C may change it, the function gives it back too: `let (rc, t) = c::tcgetattr(0, c::Termios {})`.
* **Varargs.** Variadic functions like `printf` work. Pass integers to `%lld`, because Mimic integers are 64-bit.
* **Callbacks.** A C parameter that takes a function pointer accepts any Mimic function: a named function, a lambda or a closure. C must call it before the call returns; C may not keep it for later.

```mimic
trusted extern "C" include "integrate.h"

c::integrate(x => x * x, 0.0, 3.0, 1000)      // pass a lambda to C
let k = 10
c::apply_twice(x => x + k, 1)                 // a closure works too
```

## Telling Mimic what C functions do

Mimic decides by itself what can run in parallel, but it cannot look inside C. Unless told otherwise, it assumes a C function might change anything, and runs every call strictly in order. That is always safe, but it can be slow. One word after `extern "C"` says more:

```mimic run
module main

trusted extern "C" pure {                // depends only on its arguments
    fn sqrt(x: f64) -> f64
}

trusted extern "C" concurrent {          // waits, but changes nothing others read
    fn usleep(us: u32) -> i32
}

fn nap(ms: i64) -> i64 {
    c::usleep(ms * 1000)
    ms
}

fn main() {
    print([1.0 4.0 9.0] ¨ (x => c::sqrt(x)))   // pure: in parallel
    let start = time::clock()
    let a = nap(300)                         // concurrent: both naps overlap
    let b = nap(300)
    print(a + b)
    print(time::clock() - start < 600ms)           // overlapped: one after the other takes 600ms
}
```

```output
[1.0 2.0 3.0]
600
true
```

| Word | Use it for |
|---|---|
| `pure` | Math and other functions whose result depends only on their arguments |
| `concurrent` | Reads, database queries, network requests, waiting |
| `ordered` | Logging and printing: in order, but nothing else depends on it |
| `sync` | Anything that changes shared state. This is the default. |

The word is a promise, like `trusted`. If a function marked `concurrent` secretly writes files, runs may race. Leave it out when unsure. The full rules are in [the FFI reference](../ffi.md#what-c-functions-do-effects).

## Calling Mimic from C

Mark a function `extern "C"` and it is exported with C types and the C calling convention. `--emit-header` writes the matching `.h` file:

```mimic
extern "C" fn mean(xs: ptr[f64], n: usize) -> f64 { … }
```

```sh
mimic build --emit-header      # → target/release/<name>.h
```

`--shared` builds a shared library instead of a static one (`lib<name>.dylib`, `lib<name>.so`, or `<name>.dll` on Windows), exporting only the `extern "C"` functions, to load from C, Python, C# or anything else that calls C.

## Compiling C with mimic

`mimic` contains a complete C compiler, which also cross-compiles:

```sh
mimic cc hello.c -o hello
mimic cc --target=aarch64-linux-musl hello.c -o hello-arm   # a static Linux ARM binary
```

The full details are in the [FFI reference](../ffi.md).

Next: [building for other platforms](13-targets.md).
