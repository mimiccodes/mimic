# Interfaces (E5): design proposal

Status: **agreed design** (2026-11-12): functions with named arguments, a server-driven browser target first, and `update` returning jobs. It is being implemented in the order at the end.

## The idea in one paragraph

A user interface is a pure function from the program's **state** to a description of the screen. When something happens (a click, a key press, a reply from a server), it becomes a **message**. A pure `update(state, message)` makes the next state, and `view` runs again. Mimic then works out what changed and updates only that. You never touch the screen directly, never lock anything, and never write `async`. Because `view` and `update` are pure, the compiler may run them in parallel: list items, subtrees and diffs are computed across all cores, like any other `¨`.

## A complete program

```mimic
module main

type Msg = Add | Remove(i64) | Typed(str) | Toggle(i64)
type Todo = { text: str, done: bool }
type State = { todos: [Todo], draft: str }

fn update(s: State, m: Msg) -> State {
    match m {
        Typed(t) => { s with draft: t },
        Add if trim(s.draft) == "" => s,
        Add => { s with todos: push(s.todos, Todo { text: s.draft, done: false }), draft: "" },
        Remove(i) => { s with todos: s.todos ⌿ (⍳ ≢ s.todos ¨ (k => k != i)) },
        Toggle(i) => { s with todos[i].done: not s.todos[i].done },   // new syntax, see below
    }
}

fn view(s: State) -> ui::Node[Msg] {
    ui::column(gap: 12, padding: 24, [
        ui::text("To do ({≢ s.todos})", size: 28, weight: "bold"),
        ui::row(gap: 8, [
            ui::input(s.draft, placeholder: "What needs doing?", on_input: Typed, on_enter: Add),
            ui::button("Add", on_click: Add),
        ]),
        ui::column(gap: 4, ⍳ ≢ s.todos ¨ (i => item(i, s.todos[i]))),
    ])
}

fn item(i: i64, t: Todo) -> ui::Node[Msg] {
    ui::row(gap: 8, [
        ui::checkbox(t.done, on_change: _ => Toggle(i)),
        ui::text(t.text, strike: t.done),
        ui::button("×", on_click: Remove(i)),
    ])
}

fn main() {
    print(ui::run(State { todos: [], draft: "" }, view, update))
}
```

`mimic run todo.mim` prints `open http://localhost:8080` and opens the browser. Every browser tab gets its own state.

## Decisions to make

### 1. How screens are written: functions, not markup

Elements are ordinary functions in the `ui` module. Children are a list, and settings are named arguments with defaults (`gap: 12`, `size: 28`). There is no new markup syntax (no JSX or HTML templates). Lists of children come from ordinary array code (`¨`, `⌿`), so loops never appear, and long lists are built in parallel.

`use ui::column` (one `use` per name) brings in short names for those who prefer them, following the namespacing rule.

### 2. The first target: the browser, driven by the program

`ui::run` starts an HTTP and WebSocket server on the event loop. Each browser tab is one task that holds that tab's state.
* **First load:** the page is sent as complete HTML.
* **After that:** events travel to the program over the WebSocket, and only the changed parts travel back as small patches.
* **The browser side:** about 3 KB of embedded JavaScript applies the patches. Nothing is written in JavaScript by you.

Why this target first:
* It works on every platform with no toolkit, and it reuses `http`, `ws`, `chan` and the event loop we already have.
* State never leaves the program, so secrets and database connections stay on the server.
* Thousands of open tabs are just thousands of tasks.

Later targets run the same `view` and `update` unchanged:
* **WebAssembly:** the whole app runs inside the browser, with no server.
* **Native windows:** AppKit on macOS first, then Windows and Linux.
* **Terminal:** text interfaces.

### 3. Effects: `update` stays pure; work comes back as messages

Fetching data or saving files must not freeze the screen. So when `update` wants work done, it returns the work too, and the runtime runs it as a task. The work's result arrives later as a message:

```mimic
type Msg = Search(str) | Found(Result[[Book], str])

fn update(s: State, m: Msg) -> ui::Step[State, Msg] {
    match m {
        Search(q) => ui::then({ s with query: q, loading: true }, () => Found(find_books(q))),
        Found(r) => ui::stay({ s with books: r, loading: false }),
    }
}
```

* `ui::stay(state)` means "no work".
* `ui::then(state, job)` starts `job` as a task. Several jobs started together run in parallel.
* `ui::run` accepts an `update` that returns either `State` or `ui::Step[State, Msg]`.
* Things that happen by themselves arrive through `ui::every(1s, Tick)` (timers) and `ui::listen(receiver)` (a `chan::Receiver[Msg]`), declared by an optional `subscriptions(state)` function. A chat room's messages are an example.

### 4. Layout and style

| Kind | Elements |
|---|---|
| Layout | `column`, `row`, `stack` (layers), `grid(columns: n)`, `scroll`, `spacer` |
| Content | `text`, `image(url)`, `link(text, url)`, `icon(name)`, `divider` |
| Input | `button`, `input`, `textarea`, `checkbox`, `slider`, `select(options)` |
| Shared settings | `gap`, `padding`, `width`/`height` (`ui::fill`, a number or `ui::fit`), `align`, `background`, `color`, `radius`, `border` |

Colours are `"#3366ff"` or names. Sizes are numbers (logical pixels). There is a theme with sensible defaults and a light and a dark mode, so a plain program already looks good. `ui::raw_html(text)` is an escape hatch for the browser target only.

### 5. Lists keep their identity

`ui::keyed(key, node)` marks list items, so inserting at the top of a list does not rebuild every row. Without keys, items are matched by position, which is right for most lists.

## Language additions it needs

* **Variant constructors as functions:** `on_input: Typed` instead of `on_input: t => Typed(t)`. This is useful everywhere, not just for interfaces.
* **Updating one element of a list in a record:** `{ s with todos[i].done: not s.todos[i].done }`. Today this needs a whole `¨`.
* **`ui::Node[Msg]` as a generic std type** whose event handlers carry `Msg` values. Generics already exist; the runtime stores handlers as closures.

## Build order

1. Constructors as functions, then nested `with` updates.
2. The `ui` module types and elements, and HTML rendering with a test for each element.
3. `ui::run`: the WebSocket session, event decoding, the diff and the patch protocol, with a todo-list example and a golden test that drives the app through a scripted WebSocket client.
4. `ui::Step` jobs, `subscriptions`, timers and channels.
5. A parallel diff for large trees; a benchmark with 10 000 rows.
6. Tutorial chapter 20 ("Interfaces"), `docs/std.md`.
