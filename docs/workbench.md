# The Workbench engine

Every tool for working with Mimic asks one analysis engine about the project: `mimic query` today, then the language server, MCP for AI agents and the terminal editor (stage 0.3, [versions.md](versions.md)). They all use the same questions and the same JSON, described here.

The engine loads the program exactly as `mimic check` does. In a process that keeps running (`mimic query --serve`), it keeps the parse of the standard library and of every file that did not change, and reads files open in an editor from the editor instead of the disk. Re-checking the showcase project after an edit takes about 25 ms.

When the program has errors, the engine reports them and keeps the index of the last program that checked, so lookups keep working while code is being typed (`"stale_index": true`).

## On the command line

```sh
mimic query check
mimic query symbols query=read kind=fn
mimic query definition file=src/main.mim line=12 column=9
mimic query references name=geometry::area
mimic query hover file=src/main.mim line=12 column=9
mimic query docs name=fs::read
mimic query effects
mimic query --in script.mim outline file=script.mim
```

Parameters are written `key=value`. Numbers and `true`/`false` are read as such; a value starting with `{`, `[` or `"` is read as JSON. The answer is printed as JSON. The exit status is 1 when the question fails, or when `check` finds errors. `--in FILE` analyses a standalone file instead of the project in the current folder.

## As a running process

`mimic query --serve` reads one JSON request per line on stdin and writes one answer per line on stdout:

```json
{"id": 1, "method": "definition", "params": {"file": "src/main.mim", "line": 12, "column": 9}}
{"id": 1, "result": {"name": "area", "kind": "fn", "path": "geometry::area", …}}
```

A failed request answers `{"id": …, "error": "…"}`. `{"method": "exit"}` ends the process.

## Positions and locations

Lines and columns start at 1. Columns count characters, so `⍳` is one column. File paths may be absolute or relative to the current folder.

A location in an answer is:

```json
{"file": "/abs/path/src/main.mim", "shown": "src/main.mim", "line": 3, "column": 8, "end_line": 3, "end_column": 12}
```

## Symbols

A symbol is a function, a function from C or the runtime (`extern fn`), a type, a variant or a module:

| Field | Meaning |
|---|---|
| `name` | Its own name: `read` |
| `kind` | `fn`, `extern fn`, `type`, `variant`, `module` |
| `path` | As code writes it: `fs::read`, `c::sqrt`, `geometry::area`. A program's `main` module is left out, and a library's `lib` module is named by the package. |
| `package` | The package: `std` for the standard library, or the project's or a dependency's name |
| `public` | Declared `pub` |
| `signature` | `fn read(path: str) -> Result[str, str]` |
| `doc` | The `//` comment right above it, as `mimic doc` shows it |
| `header` | For functions imported from a C header: the header (`math.h`) |
| `location` | Where its name is declared |

## Methods

| Method | Parameters | Answer |
|---|---|---|
| `check` | `again` (check even if nothing was asked to change) | The diagnostics, in the format of [diagnostics.md](diagnostics.md), with `generation` (one more for every check), `took_ms`, `files`, `functions` and `stale_index`. |
| `open`, `change` | `file`, `text` | The editor's text for `file`, used instead of the disk's until `close`; then the answer of `check`. The file on disk is never written. |
| `close` | `file` | The file is read from the disk again; the answer of `check`. |
| `symbols` | `query`, `kind`, `package`, `limit` (200) | `{"total": n, "symbols": [symbol, …]}`: exact matches first, then names starting with `query`, then paths containing it. |
| `definition` | `file`, `line`, `column` | The symbol named at that position, or `{"name", "kind": "local", "location"}` for a parameter or local variable; `null` when nothing is named there. |
| `hover` | `file`, `line`, `column` | As `definition`, plus `effects` for functions. |
| `references` | `name`, or `file`, `line`, `column` | Every location where the symbol (or the local variable) is used. |
| `outline` | `file` | The symbols declared in the file, in order. |
| `docs` | `name` | The symbol, with its comment and `effects`. |
| `effects` | `name`, or `package` (the project) | What each function does besides computing a result: `pure` (may run in parallel), `concurrent` (effects that may overlap, such as reading files or waiting), `ordered` (effects that keep their order, such as printing) or `sync` (waits for pending work first, such as writing files or running programs). |
| `types` | `file` | The inferred type of each `let` without an annotation in the file, with the position just after its name: what editors show as inlay hints. |
| `stats` | | How many files, functions, types, variants, modules and references the index holds. |

New methods are added with the steps of stage 0.3: glyphs and the manual (0.3.2), where code runs in parallel (0.3.9), audit findings (0.3.10).
