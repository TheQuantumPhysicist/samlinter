#!/usr/bin/env bash
# Gate before declaring work done: formatting, lints, then samlinter on its own source.
set -euo pipefail
cd "$(dirname "$0")"

cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo run --quiet -- --manifest-path Cargo.toml
