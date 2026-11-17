// pagerank: 20 iterations of PageRank (damping 0.85; rank from nodes
// without outgoing edges is not passed on) on the bfs graph at half the
// size, each node summing the edges into it in order of their source.
// Prints the sum of the ranks and the largest rank times n, to 6 decimals.

fn mix(v: i64) -> i64 {
    let mut x = (v as u64 + 1).wrapping_mul(0x9E3779B97F4A7C15);
    x = (x ^ x >> 30).wrapping_mul(0xBF58476D1CE4E5B9);
    x = (x ^ x >> 27).wrapping_mul(0x94D049BB133111EB);
    ((x ^ x >> 31) >> 1) as i64
}

fn main() {
    let (n, m) = (100000usize, 500000i64);
    let edges: Vec<(usize, usize)> = (0..m).map(|e| ((mix(2 * e + 1) % n as i64) as usize, (mix(2 * e + 2) % n as i64) as usize)).collect();
    let mut sorted = edges.clone();
    sorted.sort_unstable_by_key(|&(s, d)| (d, s));
    let mut start = vec![0usize; n + 1];
    let mut outdeg = vec![0usize; n];
    for &(s, d) in &edges {
        start[d + 1] += 1;
        outdeg[s] += 1;
    }
    for v in 0..n {
        start[v + 1] += start[v];
    }
    let from: Vec<usize> = sorted.iter().map(|&(s, _)| s).collect();
    let mut pr = vec![1.0 / n as f64; n];
    for _ in 0..20 {
        let share: Vec<f64> = (0..n).map(|u| if outdeg[u] > 0 { pr[u] / outdeg[u] as f64 } else { 0.0 }).collect();
        for v in 0..n {
            let s: f64 = from[start[v]..start[v + 1]].iter().fold(0.0, |a, &u| a + share[u]);
            pr[v] = 0.15 / n as f64 + 0.85 * s;
        }
    }
    let total: f64 = pr.iter().fold(0.0, |a, &x| a + x);
    let top = pr.iter().cloned().fold(0.0, f64::max);
    println!("{total:.6} {:.6}", top * n as f64);
}
