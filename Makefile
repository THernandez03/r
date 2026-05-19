.PHONY: check fmt lint test build

check: fmt lint test build

fmt:
	cargo fmt

lint:
	cargo clippy --all-targets -- -W clippy::all -W clippy::pedantic -W clippy::nursery

test:
	cargo test -- --test-threads=1

build:
	cargo build --release
