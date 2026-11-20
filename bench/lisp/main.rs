// lisp: 50,000 random s-expressions (from splitmix64) written as text, then
// tokenised and evaluated by a recursive evaluator, all arithmetic modulo
// 1,000,000,007. Prints the number of tokens, the sum of the values modulo
// p, and the largest value.

const P: i64 = 1_000_000_007;
const OPS: [&str; 5] = ["+", "-", "*", "max", "min"];

fn mix(v: i64) -> i64 {
    let mut x = (v as u64 + 1).wrapping_mul(0x9E3779B97F4A7C15);
    x = (x ^ x >> 30).wrapping_mul(0xBF58476D1CE4E5B9);
    x = (x ^ x >> 27).wrapping_mul(0x94D049BB133111EB);
    ((x ^ x >> 31) >> 1) as i64
}

fn gen(out: &mut String, s: i64, d: i32) {
    if d == 0 || mix(s) % 3 == 0 {
        out.push_str(&(mix(s + 1) % 100).to_string());
        return;
    }
    out.push('(');
    out.push_str(OPS[(mix(s + 2) % 5) as usize]);
    out.push(' ');
    gen(out, mix(s + 3), d - 1);
    out.push(' ');
    gen(out, mix(s + 4), d - 1);
    out.push(')');
}

fn eval(toks: &[&str], i: &mut usize) -> i64 {
    let t = toks[*i];
    if t != "(" {
        *i += 1;
        return t.parse().unwrap();
    }
    let op = toks[*i + 1];
    *i += 2;
    let a = eval(toks, i);
    let b = eval(toks, i);
    *i += 1;
    match op {
        "+" => (a + b) % P,
        "-" => (a - b + P) % P,
        "*" => a * b % P,
        "max" => a.max(b),
        _ => a.min(b),
    }
}

fn main() {
    let mut text = String::new();
    for k in 0..50_000 {
        if k > 0 {
            text.push('\n');
        }
        gen(&mut text, k, 7);
    }
    let spaced = text.replace('(', " ( ").replace(')', " ) ");
    let toks: Vec<&str> = spaced.split_whitespace().collect();
    let (mut i, mut sum, mut most) = (0usize, 0i64, 0i64);
    while i < toks.len() {
        let v = eval(&toks, &mut i);
        sum = (sum + v) % P;
        most = most.max(v);
    }
    println!("{} {} {}", toks.len(), sum, most);
}
