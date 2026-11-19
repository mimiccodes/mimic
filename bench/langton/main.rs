// langton: Langton's ant on a 1,024 × 1,024 torus for 20,000,000 steps
// from the centre facing up. Prints the number of black cells at the end,
// and the ant's column, row and direction.

const L: i64 = 1024;

fn main() {
    let mut grid = vec![0u8; (L * L) as usize];
    let (dx, dy) = ([0, 1, 0, -1], [-1, 0, 1, 0]);
    let (mut x, mut y, mut d) = (L / 2, L / 2, 0usize);
    for _ in 0..20_000_000 {
        let i = (y * L + x) as usize;
        d = if grid[i] == 0 { (d + 1) % 4 } else { (d + 3) % 4 };
        grid[i] ^= 1;
        x = (x + dx[d] + L) % L;
        y = (y + dy[d] + L) % L;
    }
    let black: i64 = grid.iter().map(|&c| c as i64).sum();
    println!("{} {} {} {}", black, x, y, d);
}
