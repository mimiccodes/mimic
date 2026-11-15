// fib(38), the two calls in parallel with rayon::join down to a cut-off
fn fib(n: i64) -> i64 {
    if n < 2 { n } else { fib(n - 1) + fib(n - 2) }
}

fn pfib(n: i64) -> i64 {
    if n < 25 {
        return fib(n);
    }
    let (a, b) = rayon::join(|| pfib(n - 1), || pfib(n - 2));
    a + b
}

fn main() {
    println!("{}", pfib(38));
}
