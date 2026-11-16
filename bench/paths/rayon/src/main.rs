// paths with Rayon: the two searches at once; Dijkstra and A* on a 600x600 grid with a binary heap (lazy
// deletion); prints each search's cost and nodes expanded, and the path's
// length (see main.c)

const W: i64 = 600;
const H: i64 = 600;
const N: i64 = W * H;
const FAR: i64 = 1 << 40;

fn est(v: i64, astar: bool) -> i64 { if astar { (W - 1 - v % W) + (H - 1 - v / W) } else { 0 } }

pub fn search(cost: &[i64], astar: bool) -> (i64, i64, Vec<i64>) {
    let n = N as usize;
    let (mut dist, mut prev, mut closed) = (vec![FAR; n], vec![-1i64; n], vec![false; n]);
    let mut heap: Vec<i64> = vec![est(0, astar) * N];
    dist[0] = 0;
    let mut expanded = 0;
    while !heap.is_empty() {
        let top = heap[0];
        let last = heap.pop().unwrap();
        if !heap.is_empty() {
            heap[0] = last;
            let mut k = 0;
            loop {
                let (l, r) = (2 * k + 1, 2 * k + 2);
                let mut m = k;
                if l < heap.len() && heap[l] < heap[m] { m = l; }
                if r < heap.len() && heap[r] < heap[m] { m = r; }
                if m == k { break; }
                heap.swap(k, m);
                k = m;
            }
        }
        let u = top % N;
        if closed[u as usize] { continue; }
        closed[u as usize] = true;
        expanded += 1;
        if u == N - 1 { break; }
        let (x, y) = (u % W, u / W);
        for v in [if x > 0 { u - 1 } else { -1 }, if x < W - 1 { u + 1 } else { -1 }, if y > 0 { u - W } else { -1 }, if y < H - 1 { u + W } else { -1 }] {
            if v < 0 || cost[v as usize] == 0 || closed[v as usize] { continue; }
            let d = dist[u as usize] + cost[v as usize];
            if d < dist[v as usize] {
                dist[v as usize] = d;
                prev[v as usize] = u;
                heap.push((d + est(v, astar)) * N + v);
                let mut k = heap.len() - 1;
                while k > 0 && heap[k] < heap[(k - 1) / 2] { heap.swap(k, (k - 1) / 2); k = (k - 1) / 2; }
            }
        }
    }
    (dist[(N - 1) as usize], expanded, prev)
}

fn costs() -> Vec<i64> {
    (0..N).map(|i| { let (x, y) = (i % W, i / W); if x % 50 == 25 && y % 100 != 10 && y % 100 != 60 { 0 } else { (i * 1103515245 + 12345) % 2147483648 / 65536 % 9 + 1 } }).collect()
}

fn path_len(prev: &[i64]) -> i64 {
    let (mut v, mut cells) = (N - 1, 1);
    while prev[v as usize] >= 0 { v = prev[v as usize]; cells += 1; }
    cells
}

fn main() {
    let cost = costs();
    let ((dc, de, _), (ac, ae, prev)) = rayon::join(|| search(&cost, false), || search(&cost, true));
    println!("dijkstra {dc} {de}");
    println!("astar {ac} {ae}");
    println!("path {}", path_len(&prev));
}
