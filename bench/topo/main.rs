// topo: a topological order of a DAG (the bfs graph, edges from the smaller
// node to the larger, every second edge) by Kahn's algorithm, level by
// level, ascending within a level. Prints the levels and a checksum of the
// order.

fn mix(v: i64) -> i64 {
    let mut x = (v as u64 + 1).wrapping_mul(0x9E3779B97F4A7C15);
    x = (x ^ x >> 30).wrapping_mul(0xBF58476D1CE4E5B9);
    x = (x ^ x >> 27).wrapping_mul(0x94D049BB133111EB);
    ((x ^ x >> 31) >> 1) as i64
}

fn main() {
    let (n, m) = (200000usize, 1000000i64);
    let mut out = vec![Vec::new(); n];
    let mut indeg = vec![0u32; n];
    for e in (0..m).step_by(2) {
        let (x, y) = ((mix(2 * e + 1) % n as i64) as usize, (mix(2 * e + 2) % n as i64) as usize);
        if x != y {
            out[x.min(y)].push(x.max(y));
            indeg[x.max(y)] += 1;
        }
    }
    let mut level: Vec<usize> = (0..n).filter(|&v| indeg[v] == 0).collect();
    let (mut levels, mut pos, mut check) = (0, 0i64, 0i64);
    while !level.is_empty() {
        levels += 1;
        let mut next = Vec::new();
        for &v in &level {
            check = (check + pos * v as i64) % 1000000007;
            pos += 1;
            for &w in &out[v] {
                indeg[w] -= 1;
                if indeg[w] == 0 {
                    next.push(w);
                }
            }
        }
        next.sort_unstable();
        level = next;
    }
    println!("{levels} {check}");
}
