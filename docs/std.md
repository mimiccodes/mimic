# Standard library

These are visible in every file without an import. The string, list and map functions are runtime intrinsics: they are implemented in Mimic's C runtime (`runtime/mimic_std.c`) and declared in the prelude with `extern "mimic"`. They are memory safe and need no `unsafe`, and they have no side effects, so code that uses them stays parallel.

## Strings

| Function | Result |
|---|---|
| `split(s, sep)` | `[str]`. `split(s, "")` splits into characters. |
| `join(parts, sep)` | `str` |
| `lines(s)`, `chars(s)` | `[str]` (UTF-8 characters stay whole) |
| `trim(s)`, `trim_start(s)`, `trim_end(s)` | `str` |
| `contains(s, part)` | `bool`. It also works on lists: `contains([1 2 3], 2)`. |
| `starts_with(s, p)`, `ends_with(s, p)` | `bool` |
| `find(s, part)` | `Some(byte index)` or `None` |
| `replace(s, from, to)` | `str` (every occurrence) |
| `upper(s)`, `lower(s)` | `str` (ASCII letters) |
| `repeat(s, n)` | `str` |
| `string(x)` | Any value as text, the way `print` shows it |
| `parse_int(s)`, `parse_float(s)` | `Some(number)` or `None` |
| `a + b` | Joins two strings. |
| `a == b` | Compares two whole strings. |
| `≢ s`, `s ↑ n`, `s ↓ n`, `⌽ s` | Length, take, drop and reverse, as for any array. |

## Lists

| Function | Result |
|---|---|
| `push(xs, x)` | A new list with `x` appended |
| `put(xs, i, x)` | A new list with element `i` replaced by `x` (for maps: a new map with key `i` set) |
| `concat(xs, ys)` | `xs` followed by `ys` |
| `sort(xs)` | Ascending, stable; numbers or strings |
| `contains(xs, x)` | `bool` |

Lists are values. `push` and `concat` return new lists and leave the original unchanged.

## Maps

```mimic
let born = ["ada": 1815, "alan": 1912]     // a map literal; [:] is empty
born["alan"]                               // 1912 (missing keys stop the program)
get(born, "linus")                         // None (Option)
has(born, "ada")                           // true
≢ born                                     // 2
keys(born), values(born)                   // in insertion order
let mut m = born
m["linus"] = 1969                          // copy-on-write: born is unchanged
put(born, "grace", 1906)                   // a new map with one more key
remove(born, "ada")                        // a new map without it
```

**What a key can be:** numbers, bools, characters or strings.

**How maps behave:**
* **Order.** Entries keep their insertion order, which is the order you see when printing and in `keys` and `values`.
* **Speed.** Lookups go through a hash index.
* **Updates.** `m[k] = v` on a `let mut` map changes it in place when nothing else shares it, and copies it first otherwise.

## Standard modules

Everything with effects lives in a module, used by path from any file with no import: `fs::read(p)`, `time::now()`. `use fs::read` brings in a short name.

Your own names come first. A program (or one of its modules) called `json` hides the standard `json` module. The compiler says so when that is the problem. Then `std::json::encode` still reaches the standard one, as `std::` names every standard module. The global names are only `print`, `Option`/`Result` and the string, list and map functions above.

