use rayon::prelude::*;

// a parallel scan: each chunk sums, the chunk totals are scanned, then each
// chunk scans from its offset
fn main() {
    let n = 100_000_000usize;
    let chunk = 1 << 20;
    let mut s: Vec<i64> = vec![0; n];
    let totals: Vec<i64> = s
        .par_chunks_mut(chunk)
        .enumerate()
        .map(|(c, xs)| {
            let mut acc = 0i64;
            for (k, x) in xs.iter_mut().enumerate() {
                acc += ((c * chunk + k) as i64) % 1000;
                *x = acc;
            }
            acc
        })
        .collect();
    let offsets: Vec<i64> = totals
        .iter()
        .scan(0i64, |acc, t| {
            let o = *acc;
            *acc += t;
            Some(o)
        })
        .collect();
    s.par_chunks_mut(chunk).zip(offsets.par_iter()).for_each(|(xs, o)| {
        for x in xs.iter_mut() {
            *x += o;
        }
    });
    println!("{} {}", s[n / 2], s[n - 1]);
}
