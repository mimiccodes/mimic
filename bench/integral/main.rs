// integral: a 2048×2048 8-bit image, its summed-area table (with a row and
// a column of zeros in front), and 8,000,000 box sums from it (corners from
// splitmix64). Prints the total of the box sums and the image's sum.

const W: usize = 2048;
const H: usize = 2048;

fn mix(v: i64) -> i64 {
    let mut x = (v as u64 + 1).wrapping_mul(0x9E3779B97F4A7C15);
    x = (x ^ x >> 30).wrapping_mul(0xBF58476D1CE4E5B9);
    x = (x ^ x >> 27).wrapping_mul(0x94D049BB133111EB);
    ((x ^ x >> 31) >> 1) as i64
}

fn main() {
    let img: Vec<u8> = (0..(W * H) as i64)
        .map(|i| {
            let (x, y) = (i % W as i64, i / W as i64);
            let noise = (i * 1103515245 + 12345) % 2147483648 / 65536 % 32;
            ((x * 7 + y * 13 + x * y % 31 + noise) % 256) as u8
        })
        .collect();
    let mut sat = vec![0i64; (W + 1) * (H + 1)];
    for y in 1..=H {
        let mut row = 0i64;
        for x in 1..=W {
            row += img[(y - 1) * W + x - 1] as i64;
            sat[y * (W + 1) + x] = sat[(y - 1) * (W + 1) + x] + row;
        }
    }
    let at = |x: i64, y: i64| sat[y as usize * (W + 1) + x as usize];
    let mut total = 0i64;
    for q in 0..8_000_000i64 {
        let (a, b) = (mix(4 * q) % W as i64, mix(4 * q + 1) % W as i64);
        let (c, d) = (mix(4 * q + 2) % H as i64, mix(4 * q + 3) % H as i64);
        let (x1, x2, y1, y2) = (a.min(b), a.max(b), c.min(d), c.max(d));
        total += at(x2 + 1, y2 + 1) - at(x2 + 1, y1) - at(x1, y2 + 1) + at(x1, y1);
    }
    println!("{} {}", total, sat[(W + 1) * (H + 1) - 1]);
}
