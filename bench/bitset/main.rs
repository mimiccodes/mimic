// bitset: 100 sets over the numbers below 2^20, each 16,384 words of 64
// bits (from splitmix64), and for every pair the sizes of their
// intersection and union (hardware population count). Prints the sum of
// the intersection sizes, the sum of the union sizes, and the pair with the
// largest intersection as i * 100 + j.

const S: usize = 100;
const W: usize = 16384;

fn mix(v: i64) -> u64 {
    let mut x = (v as u64 + 1).wrapping_mul(0x9E3779B97F4A7C15);
    x = (x ^ x >> 30).wrapping_mul(0xBF58476D1CE4E5B9);
    x = (x ^ x >> 27).wrapping_mul(0x94D049BB133111EB);
    (x ^ x >> 31) >> 1
}

fn main() {
    let ws: Vec<u64> = (0..(S * W) as i64).map(|k| mix(3 * k) & mix(3 * k + 1) & mix(3 * k + 2)).collect();
    let (mut inter, mut uni, mut best, mut pair) = (0i64, 0i64, -1i64, 0usize);
    for i in 0..S {
        for j in i + 1..S {
            let (a, u) = (0..W).fold((0i64, 0i64), |(a, u), w| {
                let (x, y) = (ws[i * W + w], ws[j * W + w]);
                (a + (x & y).count_ones() as i64, u + (x | y).count_ones() as i64)
            });
            inter += a;
            uni += u;
            if a > best {
                best = a;
                pair = i * 100 + j;
            }
        }
    }
    println!("{} {} {}", inter, uni, pair);
}
