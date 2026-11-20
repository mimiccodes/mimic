// hashmap: a hash table with open addressing and linear probing, 2,097,152
// slots: 1,000,000 keys (1 + mix(i) % 700,000, from splitmix64) go in, a
// repeat adding i % 100 to its value; then keys 1 + q % 1,400,000 for q
// below 2,000,000 are looked up. Prints the sum of the values found, how
// many lookups found their key, and the slots in use.

const SLOTS: usize = 2097152;

fn mix(v: i64) -> i64 {
    let mut x = (v as u64 + 1).wrapping_mul(0x9E3779B97F4A7C15);
    x = (x ^ x >> 30).wrapping_mul(0xBF58476D1CE4E5B9);
    x = (x ^ x >> 27).wrapping_mul(0x94D049BB133111EB);
    ((x ^ x >> 31) >> 1) as i64
}

fn find(keys: &[i64], k: i64) -> usize {
    let mut s = (mix(k * 31) as usize) & (SLOTS - 1);
    while keys[s] != 0 && keys[s] != k {
        s = (s + 1) & (SLOTS - 1);
    }
    s
}

fn main() {
    let n = 1_000_000i64;
    let (mut keys, mut vals) = (vec![0i64; SLOTS], vec![0i64; SLOTS]);
    for i in 0..n {
        let k = 1 + mix(i) % 700000;
        let s = find(&keys, k);
        keys[s] = k;
        vals[s] += i % 100;
    }
    let (mut total, mut found) = (0i64, 0i64);
    for q in 0..2 * n {
        let k = 1 + q % 1400000;
        let s = find(&keys, k);
        if keys[s] == k {
            total += vals[s];
            found += 1;
        }
    }
    let used = keys.iter().filter(|&&k| k > 0).count();
    println!("{} {} {}", total, found, used);
}
