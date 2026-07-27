.PHONY: check test run serve package smoke clean

check:
	cargo fmt --all -- --check
	cargo clippy --workspace --all-targets -- -D warnings

test:
	cargo test --workspace

run:
	cargo run -p rustcut-cli -- --help

serve:
	cargo run -p rustcut-server -- --bind 127.0.0.1:8787

package:
	./scripts/package.sh

smoke:
	./scripts/smoke-test.sh

clean:
	cargo clean
	rm -rf dist data/smoke
