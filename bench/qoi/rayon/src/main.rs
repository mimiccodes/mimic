// qoi with Rayon: the 5 rounds in parallel (each is sequential); a 1024x768 RGBA image encoded as QOI (qoiformat.org) and decoded
// again, 5 times; prints the encoded size, a checksum of the bytes and
// whether the decoded pixels equal the original (see main.c)

const W: i64 = 1024;
const H: i64 = 768;

fn rgba(r: u32, g: u32, b: u32, a: u32) -> u32 { r << 24 | g << 16 | b << 8 | a }
fn slot(p: u32) -> usize { (((p >> 24) * 3 + (p >> 16 & 255) * 5 + (p >> 8 & 255) * 7 + (p & 255) * 11) % 64) as usize }

fn pixel(i: i64) -> u32 {
    let (x, y) = (i % W, i / W);
    let noise = (i * 1103515245 + 12345) % 2147483648 / 65536;
    let band = y * 10 / H;
    if band < 2 { rgba(40, 90, 200, 255) }
    else if band < 5 { rgba((x * 3 % 256) as u32, (y * 5 % 256) as u32, ((x + y) % 256) as u32, 255) }
    else if band < 7 { [0xff0000ffu32, 0x00ff00ff, 0x0000ffff][((x / 5 + y) % 3) as usize] }
    else if band < 9 { rgba((noise % 256) as u32, (noise / 256 % 256) as u32, (noise / 7 % 256) as u32, 255) }
    else { rgba(((120 + x) % 256) as u32, 60, ((30 + y) % 256) as u32, (x * 4 % 256) as u32) }
}

pub fn encode(px: &[u32]) -> Vec<u8> {
    let mut out = vec![b'q', b'o', b'i', b'f', 0, 0, (W >> 8) as u8, (W & 255) as u8, 0, 0, (H >> 8) as u8, (H & 255) as u8, 4, 0];
    let (mut seen, mut prev, mut run) = ([0u32; 64], rgba(0, 0, 0, 255), 0u8);
    let n = px.len();
    for (i, &p) in px.iter().enumerate() {
        if p == prev {
            run += 1;
            if run == 62 || i == n - 1 { out.push(192 | (run - 1)); run = 0; }
            continue;
        }
        if run > 0 { out.push(192 | (run - 1)); run = 0; }
        let h = slot(p);
        if seen[h] == p { out.push(h as u8); } else {
            seen[h] = p;
            if p & 255 == prev & 255 {
                let d = |s: u32| ((p >> s & 255) as u8).wrapping_sub((prev >> s & 255) as u8) as i8 as i32;
                let (dr, dg, db) = (d(24), d(16), d(8));
                let (dr_dg, db_dg) = (dr - dg, db - dg);
                if dr > -3 && dr < 2 && dg > -3 && dg < 2 && db > -3 && db < 2 { out.push((64 | (dr + 2) << 4 | (dg + 2) << 2 | (db + 2)) as u8); }
                else if dr_dg > -9 && dr_dg < 8 && dg > -33 && dg < 32 && db_dg > -9 && db_dg < 8 { out.push((128 | (dg + 32)) as u8); out.push(((dr_dg + 8) << 4 | (db_dg + 8)) as u8); }
                else { out.extend_from_slice(&[254, (p >> 24) as u8, (p >> 16) as u8, (p >> 8) as u8]); }
            } else { out.extend_from_slice(&[255, (p >> 24) as u8, (p >> 16) as u8, (p >> 8) as u8, p as u8]); }
        }
        prev = p;
    }
    out.extend_from_slice(&[0, 0, 0, 0, 0, 0, 0, 1]);
    out
}

pub fn decode(d: &[u8], n: usize) -> Vec<u32> {
    let mut px = Vec::with_capacity(n);
    let (mut seen, mut prev, mut pos) = ([0u32; 64], rgba(0, 0, 0, 255), 14usize);
    while px.len() < n {
        let b = d[pos];
        let ch = |v: u32, s: u32| (v >> s & 255) as i32;
        let p = if b == 254 { pos += 4; rgba(d[pos - 3] as u32, d[pos - 2] as u32, d[pos - 1] as u32, prev & 255) }
        else if b == 255 { pos += 5; rgba(d[pos - 4] as u32, d[pos - 3] as u32, d[pos - 2] as u32, d[pos - 1] as u32) }
        else if b >> 6 == 0 { pos += 1; seen[b as usize] }
        else if b >> 6 == 1 { pos += 1; let f = |s: u32, k: u8| ((ch(prev, s) + (k & 3) as i32 - 2) & 255) as u32; rgba(f(24, b >> 4), f(16, b >> 2), f(8, b), prev & 255) }
        else if b >> 6 == 2 { let dg = (b & 63) as i32 - 32; let m = d[pos + 1] as i32; pos += 2; rgba(((ch(prev, 24) + dg + (m >> 4) - 8) & 255) as u32, ((ch(prev, 16) + dg) & 255) as u32, ((ch(prev, 8) + dg + (m & 15) - 8) & 255) as u32, prev & 255) }
        else { pos += 1; for _ in 0..(b & 63) + 1 { px.push(prev); } continue; };
        seen[slot(p)] = p;
        px.push(p);
        prev = p;
    }
    px
}

fn main() {
    let px: Vec<u32> = (0..W * H).map(pixel).collect();
    use rayon::prelude::*;
    let rounds: Vec<(Vec<u8>, bool)> = (0..5).into_par_iter().map(|_| { let d = encode(&px); let ok = decode(&d, px.len()) == px; (d, ok) }).collect();
    let ok = rounds.iter().all(|r| r.1);
    let data = rounds[4].0.clone();
    let check: i64 = data.iter().enumerate().map(|(i, &b)| b as i64 * (i as i64 % 251 + 1)).sum();
    println!("{} bytes\n{check}\n{ok}", data.len());
}
