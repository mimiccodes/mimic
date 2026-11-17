# Security

## Reporting a vulnerability

Please report security problems privately, not in a public issue: email **[mimic@mimic.codes](mailto:mimic@mimic.codes)** with what you found, how to reproduce it, and the Mimic version (`mimic --version`).

You will get an answer within a few days. Once a fix is released, the report is credited in the changelog unless you prefer otherwise.

## What is covered

* the `mimic` toolchain and its runtime, including memory safety of compiled programs: a Mimic program without `unsafe` that reads or writes memory it should not is a security bug
* the standard library (`http::`, `ws::`, `tcp::`, `udp::`, `fs::`, `os::`, TLS)
* the install scripts (`tools/get`, served at mimic.codes) and release downloads
* the VS Code extension ([mimic-vscode](https://github.com/mimiccodes/mimic-vscode)) and `mimic lsp`, `mimic mcp`, `mimic dap`

## Supported versions

Fixes go into the latest release. Update with `mimic upgrade`.
