// siphash: SipHash-2-4 (key bytes 0-15) of 2,000,000 messages of 16 bytes
// (two 64-bit words from splitmix64 each), as a hash table hashes its keys.
// Prints the exclusive or of the hashes and their sum modulo 1,000,000,007.

fn mix(v: i64) -> u64 {
    let mut x = (v as u64 + 1).wrapping_mul(0x9E3779B97F4A7C15);
    x = (x ^ x >> 30).wrapping_mul(0xBF58476D1CE4E5B9);
    x = (x ^ x >> 27).wrapping_mul(0x94D049BB133111EB);
    (x ^ x >> 31) >> 1
}

fn round(v: &mut [u64; 4]) {
    v[0] = v[0].wrapping_add(v[1]);
    v[1] = v[1].rotate_left(13) ^ v[0];
    v[0] = v[0].rotate_left(32);
    v[2] = v[2].wrapping_add(v[3]);
    v[3] = v[3].rotate_left(16) ^ v[2];
    v[0] = v[0].wrapping_add(v[3]);
    v[3] = v[3].rotate_left(21) ^ v[0];
    v[2] = v[2].wrapping_add(v[1]);
    v[1] = v[1].rotate_left(17) ^ v[2];
    v[2] = v[2].rotate_left(32);
}

// two words of message, 16 bytes in all
fn siphash(k0: u64, k1: u64, m0: u64, m1: u64) -> u64 {
    let mut v = [k0 ^ 0x736F6D6570736575, k1 ^ 0x646F72616E646F6D, k0 ^ 0x6C7967656E657261, k1 ^ 0x7465646279746573];
    for m in [m0, m1, 16u64 << 56] {
        v[3] ^= m;
        round(&mut v);
        round(&mut v);
        v[0] ^= m;
    }
    v[2] ^= 0xFF;
    for _ in 0..4 {
        round(&mut v);
    }
    v[0] ^ v[1] ^ v[2] ^ v[3]
}

fn main() {
    let (k0, k1) = (0x0706050403020100u64, 0x0F0E0D0C0B0A0908u64);
    let (mut x, mut s) = (0u64, 0u64);
    for j in 0..2_000_000i64 {
        let h = siphash(k0, k1, mix(2 * j), mix(2 * j + 1));
        x ^= h;
        s = (s + h % 1_000_000_007) % 1_000_000_007;
    }
    println!("{:016x} {}", x, s);
}
