#!/usr/bin/bash
# SPDX-License-Identifier: MIT
# SPDX-FileCopyrightText: <text>
# Copyright(c) 2026 Liebherr-Digital Development Center GmbH
# Written by Thomas Witte <thomas.witte@liebherr.com>
# </text>

# Runs an instrumented test run and reports the line coverage per file.

set -euo pipefail

PROFRAW_DIR="$PWD/target/coverage"
HTML_DIR=""
RUN_PYTEST=0
TARGET=x86_64-unknown-linux-gnu
PROFILE=release

usage() {
    echo "Usage: $0 [-p | --pytest] [-o | --html DIR] [-h | --help]"
    echo
    echo "Options:"
    echo "  -p, --pytest       Additionally run the pytest testsuite instrumented"
    echo "  -o, --html DIR     Also write an HTML report to DIR"
    echo "  -h, --help         Display this help message"
    echo
}

if ! OPTS=$(getopt -o "po:h" -l "pytest,html:,help" -n "$0" -- "$@"); then
    usage
    exit 1
fi
eval set -- "$OPTS"

while true; do
    case "$1" in
        -p|--pytest)
            RUN_PYTEST=1
            shift
            ;;
        -o|--html)
            HTML_DIR="$2"
            shift 2
            ;;
        -h|--help)
            usage
            exit 0
            ;;
        --)
            shift
            break
            ;;
        *)
            echo "Unknown option: $1"
            usage
            exit 1
            ;;
    esac
done

if ! command -v grcov > /dev/null; then
    echo "grcov is not installed, run: cargo install --locked grcov" >&2
    exit 1
fi

rustup component add llvm-tools-preview > /dev/null

rm -rf "$PROFRAW_DIR"
mkdir -p "$PROFRAW_DIR"

RUSTFLAGS="-C instrument-coverage" \
LLVM_PROFILE_FILE="$PROFRAW_DIR/updated-%p-%m.profraw" \
cargo test --locked --all-features --all-targets --workspace -- --test-threads=1

if [ "$RUN_PYTEST" -eq 1 ]; then
    # shellcheck disable=SC1091
    [ -d .venv ] && . .venv/bin/activate
    RUSTFLAGS="-C instrument-coverage" \
    LLVM_PROFILE_FILE="$PROFRAW_DIR/updated-pytest-%p-%m.profraw" \
    cargo build --locked --target="$TARGET" --profile="$PROFILE"
    RUSTFLAGS="-C instrument-coverage" \
    LLVM_PROFILE_FILE="$PROFRAW_DIR/updated-pytest-%p-%m.profraw" \
    pytest --target="$TARGET" --profile="$PROFILE" -vv -s
fi

LCOV_FILE=$(mktemp)
trap 'rm -f "$LCOV_FILE"' EXIT

# Restrict the report to the workspace crates, dependencies are not of interest.
GRCOV_ARGS=(
    "$PROFRAW_DIR"
    --binary-path ./target/
    -s .
    --branch
    --ignore-not-existing
    --ignore "*target*"
    --keep-only "libupdated/src/*"
    --keep-only "updated/src/*"
)

grcov "${GRCOV_ARGS[@]}" -t lcov -o "$LCOV_FILE"

if [ -n "$HTML_DIR" ]; then
    grcov "${GRCOV_ARGS[@]}" -t html -o "$HTML_DIR"
    echo "HTML report written to $HTML_DIR"
fi

echo
echo "Line coverage per file:"
awk '
    /^SF:/ { file = substr($0, 4); next }
    /^DA:/ {
        split(substr($0, 4), da, ",")
        key = file SUBSEP da[1]
        if (!(key in known)) { known[key] = 1; total[file]++ }
        if (da[2] > 0 && !(key in hit)) { hit[key] = 1; covered[file]++ }
    }
    END { for (f in total) printf "%s\t%d\t%d\n", f, covered[f] + 0, total[f] }
' "$LCOV_FILE" | sort | awk -F'\t' '
    { lines += $3; hits += $2
      printf "%7.2f%%  %5d/%-5d  %s\n", $3 ? $2 * 100 / $3 : 0, $2, $3, $1 }
    END { printf "%7.2f%%  %5d/%-5d  %s\n", lines ? hits * 100 / lines : 0, hits, lines, "TOTAL" }
'
