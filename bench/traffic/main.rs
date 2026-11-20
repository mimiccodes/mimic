// traffic: the Nagel-Schreckenberg traffic model on a ring road of
// 1,000,000 cells with 200,000 cars and top speed 5, for 500 steps (random
// slowing from splitmix64), every car moving at once. Prints the total
// distance driven, the sum of the speeds at the end, and how many cars end
// standing.

const L: i64 = 1_000_000;
const C: usize = 200_000;

fn mix(v: i64) -> i64 {
    let mut x = (v as u64 + 1).wrapping_mul(0x9E3779B97F4A7C15);
    x = (x ^ x >> 30).wrapping_mul(0xBF58476D1CE4E5B9);
    x = (x ^ x >> 27).wrapping_mul(0x94D049BB133111EB);
    ((x ^ x >> 31) >> 1) as i64
}

fn main() {
    let mut pos: Vec<i64> = (0..C as i64).map(|c| 5 * c).collect();
    let mut v = vec![0i64; C];
    let mut driven = 0i64;
    for t in 0..500i64 {
        for c in 0..C {
            let gap = (pos[(c + 1) % C] - pos[c] - 1 + L) % L;
            let mut s = (v[c] + 1).min(5).min(gap);
            if mix(C as i64 * t + c as i64) % 4 == 0 {
                s = (s - 1).max(0);
            }
            v[c] = s;
        }
        for c in 0..C {
            pos[c] = (pos[c] + v[c]) % L;
            driven += v[c];
        }
    }
    println!("{} {} {}", driven, v.iter().sum::<i64>(), v.iter().filter(|&&s| s == 0).count());
}
