#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "${SCRIPT_DIR}/.." && pwd)"
MAX_FILE_LINES=800

cd "${PROJECT_ROOT}"

echo "[1/7] Checking Cargo.toml release overflow-checks..."
python3 - <<'PY'
from pathlib import Path
import re, sys

manifest = Path("Cargo.toml").read_text(encoding="utf-8")
if not re.search(r"(?ms)^\[profile\.release\].*?^\s*overflow-checks\s*=\s*true\b", manifest):
    print("FAIL: [profile.release] overflow-checks = true not found in Cargo.toml", file=sys.stderr)
    sys.exit(1)
print("OK: [profile.release] overflow-checks = true verified.")
PY

echo "[2/7] Checking Rust source file length ceiling (<= ${MAX_FILE_LINES} lines)..."
python3 - "${MAX_FILE_LINES}" <<'PY'
from pathlib import Path
import sys

max_lines = int(sys.argv[1])
roots = [Path("crates/clickforge_sdk/src"), Path("src"), Path("tests")]
violations = []
checked_count = 0
max_observed = (0, "")

for root in roots:
    if not root.exists():
        continue
    for path in sorted(root.rglob("*.rs")):
        line_count = len(path.read_text(encoding="utf-8").splitlines())
        checked_count += 1
        if line_count > max_observed[0]:
            max_observed = (line_count, str(path))
        if line_count > max_lines:
            violations.append((str(path), line_count))

if violations:
    for path_text, count in violations:
        print(f"FAIL: {path_text} has {count} lines (ceiling: {max_lines})", file=sys.stderr)
    sys.exit(1)

print(f"OK: {checked_count} Rust files checked; longest is {max_observed[1]} ({max_observed[0]} lines).")
PY

echo "[3/7] Checking zero production .unwrap() and .expect()..."
python3 - <<'PY'
from pathlib import Path
import re, sys

roots = [Path("crates/clickforge_sdk/src"), Path("src")]
pattern = re.compile(r"\.(?:unwrap|expect)\s*\(")
violations = []

for root in roots:
    for path in sorted(root.rglob("*.rs")):
        if path.name.endswith("_tests.rs"):
            continue
        lines = path.read_text(encoding="utf-8").splitlines()
        in_test_section = False
        for line_number, raw_line in enumerate(lines, start=1):
            stripped = raw_line.strip()
            if "#[cfg(test)]" in stripped:
                in_test_section = True
            if in_test_section or stripped.startswith("//"):
                continue
            without_strings = re.sub(r'"(?:\\.|[^"\\])*"', '""', stripped)
            if pattern.search(without_strings):
                violations.append((str(path), line_number, stripped))

if violations:
    for path_text, line_number, content in violations:
        print(f"FAIL: production unwrap/expect at {path_text}:{line_number}: {content}", file=sys.stderr)
    sys.exit(1)

print("OK: zero production .unwrap() or .expect() calls found.")
PY

echo "[4/7] Checking formatting (cargo fmt --all -- --check)..."
cargo fmt --all -- --check

echo "[5/7] Running Clippy lint gate (cargo clippy --workspace --all-targets -- -D warnings)..."
cargo clippy --workspace --all-targets -- -D warnings

echo "[6/7] Running full workspace test suite (cargo test --workspace)..."
cargo test --workspace

echo "[7/7] Building release binary (cargo build --release)..."
cargo build --release

echo "ALL CHECKS PASSED."
