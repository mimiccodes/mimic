// trie: 200,000 generated words (4 to 14 letters of a 4-letter alphabet)
// put into a trie stored as flat arrays (four child slots per node, and
// whether a word ends there), then 200,000 other words looked up. Prints
// the trie's nodes, the lookups found as words and those found as
// prefixes.

fn mix(v: i64) -> i64 {
    let mut x = (v as u64 + 1).wrapping_mul(0x9E3779B97F4A7C15);
    x = (x ^ x >> 30).wrapping_mul(0xBF58476D1CE4E5B9);
    x = (x ^ x >> 27).wrapping_mul(0x94D049BB133111EB);
    ((x ^ x >> 31) >> 1) as i64
}

fn word(k: i64) -> Vec<usize> {
    (0..4 + mix(k * 31) % 11).map(|i| (mix(k * 1000 + i) % 4) as usize).collect()
}

fn main() {
    let mut kids: Vec<i32> = vec![-1; 4];
    let mut ends: Vec<bool> = vec![false];
    for k in 0..200000 {
        let mut cur = 0usize;
        for c in word(k) {
            let at = cur * 4 + c;
            if kids[at] < 0 {
                kids[at] = ends.len() as i32;
                ends.push(false);
                kids.extend([-1; 4]);
            }
            cur = kids[at] as usize;
        }
        ends[cur] = true;
    }
    let (mut as_word, mut as_prefix) = (0, 0);
    for q in 0..200000 {
        let mut node = 0i32;
        for c in word(100000 + q) {
            node = kids[node as usize * 4 + c];
            if node < 0 {
                break;
            }
        }
        if node >= 0 {
            as_prefix += 1;
            as_word += ends[node as usize] as i64;
        }
    }
    println!("{} {as_word} {as_prefix}", ends.len());
}
