# Shelly - Development Task Runner

# Default task: show help
default:
    @just --list

# Build the project
build:
    cargo build

# Build release version
build-release:
    cargo build --release

# Run the project
run:
    cargo run

# Run tests
test:
    cargo test

# Run tests with output
test-verbose:
    cargo test -- --nocapture

# Run integration tests only (requires .env with INFERENCE_* vars)
test-integration:
    cargo test --test test_brain

# Run integration tests with output
test-integration-verbose:
    cargo test --test test_brain -- --nocapture

# Run unit tests only
test-unit:
    cargo test --lib

# Run clippy lints
lint:
    cargo clippy -- -D warnings

# Format code
fmt:
    cargo fmt

# Check everything (fmt + lint + test)
check: fmt lint test
    @echo "All checks passed!"

# Run clippy with fix
fix:
    cargo clippy --fix --allow-dirty

# Clean build artifacts
clean:
    cargo clean

# Run doc generation
doc:
    cargo doc --no-deps

# Open documentation locally
doc-open: doc
    open target/doc/shelly/index.html

# Watch mode for development
watch:
    cargo watch -x check -x test

# Build and run release
release: build-release
    ./target/release/shelly

# Show project dependencies
deps:
    cargo tree

# Show project size
size:
    cargo bloat --release

# Generate shell completions
completions:
    cargo run --release -- completions bash > completions.sh

# Show required environment variables
env-info:
    @echo "=== Required Environment Variables ==="
    @echo "INFERENCE_ENDPOINT  - API endpoint URL (e.g., https://api.minimax.chat/v1)"
    @echo "INFERENCE_API_KEY  - API key for authentication"
    @echo "INFERENCE_MODEL    - Model identifier (e.g., MiniMax-M2.1)"
    @echo ""
    @echo "=== Optional Environment Variables ==="
    @echo "INFERENCE_MAX_RETRIES    - Max retry attempts (default: 3)"
    @echo "INFERENCE_RETRY_DELAY_MS - Base retry delay in ms (default: 1000)"
    @echo "INFERENCE_TIMEOUT_SECS   - Request timeout in seconds (default: 120)"
    @echo "INFERENCE_MAX_TOKENS    - Default max output tokens (default: 4096)"
    @echo ""
    @echo "Copy .env.example to .env and fill in your credentials"
