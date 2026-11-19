// numparse: 2,000,000 signed integers of 1 to 16 digits (from splitmix64)
// written one a line into one text, then read back with str::parse. Prints
// the sum, the number of negatives, the largest and the smallest.

fn mix(v: i64) -> i64 {
    let mut x = (v as u64 + 1).wrapping_mul(0x9E3779B97F4A7C15);
    x = (x ^ x >> 30).wrapping_mul(0xBF58476D1CE4E5B9);
    x = (x ^ x >> 27).wrapping_mul(0x94D049BB133111EB);
    ((x ^ x >> 31) >> 1) as i64
}

fn value(i: i64) -> i64 {
    let v = mix(i) % 10i64.pow(1 + (mix(i + 10_000_000) % 16) as u32);
    if mix(i + 20_000_000) % 2 == 1 { -v } else { v }
}

fn main() {
    let text = (0..2_000_000).map(|i| value(i).to_string()).collect::<Vec<_>>().join("\n");
    let xs: Vec<i64> = text.lines().map(|s| s.parse().unwrap()).collect();
    let neg = xs.iter().filter(|&&x| x < 0).count();
    println!("{} {} {} {}", xs.iter().sum::<i64>(), neg, xs.iter().max().unwrap(), xs.iter().min().unwrap());
}
