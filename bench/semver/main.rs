// semver: 1,000,000 version strings (MAJOR.MINOR.PATCH with an optional
// pre-release, from splitmix64) written into one text, then parsed line by
// line into one number each that orders as Semantic Versioning 2.0.0
// does, sorted, and written back. Prints the first, the median and the
// last version, and how many have a pre-release.

const KINDS: [&str; 3] = ["alpha", "beta", "rc"];

fn mix(v: i64) -> i64 {
    let mut x = (v as u64 + 1).wrapping_mul(0x9E3779B97F4A7C15);
    x = (x ^ x >> 30).wrapping_mul(0xBF58476D1CE4E5B9);
    x = (x ^ x >> 27).wrapping_mul(0x94D049BB133111EB);
    ((x ^ x >> 31) >> 1) as i64
}

fn version(i: i64) -> String {
    let k = mix(i + 40_000_000) % 20;
    let pre = match mix(i + 30_000_000) % 10 {
        6 => format!("-alpha.{}", k),
        7 => format!("-beta.{}", k),
        8 => format!("-rc.{}", k),
        9 => "-alpha".to_string(),
        _ => String::new(),
    };
    format!("{}.{}.{}{}", mix(i) % 20, mix(i + 10_000_000) % 30, mix(i + 20_000_000) % 200, pre)
}

// the order as one number (see main.mim)
fn key(v: &str) -> i64 {
    let (core, pre) = match v.split_once('-') {
        Some((c, p)) => (c, Some(p)),
        None => (v, None),
    };
    let c: Vec<i64> = core.split('.').map(|x| x.parse().unwrap()).collect();
    let base = (c[0] * 100 + c[1]) * 1000 + c[2];
    match pre {
        None => (base * 2 + 1) * 400,
        Some(p) => {
            let mut ids = p.split('.');
            let first = ids.next().unwrap();
            let kind = KINDS.iter().position(|k| *k == first).unwrap() as i64;
            let num = ids.next().map_or(0, |n| n.parse::<i64>().unwrap() + 1);
            base * 2 * 400 + kind * 100 + num
        }
    }
}

fn text_of(k: i64) -> String {
    let base = k / 800;
    let core = format!("{}.{}.{}", base / 100000, base / 1000 % 100, base % 1000);
    if k % 800 >= 400 {
        return core;
    }
    let (kind, num) = (k % 400 / 100, k % 100);
    let n = if num == 0 { String::new() } else { format!(".{}", num - 1) };
    format!("{}-{}{}", core, KINDS[kind as usize], n)
}

fn main() {
    let n = 1_000_000;
    let text = (0..n).map(version).collect::<Vec<_>>().join("\n");
    let mut keys: Vec<i64> = text.lines().map(key).collect();
    keys.sort_unstable();
    let pre = keys.iter().filter(|&&k| k % 800 < 400).count();
    let m = keys.len();
    println!("{} {} {} {}", text_of(keys[0]), text_of(keys[m / 2]), text_of(keys[m - 1]), pre);
}
