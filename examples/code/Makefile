# Makefile example — tests extensionless file handling in MDViewer.

.PHONY: all build test clean

all: build

build:
	@echo "Building project..."
	cargo build --release

test:
	@echo "Running tests..."
	cargo test --all-targets

lint:
	cargo fmt --check
	cargo clippy --all-targets -- -D warnings

clean:
	@echo "Cleaning build artifacts..."
	cargo clean
