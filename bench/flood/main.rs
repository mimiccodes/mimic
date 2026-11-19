// flood: a breadth-first flood fill on a 2,000 × 2,000 grid with 30% walls
// (from splitmix64), from the first open cell at or after the centre.
// Prints how many cells are reached, the sum of their distances from the
// start, and the largest distance.
use std::collections::VecDeque;

const W: i64 = 2000;

fn mix(v: i64) -> i64 {
    let mut x = (v as u64 + 1).wrapping_mul(0x9E3779B97F4A7C15);
    x = (x ^ x >> 30).wrapping_mul(0xBF58476D1CE4E5B9);
    x = (x ^ x >> 27).wrapping_mul(0x94D049BB133111EB);
    ((x ^ x >> 31) >> 1) as i64
}

fn main() {
    let n = (W * W) as usize;
    let wall: Vec<bool> = (0..n as i64).map(|i| mix(i) % 100 < 30).collect();
    let mut dist = vec![-1i64; n];
    let mut start = (W * 1000 + 1000) as usize;
    while wall[start] {
        start += 1;
    }
    dist[start] = 0;
    let mut q = VecDeque::from([start]);
    while let Some(c) = q.pop_front() {
        let (x, y) = (c as i64 % W, c as i64 / W);
        for (nx, ny) in [(x - 1, y), (x + 1, y), (x, y - 1), (x, y + 1)] {
            if nx < 0 || ny < 0 || nx >= W || ny >= W {
                continue;
            }
            let nc = (ny * W + nx) as usize;
            if !wall[nc] && dist[nc] < 0 {
                dist[nc] = dist[c] + 1;
                q.push_back(nc);
            }
        }
    }
    let reached: Vec<i64> = dist.into_iter().filter(|&d| d >= 0).collect();
    println!("{} {} {}", reached.len(), reached.iter().sum::<i64>(), reached.iter().max().unwrap());
}
