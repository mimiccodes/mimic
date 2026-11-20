// lcs: the longest common subsequence of two sequences of 20,000 letters
// from a 4-letter alphabet (from splitmix64), by the row-by-row table.
// Prints the length and the sum of the last row.

const N: usize = 20000;
const M: usize = 20000;

fn mix(v: i64) -> i64 {
    let mut x = (v as u64 + 1).wrapping_mul(0x9E3779B97F4A7C15);
    x = (x ^ x >> 30).wrapping_mul(0xBF58476D1CE4E5B9);
    x = (x ^ x >> 27).wrapping_mul(0x94D049BB133111EB);
    ((x ^ x >> 31) >> 1) as i64
}

fn main() {
    let a: Vec<i64> = (0..N as i64).map(|i| mix(i) % 4).collect();
    let b: Vec<i64> = (0..M as i64).map(|j| mix(1_000_000 + j) % 4).collect();
    let mut prev = vec![0i64; M + 1];
    let mut row = vec![0i64; M + 1];
    for i in 0..N {
        for j in 1..=M {
            row[j] = if a[i] == b[j - 1] { prev[j - 1] + 1 } else { prev[j].max(row[j - 1]) };
        }
        std::mem::swap(&mut prev, &mut row);
    }
    println!("{} {}", prev[M], prev.iter().sum::<i64>());
}
