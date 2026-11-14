#!/bin/sh
# Fetch and build the native dependencies the `mimic` binary embeds.
#
#   tools/deps.sh            # source + static LLD, built against the LLVM in $LLVM_PREFIX
#
# Everything goes to $MIMIC_DEPS (default ~/.mimic/deps); nothing is installed
# system-wide. The LLVM version must match the LLVM that inkwell links
# (.cargo/config.toml), because LLD and Clang are linked into the same binary.
set -eu

LLVM_VERSION=23.1.4
LLVM_PREFIX=${LLVM_PREFIX:-/opt/homebrew/opt/llvm}
DEPS=${MIMIC_DEPS:-$HOME/.mimic/deps}
JOBS=${JOBS:-$(sysctl -n hw.ncpu 2>/dev/null || nproc)}
SRC=$DEPS/llvm-project-$LLVM_VERSION.src
TARBALL=$DEPS/llvm-project-$LLVM_VERSION.src.tar.xz
URL=https://github.com/llvm/llvm-project/releases/download/llvmorg-$LLVM_VERSION/llvm-project-$LLVM_VERSION.src.tar.xz
LLVM_SHA256=819ecf7351f1a66f76b2d701d8942faaf5faeb324cb90c9d5b090de142434bb2
WASI_SDK=34
WASI_SHA256=9d813544eeebe38b7b8f2244ed591de46b6db812c6dd1a257ff9f0d2a905a2be
LLVM_MINGW=20261020
LLVM_MINGW_SHA256=55064b60b991a1e1e8090b8c2f84e82b22cfe5903d12c159d65064cf5e8ae335
MBEDTLS=3.6.7
MBEDTLS_SHA256=a7e8bcbec0e6f761b4af24f25677626b35f762f68eef79c08677a363212d11f6
MUSL_VERSION=1.2.5
MUSL_SHA256=a9a118bbe84d8764da0ea0d28b3ab3fae8477fc7e4085d90102b8596fc7c75e4

sha256() { # (a pipe hides a missing tool's failure: pick one that runs)
    if command -v sha256sum >/dev/null 2>&1; then sha256sum "$1" | cut -d' ' -f1; else shasum -a 256 "$1" | cut -d' ' -f1; fi
}
fetch() { # url file sha256
    if [ ! -f "$2" ]; then
        echo "downloading $1"
        curl -fL --retry 3 -o "$2.part" "$1"
        mv "$2.part" "$2"
    fi
    got=$(sha256 "$2")
    [ "$got" = "$3" ] || { echo "checksum mismatch for $2: $got (expected $3)" >&2; rm -f "$2"; exit 1; }
}

have=$("$LLVM_PREFIX/bin/llvm-config" --version)
[ "$have" = "$LLVM_VERSION" ] || { echo "LLVM at $LLVM_PREFIX is $have, need $LLVM_VERSION" >&2; exit 1; }

mkdir -p "$DEPS"
fetch "$URL" "$TARBALL" "$LLVM_SHA256"
if [ ! -d "$SRC/clang/tools/driver" ]; then
    echo "extracting (lld, clang driver, cmake, libunwind, compiler-rt)"
    # (without llvm/utils/mlgo-utils: symbolic links Windows cannot always make)
    tar -xJf "$TARBALL" -C "$DEPS" --exclude='*/mlgo-utils' \
        "llvm-project-$LLVM_VERSION.src/lld" \
        "llvm-project-$LLVM_VERSION.src/clang/tools/driver" \
        "llvm-project-$LLVM_VERSION.src/cmake" \
        "llvm-project-$LLVM_VERSION.src/libunwind" \
        "llvm-project-$LLVM_VERSION.src/compiler-rt" \
        "llvm-project-$LLVM_VERSION.src/llvm/utils" \
        "llvm-project-$LLVM_VERSION.src/llvm/cmake" \
        "llvm-project-$LLVM_VERSION.src/third-party"
fi

# ---- LLD (static libraries, standalone build against the installed LLVM) ----
LLD_OUT=$DEPS/lld-$LLVM_VERSION
# (an LLVM built by tools/build-llvm.sh includes LLD already)
if [ -f "$LLVM_PREFIX/lib/liblldELF.a" ] || [ -f "$LLVM_PREFIX/lib/lldELF.lib" ]; then
    LLD_OUT=$LLVM_PREFIX
