---
title: Projects and modules
description: Project layout, mim.mim, modules and paths, visibility, and using libraries.
order: 11
---

# Projects and modules

## A project

```sh
mimic create shop            # a program
mimic create money --lib     # a library
mimic init                   # turn the current directory into a project
```

The manifest `mim.mim` is itself written in Mimic syntax:

```mimic
module mim

project {
    name = "shop"
    version = "0.1.0"
    license = "MIT"
    edition = "2026"
    kind = [bin]
}

dependencies {
    money = { path = "../money" }                       // a local project
    vision = { git = "github.com/harcorp/vision.git" }  // installed with `mimic install`
    speech = { git = "github.com/harcorp/speech.git", rev = "v1.2.0" }  // a tag, branch or commit
}

ffi {
    link = []                                           // C libraries (chapter 12)
}
```

Every dependency can be used from every file as `name::item`, with no import needed.

## Modules

Every file starts with `module <name>`. A file can also contain nested modules:

```mimic run
module main

module geometry {
    pub fn area(w: f64, h: f64) -> f64 { w * h }

    pub module units {
        pub fn cm_to_m(x: f64) -> f64 { x / 100.0 }
    }

    fn helper() -> i64 { 1 }        // private: only usable inside geometry
}

fn main() {
    print(geometry::area(2.0, 3.0))
    print(geometry::units::cm_to_m(250.0))
}
```

```output
6.0
2.5
```

## Paths

| Path | Means |
|---|---|
| `geometry::area` | an item of a module in the same project |
| `money::format` | an item of the dependency `money` |
| `self::helper` | an item of the current module |
| `super::helper` | an item of the enclosing module |

The standard library's modules (`fs`, `http`, `json`, …) are found the same way, after your own. A module or program of yours named `json` hides the standard `json` module; the compiler tells you when that happens, and `std::json::encode` still reaches the standard one.

`use a::b::c` and `use a::b as x` are optional shortcuts. There are no wildcard imports, so every name is easy to trace back to where it comes from.

## Visibility

* **Private by default.** Everything is private except `fn main` in `module main`.
* **`pub`.** Mark items `pub` to make them visible outside their module, or outside their package.
* **Sub-modules.** Private items are still visible inside their own module and its sub-modules.

## Libraries

A library project exposes `module lib` from `src/lib.mim`. Users of the library call its items as `libname::item`:

Libraries come from git:

```sh
mimic install github.com/harcorp/money.git          # the default branch → ~/.mimic/libraries/money
mimic install github.com/harcorp/money.git@v1.2.0   # a tag, branch or commit → money@v1.2.0
mimic list
mimic update money                                  # the newest commit of its branch
mimic uninstall money@v1.2.0
```

Inside a project, `mimic install` with no URL installs every dependency that has a `git` URL in `mim.mim` and is missing (and the dependencies of those). It takes the `rev` you asked for; without one, the exact commit recorded in `mod.mim`, so a fresh clone of your project builds with the same library code you used.

## Workspaces

Several projects developed together share a folder whose `mim.mim` lists them:

```mimic
module mim

workspace {
    members = ["app", "money"]
}
```

`mimic check`, `build`, `test`, `fmt` and `install` in that folder run in every member, and all members build into its one `target/`. A member uses another by path: `money = { path = "../money" }`. `mimic run` runs inside one member.

## The module index

`mod.mim` is generated: `check`, `build` and `run` keep it current. It lists every module and function, with signature hashes, plus the exact revision of each dependency. Commit it with your code. `mimic mod list` prints it in readable form.

Next: [calling C](12-calling-c.md).
