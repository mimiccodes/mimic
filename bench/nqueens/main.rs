// nqueens: the number of ways to put 15 queens on a 15x15 board so that
// none attacks another, by backtracking over rows with bit masks of the
// columns and both diagonals taken. Prints the count.

fn solve(n: u32, cols: i64, left: i64, right: i64) -> i64 {
    let all = (1i64 << n) - 1;
    if cols == all {
        return 1;
    }
    let mut count = 0;
    let mut free = all & !(cols | left | right);
    while free != 0 {
        let bit = free & -free;
        free ^= bit;
        count += solve(n, cols | bit, (left | bit) << 1, (right | bit) >> 1);
    }
    count
}

fn main() {
    println!("{}", solve(15, 0, 0, 0));
}
