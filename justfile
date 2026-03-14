set shell := ["bash", "--noprofile", "--norc", "-o", "errexit", "-o", "errtrace", "-o", "nounset", "-o", "pipefail", "-c"]

# Default recipe shows available commands
default:
    just --list

# Build the project
build:
    cargo build --quiet

# Check code compiles
check:
    cargo check --quiet

# Full CI validation pipeline
ci: fmt-check lint test coverage-zero deny

# Clean build artifacts
clean:
    cargo clean --quiet

# Run cargo deny security audit
deny:
    cargo --config .cargo/deny.toml --quiet deny check >/dev/null

# Format code (fix in place)
fmt *args:
    cargo fmt-all --quiet {{ args }}

# Check formatting without changes
fmt-check:
    just fmt --check

# Lint with auto-fix
fix: fmt
    just lint-rust --allow-dirty --allow-staged --fix

# Lint (check only)
lint: lint-rust

# Lint Rust (check only, pass args for --fix)
lint-rust *args:
    cargo clippy-all --quiet {{ args }}

# Run tests
test:
    cargo --quiet test-all
    cargo --quiet test-doc

# Assert working tree is clean
assert-clean:
    #!/usr/bin/env -S bash --noprofile --norc -o errexit -o errtrace -o nounset -o pipefail

    if [[ -n "$(git status --porcelain=v1)" ]]; then
      echo 'Error: Working tree is not clean.' >&2
      echo 'Please commit or stash changes first.' >&2
      exit 1
    fi

# Run coverage (quiet on success)
coverage:
    CARGO_TARGET_DIR=target/coverage-target cargo coverage --json --summary-only --output-path target/coverage.summary.json

# Run coverage gates using the repo's configured llvm-cov thresholds.
coverage-zero:
    CARGO_TARGET_DIR=target/coverage-target cargo coverage --json --summary-only --output-path target/coverage.summary.json

# Install into ~/.local/bin (requires ~/.local/bin on PATH)
install-local:
    mkdir -p "${HOME}/.local/bin"
    cargo install --force --path . --root ~/.local
