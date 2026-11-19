// bloom: a Bloom filter of 8,388,608 one-byte cells and 7 hashes per key
// (double hashing from splitmix64): keys 0 to 999,999 go in, then keys 0 to
// 1,999,999 are looked up. Prints how many lookups say "maybe" and how
// many cells are set.

const CELLS: usize = 8388608;

fn mix(v: i64) -> i64 {
    let mut x = (v as u64 + 1).wrapping_mul(0x9E3779B97F4A7C15);
    x = (x ^ x >> 30).wrapping_mul(0xBF58476D1CE4E5B9);
    x = (x ^ x >> 27).wrapping_mul(0x94D049BB133111EB);
    ((x ^ x >> 31) >> 1) as i64
}

fn cell(x: i64, j: i64) -> usize {
    let (h1, h2) = (mix(2 * x) as u64, (mix(2 * x + 1) | 1) as u64);
    (h1.wrapping_add((j as u64).wrapping_mul(h2)) as usize) & (CELLS - 1)
}

fn main() {
    let n = 1_000_000i64;
    let mut f = vec![0u8; CELLS];
    for x in 0..n {
        for j in 0..7 {
            f[cell(x, j)] = 1;
        }
    }
    let maybe = (0..2 * n).filter(|&x| (0..7).all(|j| f[cell(x, j)] == 1)).count();
    let set: usize = f.iter().map(|&b| b as usize).sum();
    println!("{} {}", maybe, set);
}
