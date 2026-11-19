// lru: an LRU cache of 10,000 entries over 4,000,000 accesses to keys
// below 100,000, most of them to 2,000 hot keys (from splitmix64); the
// recency order is a doubly linked list through the keys. Prints the hits,
// the misses, and the sum of the keys left in the cache.

const H: usize = 100_000;
const CAP: usize = 10_000;

fn mix(v: i64) -> i64 {
    let mut x = (v as u64 + 1).wrapping_mul(0x9E3779B97F4A7C15);
    x = (x ^ x >> 30).wrapping_mul(0xBF58476D1CE4E5B9);
    x = (x ^ x >> 27).wrapping_mul(0x94D049BB133111EB);
    ((x ^ x >> 31) >> 1) as i64
}

struct Lru {
    prev: Vec<usize>,
    next: Vec<usize>,
}

impl Lru {
    fn unlink(&mut self, k: usize) {
        let (p, q) = (self.prev[k], self.next[k]);
        self.next[p] = q;
        self.prev[q] = p;
    }
    fn append(&mut self, k: usize) {
        let m = self.prev[H];
        self.next[m] = k;
        self.prev[k] = m;
        self.next[k] = H;
        self.prev[H] = k;
    }
}

fn main() {
    let mut l = Lru { prev: vec![H; H + 1], next: vec![H; H + 1] };
    let mut inc = vec![false; H + 1];
    let (mut size, mut hits, mut misses) = (0usize, 0i64, 0i64);
    for t in 0..4_000_000i64 {
        let k = (if mix(t + 10_000_000) % 4 == 0 { mix(t) % 100_000 } else { mix(t) % 2000 }) as usize;
        if inc[k] {
            hits += 1;
            l.unlink(k);
            l.append(k);
        } else {
            misses += 1;
            if size == CAP {
                let old = l.next[H];
                l.unlink(old);
                inc[old] = false;
                size -= 1;
            }
            inc[k] = true;
            size += 1;
            l.append(k);
        }
    }
    let sum: usize = (0..H).filter(|&k| inc[k]).sum();
    println!("{} {} {}", hits, misses, sum);
}
