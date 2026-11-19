// roman: 1,000,000 numbers (from splitmix64) written as Roman numerals
// (greedily) and read back. Prints the total length of the numerals, the
// sum of the values read back, and how many numerals contain "MM".

const SYMS: [(i64, &str); 13] =
    [(1000, "M"), (900, "CM"), (500, "D"), (400, "CD"), (100, "C"), (90, "XC"), (50, "L"), (40, "XL"), (10, "X"), (9, "IX"), (5, "V"), (4, "IV"), (1, "I")];

fn mix(v: i64) -> i64 {
    let mut x = (v as u64 + 1).wrapping_mul(0x9E3779B97F4A7C15);
    x = (x ^ x >> 30).wrapping_mul(0xBF58476D1CE4E5B9);
    x = (x ^ x >> 27).wrapping_mul(0x94D049BB133111EB);
    ((x ^ x >> 31) >> 1) as i64
}

fn roman(mut n: i64) -> String {
    let mut out = String::new();
    for &(v, s) in &SYMS {
        while n >= v {
            out.push_str(s);
            n -= v;
        }
    }
    out
}

fn digit(c: u8) -> i64 {
    match c {
        b'I' => 1,
        b'V' => 5,
        b'X' => 10,
        b'L' => 50,
        b'C' => 100,
        b'D' => 500,
        _ => 1000,
    }
}

fn value(r: &[u8]) -> i64 {
    (0..r.len()).map(|i| if i + 1 < r.len() && digit(r[i]) < digit(r[i + 1]) { -digit(r[i]) } else { digit(r[i]) }).sum()
}

fn main() {
    let rs: Vec<String> = (0..1_000_000).map(|k| roman(1 + mix(k) % 3999)).collect();
    let lengths: usize = rs.iter().map(|r| r.len()).sum();
    let back: i64 = rs.iter().map(|r| value(r.as_bytes())).sum();
    println!("{} {} {}", lengths, back, rs.iter().filter(|r| r.contains("MM")).count());
}
