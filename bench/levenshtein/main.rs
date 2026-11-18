// levenshtein: the edit distance of 20,000 pairs of generated DNA-like
// strings (30 to 60 letters of "acgt"), by the usual dynamic program, one
// row after another. Prints the sum of the distances and the largest.

fn mix(v: i64) -> i64 {
    let mut x = (v as u64 + 1).wrapping_mul(0x9E3779B97F4A7C15);
    x = (x ^ x >> 30).wrapping_mul(0xBF58476D1CE4E5B9);
    x = (x ^ x >> 27).wrapping_mul(0x94D049BB133111EB);
    ((x ^ x >> 31) >> 1) as i64
}

fn word(k: i64) -> Vec<u8> {
    let n = 30 + mix(k * 1000) % 31;
    (0..n).map(|i| (mix(k * 1000 + 1 + i) % 4) as u8).collect()
}

fn distance(a: &[u8], b: &[u8]) -> usize {
    let mut prev: Vec<usize> = (0..=b.len()).collect();
    let mut cur = vec![0; b.len() + 1];
    for (i, &x) in a.iter().enumerate() {
        cur[0] = i + 1;
        for (j, &y) in b.iter().enumerate() {
            cur[j + 1] = (prev[j + 1] + 1).min(cur[j] + 1).min(prev[j] + (x != y) as usize);
        }
        std::mem::swap(&mut prev, &mut cur);
    }
    prev[b.len()]
}

fn main() {
    let d: Vec<usize> = (0..20000).map(|p| distance(&word(2 * p), &word(2 * p + 1))).collect();
    println!("{} {}", d.iter().sum::<usize>(), d.iter().max().unwrap());
}
