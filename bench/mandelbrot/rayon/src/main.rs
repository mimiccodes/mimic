use rayon::prelude::*;

// rows in parallel
fn main() {
    let (n, limit) = (2000i64, 200i64);
    let count: i64 = (0..n)
        .into_par_iter()
        .map(|y| {
            let mut count = 0i64;
            for x in 0..n {
                let cr = -2.0 + 2.5 * x as f64 / n as f64;
                let ci = -1.25 + 2.5 * y as f64 / n as f64;
                let (mut zr, mut zi, mut k) = (0.0f64, 0.0f64, 0i64);
                while k < limit && zr * zr + zi * zi <= 4.0 {
                    let t = zr * zr - zi * zi + cr;
                    zi = 2.0 * zr * zi + ci;
                    zr = t;
                    k += 1;
                }
                if k == limit {
                    count += 1;
                }
            }
            count
        })
        .sum();
    println!("{count}");
}
