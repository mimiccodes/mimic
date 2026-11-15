use rayon::prelude::*;

fn main() {
    let n = 10_000_000i64;
    let mut xs: Vec<i64> = (0..n).into_par_iter().map(|i| (i * 1103515245 + 12345) % 2147483648).collect();
    xs.par_sort_unstable();
    println!("{} {} {}", xs[0], xs[(n / 2) as usize], xs[(n - 1) as usize]);
}
