// anagram: 500,000 words of 3 to 8 letters from a-f (from splitmix64),
// written as one text and split again, then grouped into anagram classes:
// a word's class is its letter counts packed into one number (4 bits a
// letter), the numbers are sorted, and the classes are the runs. Prints
// the number of classes, the size of the largest, and the sum of the
// squares of the sizes.

fn mix(v: i64) -> i64 {
    let mut x = (v as u64 + 1).wrapping_mul(0x9E3779B97F4A7C15);
    x = (x ^ x >> 30).wrapping_mul(0xBF58476D1CE4E5B9);
    x = (x ^ x >> 27).wrapping_mul(0x94D049BB133111EB);
    ((x ^ x >> 31) >> 1) as i64
}

fn word(k: i64) -> String {
    (0..3 + mix(k) % 6).map(|j| (97 + mix(16 * k + j) % 6) as u8 as char).collect()
}

fn main() {
    let text = (0..500_000).map(word).collect::<Vec<_>>().join(" ");
    let mut keys: Vec<i64> = text.split_whitespace().map(|w| w.bytes().map(|b| 1i64 << (4 * (b - 97))).sum()).collect();
    keys.sort_unstable();
    let (mut classes, mut largest, mut squares, mut i) = (0i64, 0i64, 0i64, 0usize);
    while i < keys.len() {
        let mut j = i;
        while j < keys.len() && keys[j] == keys[i] {
            j += 1;
        }
        let s = (j - i) as i64;
        classes += 1;
        largest = largest.max(s);
        squares += s * s;
        i = j;
    }
    println!("{} {} {}", classes, largest, squares);
}
