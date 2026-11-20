// lis: the longest strictly increasing subsequence of 4,000,000 values
// (from splitmix64) by patience sorting: each value replaces the first tail
// not below it (binary search) or extends the tails. Prints the length and
// the sum of the tails.

fn mix(v: i64) -> i64 {
    let mut x = (v as u64 + 1).wrapping_mul(0x9E3779B97F4A7C15);
    x = (x ^ x >> 30).wrapping_mul(0xBF58476D1CE4E5B9);
    x = (x ^ x >> 27).wrapping_mul(0x94D049BB133111EB);
    ((x ^ x >> 31) >> 1) as i64
}

fn main() {
    let mut tails: Vec<i64> = Vec::new();
    for k in 0..4_000_000i64 {
        let v = mix(k) % 1_000_000_000;
        let (mut lo, mut hi) = (0, tails.len());
        while lo < hi {
            let mid = (lo + hi) / 2;
            if tails[mid] < v {
                lo = mid + 1;
            } else {
                hi = mid;
            }
        }
        if lo == tails.len() {
            tails.push(v);
        } else {
            tails[lo] = v;
        }
    }
    println!("{} {}", tails.len(), tails.iter().sum::<i64>());
}
