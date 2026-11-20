// bwt: 256 KiB of generated text in 32 blocks of 8 KiB; for each block the
// Burrows-Wheeler transform (rotations sorted by prefix doubling),
// move-to-front over the last column, and the block back from its
// transform. Prints the sum of the primary indices, a checksum of the
// transforms, the zeros move-to-front made, and 1 when every block came back.

const B: usize = 8192;

fn main() {
    let blocks = 32;
    let n = B * blocks;
    let words = ["the", "of", "and", "to", "in", "is", "parallel", "array", "kernel", "fusion", "memory", "vector", "mimic", "cache", "thread", "core", "loop", "value", "zebra", "quartz"];
    let mut d: Vec<u8> = Vec::with_capacity(n + 32);
    let mut i: i64 = 0;
    while d.len() < n {
        let k = if i % 7 != 0 { (i * 2654435761 >> 13) % 20 } else { (i * 40503 >> 7) % 20 };
        d.extend_from_slice(words[k as usize].as_bytes());
        if i % 11 == 10 {
            d.push(b'.');
        }
        d.push(b' ');
        i += 1;
    }
    let (mut primary, mut check, mut zeros, mut same) = (0usize, 0i64, 0i64, true);
    for blk in d[..n].chunks_exact(B) {
        let mut rank: Vec<i64> = blk.iter().map(|&b| b as i64).collect();
        let mut step = 1;
        loop {
            let key: Vec<i64> = (0..B).map(|i| rank[i] * 1048576 + rank[(i + step) % B]).collect();
            let mut sa: Vec<usize> = (0..B).collect();
            sa.sort_by_key(|&i| key[i]);
            let mut g = 0;
            for j in 0..B {
                if j > 0 && key[sa[j]] != key[sa[j - 1]] {
                    g += 1;
                }
                rank[sa[j]] = g;
            }
            if g == B as i64 - 1 || step * 2 >= B {
                break;
            }
            step *= 2;
        }
        let mut sa: Vec<usize> = (0..B).collect();
        sa.sort_by_key(|&i| rank[i]);
        let last: Vec<u8> = sa.iter().map(|&i| blk[(i + B - 1) % B]).collect();
        let p = sa.iter().position(|&i| i == 0).unwrap();
        primary += p;
        check = (check + last.iter().enumerate().fold(0i64, |c, (j, &b)| (c * 31 + b as i64 + j as i64) % 1000000007)) % 1000000007;
        let mut table: Vec<u8> = (0..=255).collect();
        for &x in &last {
            let r = table.iter().position(|&t| t == x).unwrap();
            zeros += (r == 0) as i64;
            table.remove(r);
            table.insert(0, x);
        }
        let mut order: Vec<usize> = (0..B).collect();
        order.sort_by_key(|&j| last[j]);
        let mut out = Vec::with_capacity(B);
        let mut row = order[p];
        for _ in 0..B {
            out.push(last[row]);
            row = order[row];
        }
        same &= out == blk;
    }
    println!("{primary} {check} {zeros} {}", same as i32);
}
