// bsearch: 1,000,000 sorted keys (from splitmix64) and 4,000,000 lookups,
// each the lower bound by binary search (every 16th asks for a key that is
// there). Prints the sum of the positions and how many lookups found their
// key.

const N: usize = 1_000_000;
const M: i64 = 4_000_000;

fn mix(v: i64) -> i64 {
    let mut x = (v as u64 + 1).wrapping_mul(0x9E3779B97F4A7C15);
    x = (x ^ x >> 30).wrapping_mul(0xBF58476D1CE4E5B9);
    x = (x ^ x >> 27).wrapping_mul(0x94D049BB133111EB);
    ((x ^ x >> 31) >> 1) as i64
}

// the first position whose key is not below q
fn lower(keys: &[i64], q: i64) -> usize {
    let (mut lo, mut hi) = (0, keys.len());
    while lo < hi {
        let mid = (lo + hi) / 2;
        if keys[mid] < q {
            lo = mid + 1;
        } else {
            hi = mid;
        }
    }
    lo
}

fn main() {
    let mut keys: Vec<i64> = (0..N as i64).map(|k| mix(k) % 1_000_000_000_000).collect();
    keys.sort_unstable();
    let (mut sum, mut found) = (0i64, 0i64);
    for j in 0..M {
        let q = if j % 16 == 0 { keys[(j / 16 * 7919) as usize % N] } else { mix(1_000_000_000 + j) % 1_000_000_000_000 };
        let p = lower(&keys, q);
        sum += p as i64;
        found += (p < N && keys[p] == q) as i64;
    }
    println!("{} {}", sum, found);
}
