// tokenize: 200,000 lines of generated C-like source scanned into tokens
// (identifiers, numbers, strings, operators and punctuation); prints how
// many of each and the total length of the identifiers.

fn line(i: i64) -> String {
    match i % 3 {
        0 => format!("let x{i} = foo(y{}, {}) + \"s{}\" * 3;", i % 97, i * 7 % 1000, i % 13),
        1 => format!("if (count_{} >= {}) {{ total += weight[{}]; }}", i % 50, i % 9, i % 31),
        _ => format!("return bar_{}(\"a b\", z{i}) - {} / 2;", i % 7, i % 100),
    }
}

fn main() {
    let n: i64 = 200000;
    let (mut ids, mut nums, mut strs, mut ops, mut len) = (0i64, 0i64, 0i64, 0i64, 0usize);
    for i in 0..n {
        let s = line(i);
        let b = s.as_bytes();
        let mut p = 0;
        while p < b.len() {
            let c = b[p];
            if c == b' ' {
                p += 1;
            } else if c.is_ascii_alphabetic() || c == b'_' {
                let s = p;
                p += 1;
                while p < b.len() && (b[p].is_ascii_alphanumeric() || b[p] == b'_') {
                    p += 1;
                }
                ids += 1;
                len += p - s;
            } else if c.is_ascii_digit() {
                while p < b.len() && b[p].is_ascii_digit() {
                    p += 1;
                }
                nums += 1;
            } else if c == b'"' {
                p += 1 + b[p + 1..].iter().position(|&x| x == b'"').unwrap() + 1;
                strs += 1;
            } else if matches!(c, b'=' | b'>' | b'+') && p + 1 < b.len() && b[p + 1] == b'=' {
                p += 2;
                ops += 1;
            } else {
                p += 1;
                ops += 1;
            }
        }
    }
    println!("{ids} {nums} {strs} {ops} {len}");
}
