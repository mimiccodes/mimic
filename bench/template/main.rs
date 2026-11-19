// template: a template with six {{placeholders}} rendered 300,000 times
// with values from splitmix64, scanning for "{{" and "}}" on each render.
// Prints the total length of the rendered texts and the sum of their
// character codes.

const ITEMS: [&str; 5] = ["lamp", "kettle", "chair", "notebook", "umbrella"];
const DAYS: [&str; 7] = ["Monday", "Tuesday", "Wednesday", "Thursday", "Friday", "Saturday", "Sunday"];

fn mix(v: i64) -> i64 {
    let mut x = (v as u64 + 1).wrapping_mul(0x9E3779B97F4A7C15);
    x = (x ^ x >> 30).wrapping_mul(0xBF58476D1CE4E5B9);
    x = (x ^ x >> 27).wrapping_mul(0x94D049BB133111EB);
    ((x ^ x >> 31) >> 1) as i64
}

fn main() {
    let t = "Dear {{name}}, your order {{id}} of {{qty}} x {{item}} ships on {{day}}. Total: {{total}}.";
    let (mut length, mut codes) = (0i64, 0i64);
    for r in 0..300_000i64 {
        let qty = 1 + mix(r) % 9;
        let mut out = String::new();
        let mut rest = t;
        while let Some(at) = rest.find("{{") {
            out.push_str(&rest[..at]);
            let after = &rest[at + 2..];
            let end = after.find("}}").unwrap();
            match &after[..end] {
                "name" => out.push_str(&format!("user{}", r % 1000)),
                "id" => out.push_str(&r.to_string()),
                "qty" => out.push_str(&qty.to_string()),
                "item" => out.push_str(ITEMS[(mix(r + 1) % 5) as usize]),
                "day" => out.push_str(DAYS[(mix(r + 2) % 7) as usize]),
                _ => out.push_str(&(qty * (100 + mix(r + 3) % 900)).to_string()),
            }
            rest = &after[end + 2..];
        }
        out.push_str(rest);
        length += out.len() as i64;
        codes += out.bytes().map(|b| b as i64).sum::<i64>();
    }
    println!("{} {}", length, codes);
}
