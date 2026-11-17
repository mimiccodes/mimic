---
title: Networking
description: The http, ws, tcp and udp modules — fetch from the web, write an HTTP server with route patterns, WebSockets, TCP and UDP, with requests that overlap by themselves.
order: 17
---

# Networking

Networking lives in four standard modules: `http`, `ws`, `tcp` and `udp`. Like all standard modules they need no import: write `http::fetch(…)` or `tcp::connect(…)` from any file. If you prefer short names, `use http::fetch` brings one in. A module of your own with the same name, for example a project called `http`, takes precedence.

## Fetching

`http::fetch` makes an HTTP GET request and returns a `Result[http::Response, str]`:

```mimic
module main

fn main() {
    let r = http::fetch("http://example.com/") else { return }
    print(r.status)
    print(get(r.headers, "content-type"))
    print(≢ r.body)
}
```

Several requests written one after another are **in flight at the same time**: each `let` starts its request, and the program waits only where a result is used ([chapter 10](10-parallel-by-default.md)):

```mimic
let news = http::fetch("http://news.example/")
let weather = http::fetch("http://weather.example/")
print(summary(news) + summary(weather))     // both were downloading at once
```

`http::request(method, url, headers, body)` sends any method, with headers and a body:

```mimic
http::request("POST", "http://api.example/notes", ["content-type": "text/plain"], "remember the milk")
```

`https://` addresses work too. Mimic uses your operating system's own TLS, so certificates are checked against the certificates your system trusts. A site with an expired or self-signed certificate gives an `Err` instead of a response. Redirects are followed automatically:

```mimic
let r = http::fetch("http://github.com/") else { return }   // redirected to https://github.com/
print(r.status)                                             // 200
```

## Serving

A server is a function from `http::Request` to `http::Response`. Routes are ordinary `match` patterns, and route captures are typed:

```mimic
fn handle(req: http::Request) -> http::Response {
    match (req.method, req.path) {
        ("GET", "/") => http::ok("hello"),
        ("GET", "/users/{id: i64}") => http::ok("user #{id}"),
        ("POST", "/notes") => http::respond(201, "saved: {req.body}"),
        _ => http::not_found(),
    }
}

fn main() {
    print(http::serve(8080, handle))               // runs until the program is stopped
}
```

If `handle` is pure, as here, every request is handled in parallel. If it prints or writes files, requests are handled one at a time, in order, so its effects never interleave. Either way you write the same code.

The server keeps each connection open for the client's next request (HTTP/1.1 keep-alive), and answers pipelined requests together in one write. A hello-world server answers about 300,000 requests a second on one core, slightly faster than Rust's hyper (see `BENCHMARK.md`).

## A server and its client in one program

Because a server with a pure handler only has overlapping effects, starting it with `let` runs it in the background like any other implicit future:

```mimic run
module main

fn handle(req: http::Request) -> http::Response {
    match req.path {
        "/hello/{name}" => http::ok("hello, {name}!"),
        _ => http::not_found(),
    }
}

fn server(port: i64) -> Result[i64, str] { http::serve_requests(port, handle, 3) }

// wait until the server answers (it starts at the same moment we do)
fn when_up(url: str, tries: i64) -> Result[http::Response, str] {
    match http::fetch(url) {
        Ok(r) => Ok(r),
        Err(e) => if tries <= 1 { Err(e) } else { time::sleep(20ms); when_up(url, tries - 1) },
    }
}

fn main() {
    let port = 30000 + os::pid() % 20000
    let base = "http://127.0.0.1:{port}"
    let s = server(port)                         // starts serving in the background
    print(show(when_up(base + "/hello/ada", 100)))
    let a = http::fetch(base + "/hello/grace")   // these two overlap
    let b = http::fetch(base + "/missing")
    print(show(a))
    print(show(b))
    print(s)                                     // the server stops after 3 requests
}

fn show(r: Result[http::Response, str]) -> str {
    match r {
        Ok(resp) => "{resp.status}: {resp.body}",
        Err(e) => "failed: {e}",
    }
}
```

```output
200: hello, ada!
200: hello, grace!
404: not found
Ok(3)
```

## Serving HTTPS

An HTTPS server needs a **certificate** and its **private key**, usually two PEM files from a certificate authority. Load them once with `tls::identity`, then serve with `http::serve_tls`:

```mimic
let cert = fs::read("cert.pem") else { return }
let key = fs::read("key.pem") else { return }
let id = tls::identity(cert, key) else { return }
print(http::serve_tls(8443, handle, id))        // https://your-host:8443/
```

For development, `tls::self_signed("localhost")` makes a certificate and key for you. No client trusts it by default, which is the point of certificates. `tls::connect_trusting` makes a client that trusts exactly the certificate you give it:

