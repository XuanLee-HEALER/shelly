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
    cargo test --test test_brain --test test_executor

# Run integration tests with output
test-integration-verbose:
    cargo test --test test_brain --test test_executor -- --nocapture

# Run executor tests only
test-executor:
    cargo test --test test_executor

# Run executor tests with output
test-executor-verbose:
    cargo test --test test_executor -- --nocapture

# Run brain tests only
test-brain:
    cargo test --test test_brain

# Run brain tests with output
test-brain-verbose:
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

# Start shelly daemon in background (requires .env with INFERENCE_* vars)
daemon-start:
    #!/usr/bin/env bash
    set -e
    CARGO_PATH=$(which cargo)
    RUSTUP_HOME="$HOME/.rustup" CARGO_HOME="$HOME/.cargo" sudo -v
    # Then run in background using a subshell
    sudo RUSTUP_HOME="$HOME/.rustup" CARGO_HOME="$HOME/.cargo" -- bash -c "$CARGO_PATH run --bin shelly &" &
    sleep 2
    PID=$(pgrep -f "shelly" | head -1)
    if [ -z "$PID" ]; then
        echo "Failed to start shelly"
        exit 1
    fi
    echo "Shelly started with PID: $PID"
    echo $PID > .shelly.pid
    echo "Shelly daemon running on port 9700"

# Stop shelly daemon
daemon-stop:
    #!/usr/bin/env bash
    if [ -f .shelly.pid ]; then
        PID=$(cat .shelly.pid)
        kill $PID 2>/dev/null || true
        rm .shelly.pid
        echo "Shelly stopped"
    else
        echo "Shelly not running (no PID file)"
    fi

# Run shelly in foreground (requires sudo for root privileges)
run-daemon:
    #!/usr/bin/env bash
    set -e
    CARGO_PATH=$(which cargo)
    RUSTUP_HOME="$HOME/.rustup" CARGO_HOME="$HOME/.cargo" sudo -v
    sudo RUSTUP_HOME="$HOME/.rustup" CARGO_HOME="$HOME/.cargo" $CARGO_PATH run --bin shelly

# Run CLI and connect to daemon
cli:
    #!/usr/bin/env bash
    set -e
    cargo run --bin shelly-cli

# Start daemon and run CLI interactively
dev: daemon-start
    cargo run --bin shelly-cli
    just daemon-stop

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
