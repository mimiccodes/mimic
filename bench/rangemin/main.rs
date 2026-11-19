// rangemin: range-minimum queries over 1,000,000 values (from splitmix64)
// with a sparse table (level k: the minimum of each run of 2^k values),
// 4,000,000 queries. Prints the sum of the answers and the sum of the
// query lengths.

const N: usize = 1_000_000;
const M: i64 = 4_000_000;

fn mix(v: i64) -> i64 {
    let mut x = (v as u64 + 1).wrapping_mul(0x9E3779B97F4A7C15);
    x = (x ^ x >> 30).wrapping_mul(0xBF58476D1CE4E5B9);
    x = (x ^ x >> 27).wrapping_mul(0x94D049BB133111EB);
    ((x ^ x >> 31) >> 1) as i64
}

fn log2(x: usize) -> usize {
    let mut k = 0;
    while 2usize << k <= x {
        k += 1;
    }
    k
}

fn main() {
    let levels = log2(N) + 1;
    let mut t = vec![0i64; N * levels];
    for i in 0..N {
        t[i] = mix(i as i64) % 1_000_000_000;
    }
    for k in 1..levels {
        let h = 1usize << (k - 1);
        for i in 0..N {
            t[k * N + i] = t[(k - 1) * N + i].min(t[(k - 1) * N + (i + h).min(N - 1)]);
        }
    }
    let (mut sum, mut lengths) = (0i64, 0i64);
    for q in 0..M {
        let (u, v) = ((mix(M + 2 * q) % N as i64) as usize, (mix(M + 2 * q + 1) % N as i64) as usize);
        let (a, b) = (u.min(v), u.max(v));
        let k = log2(b - a + 1);
        sum += t[k * N + a].min(t[k * N + b + 1 - (1 << k)]);
        lengths += (b - a + 1) as i64;
    }
    println!("{} {}", sum, lengths);
}
