#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "${SCRIPT_DIR}/.." && pwd)"
BINARY_PATH="${PROJECT_ROOT}/target/release/clickforge"

cd "${PROJECT_ROOT}"

echo "Building ClickForge release binary..."
cargo build --release --workspace

if [[ ! -x "${BINARY_PATH}" ]]; then
    echo "Release binary missing at ${BINARY_PATH}" >&2
    exit 1
fi

echo "Release build succeeded:"
ls -lh "${BINARY_PATH}"
