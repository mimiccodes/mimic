// rotate: six 3840×2160 8-bit frames, each turned 90° clockwise four times;
// after each turn, a position-weighted checksum.

const W: usize = 3840;
const H: usize = 2160;

// a w×h image turned clockwise into out (h wide, w high)
fn turn(img: &[u8], out: &mut [u8], w: usize, h: usize) {
    for y in 0..w {
        for x in 0..h {
            out[y * h + x] = img[(h - 1 - x) * w + y];
        }
    }
}

fn check(img: &[u8]) -> i64 {
    img.iter().enumerate().map(|(i, &p)| p as i64 * (i as i64 % 251 + 1)).sum()
}

fn main() {
    let n = W * H;
    let mut a = vec![0u8; n];
    let mut b = vec![0u8; n];
    for f in 0..6i64 {
        for i in 0..n as i64 {
            let (x, y) = (i % W as i64, i / W as i64);
            let noise = ((i + f * 7919) * 1103515245 + 12345) % 2147483648 / 65536 % 32;
            a[i as usize] = ((x * 7 + y * 13 + x * y % 31 + noise + f * 3) % 256) as u8;
        }
        let mut s = [0i64; 4];
        for k in 0..4 {
            let (w, h) = if k % 2 == 1 { (H, W) } else { (W, H) };
            turn(&a, &mut b, w, h);
            s[k] = check(&b);
            std::mem::swap(&mut a, &mut b);
        }
        println!("{} {} {} {} {}", f, s[0], s[1], s[2], s[3]);
    }
}
