// intervals: 2,000,000 intervals (from splitmix64) sorted by start and
// merged where they overlap or touch. Prints the number of merged
// intervals, the total length they cover, and the longest.

fn mix(v: i64) -> i64 {
    let mut x = (v as u64 + 1).wrapping_mul(0x9E3779B97F4A7C15);
    x = (x ^ x >> 30).wrapping_mul(0xBF58476D1CE4E5B9);
    x = (x ^ x >> 27).wrapping_mul(0x94D049BB133111EB);
    ((x ^ x >> 31) >> 1) as i64
}

fn main() {
    let mut iv: Vec<(i64, i64)> = (0..2_000_000i64).map(|k| (mix(2 * k) % 1_000_000_000, 1 + mix(2 * k + 1) % 1000)).collect();
    iv.sort_unstable();
    let mut merged: Vec<(i64, i64)> = Vec::new();
    for (s, l) in iv {
        match merged.last_mut() {
            Some(last) if s <= last.1 => last.1 = last.1.max(s + l),
            _ => merged.push((s, s + l)),
        }
    }
    let lens: Vec<i64> = merged.iter().map(|(s, e)| e - s).collect();
    println!("{} {} {}", merged.len(), lens.iter().sum::<i64>(), lens.iter().max().unwrap());
}
