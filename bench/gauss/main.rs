// gauss: the rank and determinant of a 500 × 500 matrix modulo the prime
// 1,000,000,007 by Gaussian elimination, for a matrix of rank 499 and for
// the same matrix with its last row replaced (from splitmix64). Prints,
// for each matrix, the rank and the determinant.

const N: usize = 500;
const P: i64 = 1_000_000_007;

fn mix(v: i64) -> i64 {
    let mut x = (v as u64 + 1).wrapping_mul(0x9E3779B97F4A7C15);
    x = (x ^ x >> 30).wrapping_mul(0xBF58476D1CE4E5B9);
    x = (x ^ x >> 27).wrapping_mul(0x94D049BB133111EB);
    ((x ^ x >> 31) >> 1) as i64
}

fn power(mut b: i64, mut e: i64) -> i64 {
    let mut r = 1;
    while e > 0 {
        if e & 1 == 1 {
            r = r * b % P;
        }
        b = b * b % P;
        e >>= 1;
    }
    r
}

fn solve(mut a: Vec<i64>) -> (usize, i64) {
    let (mut det, mut rank) = (1i64, 0usize);
    for col in 0..N {
        let Some(piv) = (rank..N).find(|&r| a[r * N + col] != 0) else {
            det = 0;
            continue;
        };
        if piv != rank {
            for j in 0..N {
                a.swap(rank * N + j, piv * N + j);
            }
            det = (P - det) % P;
        }
        let lead = a[rank * N + col];
        let inv = power(lead, P - 2);
        det = det * lead % P;
        for r in rank + 1..N {
            let f = a[r * N + col] * inv % P;
            if f == 0 {
                continue;
            }
            for j in 0..N {
                a[r * N + j] = (a[r * N + j] - f * a[rank * N + j] % P + P) % P;
            }
        }
        rank += 1;
    }
    (rank, det)
}

fn main() {
    let base: Vec<i64> = (0..(N * N) as i64).map(|k| mix(k) % P).collect();
    let mut a1 = base.clone();
    for j in 0..N {
        a1[(N - 1) * N + j] = (base[j] + base[N + j]) % P;
    }
    let mut a2 = base.clone();
    for j in 0..N {
        a2[(N - 1) * N + j] = mix(1_000_000 + j as i64) % P;
    }
    let (r1, d1) = solve(a1);
    let (r2, d2) = solve(a2);
    println!("{} {} {} {}", r1, d1, r2, d2);
}
