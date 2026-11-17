// http_parse: 200,000 HTTP/1.1 requests back to back in one stream, read
// as a server reads them (request line, headers to the blank line, then
// Content-Length bytes of body). Prints the requests, the headers, the
// body bytes, the total path length and the POSTs.

use std::fmt::Write;

fn find(s: &[u8], from: usize, pat: &[u8]) -> usize {
    from + s[from..].windows(pat.len()).position(|w| w == pat).unwrap()
}

fn main() {
    let n: i64 = 200000;
    let mut s = String::with_capacity(n as usize * 200);
    for i in 0..n {
        let m = ["GET", "POST", "PUT"][(i % 3) as usize];
        write!(s, "{m} /api/items/{i} HTTP/1.1\r\nHost: example.com\r\nUser-Agent: bench/{}\r\nAccept: */*\r\n", i % 7).unwrap();
        if m == "GET" {
            s.push_str("\r\n");
        } else {
            let body = format!("{{\"id\":{i},\"v\":\"{}\"}}", "x".repeat((i % 17) as usize));
            write!(s, "Content-Type: application/json\r\nContent-Length: {}\r\n\r\n{body}", body.len()).unwrap();
        }
    }
    let b = s.as_bytes();
    let (mut p, mut reqs, mut headers, mut bodies, mut paths, mut posts) = (0usize, 0i64, 0i64, 0usize, 0usize, 0i64);
    while p < b.len() {
        let eol = find(b, p, b"\r\n");
        let line = &s[p..eol];
        let mut parts = line.split(' ');
        let method = parts.next().unwrap();
        reqs += 1;
        paths += parts.next().unwrap().len();
        posts += (method == "POST") as i64;
        p = eol + 2;
        let mut clen = 0;
        while &b[p..p + 2] != b"\r\n" {
            let eol = find(b, p, b"\r\n");
            headers += 1;
            let h = &s[p..eol];
            if h.len() > 16 && h[..16].eq_ignore_ascii_case("Content-Length: ") {
                clen = h[16..].parse().unwrap();
            }
            p = eol + 2;
        }
        p += 2 + clen;
        bodies += clen;
    }
    println!("{reqs} {headers} {bodies} {paths} {posts}");
}
