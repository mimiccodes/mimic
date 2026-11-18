// collatz: for every number below 3,000,000, the steps of its Collatz
// sequence (n/2 when even, 3n+1 when odd) until it reaches 1, each counted
// on its own. Prints the number with the longest sequence (the smallest,
// on a tie), its steps, and the sum of all steps.

fn main() {
    let n: i64 = 3_000_000;
    let (mut best, mut most, mut sum) = (1, 0, 0);
    for i in 1..=n {
        let (mut x, mut s) = (i, 0i64);
        while x > 1 {
            x = if x % 2 == 0 { x / 2 } else { 3 * x + 1 };
            s += 1;
        }
        sum += s;
        if s > most {
            most = s;
            best = i;
        }
    }
    println!("{} {} {}", best, most, sum);
}
