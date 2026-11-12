fn main() {
    let n = 1024usize;
    let mut a = vec![0f64; n * n];
    let mut b = vec![0f64; n * n];
    let mut c = vec![0f64; n * n];
    for i in 0..n {
        for j in 0..n {
            a[i * n + j] = ((i * j) % 7) as f64;
            b[i * n + j] = ((i + 2 * j) % 5) as f64;
        }
    }
    for i in 0..n {
        for k in 0..n {
            let x = a[i * n + k];
            for j in 0..n {
                c[i * n + j] += x * b[k * n + j];
            }
        }
    }
    let s: f64 = c.iter().sum();
    println!("{s:.1}");
}
