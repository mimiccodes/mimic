// records: a million records with a name and numbers, updated five times
// (in place), then summed
struct P {
    id: i64,
    name: String,
    x: f64,
    v: f64,
}

fn main() {
    let mut ps: Vec<P> = (0..1000000).map(|i| P { id: i, name: "p".to_string(), x: i as f64, v: 1.0 }).collect();
    for _ in 0..5 {
        for p in ps.iter_mut() {
            p.x += p.v;
            p.v *= 0.5;
        }
    }
    let sx: f64 = ps.iter().map(|p| p.x).sum();
    let se: f64 = ps.iter().map(|p| p.v * p.v * 1024.0).sum();
    assert!(ps.iter().all(|p| p.name == "p" && p.id >= 0));
    println!("{:.1}\n{:.1}", sx, se);
}
