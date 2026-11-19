// markdown: 400,000 lines of a Markdown subset (from splitmix64) written as
// one text and read back line by line, words classified by their first
// character. Prints the headings at each level, the list items, the bold
// spans, the links, the code spans and the plain words.

const WORDS: [&str; 8] = ["array", "kernel", "fusion", "memory", "vector", "thread", "cache", "loop"];

fn mix(v: i64) -> i64 {
    let mut x = (v as u64 + 1).wrapping_mul(0x9E3779B97F4A7C15);
    x = (x ^ x >> 30).wrapping_mul(0xBF58476D1CE4E5B9);
    x = (x ^ x >> 27).wrapping_mul(0x94D049BB133111EB);
    ((x ^ x >> 31) >> 1) as i64
}

fn word(k: i64, j: i64) -> String {
    let w = WORDS[(mix(16 * k + j) % 8) as usize];
    match mix(16 * k + j + 8) % 10 {
        0 => format!("**{}**", w),
        1 => format!("`{}`", w),
        2 => format!("[{}](https://x.example/{})", w, k),
        _ => w.to_string(),
    }
}

fn line(k: i64) -> String {
    match mix(k + 100_000_000) % 10 {
        0 => "#".repeat(1 + (mix(k) % 3) as usize) + " " + &(0..3).map(|j| WORDS[(mix(k + j) % 8) as usize]).collect::<Vec<_>>().join(" "),
        1 => String::new(),
        r => {
            let body = (0..2 + mix(k) % 8).map(|j| word(k, j)).collect::<Vec<_>>().join(" ");
            if r < 4 { "- ".to_string() + &body } else { body }
        }
    }
}

fn main() {
    let text = (0..400_000).map(line).collect::<Vec<_>>().join("\n");
    let (mut heads, mut items, mut bold, mut links, mut code, mut plain) = ([0i64; 4], 0, 0, 0, 0, 0);
    for l in text.split('\n') {
        let level = l.bytes().take_while(|&b| b == b'#').count();
        let body = if level > 0 && l.as_bytes().get(level) == Some(&b' ') {
            heads[level] += 1;
            &l[level + 1..]
        } else if let Some(rest) = l.strip_prefix("- ") {
            items += 1;
            rest
        } else {
            l
        };
        for w in body.split(' ').filter(|w| !w.is_empty()) {
            match w.as_bytes()[0] {
                b'*' => bold += 1,
                b'[' => links += 1,
                b'`' => code += 1,
                _ => plain += 1,
            }
        }
    }
    println!("{} {} {} {} {} {} {} {}", heads[1], heads[2], heads[3], items, bold, links, code, plain);
}
