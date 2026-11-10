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

## Planned work

* **M2: arrays across the boundary.** An array is passed as `(ptr[T], usize)` and pinned (rc+1) for the duration of the call. Pinning becomes significant once the parallel net may release handles concurrently.
* **M4: exporting Mimic to C.**
  ```mimic
  pub extern fn mm_sum(xs: ptr[f64], n: usize) -> f64 { … }
  ```
  `mimic build --emit-header` writes `target/<profile>/<name>.h`. Together with `lib<name>.a`, this makes Mimic libraries usable from C, Zig, Rust and any other language that can call C.
* **M4: `mimic ffi bind header.h`.** This reads a C header through libclang and generates an `extern "C"` block, mapping `char*` to `cstr`, `T*` to `ptr[T]`, and structs.
* **M4: callbacks.** A Mimic function is passed as a C function pointer through a generated trampoline, which in the net becomes a `REF` node.
* **M4: structs and unions** declared with `#[repr(C)]`-equivalent layout.
