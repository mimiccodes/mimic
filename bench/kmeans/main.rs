// 1,000,000 splitmix64 points (whole numbers up to 100,000) in 16 clusters
// by Lloyd's algorithm, 25 rounds, in whole numbers (a centre moves to the
// mean of its points rounded down; one with none stays): the sum of the
// centres' coordinates, the first cluster's points and the total squared
// distance to the nearest centre.
fn mix(v: u64) -> u64 {
    let mut x = v.wrapping_add(1).wrapping_mul(0x9E3779B97F4A7C15);
    x = (x ^ x >> 30).wrapping_mul(0xBF58476D1CE4E5B9);
    x = (x ^ x >> 27).wrapping_mul(0x94D049BB133111EB);
    (x ^ x >> 31) >> 1
}

fn nearest(cx: &[i64], cy: &[i64], x: i64, y: i64) -> (usize, i64) {
    let (mut at, mut best) = (0, i64::MAX);
    for j in 0..cx.len() {
        let (dx, dy) = (cx[j] - x, cy[j] - y);
        let d = dx * dx + dy * dy;
        if d < best {
            best = d;
            at = j;
        }
    }
    (at, best)
}

fn main() {
    let (n, k) = (1_000_000u64, 16);
    let xs: Vec<i64> = (0..n).map(|i| (mix(2 * i) % 100001) as i64).collect();
    let ys: Vec<i64> = (0..n).map(|i| (mix(2 * i + 1) % 100001) as i64).collect();
    let (mut cx, mut cy) = (xs[..k].to_vec(), ys[..k].to_vec());
    for _ in 0..25 {
        let (mut sx, mut sy, mut count) = (vec![0i64; k], vec![0i64; k], vec![0i64; k]);
        for (&x, &y) in xs.iter().zip(&ys) {
            let (a, _) = nearest(&cx, &cy, x, y);
            sx[a] += x;
            sy[a] += y;
            count[a] += 1;
        }
        for j in 0..k {
            if count[j] > 0 {
                cx[j] = sx[j] / count[j];
                cy[j] = sy[j] / count[j];
            }
        }
    }
    let (mut first, mut spread) = (0i64, 0i64);
    for (&x, &y) in xs.iter().zip(&ys) {
        let (a, d) = nearest(&cx, &cy, x, y);
        first += (a == 0) as i64;
        spread += d;
    }
    let sum: i64 = cx.iter().sum::<i64>() + cy.iter().sum::<i64>();
    println!("{sum} {first} {spread}");
}
