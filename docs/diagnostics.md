# Diagnostics

Every error the compiler reports has a **stable code** (`E0401`) and a **stable name** (`type-mismatch`). Most errors also have a precise source span, secondary labels, notes, and suggested fixes as exact text edits. The same data is printed two ways:

* **For people:** rustc-style text on stderr.
* **For editors, scripts and AI agents:** JSON, via `--emit-json`.

```text
error[E0401]: argument `x` of `half`: expected f64, found str
  --> src/main.mim:13:16
   |
13 |     print(half("ten"))
   |                ^^^^^
   |
 6 | fn half(x: f64) -> f64 { x / 2.0 }
   |         - parameter declared here
```

## Commands

| Command | What it does |
|---|---|
| `mimic check --emit-json [file]` | Prints one JSON document on stdout. Exit status 0 means no errors; 1 means errors. |
| `mimic build --emit-json …` | The same, plus the list of artifacts produced on success. |
| `mimic run --emit-json …` | On a compile error, prints the JSON to **stderr** (stdout belongs to the program). |
| `mimic check --fix [file]` | Applies every `machine-applicable` fix to the source files, then checks again. |
| `mimic explain E0401` | Prints the long explanation of a code, with an example and the usual fix. |
| `mimic explain` | Lists every code. `--emit-json` prints them as JSON. |

Codes are permanent. A code never changes meaning. A retired code is kept and never reused. Tools should match on `code` or `name`, never on `message`, since messages may be reworded.

## JSON schema (version 1)

```jsonc
{
  "schema": 1,                    // bumped only on incompatible changes
  "tool": "mimic",
  "version": "0.1.0",             // compiler version
  "command": "check",             // check | build | run
  "success": false,
  "summary": { "errors": 1, "warnings": 0 },
  "diagnostics": [
    {
      "severity": "error",        // error | warning
      "code": "E0303",
      "name": "unresolved-name",
      "message": "cannot find variable `totl` in this scope",
      "location": {               // null for errors with no source location
        "file": "src/main.mim",   // relative to the project root, as given for single files
        "start": { "offset": 61, "line": 6, "column": 11 },
        "end":   { "offset": 65, "line": 6, "column": 15 },
        "text": "totl"            // the source text of the span
      },
      "labels": [                 // secondary locations, possibly in other files
        { "location": { … }, "message": "declared here without `pub`" }
      ],
      "notes": [],
      "help": [],
      "fixes": [
        {
          "message": "did you mean `total`?",
          "applicability": "maybe-incorrect",
          "edits": [
            { "location": { … "text": "totl" }, "replacement": "total" }
          ]
        }
      ],
      "rendered": "error[E0303]: …"   // the human rendering, verbatim
    }
  ],
  // success only:
  "package": "demo", "files": 3, "functions": 12,   // check
  "fixed": 0,                                       // check
  "artifacts": [ { "target": "aarch64-macos", "profile": "release",
                   "kind": "executable", "path": "…" } ]   // build
}
```

### Positions

* `offset` is a byte offset into the file's UTF-8 text.
* `line` and `column` are 1-based.
* Columns count Unicode scalar values (characters), so `⍳` is one column.

### Applying edits

To apply an edit, replace the bytes `start.offset..end.offset` with `replacement`. When `start` equals `end`, the edit is an insertion. Apply all of a fix's edits together, from the last offset to the first.

### Applicability

| Value | Meaning |
|---|---|
| `machine-applicable` | Applying the edits gives the intended code. `--fix` applies these. |
| `maybe-incorrect` | Probably right, but confirm first (e.g. "did you mean"). |
| `has-placeholders` | The replacement contains `…` that must be filled in (e.g. missing `match` arms). |

### Errors without a location

Some errors have no position in a `.mim` file:
* project-loading errors (`E09xx`): a missing manifest, an unresolvable dependency;
* toolchain failures (`E0905`).

These have `"location": null`. A tool's own output, such as a linker error, is kept in `notes`.

## Fixes the compiler offers today

| Code | Fix | Applicability |
|---|---|---|
| E0204 | Rename a lowercase type or variant | maybe-incorrect (its uses need renaming too) |
| E0303 | Correct a misspelled name or path (variables, functions, modules, types) | maybe-incorrect |
| E0304 | Add `pub` to the private item's definition | machine-applicable |
| E0402 | Correct a misspelled type name | maybe-incorrect |
| E0403 | Correct a misspelled field name in a record literal | maybe-incorrect |
| E0501 | Insert the missing `match` arms with `…` bodies | has-placeholders |
| E0601 | Change `let x` to `let mut x` | machine-applicable |
| E0702 | Correct a misspelled named argument | maybe-incorrect |
| E0801 | Wrap the C call in `unsafe { … }` | machine-applicable |

## Code ranges

| Range | Area |
|---|---|
| E01xx | Lexical: characters, digraphs, literals |
| E02xx | Syntax |
| E03xx | Names, modules, visibility |
| E04xx | Types |
| E05xx | Patterns and `match` |
| E06xx | Bindings, mutability, closures |
| E07xx | Calls and arguments |
| E08xx | C FFI |
| E09xx | Project files, I/O, dependencies, toolchain |

`mimic explain` prints the full list.
