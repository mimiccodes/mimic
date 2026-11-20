// randwalk: 200,000 walkers, each taking 1,000 steps on the square lattice
// from (0, 0) in directions from splitmix64. Prints the sum of the squared
// distances at the end, the largest, and how many walkers end where they
// started.

fn mix(v: i64) -> i64 {
    let mut x = (v as u64 + 1).wrapping_mul(0x9E3779B97F4A7C15);
    x = (x ^ x >> 30).wrapping_mul(0xBF58476D1CE4E5B9);
    x = (x ^ x >> 27).wrapping_mul(0x94D049BB133111EB);
    ((x ^ x >> 31) >> 1) as i64
}

fn main() {
    let (mut sum, mut most, mut home) = (0i64, 0i64, 0i64);
    for w in 0..200_000i64 {
        let (mut x, mut y) = (0i64, 0i64);
        for t in 0..1000 {
            let d = mix(1000 * w + t) % 4;
            x += (d == 0) as i64 - (d == 2) as i64;
            y += (d == 1) as i64 - (d == 3) as i64;
        }
        let r = x * x + y * y;
        sum += r;
        most = most.max(r);
        home += (r == 0) as i64;
    }
    println!("{} {} {}", sum, most, home);
}
