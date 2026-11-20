// fenwick: a Fenwick tree over 1,048,576 counters: 1,000,000 updates (add
// 1 to 1,000 at a position, from splitmix64), then 2,000,000 prefix sums.
// Prints the sum of the prefix sums and the total.

const SIZE: usize = 1048576;

fn mix(v: i64) -> i64 {
    let mut x = (v as u64 + 1).wrapping_mul(0x9E3779B97F4A7C15);
    x = (x ^ x >> 30).wrapping_mul(0xBF58476D1CE4E5B9);
    x = (x ^ x >> 27).wrapping_mul(0x94D049BB133111EB);
    ((x ^ x >> 31) >> 1) as i64
}

fn prefix(t: &[i64], mut i: usize) -> i64 {
    let mut s = 0;
    while i > 0 {
        s += t[i];
        i -= i & i.wrapping_neg();
    }
    s
}

fn main() {
    let mut t = vec![0i64; SIZE + 1];
    for k in 0..1_000_000i64 {
        let d = 1 + mix(2 * k + 1) % 1000;
        let mut i = 1 + (mix(2 * k) % SIZE as i64) as usize;
        while i <= SIZE {
            t[i] += d;
            i += i & i.wrapping_neg();
        }
    }
    let total: i64 = (0..2_000_000i64).map(|q| prefix(&t, 1 + (mix(3_000_000 + q) % SIZE as i64) as usize)).sum();
    println!("{} {}", total, prefix(&t, SIZE));
}
