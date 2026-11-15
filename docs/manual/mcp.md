---
title: MCP for AI agents
description: Give an AI agent Mimic's tools with `mimic mcp`: what it can do, what it may touch, and the set-up for each app.
---

# MCP for AI agents

`mimic mcp` gives AI agents Mimic's own tools over the Model Context Protocol, the standard way agent apps and editors connect to tools. An agent then learns Mimic from the source of truth (the manual and the compiler) instead of guessing from what it has seen elsewhere, and checks its code the moment it writes it.

## What the agent gets

**Tools**

| Tool | What it does |
|---|---|
| `check` | Every error of the project (or a file), with its code, location and the compiler's exact fixes, as JSON |
| `build`, `run`, `test` | Build; run with arguments and input (stopped after a timeout); run the tests |
| `format` | `mimic fmt`, in words or glyphs |
| `explain` | What an error code means and how to fix it |
| `manual`, `manual_search` | Any page of the manual: chapters, guides, every standard function, module, glyph and error code |
| `symbols`, `symbol`, `effects` | Find functions and types; one symbol's signature, comment, effects and every use; which functions are pure |
| `docs_status`, `docs_write`, `docs_build` | What has no documentation yet; write a guide (`docs/<name>.md`) and run its examples at once; build the site |
| `read_file`, `write_file`, `list_files` | The project's files |
| `targets` | The platforms Mimic builds for |
| `install` | Libraries from git (only with `--allow-install`) |

**Resources:** the guide for agents (`mimic://manual/agents`), the glyph table (`mimic://glyphs`), the standard library, the syntax, and every page of the manual (`mimic://manual/{page}`).

**Prompts:** `write_mimic` (a task, written idiomatically and checked), `port_to_mimic` (code from C, Python, JavaScript or another language), `fix_errors` (fix everything `check` reports), `document_project` (comments on public items, and guides with examples that run as tests).

## What the agent may touch

Inside the project, everything: reading and writing its files, checking, formatting, building, running and testing. Outside it, nothing until you allow it:

| Flag | Allows |
|---|---|
| `--allow-outside` | Reading and writing files outside the project |
| `--allow-install` | Installing libraries into `~/.mimic` |
| `--allow-network` | Running programs that use the network (`http`, `tcp`, `udp`, `ws`, `tls`) or run other programs (`os::run`, `os::shell`) |
| `--read-only` | Narrows instead: only reading, checking and looking things up |
| `--no-run` | Narrows instead: no running programs or tests |

The network check looks at what the program calls; C code it calls through `c::` is not checked, so keep `--allow-network` off only as a guard against accidents, not against hostile code. Most agent apps also ask you before each tool call, unless you tell them not to. `--project DIR` serves another project than the one in the current folder.

## Set-up

Every app takes the same three things: the command `mimic`, the argument `mcp`, and the flags you want. Start it in your project's folder, or add `--project /path/to/project`.

**The common configuration** (most apps read an `mcpServers` object like this, in their own settings file):

```json
{
  "mcpServers": {
    "mimic": { "command": "mimic", "args": ["mcp"] }
  }
}
```

**VS Code** (GitHub Copilot's agent mode), in `.vscode/mcp.json` of the project:

```json
{
  "servers": {
    "mimic": { "type": "stdio", "command": "mimic", "args": ["mcp"] }
  }
}
```

**Cursor**, in `.cursor/mcp.json` of the project (or `~/.cursor/mcp.json` for every project): the common configuration above.

**Windsurf**, in `~/.codeium/windsurf/mcp_config.json`: the common configuration above.

**Zed**, in its `settings.json`:

```json
{
  "context_servers": {
    "mimic": { "command": { "path": "mimic", "args": ["mcp"] } }
  }
}
```

**Codex CLI**, in `~/.codex/config.toml`:

```toml
[mcp_servers.mimic]
command = "mimic"
args = ["mcp"]
```

**Any other app or agent framework:** a stdio MCP server, started as `mimic mcp`.

To check the server by hand, the MCP Inspector lists its tools and lets you call them: `npx @modelcontextprotocol/inspector mimic mcp`.

## Tips

* Ask the agent to read `mimic://manual/agents` first; the server's instructions tell it to as well.
* Agents write better Mimic with the manual at hand than from memory: "look it up with the manual tool" is a good thing to say.
* The guide's table of mistakes (`[3 -1 4]`, `print(a, b)`, `let sum = …`) is where ported code usually goes wrong.
