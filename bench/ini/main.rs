// ini: an INI file of 100,000 sections (host, port, timeout, enabled and
// name in varying order, comments and blank lines between) written into
// one text and read back line by line. Prints the number of sections, the
// number of keys, the sum of the ports, how many sections are enabled, the
// total length of the names, and the sum of each port times its section's
// place in the file.

fn mix(v: i64) -> i64 {
    let mut x = (v as u64 + 1).wrapping_mul(0x9E3779B97F4A7C15);
    x = (x ^ x >> 30).wrapping_mul(0xBF58476D1CE4E5B9);
    x = (x ^ x >> 27).wrapping_mul(0x94D049BB133111EB);
    ((x ^ x >> 31) >> 1) as i64
}

fn section(s: i64) -> String {
    let vals = [
        format!("host = h{}.example.com", mix(s) % 1000),
        format!("port = {}", 1024 + mix(s + 1) % 60000),
        format!("timeout = {}", mix(s + 2) % 120),
        format!("enabled = {}", if mix(s + 3) % 3 != 0 { "true" } else { "false" }),
        format!("name = {}", "x".repeat(1 + (mix(s + 4) % 20) as usize)),
    ];
    let r = (mix(s + 5) % 5) as usize;
    let head = if s % 7 == 0 { format!("; section {}\n", s) } else if s % 11 == 0 { "# next\n".to_string() } else { String::new() };
    let body: Vec<&str> = (0..5).map(|k| vals[(k + r) % 5].as_str()).collect();
    format!("{}[server.{}]\n{}\n", head, s, body.join("\n"))
}

fn main() {
    let text = (0..100_000).map(section).collect::<Vec<_>>().join("\n");
    let (mut sections, mut keys, mut ports, mut enabled, mut names, mut weighted) = (0i64, 0i64, 0i64, 0i64, 0i64, 0i64);
    for line in text.lines() {
        let t = line.trim();
        if t.starts_with('[') {
            sections += 1;
        } else if !t.is_empty() && !t.starts_with(';') && !t.starts_with('#') {
            if let Some((k, v)) = t.split_once('=') {
                let (k, v) = (k.trim(), v.trim());
                keys += 1;
                match k {
                    "port" => {
                        let p: i64 = v.parse().unwrap();
                        ports += p;
                        weighted += p * sections;
                    }
                    "enabled" => enabled += (v == "true") as i64,
                    "name" => names += v.len() as i64,
                    _ => {}
                }
            }
        }
    }
    println!("{} {} {} {} {} {}", sections, keys, ports, enabled, names, weighted);
}
