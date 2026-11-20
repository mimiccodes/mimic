// lz77: 512 KiB of generated text compressed with greedy LZ77 (a hash table
// of 3-byte prefixes, matches of 4 to 255 bytes up to 64 KiB back), then
// decompressed and compared. Prints the literals, the matches, the bytes
// covered by matches, and 1 when the round trip is exact.

fn main() {
    let n: usize = 1 << 19;
    let words = ["the", "quick", "brown", "fox", "jumps", "over", "lazy", "dog", "array", "parallel", "mimic", "kernel", "fusion", "vector", "memory", "cache"];
    let mut d: Vec<u8> = Vec::with_capacity(n + 16);
    let mut i: i64 = 0;
    while d.len() < n {
        d.extend_from_slice(words[((i * 2654435761 >> 13) % 16) as usize].as_bytes());
        d.push(b' ');
        i += 1;
    }
    d.truncate(n);
    let mut table = vec![0u32; 65536];
    let mut toks: Vec<(usize, usize)> = Vec::new();
    let (mut lits, mut matches, mut covered) = (0, 0, 0);
    let mut p = 0;
    while p < n {
        if p + 3 <= n {
            let h = ((((d[p] as i64) << 16 | (d[p + 1] as i64) << 8 | d[p + 2] as i64) * 2654435761 >> 15) & 65535) as usize;
            let c = table[h] as i64 - 1;
            table[h] = (p + 1) as u32;
            if c >= 0 && p as i64 - c <= 65535 {
                let c = c as usize;
                let mut l = 0;
                while l < 255 && p + l < n && d[c + l] == d[p + l] {
                    l += 1;
                }
                if l >= 4 {
                    toks.push((l, p - c));
                    matches += 1;
                    covered += l;
                    p += l;
                    continue;
                }
            }
        }
        toks.push((0, d[p] as usize));
        lits += 1;
        p += 1;
    }
    let mut out: Vec<u8> = Vec::with_capacity(n);
    for &(l, x) in &toks {
        if l == 0 {
            out.push(x as u8);
        } else {
            for _ in 0..l {
                out.push(out[out.len() - x]);
            }
        }
    }
    println!("{lits} {matches} {covered} {}", (out == d) as i32);
}
