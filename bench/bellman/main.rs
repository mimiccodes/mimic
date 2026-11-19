// bellman: single-source shortest paths from node 0 in a random directed
// graph of 50,000 nodes and 400,000 weighted edges (from splitmix64), by
// Bellman-Ford: every edge relaxed, round after round, until nothing
// changes. Prints the number of nodes reached, the sum of their distances,
// and the largest distance.

const N: i64 = 50_000;
const INF: i64 = 1 << 60;

fn mix(v: i64) -> i64 {
    let mut x = (v as u64 + 1).wrapping_mul(0x9E3779B97F4A7C15);
    x = (x ^ x >> 30).wrapping_mul(0xBF58476D1CE4E5B9);
    x = (x ^ x >> 27).wrapping_mul(0x94D049BB133111EB);
    ((x ^ x >> 31) >> 1) as i64
}

fn main() {
    let edges: Vec<(usize, usize, i64)> = (0..400_000i64).map(|e| ((mix(3 * e) % N) as usize, (mix(3 * e + 1) % N) as usize, 1 + mix(3 * e + 2) % 1000)).collect();
    let mut d = vec![INF; N as usize];
    d[0] = 0;
    let mut changed = true;
    while changed {
        changed = false;
        for &(a, b, w) in &edges {
            if d[a] < INF && d[a] + w < d[b] {
                d[b] = d[a] + w;
                changed = true;
            }
        }
    }
    let reached: Vec<i64> = d.into_iter().filter(|&x| x < INF).collect();
    println!("{} {} {}", reached.len(), reached.iter().sum::<i64>(), reached.iter().max().unwrap());
}
