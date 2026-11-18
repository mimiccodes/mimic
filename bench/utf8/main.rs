// utf8: 20,000 generated documents encoded as UTF-8 and validated by a
// strict state machine (RFC 3629), one byte at a time (the same machine as
// the other versions, not the standard library's); every fifth has a byte
// replaced by 0xFF, the one after loses its last byte. Prints the valid
// documents, their characters, and all the bytes checked.

fn mix(v: i64) -> i64 {
    let mut x = (v as u64 + 1).wrapping_mul(0x9E3779B97F4A7C15);
    x = (x ^ x >> 30).wrapping_mul(0xBF58476D1CE4E5B9);
    x = (x ^ x >> 27).wrapping_mul(0x94D049BB133111EB);
    ((x ^ x >> 31) >> 1) as i64
}

fn ch(r: i64) -> u32 {
    let (kind, v) = (r % 10, r >> 8);
    (if kind < 6 {
        0x20 + v % 95
    } else if kind < 8 {
        0xA0 + v % (0x800 - 0xA0)
    } else if kind < 9 {
        let c = 0x800 + v % (0x10000 - 0x800);
        if (0xD800..=0xDFFF).contains(&c) { c + 0x800 } else { c }
    } else {
        0x10000 + v % 0x100000
    }) as u32
}

fn check(b: &[u8]) -> Option<usize> {
    let (mut need, mut lo, mut hi, mut c) = (0, 0x80u8, 0xBFu8, 0);
    for &x in b {
        if need > 0 {
            if x < lo || x > hi {
                return None;
            }
            need -= 1;
            if need == 0 {
                c += 1;
            }
            (lo, hi) = (0x80, 0xBF);
        } else {
            match x {
                0x00..=0x7F => c += 1,
                0xC2..=0xDF => (need, lo, hi) = (1, 0x80, 0xBF),
                0xE0 => (need, lo, hi) = (2, 0xA0, 0xBF),
                0xED => (need, lo, hi) = (2, 0x80, 0x9F),
                0xE1..=0xEF => (need, lo, hi) = (2, 0x80, 0xBF),
                0xF0 => (need, lo, hi) = (3, 0x90, 0xBF),
                0xF1..=0xF3 => (need, lo, hi) = (3, 0x80, 0xBF),
                0xF4 => (need, lo, hi) = (3, 0x80, 0x8F),
                _ => return None,
            }
        }
    }
    (need == 0).then_some(c)
}

fn main() {
    let (mut valid, mut chars, mut total) = (0, 0, 0);
    for d in 0..20000i64 {
        let n = 50 + mix(d * 7777) % 251;
        let mut b = Vec::with_capacity(1300);
        let mut buf = [0u8; 4];
        for i in 0..n {
            b.extend_from_slice(char::from_u32(ch(mix(d * 100000 + i))).unwrap().encode_utf8(&mut buf).as_bytes());
        }
        if d % 5 == 0 {
            let at = (mix(d * 31) % b.len() as i64) as usize;
            b[at] = 0xFF;
        } else if d % 5 == 1 {
            b.pop();
        }
        total += b.len();
        if let Some(c) = check(&b) {
            valid += 1;
            chars += c;
        }
    }
    println!("{valid} {chars} {total}");
}