Functions with effects are known to the compiler for what they are (see [ffi.md](ffi.md#what-c-functions-do-effects)):
* **concurrent:** may overlap; `let x = …` starts an implicit future, and `¨` runs them in parallel.
* **ordered:** in program order.
* **sync:** pending futures finish first.

Failures come back as values, never as crashes. Error messages name the operation, the path and the system's reason: `cannot read data.csv: No such file or directory`.

### `fs`: files and directories

| Function | Result | Effect |
|---|---|---|
| `fs::read(path)` | `Result[str, str]` | concurrent |
| `fs::read_bytes(path)` | `Result[[u8], str]` | concurrent |
| `fs::size(path)` | `Result[i64, str]` | concurrent |
| `fs::list_dir(path)` | `Result[[str], str]`: sorted names, without `.` and `..` | concurrent |
| `fs::exists(path)`, `fs::is_file(path)`, `fs::is_dir(path)` | `bool` | concurrent |
| `fs::current_dir()`, `fs::temp_dir()` | `str` | concurrent |
| `fs::write(path, text)`, `fs::append(path, text)` | `Result[i64, str]`: bytes written | sync |
| `fs::write_bytes(path, data)` | `Result[i64, str]` | sync |
| `fs::copy(from, to)` | `Result[i64, str]` | sync |
| `fs::rename(from, to)`, `fs::remove(path)` | `Result[bool, str]`; `remove` also removes empty directories | sync |
| `fs::make_dir(path)` | `Result[bool, str]`: creates missing parents too; fine if it exists | sync |

### `chan`: channels between tasks (concurrent)

```mimic
let (tx, rx) = chan::new[str]()            // a channel of str
let worker = consume(rx)                   // a task: an implicit future
chan::send(tx, "job 1")                    // never waits
chan::close(tx)                            // no more values
fn consume(rx: chan::Receiver[str]) -> i64 {
    match chan::receive(rx) {              // waits; None once closed and drained
        Some(job) => 1 + consume(rx),
        None => 0,
    }
}
```

| Function | Result |
|---|---|
| `chan::new[T]()` | `(chan::Sender[T], chan::Receiver[T])`: an unbounded channel |
| `chan::bounded[T](capacity)` | The same, holding at most `capacity` values |
| `chan::send(tx, value)` | `bool`: `false` when the channel is closed. On a full bounded channel it waits for room |
| `chan::receive(rx)` | `Option[T]`: waits for a value; `None` once closed and empty |
| `chan::try_receive(rx)` | `Option[T]` without waiting |
| `chan::close(tx)` | No more values; receivers get what is left, then `None` |

How channels behave:
* **Unbounded or bounded.** `chan::new` makes an unbounded queue, so `send` never waits. `chan::bounded` holds a fixed number of values, and `send` waits while it is full. That is backpressure: a fast producer cannot run ahead of a slow consumer and fill memory.
* **Safe to share.** Values can be handed between tasks safely because they are immutable.
* **Ends are ordinary values.** A `Sender` or `Receiver` can be stored in a list, sent through another channel or captured by a closure.
* **Tasks.** A task is any `let` of a function whose effects may overlap, which channel operations are, and `ws::serve`/`http::serve` handlers run as tasks too.
* **Waiting is cheap.** On the event loop a `receive` that waits parks only its own task, not a thread.
* **Without threads (WebAssembly),** tasks are deferred. A `receive` that would wait first runs deferred tasks, so producer/consumer code still works. A deferred task cannot stop half-way, so a full bounded channel grows there instead of waiting.

### `bytes`: binary data (pure)

`[u8]` is an array of bytes: one byte per element, so it is compact, and each element reads as an `i64`. Arithmetic on bytes gives `i64` results instead of silently wrapping at 256. Storing a value outside 0 … 255 into a `[u8]` stops the program. `push`, `concat`, `↑`, `↓`, `⌽` and indexing keep bytes as bytes.

| Function | Result |
|---|---|
| `bytes::from(xs)` | `[u8]` from a list of integers 0 … 255 |
| `bytes::of_text(s)` | the UTF-8 bytes of a string |
| `bytes::to_text(b)` | `Result[str, str]`: `Err` unless the bytes are valid UTF-8 |
| `bytes::hex(b)`, `bytes::from_hex(s)` | `"486921"` ↔ bytes; `from_hex` returns a `Result` |
| `bytes::base64(b)`, `bytes::from_base64(s)` | standard base64 ↔ bytes; `from_base64` returns a `Result` |

`fs::read_bytes`/`fs::write_bytes` and `tcp::send_bytes`/`tcp::receive_bytes` read and write bytes. A `[u8]` parameter accepts only bytes; passing a list of `i64` is a compile error.

### `path`: paths as text (pure)

| Function | Example |
|---|---|
| `path::join(a, b)` | `path::join("logs", "today.txt")` → `"logs/today.txt"`; an absolute `b` replaces `a` |
| `path::file_name(p)` | `"a/b/c.tar.gz"` → `"c.tar.gz"` |
| `path::extension(p)` | `"a/b/c.tar.gz"` → `"gz"` |
| `path::stem(p)` | `"a/report.pdf"` → `"report"` |
| `path::parent(p)` | `"a/b/c.txt"` → `"a/b"`; `"c.txt"` → `""` |

### `io`: standard input and error (ordered)

| Function | Result |
|---|---|
| `io::read_line()` | `Option[str]`: the next line without its newline; `None` at the end |
| `io::read_input()` | `str`: everything left on standard input |
| `io::eprint(x)` | Print to standard error |

### `os`: the program and other programs

| Function | Result | Effect |
|---|---|---|
| `os::args()` | `[str]`: the command-line arguments, without the program name (`mimic run file.mim -- a b`) | concurrent |
| `os::env(name)` | `Option[str]` | concurrent |
| `os::pid()` | This process's id | concurrent |
| `os::set_env(name, value)` | `Result[bool, str]` | sync |
| `os::run(program, arguments)` | `Result[os::Output, str]`: `program` is looked up through `PATH`; `Output { status, stdout, stderr }` holds the exit status (128 + n when killed by signal n) and both outputs, captured completely | sync |
| `os::shell(command)` | `os::run("/bin/sh", ["-c" command])` | sync |
| `os::exit(code)` | Leave the program now with that exit status | ordered |

The child's standard input is empty. WebAssembly (WASI), tvOS and watchOS have no child processes, so there `os::run` returns `Err`.

### `time`: clocks and dates

Instants and durations are both `i64` nanoseconds, so duration literals mix freely: `time::now() + 90min`, `took < 2s`.

| Function | Result | Effect |
|---|---|---|
| `time::now()` | Wall-clock time: nanoseconds since 1970-01-01 00:00 UTC | concurrent |
| `time::clock()` | A monotonic clock in nanoseconds, for measuring how long something takes (never jumps) | concurrent |
| `time::sleep(d)` | Pause, e.g. `time::sleep(250ms)` | concurrent |
| `time::local(t)` | `time::DateTime` in this machine's time zone | concurrent |
| `time::utc(t)` | `time::DateTime { year, month, day, hour, minute, second, nanosecond, weekday, utc_offset }`; weekday 1 = Monday … 7 = Sunday | pure |
| `time::timestamp(y, mo, d, h, mi, s)` | The instant of a UTC date and time | pure |
| `time::format(t)` | ISO 8601 / RFC 3339 in UTC: `2026-11-12T14:30:05Z`, with `.250` milliseconds when present | pure |

The calendar is the proleptic Gregorian one, correct for any year, before 1970 included.

### `random`: random numbers (ordered)

| Function | Result |
|---|---|
| `random::float()` | A float in `[0, 1)` |
| `random::int(lo, hi)` | An integer in `[lo, hi)`, without bias |
| `random::seed(n)` | Make the sequence repeatable (per thread) |

The generator is xoshiro256**, seeded from the operating system's entropy. Each thread has its own. Drawing a number changes the generator's state, so random draws run in order and never in parallel. It is not for cryptography.

`lines(s)` (global) splits at newlines and drops a `\r` before each one. A final newline ends the last line instead of starting an empty one, so `lines(fs::read(…))` gives exactly the file's lines.

### `json`: JSON text ↔ values (pure)

```mimic
type User = { name: str, age: i64, tags: [str], email: Option[str] }

json::encode(user)                       // {"name":"Ada","age":36,"tags":["math"],"email":null}
json::encode_pretty(user)                // the same, indented
let u = json::decode[User](text) else { return }      // Result[User, str]
let v = json::parse(text) else { return }             // Result[json::Value, str]
match v { json::Object(m) => …, json::List(xs) => …, _ => … }
```

| Function | Result |
|---|---|
| `json::encode(x)`, `json::encode_pretty(x)` | `str` |
| `json::decode[T](text)` | `Result[T, str]`: `T` given explicitly in `[…]` |
| `json::parse(text)` | `Result[json::Value, str]` |
| `json::Value` | `Null \| Bool(bool) \| Number(f64) \| Text(str) \| List([Value]) \| Object([str: Value])` |

**How values map to JSON:**

| Mimic | JSON |
|---|---|
| `i64`, `f64` | numbers (floats keep a `.0`; NaN and infinity become `null`) |
| `bool`, `str` | `true`/`false`, strings |
| lists, matrices | arrays (nested, row by row, for matrices) |
| maps | objects (non-string keys are written as text) |
| records | objects with the field names |
| tuples | arrays |
| `Some(x)` / `None` | `x` / `null` |
| `Ok(x)` / `Err(e)` | `{"ok": x}` / `{"error": e}` |
| other variants | `"Name"`, `{"Name": x}`, `{"Name": [x, y]}` |

**Decoding into `T`** follows `T`'s fields and types:
* **Extra fields** in the JSON are ignored.
* **Missing fields** are `None` when their type is an `Option`, and an error otherwise.
* **Integers** must be whole numbers.
* **Errors name where the problem is:** `json: expected an integer at users[2].age, found a string` or `invalid JSON at line 3, column 7: expected ','`.
* **What can be decoded:** records (generic ones too, like `Page[User]`), lists, maps (`[str: i64]`), `Option`, numbers, bools and strings. Unions other than `Option` cannot be decoded yet.

**Raw strings** make JSON literals easy: `'{"name": "Ada"}'` uses single quotes, has no interpolation and no escapes.

## Networking: the `http`, `ws`, `tcp` and `udp` modules

Networking lives in standard modules, used by path from any file with no import: `http::fetch`, `ws::connect`, `tcp::connect`, `udp::bind`. (`use http::fetch` works too.) Your own modules come first: a project or module called `http` hides the standard one.

### `http`

```mimic
let r = http::fetch("http://example.com/") else { return }   // Result[http::Response, str]
print((r.status, get(r.headers, "content-type")))
http::request("POST", url, ["content-type": "application/json"], body)

fn handle(req: http::Request) -> http::Response {
    match (req.method, req.path) {
        ("GET", "/") => http::ok("hello"),
        ("GET", "/users/{id: i64}") => http::ok("user #{id}"),
        _ => http::not_found(),
    }
}
http::serve(8080, handle)                                     // runs until the program ends
```

| Function / type | Meaning |
|---|---|
| `http::fetch(url)` | `Result[Response, str]`: a GET request |
| `http::request(method, url, headers, body)` | Any method, with `[str: str]` headers and a body |
| `http::serve(port, handler)` | An HTTP/1.1 server; `handler` is a `fn(http::Request) -> http::Response` |
| `http::serve_requests(port, handler, n)` | The same, stopping after `n` requests; `Ok(n)` |
| `http::serve_tls(port, handler, identity)`, `http::serve_tls_requests(port, handler, identity, n)` | The same over HTTPS, with a `tls::Identity` (see [`tls`](#tls-encryption)) |
| `http::Request` | `{ method, path, query, headers, body }`: header names are lower case, `query` is the part after `?` |
| `http::Response` | `{ status, headers, body }` |
| `http::ok(body)`, `http::respond(status, body)`, `http::not_found()` | Responses (text/plain unless the headers say otherwise) |
| `http::json(value)` | A 200 response with `value` encoded as JSON (`application/json`) |

How the client and server behave:
* **Requests overlap.** `let a = http::fetch(u1)` and `let b = http::fetch(u2)` are in flight at the same time (implicit futures).
* **The server runs a pure or overlapping handler in parallel**, one task per connection on the event loop (see [the event loop](#the-event-loop)). A handler that prints or writes files handles one request at a time, in order.
* **One request per connection** (`Connection: close`). Bodies use `Content-Length`; chunked responses are decoded.
* **`https://` uses the operating system's TLS**, loaded when first needed: the Security framework on Apple platforms, OpenSSL 3 (`libssl.so.3`) on Linux. Certificates and host names are checked against the system's trusted certificates, so an expired, self-signed or wrong-host certificate is an `Err` ("TLS with example.com failed: …"). Statically linked Linux programs cannot load OpenSSL, so there `https://` returns an `Err` that says so. Serving HTTPS: see [`tls`](#tls-encryption).
* **Redirects are followed** (301, 302, 303, 307, 308), up to 10. A 303, or a 301/302 after a non-GET, continues as a GET; 307 and 308 keep the method and body.
* **WebAssembly has no sockets**, so there every network function returns `Err`.

### The event loop

Every connection of `http::serve` and `ws::serve` runs as a **task**, and so does every implicit future started by such a task. A task is a coroutine with its own small stack, not an operating-system thread. When a task would wait it parks, and its thread runs other tasks meanwhile. A task waits when it receives from a socket, sends to a full one, connects, accepts, sleeps (`time::sleep`), receives from an empty channel or awaits another task. A few scheduler threads (one per core) run all the tasks. One poller thread watches the waiting sockets (kqueue on Apple systems, epoll on Linux) and the timers.

You never write `async` or `await`, and nothing changes in your code. The difference shows at scale: a WebSocket echo server holds 3000 open connections with 14 threads, where one thread per connection would need 3000. A task always stays on the same thread, so pure work inside a handler still uses the parallel array kernels as usual.

Details:
* **Where it runs.** On Apple systems and Linux, on arm64 and x86-64. Elsewhere (Windows, WebAssembly, other CPUs) tasks are threads or deferred calls, as before.
* **Switching it off.** Set the environment variable `MIMIC_NO_EVENT_LOOP=1` to run each connection on its own thread instead.
* **Calls that still block their thread for a moment:** looking up a host name, TLS handshakes on Linux, file I/O, `os::run` and `io::read_line`. Other tasks on the same thread wait meanwhile. Tasks on other threads keep running.
* **Timeouts still apply.** A task's socket wait ends at the same timeout a thread's would: 60 seconds for an HTTP response, 30 seconds for a request a server is reading.
* **Futures started outside a task**, from `main` for example, still get a thread each. That is cheap, because there are few of them.

### `ws`: WebSockets

```mimic
fn echo(sock: ws::Socket) {                       // one call per connection
    match ws::receive(sock) {
        Ok(ws::Text(t)) => { ws::send(sock, upper(t)); echo(sock) }
        _ => {}
    }
}
ws::serve(8080, echo)

let sock = ws::connect("wss://example.com/live") else { return }
ws::send(sock, "hello")
let reply = ws::receive(sock)                     // Ok(ws::Text("…")) / Ok(ws::Binary(bytes)) / Ok(ws::Closed)
ws::close(sock)
```

| Function / type | Meaning |
|---|---|
| `ws::connect(url)` | `Result[ws::Socket, str]` for `ws://` and `wss://` (TLS as for `https://`) |
| `ws::send(socket, text)`, `ws::send_bytes(socket, data)` | `Result[i64, str]` |
| `ws::receive(socket)` | `Result[ws::Message, str]`: `Text(str)`, `Binary([u8])` or `Closed` |
| `ws::close(socket)` | Close (with the closing handshake) |
| `ws::serve(port, handler)` | Call `handler(socket)` for each connection; the socket closes when it returns |
| `ws::serve_connections(port, handler, n)` | The same, stopping after `n` connections |
| `ws::serve_tls(port, handler, identity)`, `ws::serve_tls_connections(port, handler, identity, n)` | The same over TLS (`wss://`) |

How WebSockets behave:
* **Protocol.** RFC 6455: fragmented messages are joined, pings are answered automatically, and messages may be up to 64 MB.
* **Sending.** Sends on one socket from different tasks never interleave.
* **Connections run in parallel** when the handler's effects may overlap, the same rule as `http::serve`.
* **Connections talk through channels** (`chan`): see the chat room in tutorial chapter 19.

### `tcp`

| Function | Result |
|---|---|
| `tcp::connect(host, port)` | `Result[tcp::Socket, str]` |
| `tcp::listen(port)` | `Result[tcp::Listener, str]`; port 0 picks a free port, see `.port` |
| `tcp::accept(listener)` | `Result[tcp::Socket, str]` |
| `tcp::send(socket, text)` | `Result[i64, str]`: everything is sent |
| `tcp::receive(socket, limit)` | `Result[str, str]`: up to `limit` bytes; `""` when the other side closed |
| `tcp::send_bytes(socket, data)`, `tcp::receive_bytes(socket, limit)` | The same with `[u8]` |
| `tcp::close(socket)` | Close a socket or listener |

TCP calls happen in program order.

### `tls`: encryption

```mimic
// a server: a certificate and its key, as PEM text
let cert = fs::read("cert.pem") else { return }
let key = fs::read("key.pem") else { return }
let id = tls::identity(cert, key) else { return }
http::serve_tls(8443, handle, id)                      // https://…:8443
ws::serve_tls(8444, chat, id)                          // wss://…:8444

// encrypted TCP, for protocols of your own
let s = tls::connect("example.com", 443) else { return }
tls::send(s, "…")
let reply = tls::receive(s, 4096)
```

| Function / type | Result |
|---|---|
| `tls::identity(cert, key)` | `Result[tls::Identity, str]`: a server's certificate (chain) and private key, both PEM text |
| `tls::self_signed(host)` | `Result[(str, str), str]`: a new certificate and key (PEM) for `host`, for development |
| `tls::connect(host, port)` | `Result[tls::Stream, str]`: an encrypted connection, checked against the system's trusted certificates |
| `tls::connect_trusting(host, port, cert)` | The same, but trusting exactly the certificate(s) in `cert` (PEM) instead |
| `tls::accept(listener, identity)` | `Result[tls::Stream, str]`: the next connection on a `tcp::Listener`, encrypted |
| `tls::send(stream, text)`, `tls::send_bytes(stream, data)` | `Result[i64, str]` |
| `tls::receive(stream, limit)`, `tls::receive_bytes(stream, limit)` | `Result[str, str]` / `Result[[u8], str]`: up to `limit` bytes; `""` when the other side closed |
| `tls::close(stream)` | Close it |

How TLS behaves:
* **The system's own TLS.** Mimic uses the Security framework on Apple systems and OpenSSL 3 on Linux, loaded when first used, as for `https://`. Statically linked Linux programs cannot load OpenSSL, so there these functions return `Err`.
* **Certificates are PEM text**, the usual `-----BEGIN CERTIFICATE-----` files. Read them with `fs::read`, or take them from the environment. Keys may be PKCS#8 (`BEGIN PRIVATE KEY`), EC or RSA, unencrypted. An identity is loaded once and shared by every connection.
* **Clients check certificates.** `tls::connect`, `http::fetch` and `ws::connect` accept only certificates your system trusts, and only for the host name you asked for. `tls::connect_trusting` replaces the system's list with the certificate you give, which is how you talk to your own servers during development.
* **`tls::self_signed(host)`** makes a P-256 key and a certificate valid for 90 days, for `host` (and for 127.0.0.1 and ::1 when `host` is `"localhost"`). Browsers warn about it. Use a certificate from a certificate authority, such as Let's Encrypt, in production.
* **Apple's limits.** On Apple systems Mimic speaks TLS 1.2, and certificates must follow Apple's rules. For example, Apple rejects server certificates that are valid for more than 825 days.
* **On the event loop**, handshakes and reads park only their task on Apple systems. On Linux the handshake blocks its thread briefly.

### `udp`

```mimic
let sock = udp::bind(0) else { return }                 // port 0: any free port, see sock.port
udp::send(sock, "127.0.0.1", 9999, "ping")
match udp::receive_within(sock, 2s) {
    Ok(p) => print("{p.text} from {p.host}:{p.port}"),
    Err(e) => print(e),                                  // "timed out"
}
```

| Function / type | Result |
|---|---|
| `udp::bind(port)` | `Result[udp::Socket, str]`: a socket on every interface; `.port` is the port it got |
| `udp::send(socket, host, port, text)` | `Result[i64, str]`: one packet |
| `udp::send_bytes(socket, host, port, data)` | The same with `[u8]` |
| `udp::receive(socket)` | `Result[udp::Packet, str]`: waits for one packet |
| `udp::receive_within(socket, timeout)` | The same, or `Err("timed out")` after `timeout` (a duration such as `500ms`) |
| `udp::close(socket)` | Close it |
| `udp::Packet` | `{ data: [u8], text: str, host: str, port: i64 }`: the bytes, the same bytes as text, and the sender |

How UDP behaves:
* **Packets, not streams.** Each send is one packet, at most about 64 KB. Packets can be lost, duplicated or arrive out of order. UDP never tells you, so use `receive_within` and retry when an answer matters.
* **IPv4 only, for now.** Broadcast addresses are allowed.
* **Tasks.** UDP calls may overlap, like WebSockets. A function that only uses UDP can run as a task (`let s = serve_udp(sock)`), and on the event loop a waiting `receive` parks only its task.

## Interfaces: the `ui` module

A screen is a value of type `ui::Node[Msg]`, built by `view(state)`. Events become messages, `update(state, message)` makes the next state, and the browser shows the result. The design is in [ui.md](ui.md), and tutorial chapter 20 walks through an app.

```mimic
fn view(s: State) -> ui::Node[Msg] {
    ui::column(gap: 12, [
        ui::text("Count: {s.count}", size: 24),
        ui::row([ui::button("-", on_click: Dec), ui::button("+", on_click: Inc)]),
    ])
}
ui::run(State { count: 0 }, view, update)               // http://localhost:8080/
```

| Element | Settings (named, all optional) |
|---|---|
| `ui::column(children)`, `ui::row(children)` | `gap`, `padding`, `align`, `background`, `grow` |
| `ui::grid(children)` | `columns`, `gap` |
| `ui::text(content)` | `size`, `weight`, `color`, `strike` |
| `ui::button(label, on_click: message)` | |
| `ui::input(value, on_input: fn(str) -> Msg)` | `placeholder` |
| `ui::checkbox(checked, on_change: fn(bool) -> Msg)` | |
| `ui::form(children, on_submit: message)`, `ui::submit(label)` | `gap` (Enter in an input submits) |
| `ui::link(label, url)`, `ui::image(url)` | `width`, `height`, `alt` (image) |
| `ui::spacer()`, `ui::divider()` | |

| Function | Result |
|---|---|
| `ui::run(init, view, update)` | Serve the interface (`port: 8080`, `title: "Mimic"` by default) until the program is stopped |
| `ui::run_for(init, view, update, n)` | The same, stopping after `n` connections (for tests) |
| `ui::html(node)` | The screen as HTML |
| `ui::diff(a, b, "0")` | The changes (`[ui::Patch]`) that turn screen `a` into screen `b` |
| `ui::message(node, event)` | The message a browser event (JSON) stands for, if any |

How it works:
* **Each browser tab is a task with its own state.** The first page is complete HTML. After that, events travel to the program over a WebSocket, and only the changed parts travel back.
* **`view` and `update` are pure.** Lists of children built with `¨` are built in parallel, and so is the comparison of the old and new screens. Different tabs run in parallel too.
* **Functions inside a screen must be pure** (`on_input: Typed` or `t => Typed(trim(t))`). Printing or other effects there is error E0605. Do effects in `update`'s caller, or wait for the jobs described in ui.md.
* **Built-in theme:** readable defaults with light and dark modes.
* **Not yet:** jobs (`ui::Step`), timers and subscriptions, keyed lists, other targets (WebAssembly, native windows).

