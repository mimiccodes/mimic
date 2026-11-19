// sudoku: 200 puzzles (full grids relabelled and emptied in part, from
// splitmix64) solved by backtracking: the first empty cell in reading
// order gets the digits 1 … 9 that fit, in order. Prints the sum over the
// puzzles of the top row of the solution read as a 9-digit number, and the
// number of placements tried in all.

fn mix(v: i64) -> i64 {
    let mut x = (v as u64 + 1).wrapping_mul(0x9E3779B97F4A7C15);
    x = (x ^ x >> 30).wrapping_mul(0xBF58476D1CE4E5B9);
    x = (x ^ x >> 27).wrapping_mul(0x94D049BB133111EB);
    ((x ^ x >> 31) >> 1) as i64
}

fn fits(b: &[u8; 81], i: usize, v: u8) -> bool {
    let (r, c) = (i / 9, i % 9);
    let (br, bc) = (r - r % 3, c - c % 3);
    (0..9).all(|k| b[r * 9 + k] != v && b[k * 9 + c] != v && b[(br + k / 3) * 9 + bc + k % 3] != v)
}

fn solve(b: &mut [u8; 81], tried: &mut i64) -> bool {
    let Some(i) = b.iter().position(|&x| x == 0) else { return true };
    for v in 1..=9 {
        *tried += 1;
        if fits(b, i, v) {
            b[i] = v;
            if solve(b, tried) {
                return true;
            }
            b[i] = 0;
        }
    }
    false
}

fn main() {
    let (mut total, mut tried) = (0i64, 0i64);
    for p in 0..200i64 {
        let mut perm: Vec<u8> = (1..=9).collect();
        for i in (1..9).rev() {
            let j = (mix(1000 * p + i as i64) % (i as i64 + 1)) as usize;
            perm.swap(i, j);
        }
        let mut b = [0u8; 81];
        for i in 0..81 {
            let (r, c) = (i / 9, i % 9);
            b[i] = if mix(81 * p + i as i64) % 100 < 62 { 0 } else { perm[(3 * (r % 3) + r / 3 + c) % 9] };
        }
        solve(&mut b, &mut tried);
        total += b[..9].iter().fold(0i64, |n, &d| n * 10 + d as i64);
    }
    println!("{} {}", total, tried);
}
