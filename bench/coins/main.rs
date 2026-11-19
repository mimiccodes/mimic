// coins: the number of ways, modulo 1,000,000,007, to make every amount up
// to 3,000,000 from 30 kinds of coin (k*k + 1 for k below 30), coin by
// coin: ways[x] += ways[x - c]. Prints the ways for 3,000,000 and the sum
// of all the ways.

const A: usize = 3_000_000;
const P: i64 = 1_000_000_007;

fn main() {
    let mut ways = vec![0i64; A + 1];
    ways[0] = 1;
    for k in 0..30 {
        let c = k * k + 1;
        for x in c..=A {
            ways[x] = (ways[x] + ways[x - c]) % P;
        }
    }
    let sum = ways.iter().fold(0, |s, &w| (s + w) % P);
    println!("{} {}", ways[A], sum);
}
