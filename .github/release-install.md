

## Install

Download the file for your system below and unpack it. Then run the install script inside: it copies `mimic` to `~/.mimic/bin`, puts that folder on your `PATH`, and checks what your system needs.

| System | File | Install | Also needed |
|---|---|---|---|
| **Windows** x86_64 (10, 11) | `mimic-{v}-windows-x86_64.zip` | Double-click `install.cmd` | Nothing: Windows programs are linked with the MinGW-w64 inside `mimic`. |
| **macOS** Apple silicon | `mimic-{v}-macos-arm64.tar.gz` | `./install.sh` | The Xcode Command Line Tools (`xcode-select --install`), for the macOS SDK. |
| **Linux** x86_64 | `mimic-{v}-linux-x86_64.tar.gz` | `./install.sh` | glibc 2.28 or newer (Ubuntu 20.04, Debian 10, RHEL 8 or later) and a C compiler (`cc`) to link native programs. `--target x86_64-linux` needs nothing else. |

The script says if something is missing and how to install it. Open a new terminal afterwards so the new `PATH` applies.

- **macOS:** the script clears the download quarantine. If you copy `mimic` by hand instead, run `xattr -d com.apple.quarantine mimic` once.
- **Windows:** SmartScreen may ask the first time: "More info" → "Run anyway".
- **Uninstall:** `./install.sh --uninstall`, or `install.cmd -Uninstall` on Windows. Your projects are not touched.

## First program

```sh
mimic create hello
cd hello
mimic run
```

`mimic --help` lists every command. `SHA256SUMS` has the checksum of each file.
