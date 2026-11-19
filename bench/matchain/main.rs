// matchain: the cheapest order to multiply a chain of 600 matrices
// (dimensions from splitmix64) by the interval dynamic programme, by
// increasing length. Prints the cheapest cost of the whole chain and the
// sum of the cheapest costs of all sub-chains.

const N: usize = 600;

fn mix(v: i64) -> i64 {
    let mut x = (v as u64 + 1).wrapping_mul(0x9E3779B97F4A7C15);
    x = (x ^ x >> 30).wrapping_mul(0xBF58476D1CE4E5B9);
    x = (x ^ x >> 27).wrapping_mul(0x94D049BB133111EB);
    ((x ^ x >> 31) >> 1) as i64
}

fn main() {
    let d: Vec<i64> = (0..=N as i64).map(|i| 10 + mix(i) % 90).collect();
    let mut cost = vec![0i64; N * N];
    for len in 2..=N {
        for i in 0..=N - len {
            let j = i + len - 1;
            cost[i * N + j] = (i..j).map(|k| cost[i * N + k] + cost[(k + 1) * N + j] + d[i] * d[k + 1] * d[j + 1]).min().unwrap();
        }
    }
    println!("{} {}", cost[N - 1], cost.iter().sum::<i64>());
}
