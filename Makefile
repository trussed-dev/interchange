.PHONY= test, clippy

test:
	cargo t
	RUSTFLAGS='--cfg loom' cargo t loom

clippy:
	cargo clippy --all-targets --all-features
	RUSTFLAGS='--cfg loom' cargo clippy --all-targets --all-features
