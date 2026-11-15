---
title: Coming from JavaScript
description: JavaScript habits and their Mimic forms: map, filter and reduce, objects, null, async code, JSON and servers.
---

# Coming from JavaScript

If you know `map`, `filter` and `reduce`, you already know how Mimic works: everything is a transformation of whole arrays. The difference is that Mimic compiles to native code, checks types before running, and runs those transformations on every core.

| In JavaScript | In Mimic |
|---|---|
| `xs.map(x => x * 2)` | `xs ¨ (x => x * 2)`, or just `xs × 2` |
| `xs.filter(x => x > 2)` | `xs ⌿ (xs > 2)` |
| `xs.reduce((a, b) => a + b, 0)` | `xs ▷ gather(+)`, or `+/ xs` |
| `xs.length` | `≢ xs` |
| `xs.slice(1, 4)` | `xs ↓ 1 ↑ 3` |
| `[...a, ...b]`, `[...a, x]` | `concat(a, b)`, `push(a, x)` |
| `{ name: "Ada", age: 36 }` | `User { name: "Ada", age: 36 }`, with `type User = { name: str, age: i64 }` |
| `{ ...u, age: 37 }` | `{ u with age: 37 }` |
| `null`, `undefined` | `None`, inside an `Option` |
| `` `${a} + ${b}` `` | `"{a} + {b}"` |
| `throw` / `try` | return `Err(…)` / `match` |
| `await fetch(url)` | `http::fetch(url)`: no `await`, see below |
| `JSON.stringify(u)`, `JSON.parse(t)` | `json::encode(u)`, `json::decode[User](t)` |
| `console.log(x)` | `print(x)` |

## map, filter, reduce

```mimic run
module main

fn main() {
    let prices = [12 5 30 8]
    print(prices ¨ (p => p * 2))
    print(prices ⌿ (prices > 10))
    print(prices ▷ gather(+))
    print(prices ▷ trail(+))        // running totals, like a scan
    print(prices ▷ gather(max))
}
```

```output
[24 10 60 16]
[12 30]
55
[12 17 47 55]
30
```

`gather` may combine elements in any order to work in parallel, so its function must be associative, as `+`, `×`, `max` and `min` are.

## Objects are records

```mimic run
module main

type User = { name: str, age: i64 }

fn main() {
    let ada = User { name: "Ada", age: 36 }
    let older = { ada with age: 37 }
    print(older)
    print(ada.age)                  // unchanged: values never change in place
    let users = [ada, older, User { name: "Alan", age: 41 }]
    print(users ⌿ (users ¨ (u => u.age > 36)) ¨ (u => u.name))
}
```

```output
User { name: "Ada", age: 37 }
36
["Ada" "Alan"]
```

## null and exceptions

A value that may be missing is an `Option`; an operation that may fail returns a `Result`. Both must be handled before the value inside can be used, so "cannot read property of undefined" cannot happen:

```mimic run
module main

fn main() {
    let ports = ["web": 8080, "db": 5432]
    match get(ports, "cache") {
        Some(p) => print("cache on {p}"),
        None => print("no cache configured"),
    }
    let n = parse_int("42") else { return }
    print(n + 1)
}
```

```output
no cache configured
43
```

## No async, no await

Calls that wait (network, files, timers) start as soon as their arguments are ready and run at the same time as everything else; the program waits for a result only where it is used. Two requests written one after the other overlap, like `Promise.all`, with no keywords:

```mimic
fn main() {
    let a = http::fetch("https://example.com/a")    // starts now
    let b = http::fetch("https://example.com/b")    // starts now too
    print(a)                                       // waits for a here
    print(b)
}
```

A server takes a handler function, and each connection runs as its own lightweight task (thousands at once):

```mimic
fn handle(req: http::Request) -> http::Response {
    match req.path {
        "/" => http::ok("hello"),
        "/users/{id: i64}" => http::json(User { name: "user {id}", age: 0 }),
        _ => http::not_found(),
    }
}

fn main() { http::serve(8080, handle) }
```

`mimic man networking` and `mimic man http` have the details.

## JSON

```mimic run
module main

type User = { name: str, age: i64 }

fn main() {
    let text = json::encode(User { name: "Ada", age: 36 })
    print(text)
    match json::decode[User](text) {
        Ok(u) => print(u.name),
        Err(e) => print("bad JSON: {e}"),
    }
}
```

```output
{"name":"Ada","age":36}
Ada
```

## Types

Types are checked before the program runs, as with TypeScript in strict mode, but most are inferred: annotate record fields and, when you like, function parameters. There is no `any` that silently turns off the checks.

## Web pages

`ui` programs describe an interface as data from the state (`view(state)`), much like React. The same program runs in the browser (WebAssembly), in a native window or in the terminal: `mimic man interfaces`.
