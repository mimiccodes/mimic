// diff: the shortest edit script between 1,000 pairs of generated
// "files" by Myers' algorithm (the furthest x on each diagonal for each
// number of edits d, following runs of equal lines). Prints the sum of
// the edit lengths and the largest.

fn mix(v: i64) -> i64 {
    let mut x = (v as u64 + 1).wrapping_mul(0x9E3779B97F4A7C15);
    x = (x ^ x >> 30).wrapping_mul(0xBF58476D1CE4E5B9);
    x = (x ^ x >> 27).wrapping_mul(0x94D049BB133111EB);
    ((x ^ x >> 31) >> 1) as i64
}

fn myers(a: &[i64], b: &[i64]) -> i64 {
    let (n, m) = (a.len() as i64, b.len() as i64);
    let off = n + m + 1;
    let mut v = vec![0i64; (2 * off + 1) as usize];
    for d in 0..=n + m {
        let mut k = -d;
        while k <= d {
            let at = (off + k) as usize;
            let mut x = if k == -d || (k != d && v[at - 1] < v[at + 1]) { v[at + 1] } else { v[at - 1] + 1 };
            let mut y = x - k;
            while x < n && y < m && a[x as usize] == b[y as usize] {
                x += 1;
                y += 1;
            }
            v[at] = x;
            if x >= n && y >= m {
                return d;
            }
            k += 2;
        }
    }
    -1
}

fn main() {
    let d: Vec<i64> = (0..1000i64)
        .map(|p| {
            let a: Vec<i64> = (0..1000 + mix(p * 10) % 1001).map(|i| mix(p * 100000 + i) % 50).collect();
            let mut b = Vec::with_capacity(a.len() * 2);
            for (i, &line) in a.iter().enumerate() {
                let i = i as i64;
                match mix(p * 100000 + 50000 + i) % 100 {
                    0..=2 => {}
                    3..=5 => b.push(mix(p * 100000 + 70000 + i) % 50),
                    6..=8 => b.extend([mix(p * 100000 + 90000 + i) % 50, line]),
                    _ => b.push(line),
                }
            }
            myers(&a, &b)
        })
        .collect();
    println!("{} {}", d.iter().sum::<i64>(), d.iter().max().unwrap());
}
