// varint: 4,000,000 readings of a counter (mostly small steps, every 97th
// a jump; from splitmix64), stored as differences, zigzag-mapped and
// written as LEB128 bytes, then decoded again. Prints the number of bytes,
// a position-weighted checksum, and 1 when decoding gives the values back.

const N: i64 = 4_000_000;

fn mix(v: i64) -> i64 {
    let mut x = (v as u64 + 1).wrapping_mul(0x9E3779B97F4A7C15);
    x = (x ^ x >> 30).wrapping_mul(0xBF58476D1CE4E5B9);
    x = (x ^ x >> 27).wrapping_mul(0x94D049BB133111EB);
    ((x ^ x >> 31) >> 1) as i64
}

fn main() {
    let mut vals = Vec::with_capacity(N as usize);
    let mut v = 1_000_000i64;
    for k in 0..N {
        v += if k % 97 == 0 { mix(k) % 1_000_000_000 } else { mix(k) % 2001 - 1000 };
        vals.push(v);
    }
    let mut out: Vec<u8> = Vec::new();
    let mut prev = 0i64;
    for &x in &vals {
        let d = x - prev;
        prev = x;
        let mut z = ((d << 1) ^ (d >> 63)) as u64;
        while z >= 128 {
            out.push((z & 127 | 128) as u8);
            z >>= 7;
        }
        out.push(z as u8);
    }
    let check: i64 = out.iter().enumerate().map(|(i, &b)| b as i64 * (i as i64 % 251 + 1)).sum();
    let mut back = Vec::with_capacity(N as usize);
    let (mut z, mut shift, mut prev) = (0u64, 0u32, 0i64);
    for &b in &out {
        z |= ((b & 127) as u64) << shift;
        shift += 7;
        if b < 128 {
            prev += (z >> 1) as i64 ^ -((z & 1) as i64);
            back.push(prev);
            z = 0;
            shift = 0;
        }
    }
    println!("{} {} {}", out.len(), check, (back == vals) as i32);
}
