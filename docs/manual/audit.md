---
title: Audit
description: mimic audit: quality and security rules, the gate for CI, baselines, SARIF and trends.
---

# Audit

`mimic audit` reads a program and reports what is likely to go wrong: code nobody uses, failures nobody checks, commands built from user input, secrets in the source. Each finding has a rule, a severity, its place in the code, and how to fix it.

```sh
mimic audit                       # the project in this folder
mimic audit --in tool.mim         # one file
mimic audit --format sarif > audit.sarif
```

## The rules

| Rule | Name | Severity | Finds |
|---|---|---|---|
| Q01 | unused-function | warning | A private function nothing calls |
| Q02 | unused-variable | warning | A `let` whose value is never used |
| Q03 | unchecked-result | warning | A `Result` dropped, so a failure goes unnoticed |
| Q04 | long-function | note | A function longer than 60 lines |
| Q05 | complex-function | note | More than 15 branches in one function |
| Q06 | duplicated-code | note | Two functions with the same body |
| Q07 | wider-effects | note | C functions on numbers declared with effects: maybe `pure` |
| Q08 | trusted-pointer | warning | A `trusted` C function that takes a pointer |
| S01 | shell-injection | error | `os::shell` with a command built from data |
| S02 | path-from-request | error | A file opened with a path from an HTTP request |
| S03 | secret-in-source | error | Passwords, tokens and private keys in the code |
| S04 | certificate-trust | warning | `tls::connect_trusting` or `tls::self_signed` outside tests |
| S05 | unsafe-block | note | An `unsafe` block |
| S06 | trusted-header | warning | A whole C header trusted as memory safe |
| S07 | branch-dependency | warning | A dependency that follows a branch instead of a tag or commit |

`mimic man S01` (or `mimic man shell-injection`) shows a rule's page.

## In CI

`mimic audit` exits with 1 when it finds something at `--fail-on` or worse: `error` by default (the security rules S01 to S03), or `warning`, `note`, `never`.

```sh
mimic audit                        # fails on errors
mimic audit --fail-on warning      # stricter
```

**A baseline** lets an existing project adopt the gate without fixing everything first: `mimic audit --update-baseline` records today's findings in `.mimic-audit.json`; `mimic audit --baseline .mimic-audit.json` then fails only on findings that are new. Findings are recognised by their rule, file and code, so moving code around does not make them new.

**SARIF** (`--format sarif`) is the format code-scanning services read, such as GitHub's: upload it from CI to see findings on pull requests.

**Trends:** every run adds its counts to `target/audit/`, and the text report says how the total changed since the last run.

## What the language already guarantees

Some things need no rule, because Mimic rules them out: reading freed memory, data races between parallel code, null pointers, unchecked array bounds. They hold everywhere except inside `unsafe` blocks and in C code, which is why S05, S06 and Q08 point those out.
