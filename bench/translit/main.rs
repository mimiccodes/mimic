// translit: see main.mim.
fn main() {
    let alphabet = b"abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789 .";
    let n: i64 = 8_000_000;
    let text: Vec<u8> = (0..n).map(|k| alphabet[((k * k * 7 + k * 13) % 64) as usize]).collect();
    let mut out: Vec<u8> = Vec::with_capacity((n * 2) as usize);
    for &c in &text {
        match c {
            b'a'..=b'z' => out.push(b'a' + (c - b'a' + 13) % 26),
            b'A'..=b'Z' => out.push(b'A' + (c - b'A' + 13) % 26),
            b'0'..=b'9' => {
                out.push(c);
                out.push(c);
            }
            b' ' => out.push(b'_'),
            _ => {}
        }
    }
    let sum: i64 = out.iter().enumerate().map(|(i, &b)| b as i64 * (i as i64 % 251 + 1)).sum();
    println!("{}\n{}", out.len(), sum);
}
