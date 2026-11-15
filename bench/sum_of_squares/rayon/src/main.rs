use rayon::prelude::*;

fn main() {
    let s: i64 = (0..100_000_000i64).into_par_iter().map(|i| i % 1000).map(|x| x * x).sum();
    println!("{s}");
}
