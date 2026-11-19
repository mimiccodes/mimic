// xmltags: an XML document of 300,000 elements in 5 tags, nested up to 12
// deep (a random walk from splitmix64), written and read back with one
// scan. Prints how many elements have each tag, the deepest nesting, and
// the sum of the ids.
use std::fmt::Write;

const TAGS: [&str; 5] = ["item", "group", "note", "ref", "entry"];

fn mix(v: i64) -> i64 {
    let mut x = (v as u64 + 1).wrapping_mul(0x9E3779B97F4A7C15);
    x = (x ^ x >> 30).wrapping_mul(0xBF58476D1CE4E5B9);
    x = (x ^ x >> 27).wrapping_mul(0x94D049BB133111EB);
    ((x ^ x >> 31) >> 1) as i64
}

fn main() {
    let mut text = String::from("<doc>");
    let mut stack: Vec<usize> = Vec::new();
    let (mut k, mut made) = (0i64, 0);
    while made < 300_000 {
        if (mix(k + 10_000_000) % 3 != 0 && stack.len() < 11) || stack.is_empty() {
            let tag = (mix(k) % 5) as usize;
            write!(text, "<{} id=\"{}\">t", TAGS[tag], k).unwrap();
            stack.push(tag);
            made += 1;
        } else {
            write!(text, "</{}>", TAGS[stack.pop().unwrap()]).unwrap();
        }
        k += 1;
    }
    while let Some(t) = stack.pop() {
        write!(text, "</{}>", TAGS[t]).unwrap();
    }
    text.push_str("</doc>");
    let (mut counts, mut deepest, mut ids, mut d) = ([0i64; 5], 0i64, 0i64, 0i64);
    for piece in text.split('<').skip(1) {
        if piece.starts_with('/') {
            d -= 1;
            continue;
        }
        d += 1;
        deepest = deepest.max(d);
        let name = piece.split([' ', '>']).next().unwrap();
        if let Some(t) = TAGS.iter().position(|&t| t == name) {
            counts[t] += 1;
            let rest = piece.split("id=\"").nth(1).unwrap();
            ids += rest.split('"').next().unwrap().parse::<i64>().unwrap();
        }
    }
    println!("{} {} {} {} {} {} {}", counts[0], counts[1], counts[2], counts[3], counts[4], deepest, ids);
}
