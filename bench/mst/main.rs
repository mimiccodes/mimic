// mst: Kruskal's minimum spanning forest of a random graph of 200,000
// nodes and 1,000,000 edges (from splitmix64): edges sorted by weight then
// number, union-find by size. Prints the total weight of the forest, its
// number of edges, and the sum of the numbers of the edges taken.

const N: i64 = 200_000;

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
    let mut keys: Vec<i64> = (0..1_000_000i64).map(|e| (mix(3 * e + 2) % 1_000_000) << 20 | e).collect();
    keys.sort_unstable();
    let mut parent: Vec<usize> = (0..N as usize).collect();
    let mut size = vec![1i64; N as usize];
    let (mut total, mut count, mut ids) = (0i64, 0i64, 0i64);
    for &key in &keys {
        let (w, e) = (key >> 20, key & ((1 << 20) - 1));
        let (mut a, mut b) = (find(&parent, (mix(3 * e) % N) as usize), find(&parent, (mix(3 * e + 1) % N) as usize));
        if a == b {
            continue;
        }
        if size[a] < size[b] {
            std::mem::swap(&mut a, &mut b);
        }
        parent[b] = a;
        size[a] += size[b];
        total += w;
        count += 1;
        ids += e;
    }
    println!("{} {} {}", total, count, ids);
}
