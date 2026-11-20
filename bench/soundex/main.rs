// soundex: 1,000,000 generated names coded by American Soundex, each code
// one number (letter × 1000 + digits); the codes sorted and counted in
// runs. Prints how many distinct codes there are, the most common one and
// how often it occurs, and the sum of the digits of all the codes.

fn mix(v: i64) -> i64 {
    let mut x = (v as u64 + 1).wrapping_mul(0x9E3779B97F4A7C15);
    x = (x ^ x >> 30).wrapping_mul(0xBF58476D1CE4E5B9);
    x = (x ^ x >> 27).wrapping_mul(0x94D049BB133111EB);
    ((x ^ x >> 31) >> 1) as i64
}

fn digit(c: u8) -> i64 {
    match c {
        b'b' | b'f' | b'p' | b'v' => 1,
        b'c' | b'g' | b'j' | b'k' | b'q' | b's' | b'x' | b'z' => 2,
        b'd' | b't' => 3,
        b'l' => 4,
        b'm' | b'n' => 5,
        b'r' => 6,
        _ => 0,
    }
}

fn code(name: &str) -> i64 {
    let b = name.to_lowercase().into_bytes();
    let (mut last, mut out, mut got) = (digit(b[0]), 0i64, 0);
    for &c in &b[1..] {
        if got == 3 {
            break;
        }
        if c == b'h' || c == b'w' {
            continue;
        }
        let d = digit(c);
        if d != 0 && d != last {
            out = out * 10 + d;
            got += 1;
        }
        last = d;
    }
    while got < 3 {
        out *= 10;
        got += 1;
    }
    (b[0] - b'a') as i64 * 1000 + out
}

fn main() {
    let mut codes: Vec<i64> = (0..1_000_000i64)
        .map(|k| {
            let s: String = (0..4 + mix(k) % 7).map(|j| (97 + mix(16 * k + j) % 26) as u8 as char).collect();
            let name = s[..1].to_uppercase() + &s[1..];
            code(&name)
        })
        .collect();
    codes.sort_unstable();
    let (mut distinct, mut most, mut top, mut i) = (0i64, 0usize, 0i64, 0usize);
    while i < codes.len() {
        let mut j = i;
        while j < codes.len() && codes[j] == codes[i] {
            j += 1;
        }
        distinct += 1;
        if j - i > most {
            most = j - i;
            top = codes[i];
        }
        i = j;
    }
    let digits: i64 = codes.iter().map(|c| c % 10 + c / 10 % 10 + c / 100 % 10).sum();
    println!("{} {}{:03} {} {}", distinct, (b'A' + (top / 1000) as u8) as char, top % 1000, most, digits);
}
