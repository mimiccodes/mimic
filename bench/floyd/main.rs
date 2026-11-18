// floyd: the shortest paths between all pairs of 800 nodes by
// Floyd-Warshall; each node has edges to 8 others (from splitmix64, weights
// 1 to 1,000; of two edges to one node the lighter counts). Prints the sum
// of the distances that exist, how many pairs have no path, and the
// longest shortest path.

const FAR: i64 = 1_000_000_000_000;

fn mix(v: i64) -> i64 {
    let mut x = (v as u64 + 1).wrapping_mul(0x9E3779B97F4A7C15);
    x = (x ^ x >> 30).wrapping_mul(0xBF58476D1CE4E5B9);
    x = (x ^ x >> 27).wrapping_mul(0x94D049BB133111EB);
    ((x ^ x >> 31) >> 1) as i64
}

fn main() {
    let n = 800usize;
    let mut d = vec![FAR; n * n];
    for i in 0..n {
        for e in 0..8 {
            let j = (mix(8 * i as i64 + e) % n as i64) as usize;
            let w = 1 + mix(1_000_000 + 8 * i as i64 + e) % 1000;
            d[i * n + j] = d[i * n + j].min(w);
        }
        d[i * n + i] = 0;
    }
    for k in 0..n {
        let row_k: Vec<i64> = d[k * n..(k + 1) * n].to_vec();
        for i in 0..n {
            let dik = d[i * n + k];
            for (x, &dkj) in d[i * n..(i + 1) * n].iter_mut().zip(&row_k) {
                *x = (*x).min(dik + dkj);
            }
        }
    }
    let some: Vec<i64> = d.iter().copied().filter(|&x| x < FAR).collect();
    println!("{} {} {}", some.iter().sum::<i64>(), n * n - some.len(), some.iter().max().unwrap());
}
