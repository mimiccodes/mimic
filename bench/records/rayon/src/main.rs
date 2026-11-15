use rayon::prelude::*;

// records: a million records with a name and numbers, updated five times
// (in place, in parallel), then summed
struct P {
    id: i64,
    name: String,
    x: f64,
    v: f64,
}

fn main() {
    let mut ps: Vec<P> = (0..1000000).into_par_iter().map(|i| P { id: i, name: "p".to_string(), x: i as f64, v: 1.0 }).collect();
    for _ in 0..5 {
        ps.par_iter_mut().for_each(|p| {
            p.x += p.v;
            p.v *= 0.5;
        });
    }
    let sx: f64 = ps.par_iter().map(|p| p.x).sum();
    let se: f64 = ps.par_iter().map(|p| p.v * p.v * 1024.0).sum();
    assert!(ps.par_iter().all(|p| p.name == "p" && p.id >= 0));
    println!("{:.1}\n{:.1}", sx, se);
}
