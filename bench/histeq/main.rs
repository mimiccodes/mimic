// histeq: histogram equalization of six 3840×2160 8-bit frames of low
// contrast. Prints, per frame, the sum of the result and a position-weighted
// checksum.

const W: i64 = 3840;
const H: i64 = 2160;

fn main() {
    let n = W * H;
    let mut img = vec![0u8; n as usize];
    let mut out = vec![0u8; n as usize];
    for f in 0..6i64 {
        for i in 0..n {
            let (x, y) = (i % W, i / W);
            let noise = ((i + f * 7919) * 1103515245 + 12345) % 2147483648 / 65536 % 32;
            img[i as usize] = (64 + (x * 7 + y * 13 + x * y % 31 + noise + f * 3) % 128) as u8;
        }
        let mut hist = [0i64; 256];
        for &p in &img {
            hist[p as usize] += 1;
        }
        let mut cdf = [0i64; 256];
        let (mut run, mut m) = (0i64, -1i64);
        for v in 0..256 {
            run += hist[v];
            cdf[v] = run;
            if m < 0 && hist[v] > 0 {
                m = run;
            }
        }
        let lut: Vec<u8> = cdf.iter().map(|&c| (((c - m) * 255 + (n - m) / 2).div_euclid(n - m)).max(0) as u8).collect();
        let (mut sum, mut check) = (0i64, 0i64);
        for i in 0..n as usize {
            let o = lut[img[i] as usize];
            out[i] = o;
            sum += o as i64;
            check += o as i64 * (i as i64 % 251 + 1);
        }
        println!("{} {} {}", f, sum, check);
    }
}
