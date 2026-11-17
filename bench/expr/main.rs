// expr: 300,000 arithmetic expressions (four shapes, with parentheses and
// precedence) written out as text, then tokenized, parsed by precedence
// climbing and evaluated; prints the sum of their values.

fn num(i: i64, k: i64) -> i64 {
    (i * k + k * 31) % 97 + 1
}

fn text(i: i64) -> String {
    let (a, b, c, d, e, f) = (num(i, 3), num(i, 7), num(i, 11), num(i, 13), num(i, 17), num(i, 19));
    match i % 4 {
        0 => format!("{a} + {b} * {c} - {d}"),
        1 => format!("( {a} + {b} ) * ( {c} - {d} ) + {e}"),
        2 => format!("{a} * ( {b} + {c} * ( {d} - {e} ) ) - {f}"),
        _ => format!("( ( {a} - {b} ) * {c} + {d} ) * ( {e} + {f} )"),
    }
}

fn prec(op: &str) -> i64 {
    match op {
        "+" | "-" => 1,
        "*" => 2,
        _ => 0,
    }
}

fn atom(t: &[&str], p: &mut usize) -> i64 {
    if t[*p] == "(" {
        *p += 1;
        let v = expr(t, p, 1);
        *p += 1;
        v
    } else {
        *p += 1;
        t[*p - 1].parse().unwrap()
    }
}

fn expr(t: &[&str], p: &mut usize, least: i64) -> i64 {
    let mut v = atom(t, p);
    while *p < t.len() && prec(t[*p]) >= least {
        let op = t[*p];
        *p += 1;
        let r = expr(t, p, prec(op) + 1);
        v = match op {
            "+" => v + r,
            "-" => v - r,
            _ => v * r,
        };
    }
    v
}

fn main() {
    let n: i64 = 300000;
    let mut sum = 0i64;
    for i in 0..n {
        let s = text(i);
        let t: Vec<&str> = s.split(' ').collect();
        sum += expr(&t, &mut 0, 1);
    }
    println!("{sum}");
}
