// heap: a binary min-heap in one array: 1,000,000 numbers below
// 1,000,000,000 (from splitmix64) pushed, then popped smallest first
// (heapsort). Prints the first, middle and last popped and the sum of each
// value times its position (wrapping).

fn mix(v: i64) -> i64 {
    let mut x = (v as u64 + 1).wrapping_mul(0x9E3779B97F4A7C15);
    x = (x ^ x >> 30).wrapping_mul(0xBF58476D1CE4E5B9);
    x = (x ^ x >> 27).wrapping_mul(0x94D049BB133111EB);
    ((x ^ x >> 31) >> 1) as i64
}

fn main() {
    let m = 1_000_000usize;
    let mut a: Vec<i64> = Vec::with_capacity(m);
    for k in 0..m {
        a.push(mix(k as i64) % 1_000_000_000);
        let mut i = a.len() - 1;
        while i > 0 && a[(i - 1) / 2] > a[i] {
            a.swap(i, (i - 1) / 2);
            i = (i - 1) / 2;
        }
    }
    let mut out = Vec::with_capacity(m);
    while !a.is_empty() {
        out.push(a[0]);
        let last = a.pop().unwrap();
        if a.is_empty() {
            break;
        }
        a[0] = last;
        let (n, mut i) = (a.len(), 0);
        loop {
            let (l, r) = (2 * i + 1, 2 * i + 2);
            if l >= n {
                break;
            }
            let c = if r < n && a[r] < a[l] { r } else { l };
            if a[i] <= a[c] {
                break;
            }
            a.swap(i, c);
            i = c;
        }
    }
    let check = out.iter().enumerate().fold(0i64, |s, (i, &v)| s.wrapping_add(v.wrapping_mul(i as i64)));
    println!("{} {} {} {}", out[0], out[m / 2], out[m - 1], check);
}
