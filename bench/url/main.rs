// url: 300,000 generated URLs split into their parts as RFC 3986 reads
// them; prints the sum of the ports, the path segments, the query
// parameters, the total host length and the sum of the `id` parameters.

fn main() {
    let n: i64 = 300000;
    let (mut ports, mut segs, mut params, mut hosts, mut ids) = (0i64, 0usize, 0usize, 0usize, 0i64);
    for i in 0..n {
        let u = format!(
            "https://user{}:pw@host{}.example.com:{}/api/v{}/items/{i}?id={i}&page={}&sort=asc#sec{}",
            i % 10,
            i % 50,
            8000 + i % 100,
            i % 3,
            i % 20,
            i % 5
        );
        let rest = &u[u.find("://").unwrap() + 3..];
        let authority = &rest[..rest.find('/').unwrap_or(rest.len())];
        let host_port = authority.split_once('@').map_or(authority, |(_, h)| h);
        let (host, port) = host_port.split_once(':').unwrap_or((host_port, ""));
        hosts += host.len();
        ports += port.parse::<i64>().unwrap_or(0);
        let tail = &rest[authority.len()..];
        let tail = tail.split('#').next().unwrap();
        let (path, query) = tail.split_once('?').unwrap_or((tail, ""));
        segs += path.split('/').filter(|s| !s.is_empty()).count();
        for p in query.split('&') {
            params += 1;
            if let Some(v) = p.strip_prefix("id=") {
                ids += v.parse::<i64>().unwrap();
            }
        }
    }
    println!("{ports} {segs} {params} {hosts} {ids}");
}
