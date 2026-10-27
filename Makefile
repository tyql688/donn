CARGO ?= cargo

.PHONY: help run install check clean

help:
	@printf '%s\n' \
		'make run [ARGS="..."]  Run the TUI or a donn subcommand' \
		'make install           Install donn from this checkout' \
		'make check             Run all pre-commit checks (fmt, clippy, release build)' \
		'make clean             Remove build artifacts (target/)'

run:
	$(CARGO) run -p donn-cli -- $(ARGS)

install:
	$(CARGO) install --path crates/donn-cli --locked

check:
	$(CARGO) fmt --all -- --check
	$(CARGO) clippy --workspace --all-targets -- -D warnings
	$(CARGO) build --release --locked

clean:
	$(CARGO) clean
