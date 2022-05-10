_default:
	@just --list


# Runs clippy on the source
check:
	cargo clippy --locked -- -D warnings

# Run unit tests
test:
	cargo test -- --nocapture
