// wordwrap: 2,000,000 words (from splitmix64) joined by spaces into one
// text, then wrapped greedily at 72 columns. Prints the number of lines,
// the sum over the lines of the square of the room left, and the length of
// the longest line.

const WIDTH: i64 = 72;

fn mix(v: i64) -> i64 {
    let mut x = (v as u64 + 1).wrapping_mul(0x9E3779B97F4A7C15);
    x = (x ^ x >> 30).wrapping_mul(0xBF58476D1CE4E5B9);
    x = (x ^ x >> 27).wrapping_mul(0x94D049BB133111EB);
    ((x ^ x >> 31) >> 1) as i64
}

fn main() {
    let text = (0..2_000_000i64)
        .map(|k| ((97 + mix(k) % 26) as u8 as char).to_string().repeat(1 + (mix(k) % 12) as usize))
        .collect::<Vec<_>>()
        .join(" ");
    let (mut line, mut lines, mut ragged, mut longest) = (0i64, 0i64, 0i64, 0i64);
    for w in text.split_whitespace() {
        let l = w.len() as i64;
        if line == 0 {
            line = l;
        } else if line + 1 + l <= WIDTH {
            line += 1 + l;
        } else {
            lines += 1;
            ragged += (WIDTH - line) * (WIDTH - line);
            longest = longest.max(line);
            line = l;
        }
    }
    println!("{} {} {}", lines + 1, ragged + (WIDTH - line) * (WIDTH - line), longest.max(line));
}
