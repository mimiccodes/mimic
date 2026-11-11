#!/bin/sh
# Build the LLVM that `mimic` links: LLVM + Clang + LLD 23.1.4, static,
# every backend including the experimental ones, and no optional system
# dependencies (z3, zstd, libxml2, terminfo, libedit, libffi).
#
#   tools/build-llvm.sh                 # uses the sources tools/deps.sh fetched
#   JOBS=4 NICE=19 tools/build-llvm.sh  # gentler on the machine (the default)
#
# Result: $MIMIC_DEPS/llvm-23.1.4 (point LLVM_SYS_231_PREFIX at it).
set -eu

LLVM_VERSION=23.1.4
DEPS=${MIMIC_DEPS:-$HOME/.mimic/deps}
JOBS=${JOBS:-4}
NICE=${NICE:-19}
TARBALL=$DEPS/llvm-project-$LLVM_VERSION.src.tar.xz
SRC=$DEPS/llvm-full-$LLVM_VERSION.src
OUT=$DEPS/llvm-$LLVM_VERSION
BUILD=$DEPS/build-llvm-$LLVM_VERSION

[ -f "$TARBALL" ] || { echo "run tools/deps.sh first (it fetches and verifies $TARBALL)" >&2; exit 1; }
if [ ! -d "$SRC/libc" ]; then
    rm -rf "$SRC" "$SRC.part"
    echo "extracting llvm, clang, lld, cmake, third-party, libunwind, libc"
    mkdir -p "$SRC.part"
    tar -xJf "$TARBALL" -C "$SRC.part" --strip-components=1 \
        "llvm-project-$LLVM_VERSION.src/llvm" "llvm-project-$LLVM_VERSION.src/clang" \
        "llvm-project-$LLVM_VERSION.src/lld" "llvm-project-$LLVM_VERSION.src/cmake" \
        "llvm-project-$LLVM_VERSION.src/third-party" "llvm-project-$LLVM_VERSION.src/libunwind" \
        "llvm-project-$LLVM_VERSION.src/libc"
    mv "$SRC.part" "$SRC"
fi

cmake -S "$SRC/llvm" -B "$BUILD" -G "Unix Makefiles" \
    -DCMAKE_BUILD_TYPE=Release \
    -DCMAKE_INSTALL_PREFIX="$OUT" \
    -DLLVM_ENABLE_PROJECTS="clang;lld" \
    -DLLVM_TARGETS_TO_BUILD=all \
    -DLLVM_EXPERIMENTAL_TARGETS_TO_BUILD="ARC;CSKY;M68k;Xtensa;DirectX" \
    -DLLVM_BUILD_LLVM_DYLIB=OFF -DLLVM_LINK_LLVM_DYLIB=OFF -DBUILD_SHARED_LIBS=OFF \
    -DLLVM_ENABLE_ASSERTIONS=OFF \
    -DLLVM_ENABLE_ZSTD=OFF -DLLVM_ENABLE_ZLIB=OFF -DLLVM_ENABLE_LIBXML2=OFF \
    -DLLVM_ENABLE_Z3_SOLVER=OFF -DLLVM_ENABLE_TERMINFO=OFF -DLLVM_ENABLE_LIBEDIT=OFF \
    -DLLVM_ENABLE_FFI=OFF -DLLVM_ENABLE_LIBPFM=OFF -DLLVM_ENABLE_CURL=OFF -DLLVM_ENABLE_HTTPLIB=OFF \
    -DLLVM_INCLUDE_TESTS=OFF -DLLVM_INCLUDE_BENCHMARKS=OFF -DLLVM_INCLUDE_EXAMPLES=OFF \
    -DLLVM_INCLUDE_DOCS=OFF -DCLANG_INCLUDE_TESTS=OFF -DCLANG_INCLUDE_DOCS=OFF \
    -DCLANG_ENABLE_STATIC_ANALYZER=ON -DCLANG_ENABLE_ARCMT=OFF \
    -DLLVM_ENABLE_BINDINGS=OFF -DLLVM_ENABLE_OCAMLDOC=OFF

nice -n "$NICE" cmake --build "$BUILD" -j "$JOBS"
cmake --install "$BUILD"
echo "llvm: $OUT"
