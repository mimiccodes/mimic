---
title: Editors
description: Mimic in VS Code, Neovim, Helix, Emacs, Sublime Text and Zed: the language server, highlighting and glyph input.
---

# Editors

`mimic lsp` is Mimic's language server. Any editor that speaks the Language Server Protocol gets, from the same `mimic` you build with:

* **Errors as you type**, with the compiler's fixes as quick fixes ("make `y` mutable").
* **Hover:** a function's signature, comment and effects (`pure`, `concurrent`, …); for a function from C, the header it came from; for a glyph or its word, what it does and how to type it.
* **Go to definition and find references**, also into the standard library.
* **Glyph input:** type `\` and a letter, and completion offers the glyph (`\i` becomes `⍳`, `\k` becomes `⌿`); `mimic glyphs` lists them all.
* **Completion** of names, modules (`fs::`), the primitives' words and keywords.
* **Formatting** (`mimic fmt`), in the style the file mostly uses: words or glyphs.
* **The outline** of a file, and a search for symbols across the project.
* **Names from C** (`c::sqrt`, `cpp::`, an `as` name) as semantic tokens with the modifier `foreign`, so themes can colour them apart.
* **Where code runs**, as labels after the code (inlay hints): `∥ parallel`, `⇶ fused`, `≋ SIMD`, `▣ GPU`, `◷ task`, `→ in order`, with the reason on hover (`mimic why` lists them all).

Syntax highlighting comes from the grammars in [mimic-syntax](https://github.com/mimiccodes/mimic-syntax): TextMate for VS Code, tree-sitter for Neovim, Helix and Zed.

**Debugging** goes through `mimic dap`, the Debug Adapter Protocol: breakpoints on `.mim` lines, stepping, the call stack, and variables shown as Mimic prints them (`[3 1 4]`, `Point { x: 1.5, y: 2.0 }`, `["a": 1]`, `Some(7)`). It runs LLVM's `lldb-dap`, which comes with Xcode on macOS and with the `lldb` package on Linux. A launch request takes `program`: a `.mim` file or a project folder, which it builds with debug information.

## Mimic's own editor

`mimic edit` needs nothing installed: it is part of `mimic`, and knows everything the language server knows.

| Key | Does |
|---|---|
| Ctrl-O | Open a file of the project (type part of its name) |
| Ctrl-N, Ctrl-W | A new program; close the file |
| Ctrl-S, Ctrl-Q | Save; quit (it asks when something is unsaved) |
| Ctrl-G | The glyph palette (type a word or what it does); or type `\i`, `\e`, `\k` … |
| Ctrl-F, F3, Ctrl-H | Find, find again, replace |
| Ctrl-Z, Ctrl-Y | Undo, redo |
| Ctrl-C, Ctrl-X, Ctrl-V, Ctrl-A | Copy, cut, paste, select all (Shift and the arrows select too) |
| Ctrl-L | Go to a line |
| F5, F6 | Run, test (in the background; the output appears in its pane) |
| F7, F8, F9 | Problems, audit, where code runs |
| F1 | The manual for the word or glyph under the cursor |
| Ctrl-B, Ctrl-J | Show or hide the files, the bottom pane |
| Alt-← Alt-→ | The other open files |

Errors appear as you type: a red mark beside the line, the code underlined, the message after it. Where code runs shows after each line as a label (`∥ parallel`, `◷ task`, `→ in order`).

## The REPL

`mimic repl` is the quickest way to try an expression: type `⍳ 10 ▷ +/` (or
`\i 10 ▷ +/`) and see `45`. `let`s and `fn`s you type stay for the lines after,
and `:show` prints them as a program. On macOS, the system checks every new
program the first time it runs, which adds about a second to each line; add
your terminal app under System Settings → Privacy & Security → Developer
Tools to skip the check (it also speeds up `mimic run` and `mimic test`).

## VS Code

The Mimic extension ([mimic-vscode](https://github.com/mimiccodes/mimic-vscode)) starts `mimic lsp` by itself and adds:

* the highlighting, and the Mimic file icon;
* the glyph palette (`Cmd+Alt+G` on macOS, `Ctrl+Alt+G` elsewhere): every glyph with its word, how to type it and what it does;
* **▶ Run** and **Debug** above `fn main`, **▶ Run test** above each test, and the Test Explorer (also for examples in comments); F5 debugs the current project or file;
* the manual beside your code (*Mimic: Open the Manual…*, or `Cmd/Ctrl+Alt+M` for the word under the cursor);
* *Convert File to Glyphs / Words*;
* a check that `mimic` is new enough, with the download when it is not.

Until it is in the VS Code Marketplace and Open VSX, install the `.vsix` from the extension's releases: *Extensions* view → `…` → *Install from VSIX…*.

## Neovim (0.11 or newer)

```lua
vim.filetype.add({ extension = { mim = "mimic" } })
vim.lsp.config("mimic", {
  cmd = { "mimic", "lsp" },
  filetypes = { "mimic" },
  root_markers = { "mim.mim" },
})
vim.lsp.enable("mimic")
```

For highlighting, add the tree-sitter grammar as the mimic-syntax README shows.

## Helix

In `~/.config/helix/languages.toml` (with `debugger` for `:debug-start`):

```toml
[language-server.mimic]
command = "mimic"
args = ["lsp"]

[[language]]
name = "mimic"
scope = "source.mimic"
file-types = ["mim"]
roots = ["mim.mim"]
comment-token = "//"
indent = { tab-width = 4, unit = "    " }
language-servers = ["mimic"]

[language.debugger]
name = "mimic"
transport = "stdio"
command = "mimic"
args = ["dap"]

[[language.debugger.templates]]
name = "project"
request = "launch"
completion = [ { name = "project folder", completion = "directory", default = "." } ]
args = { program = "{0}" }
```

## Emacs (eglot)

```elisp
(define-derived-mode mimic-mode prog-mode "Mimic")
(add-to-list 'auto-mode-alist '("\\.mim\\'" . mimic-mode))
(with-eval-after-load 'eglot
  (add-to-list 'eglot-server-programs '(mimic-mode "mimic" "lsp")))
```

Then `M-x eglot` in a `.mim` file. For debugging, dape can use `mimic dap` as its adapter.

## Sublime Text (with the LSP package)

In the LSP package's settings:

```json
{
  "clients": {
    "mimic": {
      "enabled": true,
      "command": ["mimic", "lsp"],
      "selector": "source.mimic"
    }
  }
}
```

## Zed

Zed adds languages through extensions; until a Mimic extension for Zed is published, use the tree-sitter grammar and `mimic lsp` in a dev extension (Zed's documentation explains how).

## Without a language server

`mimic check --emit-json` and `mimic query` (`mimic man workbench`) answer the same questions as JSON, for scripts and tools of your own.
