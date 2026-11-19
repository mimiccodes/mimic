// catalan: the Catalan numbers C(0) … C(20,000) modulo 1,000,000,007 by
// the convolution C(n + 1) = sum of C(i) C(n - i). Prints C(20,000) and
// the sum of all of them modulo the prime.

const P: i64 = 1_000_000_007;

fn main() {
    let n = 20_000usize;
    let mut cs = vec![1i64];
    for m in 0..n {
        let s: i64 = (0..=m).map(|i| cs[i] * cs[m - i] % P).sum();
        cs.push(s % P);
    }
    println!("{} {}", cs[n], cs.iter().fold(0, |a, &c| (a + c) % P));
}
