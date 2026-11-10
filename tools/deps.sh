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
MUSL_VERSION=1.2.5
MUSL_SHA256=a9a118bbe84d8764da0ea0d28b3ab3fae8477fc7e4085d90102b8596fc7c75e4

sha256() { shasum -a 256 "$1" 2>/dev/null | cut -d' ' -f1 || sha256sum "$1" | cut -d' ' -f1; }
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
    tar -xJf "$TARBALL" -C "$DEPS" \
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
if [ ! -f "$LLD_OUT/lib/liblldELF.a" ]; then
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
