// lzw: 4,000,000 bytes (letters a-e in runs, from splitmix64) compressed
// by LZW with 12-bit codes (the dictionary a table child[code * 256 +
// byte], frozen once full) and decompressed again (prefix, last byte,
// first byte and length per code). Prints the number of codes, a
// position-weighted checksum, and 1 when decompressing gives the bytes
// back.

const N: usize = 4_000_000;
const CODES: usize = 4096;

fn mix(v: i64) -> i64 {
    let mut x = (v as u64 + 1).wrapping_mul(0x9E3779B97F4A7C15);
    x = (x ^ x >> 30).wrapping_mul(0xBF58476D1CE4E5B9);
    x = (x ^ x >> 27).wrapping_mul(0x94D049BB133111EB);
    ((x ^ x >> 31) >> 1) as i64
}

fn main() {
    let data: Vec<u8> = (0..N as i64).map(|i| (97 + mix(i / 4 + mix(i / 64) % 3) % 5) as u8).collect();
    let mut child = vec![-1i32; CODES * 256];
    let mut codes: Vec<usize> = Vec::new();
    let (mut next, mut w) = (256usize, data[0] as usize);
    for &b in &data[1..] {
        let ch = b as usize;
        let k = child[w * 256 + ch];
        if k >= 0 {
            w = k as usize;
            continue;
        }
        codes.push(w);
        if next < CODES {
            child[w * 256 + ch] = next as i32;
            next += 1;
        }
        w = ch;
    }
    codes.push(w);
    let check: i64 = codes.iter().enumerate().map(|(i, &c)| c as i64 * (i as i64 % 251 + 1)).sum();
    // decoding: each code's prefix, last and first byte, and length
    let mut prefix = vec![usize::MAX; CODES];
    let mut last: Vec<u8> = (0..CODES).map(|c| c as u8).collect();
    let mut first = last.clone();
    let mut len = vec![1usize; CODES];
    let mut out = vec![0u8; N];
    let (mut next, mut pos, mut prev) = (256usize, 0usize, usize::MAX);
    for &c in &codes {
        if prev != usize::MAX && next < CODES {
            prefix[next] = prev;
            last[next] = if c < next { first[c] } else { first[prev] };
            first[next] = first[prev];
            len[next] = len[prev] + 1;
            next += 1;
        }
        // the string of c, written from its end
        let (mut k, mut at) = (c, pos + len[c]);
        while k != usize::MAX {
            at -= 1;
            out[at] = last[k];
            k = prefix[k];
        }
        pos += len[c];
        prev = c;
    }
    println!("{} {} {}", codes.len(), check, (pos == N && out == data) as i32);
}
