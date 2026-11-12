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

## Program, files and directories

These functions have side effects. The compiler knows this: a function that calls one of them, even indirectly, never runs in parallel and never moves past other effects, the same as `print`. Failures come back as values, never as crashes.

| Function | Result |
|---|---|
| `args()` | `[str]`: the command-line arguments, without the program name (`mimic run file.mim -- a b`) |
| `env(name)` | `Option[str]` |
| `set_env(name, value)` | `Result[bool, str]` |
| `read_line()` | `Option[str]`: the next line of standard input without its newline; `None` at the end |
| `read_input()` | `str`: everything left on standard input |
| `eprint(x)` | Print to standard error |
| `exit(code)` | Leave the program now with that exit status |
| `read_file(path)` | `Result[str, str]` |
| `write_file(path, text)`, `append_file(path, text)` | `Result[i64, str]`: bytes written |
| `file_size(path)` | `Result[i64, str]` |
| `copy_file(from, to)` | `Result[i64, str]` |
| `rename(from, to)`, `remove_file(path)` | `Result[bool, str]`; `remove_file` also removes empty directories |
| `exists(path)`, `is_file(path)`, `is_dir(path)` | `bool` |
| `list_dir(path)` | `Result[[str], str]`: sorted names, without `.` and `..` |
| `make_dir(path)` | `Result[bool, str]`: creates missing parents too; fine if it exists |
| `current_dir()`, `temp_dir()` | `str` |

Error messages name the operation, the path and the system's reason: `cannot read data.csv: No such file or directory`.

## Time

Instants and durations are both `i64` nanoseconds, so duration literals mix freely: `now() + 90min`, `took < 2s`.

| Function | Result |
|---|---|
| `now()` | Wall-clock time: nanoseconds since 1970-01-01 00:00 UTC |
| `clock()` | A monotonic clock in nanoseconds, for measuring how long something takes (never jumps) |
| `sleep(d)` | Pause this thread, e.g. `sleep(250ms)` |
| `timestamp(y, mo, d, h, mi, s)` | The instant of a UTC date and time |
| `utc(t)`, `local(t)` | A `DateTime { year, month, day, hour, minute, second, nanosecond, weekday, utc_offset }`; weekday 1 = Monday … 7 = Sunday; `utc_offset` in seconds |
| `format_time(t)` | ISO 8601 / RFC 3339 in UTC: `2026-11-12T14:30:05Z`, with `.250` milliseconds when present |

The calendar is the proleptic Gregorian one, correct for any year, before 1970 included.

## Random numbers

| Function | Result |
|---|---|
| `random()` | A float in `[0, 1)` |
| `random_int(lo, hi)` | An integer in `[lo, hi)`, without bias |
| `seed_random(n)` | Make the sequence repeatable (per thread) |

The generator is xoshiro256**, seeded from the operating system's entropy. Each thread has its own. Drawing a number changes the generator's state, so code that uses random numbers runs in order and never in parallel. It is not for cryptography.

## Running programs

| Function | Result |
|---|---|
| `run(program, arguments)` | `Result[Output, str]`. `program` is looked up through `PATH`. `Output { status, stdout, stderr }` holds the exit status (128 + n when killed by signal n) and both outputs, captured completely. |
| `shell(command)` | `run("/bin/sh", ["-c" command])` |
| `pid()` | This process's id |

The child's standard input is empty. WebAssembly (WASI), tvOS and watchOS have no child processes, so there `run` returns `Err`.

## Paths

Pure text operations, so they stay parallel:

| Function | Example |
|---|---|
| `path_join(a, b)` | `path_join("logs", "today.txt")` → `"logs/today.txt"`; an absolute `b` replaces `a` |
| `file_name(p)` | `"a/b/c.tar.gz"` → `"c.tar.gz"` |
| `extension(p)` | `"a/b/c.tar.gz"` → `"gz"` |
| `stem(p)` | `"a/report.pdf"` → `"report"` |
| `parent(p)` | `"a/b/c.txt"` → `"a/b"`; `"c.txt"` → `""` |

`lines(s)` splits at newlines and drops a `\r` before each one. A final newline ends the last line instead of starting an empty one, so `lines(read_file(…))` gives exactly the file's lines.

The names in the standard library never block your own: a variable or function of yours may be called `args`, `parent` or `remove_file`, and your definition wins in your code.

## Networking

### HTTP

```mimic
let r = fetch("http://example.com/") else { return }      // Result[Response, str]
print((r.status, r.body, get(r.headers, "content-type")))
request("POST", url, ["content-type": "application/json"], body)

fn handle(req: Request) -> Response {
    match (req.method, req.path) {
        ("GET", "/") => ok("hello"),
        ("GET", "/users/{id: i64}") => ok("user #{id}"),
        _ => not_found(),
    }
}
serve(8080, handle)                                        // runs until the program ends
```

| Function / type | Meaning |
|---|---|
| `fetch(url)` | `Result[Response, str]`: a GET request |
| `request(method, url, headers, body)` | Any method, with `[str: str]` headers and a body |
| `serve(port, handler)` | An HTTP/1.1 server; `handler` is a `fn(Request) -> Response` |
| `serve_requests(port, handler, n)` | The same, stopping after `n` requests; `Ok(n)` |
| `Request` | `{ method, path, query, headers, body }`: header names are lower case, `query` is the part after `?` |
| `Response` | `{ status, headers, body }` |
| `ok(body)`, `respond(status, body)`, `not_found()` | Responses (text/plain unless the headers say otherwise) |

How the client and server behave:
* **Requests overlap.** `let a = fetch(u1)` and `let b = fetch(u2)` are in flight at the same time (implicit futures).
* **The server runs a pure handler in parallel**, or one whose effects may overlap: each connection gets its own thread. A handler that prints or writes files handles one request at a time, in order.
* **One request per connection** (`Connection: close`). Request and response bodies use `Content-Length`; chunked responses are decoded.
* **`https://` is not supported yet** (TLS is planned) and returns `Err`. WebAssembly has no sockets, so there every network function returns `Err`.

### TCP

| Function | Result |
|---|---|
| `tcp_connect(host, port)` | `Result[Socket, str]` |
| `tcp_listen(port)` | `Result[Listener, str]`; port 0 picks a free port, see `.port` |
| `tcp_accept(listener)` | `Result[Socket, str]` |
| `send(socket, text)` | `Result[i64, str]`: everything is sent |
| `receive(socket, limit)` | `Result[str, str]`: up to `limit` bytes; `""` when the other side closed |
| `close(socket)` | Close a socket or listener |

TCP calls happen in program order.
