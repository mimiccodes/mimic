// wordfreq: about 1,000,000 generated words with spaces, commas, full
// stops and line breaks. The words are found where letters start and
// stop, each hashed with FNV-1a, the hashes sorted and the runs of equal
// ones counted. Prints the words, the distinct words, and how often the
// most common one occurs.

fn mix(v: i64) -> i64 {
    let mut x = (v as u64 + 1).wrapping_mul(0x9E3779B97F4A7C15);
    x = (x ^ x >> 30).wrapping_mul(0xBF58476D1CE4E5B9);
    x = (x ^ x >> 27).wrapping_mul(0x94D049BB133111EB);
    ((x ^ x >> 31) >> 1) as i64
}

fn main() {
    let (cons, vowels) = (b"bcdfghklmnprstvz", b"aeiou");
    let mut t: Vec<u8> = Vec::with_capacity(1000000 * 10);
    for i in 0..1000000i64 {
        let v = mix(2 * i) % (1 + mix(2 * i + 1) % 6000);
        for k in 0..1 + v % 4 {
            t.push(cons[((v >> (3 * k + 2)) % 16) as usize]);
            t.push(vowels[((v >> (3 * k + 7)) % 5) as usize]);
        }
        t.extend_from_slice(if i % 15 == 14 { b"\n" } else if i % 13 == 12 { b". " } else if i % 13 == 5 { b", " } else { b" " });
    }
    let mut h: Vec<u64> = t
        .split(|c| !c.is_ascii_lowercase())
        .filter(|w| !w.is_empty())
        .map(|w| w.iter().fold(0xCBF29CE484222325u64, |x, &c| (x ^ c as u64).wrapping_mul(0x100000001B3)))
        .collect();
    h.sort_unstable();
    let (mut distinct, mut most, mut run) = (0, 0, 0);
    for i in 0..h.len() {
        if i == 0 || h[i] != h[i - 1] {
            distinct += 1;
            run = 0;
        }
        run += 1;
        most = most.max(run);
    }
    println!("{} {distinct} {most}", h.len());
}
