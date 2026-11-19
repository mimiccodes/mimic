// bipartite: a random undirected graph of 300,000 nodes and 200,000 edges
// (from splitmix64), each component two-coloured by breadth-first search
// from its smallest node (adjacency lists in edge order). Prints the
// number of components, how many are bipartite, and the size of the
// largest.
use std::collections::VecDeque;

const N: usize = 300_000;
const M: i64 = 200_000;

fn mix(v: i64) -> i64 {
    let mut x = (v as u64 + 1).wrapping_mul(0x9E3779B97F4A7C15);
    x = (x ^ x >> 30).wrapping_mul(0xBF58476D1CE4E5B9);
    x = (x ^ x >> 27).wrapping_mul(0x94D049BB133111EB);
    ((x ^ x >> 31) >> 1) as i64
}

fn main() {
    let mut adj: Vec<Vec<usize>> = vec![Vec::new(); N];
    for e in 0..M {
        let (a, b) = ((mix(2 * e) % N as i64) as usize, (mix(2 * e + 1) % N as i64) as usize);
        adj[a].push(b);
        adj[b].push(a);
    }
    let mut colour = vec![-1i8; N];
    let (mut comps, mut bip, mut largest) = (0, 0, 0);
    for s in 0..N {
        if colour[s] >= 0 {
            continue;
        }
        comps += 1;
        colour[s] = 0;
        let (mut q, mut ok, mut size) = (VecDeque::from([s]), true, 0);
        while let Some(u) = q.pop_front() {
            size += 1;
            for &v in &adj[u] {
                if colour[v] < 0 {
                    colour[v] = 1 - colour[u];
                    q.push_back(v);
                } else if colour[v] == colour[u] {
                    ok = false;
                }
            }
        }
        bip += ok as i64;
        largest = largest.max(size);
    }
    println!("{} {} {}", comps, bip, largest);
}
