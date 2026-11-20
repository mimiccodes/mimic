// sandpile: an abelian sandpile on a 128 × 128 grid (start from
// splitmix64), toppled in synchronous rounds (each cell sends s div 4 to
// each neighbour, keeps s mod 4) until stable. Prints the number of rounds,
// the grains left, and how many cells end with 0, 1, 2 and 3.

const L: usize = 128;

fn mix(v: i64) -> i64 {
    let mut x = (v as u64 + 1).wrapping_mul(0x9E3779B97F4A7C15);
    x = (x ^ x >> 30).wrapping_mul(0xBF58476D1CE4E5B9);
    x = (x ^ x >> 27).wrapping_mul(0x94D049BB133111EB);
    ((x ^ x >> 31) >> 1) as i64
}

fn main() {
    let mut s: Vec<i64> = (0..(L * L) as i64).map(|i| 4 + mix(i) % 30).collect();
    let mut t = vec![0i64; L * L];
    let mut rounds = 0;
    while *s.iter().max().unwrap() >= 4 {
        for y in 0..L {
            for x in 0..L {
                let mut v = s[y * L + x] % 4;
                if x > 0 {
                    v += s[y * L + x - 1] / 4;
                }
                if x < L - 1 {
                    v += s[y * L + x + 1] / 4;
                }
                if y > 0 {
                    v += s[(y - 1) * L + x] / 4;
                }
                if y < L - 1 {
                    v += s[(y + 1) * L + x] / 4;
                }
                t[y * L + x] = v;
            }
        }
        std::mem::swap(&mut s, &mut t);
        rounds += 1;
    }
    let count = |v: i64| s.iter().filter(|&&x| x == v).count();
    println!("{} {} {} {} {} {}", rounds, s.iter().sum::<i64>(), count(0), count(1), count(2), count(3));
}
