#!/usr/bin/env bash
# Build the frontend, run all tests, and produce a release binary in dist/.
set -euo pipefail
cd "$(dirname "$0")"

version=$(grep -m1 '^version' Cargo.toml | cut -d'"' -f2)
echo "== logby v$version — frontend"
(cd frontend && npm ci --silent && npm run check && npm test -- --run && npm run build)

echo "== backend tests"
cargo test --quiet

echo "== release build"
cargo build --release --locked
mkdir -p dist
tar czf "dist/logby-v$version-$(uname -m)-linux.tar.gz" -C target/release logby -C "$PWD" README.md
(cd dist && sha256sum *.tar.gz > sha256sums.txt)
ls -l dist/
