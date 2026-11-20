// pip: 1,000,000 points tested against a polygon of 500 vertices (a jagged
// square, from splitmix64) by ray casting with the even-odd rule, in exact
// integer arithmetic. Prints how many points are inside and the sum of
// their x coordinates.

const R: i64 = 1_000_000;

fn mix(v: i64) -> i64 {
    let mut x = (v as u64 + 1).wrapping_mul(0x9E3779B97F4A7C15);
    x = (x ^ x >> 30).wrapping_mul(0xBF58476D1CE4E5B9);
    x = (x ^ x >> 27).wrapping_mul(0x94D049BB133111EB);
    ((x ^ x >> 31) >> 1) as i64
}

fn main() {
    let poly: Vec<(i64, i64)> = (0..500)
        .map(|v| {
            let (s, j) = (v / 125, v % 125);
            let (t, d) = (16000 * j - R, mix(500 + v) % 400000);
            [(t, d - R), (R - d, t), (-t, R - d), (d - R, -t)][s as usize]
        })
        .collect();
    let (mut count, mut sx) = (0i64, 0i64);
    for k in 0..1_000_000i64 {
        let (x, y) = (mix(2 * k) % 2200001 - 1100000, mix(2 * k + 1) % 2200001 - 1100000);
        let mut inside = false;
        for i in 0..poly.len() {
            let ((ax, ay), (bx, by)) = (poly[i], poly[(i + 1) % poly.len()]);
            let d = by - ay;
            let t = (x - ax) * d - (y - ay) * (bx - ax);
            if (ay > y) != (by > y) && (if d > 0 { t < 0 } else { t > 0 }) {
                inside = !inside;
            }
        }
        if inside {
            count += 1;
            sx += x;
        }
    }
    println!("{} {}", count, sx);
}