```mimic run
module main

fn handle(req: http::Request) -> http::Response { http::ok("secret {req.path}") }

fn server(port: i64, id: tls::Identity) -> Result[i64, str] { http::serve_tls_requests(port, handle, id, 2) }

fn main() {
    let made = tls::self_signed("localhost") else { return }
    let id = tls::identity(made.0, made.1) else { return }
    let port = 30000 + os::pid() % 20000
    let s = server(port, id)
    time::sleep(100ms)
    print(http::fetch("https://localhost:{port}/") ▷ is_ok)   // not trusted
    let conn = tls::connect_trusting("localhost", port, made.0) else { return }
    tls::send(conn, "GET /hello HTTP/1.1\r\nHost: localhost\r\n\r\n")
    let reply = tls::receive(conn, 4096) else { return }
    print(lines(reply)[0])
    tls::close(conn)
    print(s)
}

fn is_ok(r: Result[http::Response, str]) -> bool {
    match r {
        Ok(_) => true,
        Err(_) => false,
    }
}
```

```output
false
HTTP/1.1 200 OK
Ok(2)
```

`ws::serve_tls` does the same for WebSockets (`wss://`). `tls::connect` and `tls::accept` give you encrypted TCP streams for protocols of your own.

## WebSockets

A WebSocket stays open, and both sides can send messages at any time. A WebSocket server is a function called once per connection. When it returns, the connection closes:

```mimic run
module main

// answer every message upper-cased, until the client goes away
fn shout(sock: ws::Socket) {
    match ws::receive(sock) {
        Ok(ws::Text(t)) => {
            ws::send(sock, upper(t))
            shout(sock)
        }
        _ => {}
    }
}

fn server(port: i64) -> Result[i64, str] { ws::serve_connections(port, shout, 1) }

fn connect_when_up(url: str, tries: i64) -> Result[ws::Socket, str] {
    match ws::connect(url) {
        Ok(s) => Ok(s),
        Err(e) => if tries <= 1 { Err(e) } else { time::sleep(20ms); connect_when_up(url, tries - 1) },
    }
}

fn main() {
    let port = 30000 + os::pid() % 20000
    let s = server(port)
    let sock = connect_when_up("ws://127.0.0.1:{port}/", 100) else { return }
    print(ws::send(sock, "hello"))
    print(ws::receive(sock))
    print(ws::send(sock, "still here?"))
    print(ws::receive(sock))
    ws::close(sock)
    print(s)
}
```

```output
Ok(5)
Ok(Text("HELLO"))
Ok(11)
Ok(Text("STILL HERE?"))
Ok(1)
```

`wss://` addresses use TLS, just like `https://`. `ws::receive` returns `ws::Text(…)`, `ws::Binary(bytes)` or, once the other side has closed, `ws::Closed`.

## TCP

For other protocols, use TCP sockets directly:

```mimic
let sock = tcp::connect("example.com", 80) else { return }
print(tcp::send(sock, "GET / HTTP/1.1\r\nHost: example.com\r\nConnection: close\r\n\r\n"))
let reply = tcp::receive(sock, 4096) else { return }
print(lines(reply)[0])                          // HTTP/1.1 200 OK
tcp::close(sock)
```

A server side uses `tcp::listen(port)` and `tcp::accept(listener)`. All TCP calls happen in program order.

## UDP

UDP sends single packets instead of a stream. There is no connection, and a packet can get lost on the way. In return it is quick and simple, and it is what games, DNS and many local-network protocols use.

`udp::bind(port)` opens a socket. Port `0` picks any free port. `udp::send(socket, host, port, text)` sends one packet. `udp::receive(socket)` waits for one and returns a `udp::Packet` with its text, its bytes and who sent it. Because packets can get lost, `udp::receive_within(socket, timeout)` gives up after a while:

```mimic run
module main

// UDP: an echo server task and a client in one program.

fn echo(sock: udp::Socket, n: i64) -> i64 {
    if n == 0 { 0 } else {
        match udp::receive(sock) {
            Ok(p) => {
                udp::send(sock, p.host, p.port, upper(p.text))
                1 + echo(sock, n - 1)
            }
            Err(_) => 0,
        }
    }
}

fn text_of(r: Result[udp::Packet, str]) -> str {
    match r {
        Ok(p) => "{p.text} from {p.host}",
        Err(e) => "error: {e}",
    }
}

fn main() {
    let server = match udp::bind(0) {
        Ok(s) => s,
        Err(e) => {
            print("no UDP: {e}")
            return
        }
    }
    let answered = echo(server, 2)              // a task of its own
    let client = udp::bind(0) else { return }
    print(server.port > 0)
    print(udp::send(client, "127.0.0.1", server.port, "ping"))
    print(text_of(udp::receive_within(client, 2s)))
    udp::send_bytes(client, "localhost", server.port, bytes::from([104 105]))
    let r = udp::receive_within(client, 2s) else { return }
    print(bytes::hex(r.data))
    print(r.port == server.port)
    print(answered)
    print(text_of(udp::receive_within(client, 50ms)))   // nobody answers now
    udp::close(client)
    udp::close(server)
}
```

```output
true
Ok(4)
PING from 127.0.0.1
4849
true
2
error: timed out
```

`echo` runs as a task of its own while `main` talks to it. It answers to whatever address each packet came from.

Next: the full list of networking functions is in the [standard library reference](../std.md#networking-the-http-ws-tcp-and-udp-modules). Back to the [index](index.md).
