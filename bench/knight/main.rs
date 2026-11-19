// knight: the number of knight's tours (open or closed) on a 5 × 5 board
// from each square, by backtracking over the eight moves in a fixed order
// with the visited squares as a bitmask. Prints the total number of tours
// (1,728), the number from a corner (304), and the number of moves tried.

const N: i32 = 5;
const MOVES: [(i32, i32); 8] = [(1, 2), (2, 1), (2, -1), (1, -2), (-1, -2), (-2, -1), (-2, 1), (-1, 2)];

fn tours(x: i32, y: i32, mask: u32, depth: i32, tried: &mut i64) -> i64 {
    if depth == N * N {
        return 1;
    }
    let mut t = 0;
    for (dx, dy) in MOVES {
        let (nx, ny) = (x + dx, y + dy);
        *tried += 1;
        if nx < 0 || ny < 0 || nx >= N || ny >= N {
            continue;
        }
        let bit = 1u32 << (ny * N + nx);
        if mask & bit == 0 {
            t += tours(nx, ny, mask | bit, depth + 1, tried);
        }
    }
    t
}

fn main() {
    let mut tried = 0i64;
    let counts: Vec<i64> = (0..N * N).map(|s| tours(s % N, s / N, 1 << s, 1, &mut tried)).collect();
    println!("{} {} {}", counts.iter().sum::<i64>(), counts[0], tried);
}
