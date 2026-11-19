// segments: 8,000 short segments on a 100,000 × 100,000 grid (from
// splitmix64), every pair tested for meeting by orientations (collinear
// touching counts). Prints the number of pairs that meet, and the sum of
// i + j over them.

type P = (i64, i64);

fn mix(v: i64) -> i64 {
    let mut x = (v as u64 + 1).wrapping_mul(0x9E3779B97F4A7C15);
    x = (x ^ x >> 30).wrapping_mul(0xBF58476D1CE4E5B9);
    x = (x ^ x >> 27).wrapping_mul(0x94D049BB133111EB);
    ((x ^ x >> 31) >> 1) as i64
}

fn orient(p: P, q: P, r: P) -> i64 {
    ((q.0 - p.0) * (r.1 - p.1) - (q.1 - p.1) * (r.0 - p.0)).signum()
}

fn on(p: P, q: P, r: P) -> bool {
    p.0.min(q.0) <= r.0 && r.0 <= p.0.max(q.0) && p.1.min(q.1) <= r.1 && r.1 <= p.1.max(q.1)
}

fn meet(a: P, b: P, c: P, d: P) -> bool {
    let (o1, o2, o3, o4) = (orient(a, b, c), orient(a, b, d), orient(c, d, a), orient(c, d, b));
    (o1 != o2 && o3 != o4) || (o1 == 0 && on(a, b, c)) || (o2 == 0 && on(a, b, d)) || (o3 == 0 && on(c, d, a)) || (o4 == 0 && on(c, d, b))
}

fn main() {
    let n = 8000i64;
    let segs: Vec<(P, P)> = (0..n)
        .map(|k| {
            let a = (mix(4 * k) % 100000, mix(4 * k + 1) % 100000);
            (a, (a.0 + mix(4 * k + 2) % 4001 - 2000, a.1 + mix(4 * k + 3) % 4001 - 2000))
        })
        .collect();
    let (mut count, mut total) = (0i64, 0i64);
    for i in 0..segs.len() {
        for j in i + 1..segs.len() {
            if meet(segs[i].0, segs[i].1, segs[j].0, segs[j].1) {
                count += 1;
                total += (i + j) as i64;
            }
        }
    }
    println!("{} {}", count, total);
}
