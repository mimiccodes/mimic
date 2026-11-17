---
title: Vocabulary design (stage 0.4)
description: The glyphs, words and digraphs added in 0.4.5 to 0.4.7, what each means, and how each fuses.
---
# Vocabulary design (stage 0.4)

**Status: draft for review.** Nothing here is implemented yet. It is the plan for steps 0.4.5 (the array core), 0.4.6 (numbers) and 0.4.7 (bits, bytes and small vectors) in [versions.md](../versions.md).

## Principles

* **Glyphs for general array operations; modules for domains.** A glyph earns its place when code in every field uses it, as it does `¨` and `⊠`. Graphics, quantum and protocol code get their own modules (`gfx::`, `quantum::`) in stage 0.7, built on these glyphs.
* **Every glyph has a word and a digraph**, as today: glyphs and words mix freely, `mimic fmt` writes either, and `\k` types `⌿`.
* **Every existing glyph, word and digraph stays** with its meaning.
* **Every new primitive fuses and runs in parallel** like `¨`: the compiler understands it, so a chain of them is one loop over the data, split over the cores, with SIMD and (from stage 0.5) on the GPU. That is why they belong in the language and not in a library.
* **Added on the IR** ([ir.md](ir.md)): each primitive is one lowering plus its fusion rules, written once.

## The array core (0.4.5)

Digraphs are case-sensitive (`\M` and `\m` are both taken today); none below clashes with an existing one.

| Glyph | Word | Digraph | Arity | Meaning | Fuses as |
|---|---|---|---|---|---|
| `⍋` | `grade` | `\A` | 1 | the indices that sort a list ascending (stable) | a parallel sort of indices; `xs ⊇ ⍋ xs` is `sort(xs)` |
| `⍒` | `grade_down` | `\D` | 1 | the same, descending | as `⍋` |
| `⍸` | `where` | `\w` | 1 | the indices where a list of truth values is true | the `⌿` machinery: counts per chunk, then a parallel write |
| `∊` | `in` | `\E` | 2 | for each element of the left, whether it occurs in the right | a hash set of the right (or a sorted search), then a map |
| `⍷` | `find` | `\F` | 2 | where the left list occurs as a run in the right (a mask) | a map over positions comparing a window |
| `⊐` | `index_of` | `\I` | 2 | for each element of the right, its first position in the left (or the length) | a hash index of the left, then a map |
| `⊇` | `select` | `\L` | 2 | `xs ⊇ is`: the elements (rows, cells) of `xs` at the indices `is` | a gather inside the consuming loop |
| `⌸` | `group` | `\G` | 2 | `keys ⌸ values`: the values grouped by key, keys in order of first appearance | a parallel hash grouping |
| `⍤` | `rank` | `\n` | 2 | `f ⍤ k`: `f` applied to every cell of rank `k` (rows of a matrix, pixels of an image, matrices of a stack) | nested maps; rank polymorphism for every function |
| `∘.` | `outer` | `\O` | 2 | `xs ∘.f ys`: `f` of every pair, as a matrix (`∘.×` is the multiplication table) | a 2-D map |
| `·` | `inner` | `\N` | 2 | `f · g`: the inner product (`+ · ×` is the matrix product; `⌈ · +` is a shortest-path step) | `+ · ×` lowers to `⊠`; others to a blocked map and reduction |
| `↕` | `windows` | `\W` | 2 | `xs ↕ n`: every run of `n` neighbours, as a view (no copy) | reads at offsets inside the consuming loop |
| `⌺` | `stencil` | `\Z` | 2 | `img ⌺ (3, f)`: `f` of each neighbourhood, edges clamped (or a fill given) | one map reading neighbours; tiled for the cache |
| `«` `»` | `shift_left`, `shift_right` | `\[` `\]` | 2 | shift by one or `n`, filling with a value | reads at an offset, or the fill |
| `⊖` | `rotate` | `\Q` | 2 | rotate along an axis (`⌽` stays: reverse) | reads at a rotated index |
| `⊤` | `encode` | `\B` | 2 | a number as digits in a base or mixed radix (`[24 60 60] ⊤ 3661`) | a map |
| `⊥` | `decode` | `\b` | 2 | digits back to a number (`2 ⊥ bits`) | a reduction |
| `⌹` | `solve` | `\V` | 1, 2 | `⌹ m`: the inverse; `b ⌹ m`: the solution of `m x = b` (least squares when not square) | LAPACK where present, else Mimic's blocked kernel |
| `√` | `sqrt` | `\q` | 1 | the square root (the same as `math::sqrt`) | one instruction |

