// hull: the convex hull of 2,000,000 points with integer coordinates in a
// disc of radius 10^6 (from splitmix64), by Andrew's monotone chain:
// sorted by x then y, then the lower and upper chains as stacks. Prints
// the number of hull points, twice the hull's area, and the sum of the
// hull's x coordinates.

const N: i64 = 2_000_000;
const R: i64 = 1_000_000;

fn mix(v: i64) -> i64 {
    let mut x = (v as u64 + 1).wrapping_mul(0x9E3779B97F4A7C15);
    x = (x ^ x >> 30).wrapping_mul(0xBF58476D1CE4E5B9);
    x = (x ^ x >> 27).wrapping_mul(0x94D049BB133111EB);
    ((x ^ x >> 31) >> 1) as i64
}

fn cross(o: (i64, i64), a: (i64, i64), b: (i64, i64)) -> i64 {
    (a.0 - o.0) * (b.1 - o.1) - (a.1 - o.1) * (b.0 - o.0)
}

// one chain, the last point left off
fn chain<'a>(ps: impl Iterator<Item = &'a (i64, i64)>) -> Vec<(i64, i64)> {
    let mut h: Vec<(i64, i64)> = Vec::new();
    for &p in ps {
        while h.len() >= 2 && cross(h[h.len() - 2], h[h.len() - 1], p) <= 0 {
            h.pop();
        }
        h.push(p);
    }
    h.pop();
    h
}

fn main() {
    let mut ps: Vec<(i64, i64)> = (0..N)
        .map(|k| {
            let (x, y) = (mix(2 * k) % (2 * R + 1) - R, mix(2 * k + 1) % (2 * R + 1) - R);
            if x * x + y * y <= R * R { (x, y) } else { (x.div_euclid(2), y.div_euclid(2)) }
        })
        .collect();
    ps.sort_unstable();
    let mut hull = chain(ps.iter());
    hull.extend(chain(ps.iter().rev()));
    let m = hull.len();
    let area2: i64 = (0..m).map(|i| hull[i].0 * hull[(i + 1) % m].1 - hull[(i + 1) % m].0 * hull[i].1).sum();
    println!("{} {} {}", m, area2, hull.iter().map(|p| p.0).sum::<i64>());
}
