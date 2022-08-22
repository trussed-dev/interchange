.PHONY= test, clippy docsrs

test:
	cargo t
	RUSTFLAGS='--cfg loom' cargo t loom

clippy:
	cargo clippy --all-targets
	RUSTFLAGS='--cfg loom' cargo clippy --all-targets

docsrs:
	RUSTDOCFLAGS='--cfg docsrs' cargo +nightly doc --all-features
