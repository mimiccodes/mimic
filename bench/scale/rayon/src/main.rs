// scale with Rayon: the frames, the sizes and each output's pixels in
// parallel; the same bilinear scaling as main.rs

use rayon::prelude::*;

fn frame(w: i64, h: i64, f: i64) -> Vec<u8> {
    (0..w * h).into_par_iter().map(|i| { let (x, y) = (i % w, i / w); ((x * 7 + y * 3 + x * y % 13 + f * 5) % 256) as u8 }).collect()
}

fn scale(src: &[u8], w: i64, h: i64, w2: i64, h2: i64) -> Vec<u8> {
    (0..w2 * h2)
        .into_par_iter()
        .map(|i| {
            let sx = i % w2 * (w - 1) * 256 / (w2 - 1);
            let sy = i / w2 * (h - 1) * 256 / (h2 - 1);
            let (x0, y0, fx, fy) = (sx >> 8, sy >> 8, sx & 255, sy & 255);
            let x1 = (x0 + 1).min(w - 1);
            let y1 = (y0 + 1).min(h - 1);
            let at = |x: i64, y: i64| src[(y * w + x) as usize] as i64;
            let top = at(x0, y0) * (256 - fx) + at(x1, y0) * fx;
            let bot = at(x0, y1) * (256 - fx) + at(x1, y1) * fx;
            ((top * (256 - fy) + bot * fy + 32768) >> 16) as u8
        })
        .collect()
}

fn main() {
    let sizes = [(1280, 720), (854, 480), (640, 360), (426, 240)];
    let total: i64 = (0..30i64)
        .into_par_iter()
        .map(|f| {
            let src = frame(1920, 1080, f);
            sizes.par_iter().map(|&(w2, h2)| scale(&src, 1920, 1080, w2, h2).iter().map(|&b| b as i64).sum::<i64>()).sum::<i64>()
        })
        .sum();
    println!("{total}");
}
