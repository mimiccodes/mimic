# Mimic showcase — `make help` lists every target.
#
# Uses `mimic` from your PATH, or else the compiler built in this repository
# (`make compiler` builds it). Override with: make run MIMIC=/path/to/mimic

REPO   := ../..
MIMIC  ?= $(shell command -v mimic 2>/dev/null || echo $(REPO)/target/release/mimic)
BIN    := target/release/showcase
TARGET ?= x86_64-linux-gnu

.DEFAULT_GOAL := help
.PHONY: help compiler check run debug build release parallel ir net cross cross-one glyphs mods clean

help: ## show this list
	@echo "Mimic showcase — targets:"
	@grep -E '^[a-z-]+:.*## ' $(MAKEFILE_LIST) | awk 'BEGIN {FS = ":.*## "} {printf "  make %-10s %s\n", $$1, $$2}'
	@echo ""
	@echo "compiler: $(MIMIC)"

compiler: ## build the mimic compiler from this repository (Rust + LLVM 23 + Zig)
	cd $(REPO) && cargo build --release
	@echo "built $(REPO)/target/release/mimic"

check: _need-mimic ## type-check every module without generating code
	$(MIMIC) check

run: _need-mimic ## build in debug mode (leak-checked) and run
	$(MIMIC) run

debug: run ## same as `make run`

build: _need-mimic ## build the fastest binary (release is the default)
	$(MIMIC) build

release: build ## build the fastest binary and run it
	./$(BIN)

parallel: build ## time the release binary on 1 thread vs all cores
	@echo "--- 1 thread"
	@time MIMIC_THREADS=1 ./$(BIN) > /dev/null
	@echo "--- all cores"
	@time ./$(BIN) > /dev/null

ir: _need-mimic ## write the LLVM IR and the generated interaction nets to target/release/
	$(MIMIC) build --emit-ir --emit-net
	@echo "see target/release/showcase.ll and target/release/showcase.mnet"

net: _need-mimic ## run a hand-written interaction net (nets/fib.mnet) with statistics
	$(MIMIC) net run nets/fib.mnet --release --stats

cross: _need-mimic ## cross-compile for every target in build.mim (Linux, musl, Windows)
	$(MIMIC) build --all-targets
	@file target/release/*/showcase target/release/*/showcase.exe 2>/dev/null

cross-one: _need-mimic ## cross-compile for one target: make cross-one TARGET=aarch64-macos
	$(MIMIC) build --target $(TARGET)

glyphs: _need-mimic ## print the glyph ↔ word ↔ ASCII digraph table
	$(MIMIC) glyphs

mods: _need-mimic ## list every module and function (from mod.mim)
	$(MIMIC) mod list --all

clean: ## remove build output
	rm -rf target/debug target/release

_need-mimic:
	@command -v $(MIMIC) >/dev/null 2>&1 || test -x $(MIMIC) || { \
	  echo "error: mimic compiler not found at '$(MIMIC)'"; \
	  echo "       run 'make compiler' (builds it from this repository) or put mimic on your PATH"; \
	  exit 1; }
