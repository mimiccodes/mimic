// Load generator for the http_route benchmark: `conns` keep-alive
// connections send `total` requests between them, `depth` at a time per
// connection (HTTP/1.1 pipelining). Request i of connection c asks for
// GET /users/{(c * 7919 + i) mod 10000}; every 16th asks for an id with
// letters in it, which must be a 404. Each 200 body must be the user's
// JSON, {"id":ID,"name":"userID","score":ID*7 mod 100,"active":ID mod 3 != 0}.
// Prints the elapsed seconds. One thread per connection; the standard
// library only.
//
//     load <port> <conns> <total> <depth> <name>

use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpStream;
use std::time::Instant;

// the path of request i on connection c, and the body it must get (None: a 404)
fn request(c: usize, i: usize) -> (String, Option<String>) {
    let id = (c * 7919 + i) % 10000;
    if i % 16 == 15 {
        return (format!("/users/{id}x"), None);
    }
    let body = format!(r#"{{"id":{id},"name":"user{id}","score":{},"active":{}}}"#, id * 7 % 100, id % 3 != 0);
    (format!("/users/{id}"), Some(body))
}

fn main() {
    let a: Vec<String> = std::env::args().collect();
    let num = |i: usize| a[i].parse::<usize>().expect("a number");
    let (port, conns, total, depth) = (a[1].clone(), num(2), num(3), num(4));
    let start = Instant::now();
    let workers: Vec<_> = (0..conns)
        .map(|c| {
            let n = total / conns + usize::from(c < total % conns);
            let port = port.clone();
            std::thread::spawn(move || -> Result<(), String> {
                let mut conn = TcpStream::connect(format!("127.0.0.1:{port}")).map_err(|e| e.to_string())?;
                conn.set_nodelay(true).ok();
                let mut r = BufReader::new(conn.try_clone().map_err(|e| e.to_string())?);
                let mut i = 0;
                while i < n {
                    let k = depth.min(n - i);
                    let reqs: Vec<_> = (i..i + k).map(|j| request(c, j)).collect();
                    let mut batch = Vec::new();
                    for (path, _) in &reqs {
                        batch.extend_from_slice(format!("GET {path} HTTP/1.1\r\nHost: localhost\r\n\r\n").as_bytes());
                    }
                    conn.write_all(&batch).map_err(|e| e.to_string())?;
                    for (path, want) in &reqs {
                        let (status, body) = response(&mut r)?;
                        match want {
                            Some(w) if status != 200 || body != *w => return Err(format!("{path}: {status} {body:?}, expected {w:?}")),
                            None if status != 404 => return Err(format!("{path}: {status}, expected 404")),
                            _ => {}
                        }
                    }
                    i += k;
                }
                Ok(())
            })
        })
        .collect();
    let mut failed = None;
    for w in workers {
        if let Err(e) = w.join().unwrap_or_else(|_| Err("a connection panicked".into())) {
            failed.get_or_insert(e);
        }
    }
    let elapsed = start.elapsed().as_secs_f64();
    if let Some(e) = failed {
        eprintln!("load: {e}");
        std::process::exit(1);
    }
    println!("{elapsed:.6}");
}

// one response: status line, headers (Content-Length), body
fn response(r: &mut BufReader<TcpStream>) -> Result<(u32, String), String> {
    let mut line = String::new();
    r.read_line(&mut line).map_err(|e| e.to_string())?;
    let status = line.split(' ').nth(1).and_then(|s| s.parse().ok()).ok_or(format!("status line {:?}", line.trim()))?;
    let mut length = None;
    loop {
        line.clear();
        if r.read_line(&mut line).map_err(|e| e.to_string())? == 0 {
            return Err("connection closed".into());
        }
        let l = line.trim_end_matches(['\r', '\n']);
        if l.is_empty() {
            break;
        }
        if let Some((k, v)) = l.split_once(':')
            && k.eq_ignore_ascii_case("content-length")
        {
            length = v.trim().parse::<usize>().ok();
        }
    }
    let mut body = vec![0; length.ok_or("no Content-Length")?];
    r.read_exact(&mut body).map_err(|e| e.to_string())?;
    Ok((status, String::from_utf8(body).map_err(|e| e.to_string())?))
}
