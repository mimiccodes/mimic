// maze: a 500 × 500 maze carved by a depth-first search with a stack (the
// way on chosen by splitmix64), then a breadth-first search from the
// top-left cell. Prints the distance to the bottom-right cell, the number
// of dead ends, and the sum of all distances.
use std::collections::VecDeque;

const W: usize = 500;
const N: usize = W * W;

fn mix(v: i64) -> i64 {
    let mut x = (v as u64 + 1).wrapping_mul(0x9E3779B97F4A7C15);
    x = (x ^ x >> 30).wrapping_mul(0xBF58476D1CE4E5B9);
    x = (x ^ x >> 27).wrapping_mul(0x94D049BB133111EB);
    ((x ^ x >> 31) >> 1) as i64
}

fn step_to(c: usize, d: usize) -> Option<usize> {
    let (x, y) = (c % W, c / W);
    match d {
        0 => (y > 0).then(|| c - W),
        1 => (x < W - 1).then(|| c + 1),
        2 => (y < W - 1).then(|| c + W),
        _ => (x > 0).then(|| c - 1),
    }
}

fn main() {
    let (mut opened, mut seen) = (vec![0u32; N], vec![false; N]);
    let mut stack = vec![0usize];
    seen[0] = true;
    let mut step = 0i64;
    while let Some(&c) = stack.last() {
        let free: Vec<usize> = (0..4).filter(|&d| step_to(c, d).is_some_and(|t| !seen[t])).collect();
        if free.is_empty() {
            stack.pop();
        } else {
            let d = free[(mix(step) % free.len() as i64) as usize];
            let t = step_to(c, d).unwrap();
            opened[c] |= 1 << d;
            opened[t] |= 1 << ((d + 2) % 4);
            seen[t] = true;
            stack.push(t);
        }
        step += 1;
    }
    let mut dist = vec![-1i64; N];
    dist[0] = 0;
    let mut q = VecDeque::from([0usize]);
    while let Some(c) = q.pop_front() {
        for d in 0..4 {
            if opened[c] >> d & 1 == 1 {
                let t = step_to(c, d).unwrap();
                if dist[t] < 0 {
                    dist[t] = dist[c] + 1;
                    q.push_back(t);
                }
            }
        }
    }
    let dead = opened.iter().filter(|o| o.count_ones() == 1).count();
    println!("{} {} {}", dist[N - 1], dead, dist.iter().sum::<i64>());
}
