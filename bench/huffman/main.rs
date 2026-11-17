// huffman: 256 KiB of generated text Huffman-coded and decoded again (the
// tree merged by weight, then node number; canonical codes; packed into
// 64-bit words most significant bit first; decoded bit by bit). Prints the
// encoded size in bits, the longest code, a checksum of the packed words,
// and 1 when decoding gives the text back.

use std::cmp::Reverse;
use std::collections::BinaryHeap;

fn main() {
    let n: usize = 1 << 18;
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
    d.truncate(n);
    let mut count = [0i64; 256];
    for &b in &d {
        count[b as usize] += 1;
    }
    let mut heap: BinaryHeap<Reverse<(i64, usize)>> = (0..256).filter(|&b| count[b] > 0).map(|b| Reverse((count[b], b))).collect();
    let mut parent = vec![usize::MAX; 512];
    let mut next = 256;
    while heap.len() > 1 {
        let Reverse((w1, a)) = heap.pop().unwrap();
        let Reverse((w2, b)) = heap.pop().unwrap();
        parent[a] = next;
        parent[b] = next;
        heap.push(Reverse((w1 + w2, next)));
        next += 1;
    }
    let mut length = [0u32; 256];
    for b in 0..256 {
        if count[b] > 0 {
            let mut q = b;
            while parent[q] != usize::MAX {
                q = parent[q];
                length[b] += 1;
            }
        }
    }
    let mut order: Vec<usize> = (0..256).filter(|&b| count[b] > 0).collect();
    order.sort_by_key(|&b| (length[b], b));
    let mut code = [0u64; 256];
    let (mut c, mut prev) = (0u64, 0u32);
    for &b in &order {
        c <<= length[b] - prev;
        code[b] = c;
        c += 1;
        prev = length[b];
    }
    let bits: i64 = (0..256).map(|b| count[b] * length[b] as i64).sum();
    let mut w = vec![0u64; (bits as usize).div_ceil(64) + 1];
    let mut pos = 0usize;
    for &b in &d {
        let (l, cc) = (length[b as usize] as usize, code[b as usize]);
        let (wi, off) = (pos / 64, pos % 64);
        if off + l <= 64 {
            w[wi] |= cc << (64 - off - l);
        } else {
            let spill = off + l - 64;
            w[wi] |= cc >> spill;
            w[wi + 1] |= (cc & ((1 << spill) - 1)) << (64 - spill);
        }
        pos += l;
    }
    let nw = (bits as usize).div_ceil(64);
    let check = w[..nw].iter().fold(0u64, |a, &x| (a * 31 + x % 1000000007) % 1000000007);
    let mut table = vec![-1i16; 16 * 32768];
    for &b in &order {
        table[length[b] as usize * 32768 + code[b] as usize] = b as i16;
    }
    let mut out = Vec::with_capacity(n);
    let (mut cur, mut l) = (0usize, 0usize);
    for p in 0..bits as usize {
        cur = cur << 1 | (w[p / 64] >> (63 - p % 64) & 1) as usize;
        l += 1;
        let hit = table[l * 32768 + cur];
        if hit >= 0 {
            out.push(hit as u8);
            cur = 0;
            l = 0;
        }
    }
    println!("{bits} {} {check} {}", length.iter().max().unwrap(), (out == d) as i32);
}
