// select: 20,000,000 values (from splitmix64) and their k-th smallest for
// k at 1%, 10%, 50%, 90% and 99%, each by quickselect on a fresh copy:
// the pivot is the middle element, a three-way partition in place, then
// the side that holds k. Prints the five values.

const N: usize = 20_000_000;

fn mix(v: i64) -> i64 {
    let mut x = (v as u64 + 1).wrapping_mul(0x9E3779B97F4A7C15);
    x = (x ^ x >> 30).wrapping_mul(0xBF58476D1CE4E5B9);
    x = (x ^ x >> 27).wrapping_mul(0x94D049BB133111EB);
    ((x ^ x >> 31) >> 1) as i64
}

fn select_k(mut a: &mut [i64], mut k: usize) -> i64 {
    loop {
        let n = a.len();
        let p = a[n / 2];
        // a[0…lt) < p, a[lt…i) == p, a[gt…n) > p
        let (mut lt, mut i, mut gt) = (0, 0, n);
        while i < gt {
            let x = a[i];
            if x < p {
                a.swap(i, lt);
                i += 1;
                lt += 1;
            } else if x > p {
                gt -= 1;
                a.swap(i, gt);
            } else {
                i += 1;
            }
        }
        if k < lt {
            a = &mut a[..lt];
        } else if k < gt {
            return p;
        } else {
            a = &mut a[gt..];
            k -= gt;
        }
    }
}

fn main() {
    let xs: Vec<i64> = (0..N as i64).map(|k| mix(k) % 1_000_000_000).collect();
    let out: Vec<String> = [1, 10, 50, 90, 99]
        .iter()
        .map(|p| {
            let mut work = xs.clone();
            select_k(&mut work, N * p / 100).to_string()
        })
        .collect();
    println!("{}", out.join(" "));
}
