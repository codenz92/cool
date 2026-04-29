#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

run() {
    printf '\n==> %s\n' "$*"
    "$@"
}

command_text() {
    local out=""
    for part in "$@"; do
        out+=" $(printf '%q' "$part")"
    done
    printf '%s' "${out# }"
}

expect_output() {
    local expected="$1"
    shift
    local output
    output="$("$@")"
    if [[ "$output" != "$expected" ]]; then
        printf 'release gate: output mismatch\n' >&2
        printf 'command: %s\n' "$(command_text "$@")" >&2
        printf 'expected:\n%s\n' "$expected" >&2
        printf 'actual:\n%s\n' "$output" >&2
        return 1
    fi
}

require_command() {
    if ! command -v "$1" >/dev/null 2>&1; then
        printf 'release gate: missing required command: %s\n' "$1" >&2
        return 1
    fi
}

require_command cargo
require_command cc

HOST_OS="$(uname -s)"
HOSTED_NATIVE_BINARY=1
case "$HOST_OS" in
    MINGW*|MSYS*|CYGWIN*)
        HOSTED_NATIVE_BINARY=0
        ;;
esac
if [[ -n "${COOL_RELEASE_GATE_HOSTED_NATIVE_BINARY:-}" ]]; then
    HOSTED_NATIVE_BINARY="$COOL_RELEASE_GATE_HOSTED_NATIVE_BINARY"
fi

run cargo fmt --check
run cargo build -q --bin cool
if [[ "$HOSTED_NATIVE_BINARY" -eq 1 ]]; then
    run cargo test -q
else
    run cargo test -q --bins
fi

COOL_BIN="${COOL_BIN:-$ROOT/target/debug/cool}"
if [[ ! -x "$COOL_BIN" ]]; then
    printf 'release gate: cool binary not found or not executable: %s\n' "$COOL_BIN" >&2
    exit 1
fi

run "$COOL_BIN" check examples/hello.cool
run "$COOL_BIN" check examples/coolboard/src/main.cool

TMP_ROOT="${TMPDIR:-/tmp}"
TMP_DIR="$(mktemp -d "$TMP_ROOT/cool-release-gate.XXXXXX")"
trap 'rm -rf "$TMP_DIR"' EXIT

PARITY_SRC="$TMP_DIR/parity.cool"
cat > "$PARITY_SRC" <<'COOL'
def make_adder(n):
    def adder(x):
        return x + n
    return adder

add5 = make_adder(5)
print(add5(7))

n = 7
add = lambda x: x + n
n = 100
print(add(5))
COOL

PARITY_EXPECTED=$'12\n105'
expect_output "$PARITY_EXPECTED" "$COOL_BIN" "$PARITY_SRC"
expect_output "$PARITY_EXPECTED" "$COOL_BIN" --vm "$PARITY_SRC"
if [[ "$HOSTED_NATIVE_BINARY" -eq 1 ]]; then
    run "$COOL_BIN" build --emit binary "$PARITY_SRC"
    expect_output "$PARITY_EXPECTED" "${PARITY_SRC%.cool}"
else
    printf '\n==> skipping hosted native binary parity on %s\n' "$HOST_OS"
fi

FREESTANDING_SRC="$TMP_DIR/freestanding.cool"
cat > "$FREESTANDING_SRC" <<'COOL'
import core

def _start():
    entry: "_start"
    return 0
COOL

run "$COOL_BIN" build --freestanding --emit object "$FREESTANDING_SRC"
if [[ ! -s "${FREESTANDING_SRC%.cool}.o" ]]; then
    printf 'release gate: freestanding object was not created\n' >&2
    exit 1
fi

printf '\nrelease gate: ok\n'
