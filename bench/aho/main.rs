// aho: Aho-Corasick over 4 MiB of generated text with 45 patterns: the
// trie, failure links breadth-first folded into a full transition table,
// then one pass. The text is cut into 64 pieces scanned one after another
// (each starting the longest pattern's length early), as the Mimic version
// cuts it. Prints the matches of all patterns and the positions where at
// least one ends.

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
    let mut pats: Vec<String> = words.iter().map(|w| w.to_string()).collect();
    pats.extend((0..20).map(|i| format!("{} {}", words[i], words[(i * 7 + 3) % 20])));
    pats.extend(["he", "ar", "or", "e t", "ll"].iter().map(|w| w.to_string()));

    let size = 1 + pats.iter().map(|p| p.len()).sum::<usize>();
    let longest = pats.iter().map(|p| p.len()).max().unwrap();
    let mut go = vec![-1i32; size * 256];
    let mut out = vec![0i64; size];
    let mut states = 1;
    for p in &pats {
        let mut cur = 0usize;
        for &c in p.as_bytes() {
            let at = cur * 256 + c as usize;
            if go[at] < 0 {
                go[at] = states;
                states += 1;
            }
            cur = go[at] as usize;
        }
        out[cur] += 1;
    }
    let (mut fail, mut queue) = (vec![0usize; size], vec![0usize; size]);
    let (mut head, mut tail) = (0, 1);
    while head < tail {
        let r = queue[head];
        head += 1;
        if r != 0 {
            out[r] += out[fail[r]];
        }
        for c in 0..256 {
            let u = go[r * 256 + c];
            let back = if r == 0 { 0 } else { go[fail[r] * 256 + c] };
            if u >= 0 {
                fail[u as usize] = back as usize;
                queue[tail] = u as usize;
                tail += 1;
            } else {
                go[r * 256 + c] = back;
            }
        }
    }
    let (mut total, mut ends) = (0i64, 0i64);
    let cut = n / 64;
    for j in 0..64 {
        let start = j * cut;
        let mut s = 0usize;
        for i in start.saturating_sub(longest - 1)..(j + 1) * cut {
            s = go[s * 256 + t[i] as usize] as usize;
            if i >= start {
                total += out[s];
                ends += (out[s] > 0) as i64;
            }
        }
    }
    println!("{total} {ends}");
}