elif [ ! -f "$LLD_OUT/lib/liblldELF.a" ]; then
    B=$DEPS/build-lld
    rm -rf "$B"
    cmake -S "$SRC/lld" -B "$B" -G "Unix Makefiles" \
        -DCMAKE_BUILD_TYPE=Release \
        -DCMAKE_INSTALL_PREFIX="$LLD_OUT" \
        -DLLVM_DIR="$LLVM_PREFIX/lib/cmake/llvm" \
        -DLLVM_INCLUDE_TESTS=OFF \
        -DLLD_BUILD_TOOLS=OFF \
        -DBUILD_SHARED_LIBS=OFF \
        -DLLVM_LINK_LLVM_DYLIB=OFF
    cmake --build "$B" -j "$JOBS"
    cmake --install "$B"
    # headers the shim needs are installed; the build tree is no longer needed
    rm -rf "$B"
fi
echo "lld: $LLD_OUT"

# ---- musl (C library for the *-linux-musl* targets) ----------------------------
fetch "https://musl.libc.org/releases/musl-$MUSL_VERSION.tar.gz" "$DEPS/musl-$MUSL_VERSION.tar.gz" "$MUSL_SHA256"
echo "musl: $DEPS/musl-$MUSL_VERSION.tar.gz"

# ---- wasi-libc (the C library of the wasm32-wasi targets), from wasi-sdk ------
# The sysroot is WebAssembly bytecode, the same on every host; only the
# wasm32-wasip1 (and -threads) parts are kept and bundled.
WASI_TGZ=$DEPS/wasi-sysroot-$WASI_SDK.0.tar.gz
fetch "https://github.com/WebAssembly/wasi-sdk/releases/download/wasi-sdk-$WASI_SDK/wasi-sysroot-$WASI_SDK.0.tar.gz" "$WASI_TGZ" "$WASI_SHA256"
WASI_OUT=$DEPS/wasi-sysroot-$WASI_SDK
if [ ! -d "$WASI_OUT/lib/wasm32-wasip1" ]; then
    rm -rf "$WASI_OUT" "$DEPS/wasi-extract"
    mkdir -p "$DEPS/wasi-extract"
    tar -xzf "$WASI_TGZ" -C "$DEPS/wasi-extract"
    top=$(ls "$DEPS/wasi-extract")
    mkdir -p "$WASI_OUT/include" "$WASI_OUT/lib"
    for t in wasm32-wasip1 wasm32-wasip1-threads; do
        cp -R "$DEPS/wasi-extract/$top/include/$t" "$WASI_OUT/include/$t"
        cp -R "$DEPS/wasi-extract/$top/lib/$t" "$WASI_OUT/lib/$t"
    done
    # C++ headers and libraries stay for later (libc++)
    rm -rf "$WASI_OUT"/include/*/c++ "$WASI_OUT"/include/*/eh "$WASI_OUT"/include/*/noeh \
        "$WASI_OUT"/lib/*/libc++* "$WASI_OUT"/lib/*/eh "$WASI_OUT"/lib/*/noeh "$WASI_OUT"/lib/*/llvm-lto
    # static linking only
    rm -f "$WASI_OUT"/lib/*/*.so
    rm -rf "$DEPS/wasi-extract"
fi
echo "wasi: $WASI_OUT"

