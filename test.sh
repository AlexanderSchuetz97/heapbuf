#!/bin/bash

set -e
cargo build --features all
cargo build
cargo clean
cargo +nightly miri test --features all
cargo clean
cargo test --features all
cargo test
cargo test --release --features all
cargo test --release