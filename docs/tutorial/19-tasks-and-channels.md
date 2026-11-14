---
title: Tasks and channels
description: Background tasks that talk to each other through typed channels — a worker pool and a WebSocket chat room.
order: 19
---

# Tasks and channels

Mimic has no shared mutable state: values never change, and no two pieces of code ever write to the same place. That is what makes its automatic parallelism safe. When independent parts of a program need to talk to each other, for example the connections of a chat server, they send messages through **channels**.

## Tasks

A task is something you already know: a `let` whose function only has overlapping effects starts in the background as an implicit future ([chapter 10](10-parallel-by-default.md)). Channel operations are overlapping effects, so a function that loops over a channel is a task as soon as you `let` it.

A task never outlives the function that started it. When the function finishes, through its last line or through any `return`, it first waits for every task it started and has not waited for yet. So no task is ever forgotten half-way. It also means that if a task never ends, the function never ends either. A server task started with `http::serve(port, handle)` runs forever, so its function returns only when the program is stopped. A server that should stop uses `http::serve_requests(port, handle, n)`, which ends after `n` requests.

## Channels

`chan::new[T]()` makes a channel for values of type `T`. It returns a sender and a receiver:

* **`chan::send(tx, value)`** never waits (but see [bounded channels](#bounded-channels)).
* **`chan::receive(rx)`** waits for the next value, and returns `None` once the channel is closed and empty.
* **`chan::close(tx)`** says no more values will come.

A worker that squares numbers until its input closes:

```mimic run
module main

// a worker task: squares numbers until the channel closes, then reports
fn worker(jobs: chan::Receiver[i64], results: chan::Sender[i64]) -> i64 {
    match chan::receive(jobs) {
        Some(n) => {
            chan::send(results, n * n)
            1 + worker(jobs, results)
        }
        None => {
            chan::close(results)
            0
        }
    }
}

fn collect(rx: chan::Receiver[i64], acc: [i64]) -> [i64] {
    match chan::receive(rx) {
        Some(x) => collect(rx, push(acc, x)),
        None => acc,
    }
}

fn main() {
    let (jobs_tx, jobs_rx) = chan::new[i64]()
    let (res_tx, res_rx) = chan::new[i64]()
    let done = worker(jobs_rx, res_tx)              // runs in the background
    print(chan::send(jobs_tx, 3))
    chan::send(jobs_tx, 4)
    chan::send(jobs_tx, 5)
    chan::close(jobs_tx)
    print(chan::send(jobs_tx, 6))                   // closed: false
    print(collect(res_rx, []))
    print(done)
    let (tx, rx) = chan::new[str]()
    print(chan::try_receive(rx))
    chan::send(tx, "hi")
    print(chan::try_receive(rx))
}
```

```output
true
false
[9 16 25]
3
None
Some("hi")
```

The worker runs at the same time as `main`. `collect` gathers results until the worker closes its output channel, and `done` (the worker's count) is awaited where it is printed.

The ends of a channel are ordinary values, so they can be put in lists, captured by closures, or even sent through other channels. A sender sent through a channel is how a server hands each new connection a way to be reached, as the chat room below shows.

## Bounded channels

A channel from `chan::new` has no limit. If a producer is faster than its consumer, values pile up in memory. `chan::bounded[T](capacity)` makes a channel that holds at most `capacity` values. A `send` to a full channel waits until the receiver has taken one. This is called **backpressure**:

```mimic run
module main

// A bounded channel holds at most `capacity` values: a sender waits while it
// is full, so a fast producer cannot run far ahead of a slow consumer.

fn produce(tx: chan::Sender[i64], log: chan::Sender[i64], i: i64, n: i64) -> i64 {
    if i > n {
        chan::close(tx)
        chan::close(log)
        n
    } else {
        chan::send(tx, i)
        chan::send(log, i)                 // after the send went through
        produce(tx, log, i + 1, n)
    }
}

fn drain(rx: chan::Receiver[i64], acc: [i64]) -> [i64] {
    match chan::receive(rx) {
        Some(x) => drain(rx, push(acc, x)),
        None => acc,
    }
}

fn main() {
    let (tx, rx) = chan::bounded[i64](2)
    let (log_tx, log_rx) = chan::new[i64]()
    let p = produce(tx, log_tx, 1, 6)
    time::sleep(50ms)
    // the producer is waiting to send 3: only 1 and 2 got in
    print(chan::try_receive(log_rx))
    print(chan::try_receive(log_rx))
    print(chan::try_receive(log_rx))
    print(drain(rx, []))
    print(drain(log_rx, []))
    print(p)
}
```

```output
Some(1)
Some(2)
None
[1 2 3 4 5 6]
[3 4 5 6]
6
```

The producer starts at once, sends `1` and `2`, and then waits: the channel is full. It goes on only when `drain` takes values out. It never gets more than two values ahead.

## A chat room

A chat server needs every connection to hear what the others say. One **hub** task owns the list of connected clients, and each connection talks to the hub through channels:

* **Joining.** A connection creates its own outgoing channel, sends `Join(id, out_tx)` to the hub, and then does two things at once. It forwards what its client says to the hub (`forward`, a task) and writes whatever arrives on its outgoing channel back to the client (`pump`).
* **Broadcasting.** The hub broadcasts every `Say` to all clients. When a client leaves, the hub closes that client's channel, which ends its `pump`.

```mimic run
module main

// A chat room: one hub task owns the list of clients; every WebSocket
// connection talks to it through channels.
type Event = Join(i64, chan::Sender[str]) | Leave(i64) | Say(str)

fn hub(events: chan::Receiver[Event], clients: [(i64, chan::Sender[str])]) -> i64 {
    match chan::receive(events) {
        Some(Join(id, out)) => {
            let now = push(clients, (id, out))
            broadcast(now, "{≢ now} online")
            hub(events, now)
        }
        Some(Leave(id)) => {
            let gone = clients ⌿ (clients ¨ (c => c.0 == id))
            gone ¨ (c => chan::close(c.1))
            hub(events, clients ⌿ (clients ¨ (c => c.0 != id)))
        }
        Some(Say(text)) => {
            broadcast(clients, text)
            hub(events, clients)
        }
        None => ≢ clients
    }
}

fn broadcast(clients: [(i64, chan::Sender[str])], text: str) -> [bool] {
    clients ¨ (c => chan::send(c.1, text))
}

// one connection: join, then read from the client and write to it at once
fn session(sock: ws::Socket, events: chan::Sender[Event]) {
    let (out_tx, out_rx) = chan::new[str]()
    chan::send(events, Join(sock.id, out_tx))
    let reading = forward(sock, events)            // a task of its own
    pump(out_rx, sock)
}

fn forward(sock: ws::Socket, events: chan::Sender[Event]) -> i64 {
    match ws::receive(sock) {
        Ok(ws::Text(t)) => {
            chan::send(events, Say(t))
            1 + forward(sock, events)
        }
        _ => {
            chan::send(events, Leave(sock.id))
            0
        }
    }
}

fn pump(rx: chan::Receiver[str], sock: ws::Socket) -> i64 {
    match chan::receive(rx) {
        Some(m) => {
            ws::send(sock, m)
            1 + pump(rx, sock)
        }
        None => 0
    }
}

fn server(port: i64, events: chan::Sender[Event]) -> Result[i64, str] {
    let served = ws::serve_connections(port, s => session(s, events), 2)
    chan::close(events)                            // the room closes with the server
    served
}

fn connect_when_up(url: str, tries: i64) -> Result[ws::Socket, str] {
    match ws::connect(url) {
        Ok(s) => Ok(s),
        Err(e) => if tries <= 1 { Err(e) } else { time::sleep(20ms); connect_when_up(url, tries - 1) },
    }
}

fn text_of(r: Result[ws::Message, str]) -> str {
    match r {
        Ok(ws::Text(t)) => t,
        Ok(other) => string(other),
        Err(e) => "error: {e}",
    }
}

fn main() {
    let port = 30000 + os::pid() % 20000
    let (ev_tx, ev_rx) = chan::new[Event]()
    let room = hub(ev_rx, [])
    let s = server(port, ev_tx)
    let ada = match connect_when_up("ws://127.0.0.1:{port}/", 100) {
        Ok(x) => x,
        Err(e) => {
            print("no WebSocket: {e}")
            return
        }
    }
    print("ada sees: " + text_of(ws::receive(ada)))
    let bob = ws::connect("ws://127.0.0.1:{port}/") else { return }
    print("ada sees: " + text_of(ws::receive(ada)))
    print("bob sees: " + text_of(ws::receive(bob)))
    ws::send(ada, "hello from ada")
    print("ada sees: " + text_of(ws::receive(ada)))
    print("bob sees: " + text_of(ws::receive(bob)))
    ws::send(bob, "hi ada")
    print("ada sees: " + text_of(ws::receive(ada)))
    print("bob sees: " + text_of(ws::receive(bob)))
    ws::close(ada)
    ws::close(bob)
    print(s)
    print(room)
}
```

```output
ada sees: 1 online
ada sees: 2 online
bob sees: 2 online
ada sees: hello from ada
bob sees: hello from ada
ada sees: hi ada
bob sees: hi ada
Ok(2)
0
```

Notice what you did not write: no locks, no threads, no `async`. The server runs each connection in parallel because `session`'s effects (WebSockets and channels) may overlap. The hub owns its state in its parameters, and everything else is a message.

## Thousands of connections

Each connection is a **task**, not a thread. A task that waits for a message, a socket or a timer steps aside, and its thread runs other tasks meanwhile. A few threads, one per core, carry all of them. The chat room above serves thousands of people as easily as two, with the same code.

The details, and how to switch this off, are in the [standard library reference](../std.md#the-event-loop).

Back to the [index](index.md).
