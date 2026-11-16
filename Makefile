.PHONY= test, clippy, test-miri, lint

test:
	cargo t
	cargo t --features portable-atomic
	RUSTFLAGS='--cfg loom' cargo t loom

test-miri:
	cargo +nightly miri test

check-fmt:
	cargo fmt -- --check

clippy:
	cargo clippy --all-targets --all-features
	RUSTFLAGS='--cfg loom' cargo clippy --all-targets --all-features

lint:
	cargo clippy --all-targets
	cargo clippy --all-targets --all-features
	cargo doc --all-features --no-deps
	cargo fmt --check
	RUSTFLAGS="$(RUSTFLAGS) --cfg loom" cargo clippy --all-targets --all-features

ci: test test-miri lint
