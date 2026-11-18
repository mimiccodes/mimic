// Load generator for the http benchmark: `conns` keep-alive connections
// send `total` GET requests between them, `depth` at a time per connection
// (HTTP/1.1 pipelining), and check every response body. Prints the
// elapsed seconds. One thread per connection; the standard library only.
//
//     load <port> <conns> <total> <depth> <expected body>

use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpStream;
use std::time::Instant;

fn main() {
    let a: Vec<String> = std::env::args().collect();
    let num = |i: usize| a[i].parse::<usize>().expect("a number");
    let (port, conns, total, depth, want) = (a[1].clone(), num(2), num(3), num(4), a[5].clone());
    let req = b"GET / HTTP/1.1\r\nHost: localhost\r\n\r\n";
    let batch: Vec<u8> = req.repeat(depth);
    let start = Instant::now();
    let workers: Vec<_> = (0..conns)
        .map(|c| {
            let n = total / conns + usize::from(c < total % conns);
            let (port, want, batch) = (port.clone(), want.clone(), batch.clone());
            std::thread::spawn(move || -> Result<(), String> {
                let mut conn = TcpStream::connect(format!("127.0.0.1:{port}")).map_err(|e| e.to_string())?;
                conn.set_nodelay(true).ok();
                let mut r = BufReader::new(conn.try_clone().map_err(|e| e.to_string())?);
                let mut i = 0;
                while i < n {
                    let k = depth.min(n - i);
                    conn.write_all(&batch[..k * req.len()]).map_err(|e| e.to_string())?;
                    for _ in 0..k {
                        let body = response(&mut r)?;
                        if body != want {
                            return Err(format!("body {body:?}, expected {want:?}"));
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
fn response(r: &mut BufReader<TcpStream>) -> Result<String, String> {
    let mut line = String::new();
    r.read_line(&mut line).map_err(|e| e.to_string())?;
    if !line.starts_with("HTTP/1.1 200") {
        return Err(format!("status {:?}", line.trim()));
    }
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
    String::from_utf8(body).map_err(|e| e.to_string())
}
