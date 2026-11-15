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

Syntax highlighting comes from the grammars in [mimic-syntax](https://github.com/mimiccodes/mimic-syntax): TextMate for VS Code, tree-sitter for Neovim, Helix and Zed.

## VS Code

The Mimic extension ([mimic-vscode](https://github.com/mimiccodes/mimic-vscode)) starts `mimic lsp` by itself and adds:

* the highlighting, and the Mimic file icon;
* the glyph palette (`Cmd+Alt+G` on macOS, `Ctrl+Alt+G` elsewhere): every glyph with its word, how to type it and what it does;
* **▶ Run** above `fn main` and **▶ Run test** above each test, and the Test Explorer (also for examples in comments);
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

In `~/.config/helix/languages.toml`:

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
```

## Emacs (eglot)

```elisp
(define-derived-mode mimic-mode prog-mode "Mimic")
(add-to-list 'auto-mode-alist '("\\.mim\\'" . mimic-mode))
(with-eval-after-load 'eglot
  (add-to-list 'eglot-server-programs '(mimic-mode "mimic" "lsp")))
```

Then `M-x eglot` in a `.mim` file.

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
