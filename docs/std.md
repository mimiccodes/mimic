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

## Standard modules

Everything with effects lives in a module, used by path from any file with no import: `fs::read(p)`, `time::now()`. `use fs::read` brings in a short name. A module of your program with the same name comes first. The global names are only `print`, `Option`/`Result` and the string, list and map functions above.

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

## Networking: the `http` and `tcp` modules

Networking lives in two standard modules, used by path from any file with no import: `http::fetch`, `tcp::connect`. (`use http::fetch` works too.) Your own modules come first: a project or module called `http` hides the standard one.

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
| `http::Request` | `{ method, path, query, headers, body }`: header names are lower case, `query` is the part after `?` |
| `http::Response` | `{ status, headers, body }` |
| `http::ok(body)`, `http::respond(status, body)`, `http::not_found()` | Responses (text/plain unless the headers say otherwise) |
| `http::json(value)` | A 200 response with `value` encoded as JSON (`application/json`) |

How the client and server behave:
* **Requests overlap.** `let a = http::fetch(u1)` and `let b = http::fetch(u2)` are in flight at the same time (implicit futures).
* **The server runs a pure or overlapping handler in parallel**, one thread per connection. A handler that prints or writes files handles one request at a time, in order.
* **One request per connection** (`Connection: close`). Bodies use `Content-Length`; chunked responses are decoded.
* **`https://` is not supported yet** (TLS is planned) and returns `Err`. WebAssembly has no sockets, so there every network function returns `Err`.

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
