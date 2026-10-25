.PHONY: test bench

test:
	cargo test --workspace

bench:
	cargo bench --workspace
