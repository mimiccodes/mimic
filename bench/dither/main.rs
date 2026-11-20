// dither: 24 frames of 1280×720 8-bit pixels, each dithered to black and
// white by Floyd–Steinberg error diffusion (shifts floor). Prints, per
// frame, the number of white pixels and a position-weighted checksum.

const W: usize = 1280;
const H: usize = 720;

fn main() {
    let n = W * H;
    let mut buf = vec![0i64; n];
    for f in 0..24i64 {
        for i in 0..n {
            let (x, y) = ((i % W) as i64, (i / W) as i64);
            let noise = ((i as i64 + f * 7919) * 1103515245 + 12345) % 2147483648 / 65536 % 32;
            buf[i] = (x * 7 + y * 13 + x * y % 31 + noise + f * 3) % 256;
        }
        let (mut white, mut check) = (0i64, 0i64);
        for i in 0..n {
            let old = buf[i];
            let nw = if old >= 128 { 255 } else { 0 };
            let e = old - nw;
            let x = i % W;
            if x + 1 < W {
                buf[i + 1] += e * 7 >> 4;
            }
            if i + W < n {
                if x > 0 {
                    buf[i + W - 1] += e * 3 >> 4;
                }
                buf[i + W] += e * 5 >> 4;
                if x + 1 < W {
                    buf[i + W + 1] += e >> 4;
                }
            }
            if nw != 0 {
                white += 1;
                check += i as i64 % 251 + 1;
            }
        }
        println!("{} {} {}", f, white, check);
    }
}