## Numbers (0.4.6)

* **Complex numbers:** element types `c64` (two `f32`) and `c128` (two `f64`); literals `3j4` and `0.5j¯1`; `math::real`, `math::imag`, `math::abs`, `math::arg`, `math::conj`, `math::cis`. Arithmetic and every glyph work on them; SIMD and GPU kernels handle them as pairs.

| Glyph | Word | Digraph | Arity | Meaning | Fuses as |
|---|---|---|---|---|---|
| `⊗` | `kron` | `\K` | 2 | the tensor (Kronecker) product of two matrices or vectors | a 4-D map, blocked |
| `†` | `adjoint` | `\H` | 1 | the conjugate transpose (the transpose for real numbers) | `⍉` with a map |

* **Libraries on them:** `math::fft` and `math::ifft` (radix 2, 3, 5 and mixed, in parallel), linear algebra (`math::lu`, `math::qr`, `math::svd`, `math::eig`), statistics (`math::mean`, `math::variance`, `math::median`, `math::percentile`, `math::histogram`).

## Bits, bytes and small vectors (0.4.7)

* **Sized and unsigned integers:** `u16`, `u32`, `u64`, `i16` beside today's `u8`, `i32`, `i64` (wrapping arithmetic, as `i32` today).
* **Bit syntax**, for binary data, in patterns and in construction:

  ```text
  match packet {
      <<version: 4, ihl: 4, _: 8, length: 16, rest: bytes>> => …
  }
  let header = <<1: 8, kind: 8, length: 32 big, payload: bytes>>
  ```

  Sizes in bits; byte order `big` (the default, network order) or `little`; `bytes` for the rest. A match compiles to loads and shifts, with no copies.
* **Packed records:** `type Header = packed { magic: u32, flags: u16, size: u64 }`, with exactly that layout in memory (for files, hardware and C).
* **Byte views:** a part of a `[u8]` without copying (`data[4 ‥ 20]` as a view), for parsers and protocols.
* **Swizzles:** `v.x`, `v.xy`, `c.rgb`, `p.zyx` on lists of up to 4 numbers (`x y z w`, `r g b a`). Register moves, no calls.
* **Literals:** colors `#ff8800` and `#ff880080` (as `[r g b a]` numbers), and units `12px`, `50%`, `2em` for UI layout (beside today's durations, `250ms`).

## Open questions

1. **The inner product's glyph.** APL writes `+.×`; `⍟` was suggested first but means the logarithm in APL. Proposed: `·` (`+ · ×`). Alternatives: a word only (`inner(+, ×)`).
2. **The outer product's form.** `∘.f` (APL) reads as one glyph with a function after it; or a word only (`outer(f)`).
3. **`«` and `»`** are small in some fonts; `⇠` and `⇢` are alternatives.
4. **Ket literals** for quantum code (`|0⟩`, `|ψ⟩`): decided with `quantum::` in stage 0.7.
5. **Tacit trains** (`(+/ ÷ ≢)` as an average) and user-defined operators: powerful, but more to learn; proposed for the language-completion stage (0.12).
6. **Which of these also get net rules** (for pure recursive code on interaction nets): at least `⊇`, `⊤`, `⊥`, `√`.
