CARGO ?= cargo

.PHONY: help run install test check build

help:
	@printf '%s\n' \
		'make run [ARGS="..."]  Run the TUI or a donn subcommand' \
		'make install           Install donn from this checkout' \
		'make test              Run the workspace tests' \
		'make check             Run all pre-commit checks (fmt, tests, clippy, release build)' \
		'make build             Build the optimized binary'

run:
	$(CARGO) run -p donn-cli -- $(ARGS)

install:
	$(CARGO) install --path crates/donn-cli --locked

test:
	$(CARGO) test --workspace

check:
	$(CARGO) fmt --all -- --check
	$(CARGO) test --workspace
	$(CARGO) clippy --workspace --all-targets -- -D warnings
	$(CARGO) build --release --locked

build:
	$(CARGO) build --release --locked
