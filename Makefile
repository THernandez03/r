.PHONY: check fmt lint test build

check: fmt lint test build

fmt:
	cargo fmt

lint:
	cargo clippy --all-targets --all-features -- -D warnings -D clippy::all -D clippy::pedantic -D clippy::nursery -D clippy::cargo -A clippy::multiple_crate_versions

test:
	cargo test -- --test-threads=1

build:
	cargo build --release
