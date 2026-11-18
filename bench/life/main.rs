// Conway's Game of Life on a 1,024 × 1,024 board that wraps around, 150
// generations from a splitmix64 start (a third alive): the live cells,
// and those of the first row.
fn mix(v: u64) -> u64 {
    let mut x = v.wrapping_add(1).wrapping_mul(0x9E3779B97F4A7C15);
    x = (x ^ x >> 30).wrapping_mul(0xBF58476D1CE4E5B9);
    x = (x ^ x >> 27).wrapping_mul(0x94D049BB133111EB);
    (x ^ x >> 31) >> 1
}

fn main() {
    let (n, steps) = (1024usize, 150);
    let mut g: Vec<u8> = (0..n * n).map(|p| (mix(p as u64) % 3 == 0) as u8).collect();
    let mut h = vec![0u8; n * n];
    for _ in 0..steps {
        for r in 0..n {
            let (up, row, down) = ((r + n - 1) % n * n, r * n, (r + 1) % n * n);
            for c in 0..n {
                let (l, rt) = ((c + n - 1) % n, (c + 1) % n);
                let around = g[up + l] + g[up + c] + g[up + rt] + g[row + l] + g[row + rt] + g[down + l] + g[down + c] + g[down + rt];
                h[row + c] = (around == 3 || (around == 2 && g[row + c] == 1)) as u8;
            }
        }
        std::mem::swap(&mut g, &mut h);
    }
    let live: u64 = g.iter().map(|&x| x as u64).sum();
    let first: u64 = g[..n].iter().map(|&x| x as u64).sum();
    println!("{live} {first}");
}
