// csv: 400,000 rows of generated CSV (id, name, score, city; a city with a
// comma is quoted, RFC 4180), parsed back: the sum of the scores, the total
// length of the city names, and how many fields were quoted.

use std::fmt::Write;

fn fields(line: &str) -> Vec<&str> {
    let mut out = Vec::with_capacity(4);
    let mut rest = line;
    loop {
        if let Some(r) = rest.strip_prefix('"') {
            let close = r.find('"').unwrap();
            out.push(&r[..close]);
            rest = &r[close + 1..];
        } else {
            let end = rest.find(',').unwrap_or(rest.len());
            out.push(&rest[..end]);
            rest = &rest[end..];
        }
        match rest.strip_prefix(',') {
            Some(r) => rest = r,
            None => return out,
        }
    }
}

fn main() {
    let n: i64 = 400000;
    let mut text = String::with_capacity(n as usize * 48);
    for i in 0..n {
        let city = if i % 7 == 0 { "\"Springfield, IL\"" } else if i % 3 == 0 { "Paris" } else { "Rome" };
        if i > 0 {
            text.push('\n');
        }
        write!(text, "{},user{},{},{}", i, i % 1000, i * 7919 % 1000, city).unwrap();
    }
    let (mut score, mut city, mut quoted) = (0i64, 0usize, 0i64);
    for line in text.lines() {
        let f = fields(line);
        score += f[2].parse::<i64>().unwrap();
        city += f[3].len();
        quoted += f[3].contains(',') as i64;
    }
    println!("{score} {city} {quoted}");
}
