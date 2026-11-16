// filters: a 1920x1080 8-bit image through five filters (edges clamped):
// 3x3 box blur, 5x5 Gaussian, Sobel, 3x3 median, integer bilateral; prints
// each result's sum and a position-weighted checksum (see main.c)

const W: i64 = 1920;
const H: i64 = 1080;

fn at(img: &[u8], x: i64, y: i64) -> i64 {
    img[(y.clamp(0, H - 1) * W + x.clamp(0, W - 1)) as usize] as i64
}

fn report(name: &str, out: &[u8]) {
    let sum: i64 = out.iter().map(|&b| b as i64).sum();
    let check: i64 = out.iter().enumerate().map(|(i, &b)| b as i64 * (i as i64 % 251 + 1)).sum();
    println!("{name} {sum} {check}");
}

fn filter(img: &[u8], f: impl Fn(&[u8], i64, i64, i64) -> i64) -> Vec<u8> {
    (0..W * H).map(|i| f(img, i % W, i / W, i) as u8).collect()
}

fn main() {
    let img: Vec<u8> = (0..W * H)
        .map(|i| { let (x, y) = (i % W, i / W); let noise = (i * 1103515245 + 12345) % 2147483648 / 65536 % 32; ((x * 7 + y * 13 + x * y % 31 + noise) % 256) as u8 })
        .collect();
    report("box", &filter(&img, |m, x, y, _| (0..9).map(|j| at(m, x + j % 3 - 1, y + j / 3 - 1)).sum::<i64>() / 9));
    let g = [1i64, 4, 6, 4, 1];
    report("gaussian", &filter(&img, |m, x, y, _| (0..25).map(|j| g[(j / 5) as usize] * g[(j % 5) as usize] * at(m, x + j % 5 - 2, y + j / 5 - 2)).sum::<i64>() / 256));
    let (sx, sy) = ([-1i64, 0, 1, -2, 0, 2, -1, 0, 1], [-1i64, -2, -1, 0, 0, 0, 1, 2, 1]);
    report("sobel", &filter(&img, |m, x, y, _| {
        let (mut a, mut b) = (0, 0);
        for j in 0..9 { let p = at(m, x + j % 3 - 1, y + j / 3 - 1); a += sx[j as usize] * p; b += sy[j as usize] * p; }
        (a.abs() + b.abs()).min(255)
    }));
    report("median", &filter(&img, |m, x, y, _| {
        let mut v = [0i64; 9];
        for j in 0..9 { v[j as usize] = at(m, x + j % 3 - 1, y + j / 3 - 1); }
        v.sort_unstable();
        v[4]
    }));
    let sw = [1i64, 2, 1, 2, 4, 2, 1, 2, 1];
    report("bilateral", &filter(&img, |m, x, y, i| {
        let p = m[i as usize] as i64;
        let (mut num, mut den) = (0, 0);
        for j in 0..9 { let q = at(m, x + j % 3 - 1, y + j / 3 - 1); let wt = sw[j as usize] * (32 - (q - p).abs() / 2).max(0); num += wt * q; den += wt; }
        (num + den / 2) / den
    }));
}