# ---- mingw-w64 (the C library of the *-windows-gnu targets), from llvm-mingw --
# Only the Windows side is kept: the C headers, and for x86_64 and aarch64
# the CRT objects, import libraries (UCRT), static winpthreads and
# compiler-rt's builtins. (The tarball's host tools are not used.)
MINGW_TXZ=$DEPS/llvm-mingw-$LLVM_MINGW-ucrt-ubuntu-22.04-x86_64.tar.xz
fetch "https://github.com/mstorsjo/llvm-mingw/releases/download/$LLVM_MINGW/llvm-mingw-$LLVM_MINGW-ucrt-ubuntu-22.04-x86_64.tar.xz" "$MINGW_TXZ" "$LLVM_MINGW_SHA256"
MINGW_OUT=$DEPS/mingw-sysroot-$LLVM_MINGW
if [ ! -f "$MINGW_OUT/lib/x86_64-w64-mingw32/crt2.o" ]; then
    rm -rf "$MINGW_OUT" "$DEPS/mingw-extract"
    mkdir -p "$DEPS/mingw-extract" "$MINGW_OUT/lib"
    # (not its host tools, whose symbolic links Windows cannot always make)
    tar -xJf "$MINGW_TXZ" -C "$DEPS/mingw-extract" --exclude='*/bin/*' --exclude='*/share/*' --exclude='*/python/*' \
        --exclude='*/lib/lib*.so*' --exclude='*/x86_64-w64-mingw32/include' --exclude='*/aarch64-w64-mingw32/include' \
        --exclude='*/i686-w64-mingw32/include' --exclude='*/armv7-w64-mingw32/include'
    top=$DEPS/mingw-extract/$(ls "$DEPS/mingw-extract")
    # C headers (no C++, IDL sources or driver kit)
    cp -R "$top/generic-w64-mingw32/include" "$MINGW_OUT/include"
    rm -rf "$MINGW_OUT/include/c++" "$MINGW_OUT/include/ddk"
    find "$MINGW_OUT/include" -name '*.idl' -exec rm -f {} +
    for a in x86_64 aarch64; do
        cp -R "$top/$a-w64-mingw32/lib" "$MINGW_OUT/lib/$a-w64-mingw32"
        # static linking only; no C++ runtime
        rm -f "$MINGW_OUT"/lib/$a-w64-mingw32/libc++* "$MINGW_OUT"/lib/$a-w64-mingw32/libunwind* \
            "$MINGW_OUT"/lib/$a-w64-mingw32/libomp* "$MINGW_OUT"/lib/$a-w64-mingw32/*.dll.a \
            "$MINGW_OUT"/lib/$a-w64-mingw32/*.la
        rm -rf "$MINGW_OUT"/lib/$a-w64-mingw32/cmake "$MINGW_OUT"/lib/$a-w64-mingw32/pkgconfig
    done
    cp "$top"/lib/clang/*/lib/windows/libclang_rt.builtins-x86_64.a \
        "$top"/lib/clang/*/lib/windows/libclang_rt.builtins-aarch64.a "$MINGW_OUT/lib/"
    cp "$top/LICENSE.TXT" "$MINGW_OUT/LICENSE.TXT"
    rm -rf "$DEPS/mingw-extract"
fi
echo "mingw-w64: $MINGW_OUT"

# ---- Mbed TLS (TLS inside static Linux programs, which cannot load OpenSSL) --
# Only the library sources, headers and licence are kept and bundled.
MBEDTLS_TBZ=$DEPS/mbedtls-$MBEDTLS.tar.bz2
fetch "https://github.com/Mbed-TLS/mbedtls/releases/download/mbedtls-$MBEDTLS/mbedtls-$MBEDTLS.tar.bz2" "$MBEDTLS_TBZ" "$MBEDTLS_SHA256"
MBEDTLS_OUT=$DEPS/mbedtls-$MBEDTLS
if [ ! -f "$MBEDTLS_OUT/library/ssl_tls.c" ]; then
    rm -rf "$MBEDTLS_OUT" "$DEPS/mbedtls-extract"
    mkdir -p "$DEPS/mbedtls-extract" "$MBEDTLS_OUT"
    tar -xjf "$MBEDTLS_TBZ" -C "$DEPS/mbedtls-extract" "mbedtls-$MBEDTLS/library" "mbedtls-$MBEDTLS/include" "mbedtls-$MBEDTLS/LICENSE"
    cp -R "$DEPS/mbedtls-extract/mbedtls-$MBEDTLS/library" "$DEPS/mbedtls-extract/mbedtls-$MBEDTLS/include" \
        "$DEPS/mbedtls-extract/mbedtls-$MBEDTLS/LICENSE" "$MBEDTLS_OUT/"
    # sources only (the release has build files and generators in there too)
    find "$MBEDTLS_OUT/library" -type f ! -name '*.c' ! -name '*.h' -exec rm -f {} +
    rm -rf "$DEPS/mbedtls-extract"
fi
echo "mbedtls: $MBEDTLS_OUT"

# the LLVM this repository builds against (.cargo/config.toml): keep a static
# build from tools/build-llvm.sh if one is linked, else use $LLVM_PREFIX
REPO=$(cd "$(dirname "$0")/.." && pwd)
if [ ! -e "$REPO/.llvm" ] && [ -z "${LLVM_SYS_231_PREFIX:-}" ]; then
    ln -sfn "$LLVM_PREFIX" "$REPO/.llvm"
    echo "linked .llvm -> $LLVM_PREFIX"
fi
