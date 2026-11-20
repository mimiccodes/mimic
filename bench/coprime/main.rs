// coprime: over all pairs (i, j) with 1 <= i, j <= 4,000, how many have
// gcd 1, and the sum of all the gcds, by Euclid's algorithm.

fn gcd(mut a: i64, mut b: i64) -> i64 {
    while b != 0 {
        let t = a % b;
        a = b;
        b = t;
    }
    a
}

fn main() {
    let (mut ones, mut sum) = (0i64, 0i64);
    for i in 1..=4000 {
        for j in 1..=4000 {
            let g = gcd(i, j);
            ones += (g == 1) as i64;
            sum += g;
        }
    }
    println!("{} {}", ones, sum);
}
