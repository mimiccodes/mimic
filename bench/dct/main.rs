// dct: the 8 × 8 two-dimensional DCT (as JPEG uses it) of every block of
// ten 1920 × 1080 8-bit frames, in fixed point (a cosine table scaled by
// 4096): the rows, then the columns, then >> 24 rounded down. Prints the
// sum of the absolute values of the coefficients and a position-weighted
// checksum.

const W: usize = 1920;
const H: usize = 1080;
const C: [i64; 64] = [
    1448, 1448, 1448, 1448, 1448, 1448, 1448, 1448, 2009, 1703, 1138, 400, -400, -1138, -1703, -2009, 1892, 784, -784, -1892, -1892, -784, 784, 1892,
    1703, -400, -2009, -1138, 1138, 2009, 400, -1703, 1448, -1448, -1448, 1448, 1448, -1448, -1448, 1448, 1138, -2009, 400, 1703, -1703, -400, 2009,
    -1138, 784, -1892, 1892, -784, -784, 1892, -1892, 784, 400, -1138, 1703, -2009, 2009, -1703, 1138, -400,
];

fn main() {
    let n = W * H;
    let (mut b, mut r, mut out) = (vec![0i64; n], vec![0i64; n], vec![0i64; n]);
    let (mut total, mut check) = (0i64, 0i64);
    for f in 0..10i64 {
        for i in 0..n {
            let (x, y, ii) = ((i % W) as i64, (i / W) as i64, i as i64);
            let noise = ((ii + f * 7919) * 1103515245 + 12345) % 2147483648 / 65536 % 32;
            b[i] = (x * 7 + y * 13 + x * y % 31 + noise + f * 3) % 256 - 128;
        }
        for i in 0..n {
            let (u, start) = (i % 8, i - i % 8);
            r[i] = (0..8).map(|x| C[u * 8 + x] * b[start + x]).sum();
        }
        for i in 0..n {
            let (v, top) = (i / W % 8, i - i / W % 8 * W);
            let s: i64 = (0..8).map(|y| C[v * 8 + y] * r[top + y * W]).sum();
            out[i] = (s + (1 << 23)) >> 24;
        }
        for i in 0..n {
            total += out[i].abs();
            check += out[i] * (i as i64 % 251 + 1);
        }
    }
    println!("{} {}", total, check);
}
