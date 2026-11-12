fn main() {
    let n = 100_000_000usize;
    let s: Vec<i64> = (0..n as i64)
        .scan(0i64, |acc, i| {
            *acc += i % 1000;
            Some(*acc)
        })
        .collect();
    println!("{} {}", s[n / 2], s[n - 1]);
}
