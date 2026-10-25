.PHONY: check test fmt demo

check:
	cargo check --workspace --all-targets

test:
	cargo test --workspace

fmt:
	cargo fmt --all

demo:
	cargo run -p bulkhead-gateway -- --demo --policy examples/policy.example.json
