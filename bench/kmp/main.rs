// kmp: every occurrence, overlapping ones too, of 8 patterns in 4 MiB of
// generated text, by Knuth-Morris-Pratt. Prints each pattern's count.

fn count(t: &[u8], p: &[u8]) -> usize {
    let mut fail = vec![0usize; p.len()];
    let mut k = 0;
    for i in 1..p.len() {
        while k > 0 && p[k] != p[i] {
            k = fail[k - 1];
        }
        if p[k] == p[i] {
            k += 1;
        }
        fail[i] = k;
    }
    let (mut c, mut k) = (0, 0);
    for &x in t {
        while k > 0 && p[k] != x {
            k = fail[k - 1];
        }
        if p[k] == x {
            k += 1;
        }
        if k == p.len() {
            c += 1;
            k = fail[k - 1];
        }
    }
    c
}

fn main() {
    let n: usize = 1 << 22;
    let words = ["the", "of", "and", "to", "in", "is", "parallel", "array", "kernel", "fusion", "memory", "vector", "mimic", "cache", "thread", "core", "loop", "value", "zebra", "quartz"];
    let mut t: Vec<u8> = Vec::with_capacity(n + 32);
    let mut i: i64 = 0;
    while t.len() < n {
        let k = if i % 7 != 0 { (i * 2654435761 >> 13) % 20 } else { (i * 40503 >> 7) % 20 };
        t.extend_from_slice(words[k as usize].as_bytes());
        if i % 11 == 10 {
            t.push(b'.');
        }
        t.push(b' ');
        i += 1;
    }
    t.truncate(n);
    let pats = ["the", "parallel array", "core loop", "a", "zebra quartz", "e the", "mimic", "of the of"];
    let counts: Vec<String> = pats.iter().map(|p| count(&t, p.as_bytes()).to_string()).collect();
    println!("{}", counts.join(" "));
}
