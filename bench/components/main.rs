// components: the connected components of the bfs graph at half the size,
// undirected, every third edge left out, with union-find (each root
// joined under the smaller; no path compression).
// Prints the components and the size of the largest.

fn mix(v: i64) -> i64 {
    let mut x = (v as u64 + 1).wrapping_mul(0x9E3779B97F4A7C15);
    x = (x ^ x >> 30).wrapping_mul(0xBF58476D1CE4E5B9);
    x = (x ^ x >> 27).wrapping_mul(0x94D049BB133111EB);
    ((x ^ x >> 31) >> 1) as i64
}

fn find(parent: &[usize], mut x: usize) -> usize {
    while parent[x] != x {
        x = parent[x];
    }
    x
}

fn main() {
    let (n, m) = (100000usize, 500000i64);
    let mut parent: Vec<usize> = (0..n).collect();
    for e in (0..m).filter(|e| e % 3 != 0) {
        let a = find(&parent, (mix(2 * e + 1) % n as i64) as usize);
        let b = find(&parent, (mix(2 * e + 2) % n as i64) as usize);
        if a != b {
            parent[a.max(b)] = a.min(b);
        }
    }
    let mut size = vec![0usize; n];
    for v in 0..n {
        let r = find(&parent, v);
        size[r] += 1;
    }
    println!("{} {}", size.iter().filter(|&&s| s > 0).count(), size.iter().max().unwrap());
}
