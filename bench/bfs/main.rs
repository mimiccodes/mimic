// bfs: breadth-first distances from node 0 in a random graph of 200,000
// nodes and 1,000,000 directed edges (both ends from splitmix64 of the
// edge number), with a queue over adjacency lists (CSR). Prints the nodes
// reached, the sum of their distances and the largest.

use std::collections::VecDeque;

fn mix(v: i64) -> i64 {
    let mut x = (v as u64 + 1).wrapping_mul(0x9E3779B97F4A7C15);
    x = (x ^ x >> 30).wrapping_mul(0xBF58476D1CE4E5B9);
    x = (x ^ x >> 27).wrapping_mul(0x94D049BB133111EB);
    ((x ^ x >> 31) >> 1) as i64
}

fn main() {
    let (n, m) = (200000usize, 1000000i64);
    let edges: Vec<(usize, usize)> = (0..m).map(|e| ((mix(2 * e + 1) % n as i64) as usize, (mix(2 * e + 2) % n as i64) as usize)).collect();
    let mut start = vec![0usize; n + 1];
    for &(u, _) in &edges {
        start[u + 1] += 1;
    }
    for v in 0..n {
        start[v + 1] += start[v];
    }
    let mut fill = start.clone();
    let mut adj = vec![0usize; m as usize];
    for &(u, v) in &edges {
        adj[fill[u]] = v;
        fill[u] += 1;
    }
    let mut dist = vec![-1i64; n];
    dist[0] = 0;
    let mut q = VecDeque::from([0usize]);
    while let Some(u) = q.pop_front() {
        for &v in &adj[start[u]..start[u + 1]] {
            if dist[v] < 0 {
                dist[v] = dist[u] + 1;
                q.push_back(v);
            }
        }
    }
    let reached: Vec<i64> = dist.into_iter().filter(|&d| d >= 0).collect();
    println!("{} {} {}", reached.len(), reached.iter().sum::<i64>(), reached.iter().max().unwrap());
}
