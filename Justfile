set dotenv-load := true

default:
    @just --list

fmt:
    cargo fmt --all --check

lint:
    cargo clippy --all-targets --all-features -- -D warnings

test:
    cargo test --all-targets --all-features

ai-gates:
    scripts/ai-dev-gates.sh

install-hooks:
    scripts/install-git-hooks.sh

architecture:
    @test -f docs/foia-rust-design.md
    @test -f src/main.rs
    @test -f src/lib.rs
    @test -f src/mcp/tools.rs
    @test -f src/mcp/output.rs
    @test -f src/config.rs
    @test -f src/errors.rs
    @test -f src/model.rs
    @grep -R "FOIA_SEARCH_DATA_DIR" -n src >/dev/null
    @grep -R "FOIA_SEARCH_NARA_API_KEY" -n src >/dev/null
