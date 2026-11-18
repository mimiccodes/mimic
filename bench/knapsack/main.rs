// knapsack: the best value from 2,000 items (weights 1 to 1,000, values 1
// to 10,000, from splitmix64) in a knapsack of capacity 100,000, each item
// at most once, by the usual table, one row per item (kept in one array,
// filled from the top down). Prints the best value and the best at half
// the capacity.

fn mix(v: i64) -> i64 {
    let mut x = (v as u64 + 1).wrapping_mul(0x9E3779B97F4A7C15);
    x = (x ^ x >> 30).wrapping_mul(0xBF58476D1CE4E5B9);
    x = (x ^ x >> 27).wrapping_mul(0x94D049BB133111EB);
    ((x ^ x >> 31) >> 1) as i64
}

fn main() {
    let (n, cap) = (2000i64, 100000usize);
    let mut best = vec![0i64; cap + 1];
    for i in 0..n {
        let (w, v) = (1 + (mix(2 * i) % 1000) as usize, 1 + mix(2 * i + 1) % 10000);
        for c in (w..=cap).rev() {
            best[c] = best[c].max(best[c - w] + v);
        }
    }
    println!("{} {}", best[cap], best[cap / 2]);
}
