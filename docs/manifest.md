# Project layout and config files

```
myproject/
├── mim.mim        project manifest (you edit)
├── build.mim      build settings (you edit)
├── mod.mim        module index + dependency lock (generated; commit it)
├── src/
│   ├── main.mim   entry point of a binary  (module main, fn main)
│   ├── lib.mim    entry point of a library (module lib)
│   ├── test.mim   tests (optional; not compiled into builds)
│   └── *.mim      any other modules, any depth
└── target/
    ├── debug/     mimic build / run
    └── release/   … --release   (cross builds go in target/<profile>/<triple>/)
```

All three config files use the same declarative Mimic syntax. Each one starts with a `module` header. A block is written as `key { … }`, and a value as `key = value`. Lists use `[ … ]`, inline tables use `{ k = v }`, and comments use `//`.

## mim.mim

```mimic
module mim

project {
    name = "ai"                      // required; a valid identifier
    version = "0.1.0"
    authors = ["Ada <ada@example.com>"]
    description = "speech + vision toolkit"
    readme = "README.md"
    license = "MIT"
    repository = "https://github.com/harcorp/ai"
    edition = "2026"
    kind = [bin, lib]                // bin, lib, or both (default: bin)
}

dependencies {
    speech = {}                                       // installed in ~/.mimic/libraries
    vision = { git = "github.com/harcorp/vision.git" } // must be `mimic install`ed (auto-fetch: planned)
    util   = { path = "../util" }                     // local project
    tensor = "1.2"                                    // version string (registry: planned)
}

ffi {
    link = ["m", "portaudio"]   // -l flags
    search = ["vendor/lib"]     // -L flags, relative to the project root
}
```

Every declared dependency is in scope in every file of the project as `name::…`, so files do not need imports. `use name::item` is also available.

### Planned fields

| Field | Purpose |
|---|---|
| `bin { name, path }` | Multiple binaries. |
| `features { simd = [], gpu = ["dep/gpu"] }` | Conditional compilation. |
| `ffi.headers` | Generate bindings with `mimic ffi bind`. |
| `dev-dependencies` | Test-only dependencies. |
| `workspace { members = [...] }` | Multi-project workspaces. |
| `targets` per project | Platform-specific dependencies. |

## build.mim

```mimic
module build

profile debug   { opt = 0, debug = true,  leak_check = true }
profile release { opt = 3, debug = false, leak_check = false }

targets = ["x86_64-linux-musl", "aarch64-linux", "x86_64-apple-macosx"]   // mimic build --all-targets
```

* **`opt`** is the LLVM optimisation level, 0 to 3.
* **`leak_check`** makes the program verify at exit that every array was freed.
* **`targets`** are LLVM target triples (vendor optional). Every architecture with an LLVM backend can be used; see [targets.md](targets.md).

## mod.mim (generated)

`mod.mim` is managed by mimic. `check`, `build` and `run` rewrite it whenever the module tree changes. `mimic mod update` regenerates it explicitly. The file lists:

* **`modules`:** every module in the project *and its dependencies*, with its file, line and visibility.
* **`functions`:** every function, with its kind (`fn`/`extern`), visibility, location, signature and a stable **signature hash**. The incremental compiler will use the hashes to skip re-checking dependents when only a function body changed.
* **`lock`:** each dependency's source and resolved git revision.

The related CLI commands are `mimic mod init | update | list [--all] | delete <module> [--yes]`.
