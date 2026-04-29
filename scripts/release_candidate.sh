#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

DIST_DIR="$ROOT/dist"
RUN_GATE=1
VERSION_OVERRIDE=""
PLATFORM_OVERRIDE=""
REQUIRE_CLEAN=0

usage() {
    cat <<'USAGE'
release candidate v1.0 — build and package the Cool compiler distribution

Usage:
  bash scripts/release_candidate.sh [--skip-gate] [--require-clean] [--version X.Y.Z] [--platform PLATFORM] [--dist-dir DIR]

Options:
  --skip-gate      Package after a previously successful release gate run.
  --require-clean  Fail when the git worktree has uncommitted changes.
  --version X.Y.Z  Override the Cargo.toml package version in artifact metadata.
  --platform NAME  Override the platform label, e.g. linux-x86_64 or windows-x86_64.
  --dist-dir DIR   Write artifacts under DIR instead of ./dist.
USAGE
}

while [[ $# -gt 0 ]]; do
    case "$1" in
        --skip-gate)
            RUN_GATE=0
            shift
            ;;
        --require-clean)
            REQUIRE_CLEAN=1
            shift
            ;;
        --version)
            if [[ $# -lt 2 || -z "${2:-}" ]]; then
                printf 'release candidate: --version requires a value\n' >&2
                exit 2
            fi
            VERSION_OVERRIDE="$2"
            shift 2
            ;;
        --version=*)
            VERSION_OVERRIDE="${1#--version=}"
            if [[ -z "$VERSION_OVERRIDE" ]]; then
                printf 'release candidate: --version requires a value\n' >&2
                exit 2
            fi
            shift
            ;;
        --platform)
            if [[ $# -lt 2 || -z "${2:-}" ]]; then
                printf 'release candidate: --platform requires a value\n' >&2
                exit 2
            fi
            PLATFORM_OVERRIDE="$2"
            shift 2
            ;;
        --platform=*)
            PLATFORM_OVERRIDE="${1#--platform=}"
            if [[ -z "$PLATFORM_OVERRIDE" ]]; then
                printf 'release candidate: --platform requires a value\n' >&2
                exit 2
            fi
            shift
            ;;
        --dist-dir)
            if [[ $# -lt 2 || -z "${2:-}" ]]; then
                printf 'release candidate: --dist-dir requires a path\n' >&2
                exit 2
            fi
            DIST_DIR="$2"
            shift 2
            ;;
        --dist-dir=*)
            DIST_DIR="${1#--dist-dir=}"
            if [[ -z "$DIST_DIR" ]]; then
                printf 'release candidate: --dist-dir requires a path\n' >&2
                exit 2
            fi
            shift
            ;;
        -h|--help|help)
            usage
            exit 0
            ;;
        *)
            printf 'release candidate: unexpected argument: %s\n' "$1" >&2
            usage >&2
            exit 2
            ;;
    esac
done

run() {
    printf '\n==> %s\n' "$*"
    "$@"
}

require_command() {
    if ! command -v "$1" >/dev/null 2>&1; then
        printf 'release candidate: missing required command: %s\n' "$1" >&2
        exit 1
    fi
}

require_checksum_command() {
    if command -v shasum >/dev/null 2>&1 || command -v sha256sum >/dev/null 2>&1; then
        return
    fi
    printf 'release candidate: missing required command: shasum or sha256sum\n' >&2
    exit 1
}

sha256_file() {
    if command -v shasum >/dev/null 2>&1; then
        shasum -a 256 "$1" | awk '{print $1}'
    else
        sha256sum "$1" | awk '{print $1}'
    fi
}

file_size() {
    if stat -f %z "$1" >/dev/null 2>&1; then
        stat -f %z "$1"
    else
        stat -c %s "$1"
    fi
}

json_escape() {
    local value="$1"
    value="${value//\\/\\\\}"
    value="${value//\"/\\\"}"
    value="${value//$'\n'/\\n}"
    value="${value//$'\r'/\\r}"
    value="${value//$'\t'/\\t}"
    printf '%s' "$value"
}

json_string() {
    printf '"%s"' "$(json_escape "$1")"
}

manifest_value() {
    local key="$1"
    awk -F '"' -v key="$key" '
        $0 ~ "^[[:space:]]*" key "[[:space:]]*=" {
            print $2
            exit
        }
    ' Cargo.toml
}

platform_os() {
    local name
    name="$(uname -s)"
    case "$name" in
        Darwin) printf 'macos' ;;
        Linux) printf 'linux' ;;
        MINGW*|MSYS*|CYGWIN*) printf 'windows' ;;
        *) printf '%s' "$name" | tr '[:upper:]' '[:lower:]' ;;
    esac
}

platform_arch() {
    local arch
    arch="$(uname -m)"
    case "$arch" in
        arm64|aarch64) printf 'arm64' ;;
        x86_64|amd64) printf 'x86_64' ;;
        *) printf '%s' "$arch" | tr '[:upper:]' '[:lower:]' ;;
    esac
}

exe_suffix() {
    case "$1" in
        windows-*|*-windows*) printf '.exe' ;;
        *) printf '' ;;
    esac
}

relative_to_root() {
    local path_value="$1"
    case "$path_value" in
        "$ROOT"/*) printf '%s' "${path_value#"$ROOT"/}" ;;
        "$ROOT") printf '.' ;;
        *) printf '%s' "$path_value" ;;
    esac
}

list_payload_files() {
    (
        cd "$RC_DIR"
        find . -type f \
            ! -name manifest.json \
            ! -name checksums.txt \
            | sed 's#^\./##' \
            | LC_ALL=C sort
    )
}

write_checksums() {
    : > "$CHECKSUMS_PATH"
    while IFS= read -r rel; do
        [[ -z "$rel" ]] && continue
        printf '%s  %s\n' "$(sha256_file "$RC_DIR/$rel")" "$rel" >> "$CHECKSUMS_PATH"
    done < <(list_payload_files)
}

write_artifact_entries() {
    local first=1
    while IFS= read -r rel; do
        [[ -z "$rel" ]] && continue
        if [[ "$first" -eq 0 ]]; then
            printf ',\n'
        fi
        first=0
        printf '      {"path": '
        json_string "$rel"
        printf ', "sha256": '
        json_string "$(sha256_file "$RC_DIR/$rel")"
        printf ', "bytes": %s}' "$(file_size "$RC_DIR/$rel")"
    done < <(list_payload_files)

    if [[ -s "$CHECKSUMS_PATH" ]]; then
        if [[ "$first" -eq 0 ]]; then
            printf ',\n'
        fi
        printf '      {"path": "checksums.txt", "sha256": '
        json_string "$(sha256_file "$CHECKSUMS_PATH")"
        printf ', "bytes": %s}' "$(file_size "$CHECKSUMS_PATH")"
    fi
}

write_manifest() {
    cat > "$MANIFEST_PATH" <<EOF
{
  "schema_version": 1,
  "package": {
    "name": $(json_string "$PACKAGE_NAME"),
    "version": $(json_string "$VERSION")
  },
  "release_candidate": {
    "platform": $(json_string "$PLATFORM"),
    "generated_at": $(json_string "$GENERATED_AT"),
    "release_gate": $(json_string "$GATE_STATUS")
  },
  "git": {
    "commit": $(json_string "$COMMIT"),
    "branch": $(json_string "$BRANCH"),
    "dirty": $GIT_DIRTY
  },
  "toolchain": {
    "cargo": $(json_string "$CARGO_VERSION"),
    "rustc": $(json_string "$RUSTC_VERSION")
  },
  "host": {
    "os": $(json_string "$OS_LABEL"),
    "arch": $(json_string "$ARCH_LABEL"),
    "platform": $(json_string "$HOST_PLATFORM"),
    "uname": $(json_string "$UNAME_VALUE")
  },
  "artifacts": [
$(write_artifact_entries)
  ]
}
EOF
}

write_release_notes() {
    cat > "$RELEASE_NOTES_PATH" <<EOF
# Cool $VERSION Release Candidate

Generated: $GENERATED_AT
Platform: $PLATFORM
Commit: $COMMIT
Branch: $BRANCH
Release gate: $GATE_STATUS

## Contents

- \`bin/$BINARY_NAME\`
- \`README.md\`
- \`CHANGELOG.md\`
- \`ROADMAP.md\`
- \`LICENSE\`
- \`install.sh\`
- \`docs/INSTALL.md\`
- \`docs/RELEASE_TRUST.md\`
- \`docs/PACKAGE_CHANNELS.md\`
- \`scripts/release_gate.sh\`
- \`scripts/release_candidate.sh\`
- \`scripts/promote_release.sh\`
- \`scripts/trust_release.sh\`
- \`scripts/trust_release.py\`
- \`scripts/publish_release.sh\`
- \`scripts/package_channels.sh\`
- \`scripts/package_channels.py\`
- \`scripts/assemble_matrix_release.sh\`
- \`scripts/assemble_matrix_release.py\`
- \`manifest.json\`
- \`checksums.txt\`

## Verification

Use the SHA-256 hashes in \`checksums.txt\` to verify the extracted payload.
The distribution manifest records the git commit, worktree cleanliness, host
platform, Rust toolchain, and whether the release gate passed or was skipped.
EOF
}

write_latest() {
    local archive_sha
    local zip_sha
    archive_sha="$(sha256_file "$ARCHIVE_PATH")"
    zip_sha="$(sha256_file "$ZIP_ARCHIVE_PATH")"
    cat > "$LATEST_PATH" <<EOF
{
  "schema_version": 1,
  "package": {
    "name": $(json_string "$PACKAGE_NAME"),
    "version": $(json_string "$VERSION")
  },
  "platform": $(json_string "$PLATFORM"),
  "generated_at": $(json_string "$GENERATED_AT"),
  "release_gate": $(json_string "$GATE_STATUS"),
  "git": {
    "commit": $(json_string "$COMMIT"),
    "branch": $(json_string "$BRANCH"),
    "dirty": $GIT_DIRTY
  },
  "archive": {
    "path": $(json_string "$(relative_to_root "$ARCHIVE_PATH")"),
    "sha256": $(json_string "$archive_sha"),
    "bytes": $(file_size "$ARCHIVE_PATH")
  },
  "zip_archive": {
    "path": $(json_string "$(relative_to_root "$ZIP_ARCHIVE_PATH")"),
    "sha256": $(json_string "$zip_sha"),
    "bytes": $(file_size "$ZIP_ARCHIVE_PATH")
  },
  "manifest": {
    "path": $(json_string "$(relative_to_root "$MANIFEST_PATH")"),
    "sha256": $(json_string "$(sha256_file "$MANIFEST_PATH")"),
    "bytes": $(file_size "$MANIFEST_PATH")
  }
}
EOF
}

require_command awk
require_command cargo
require_command date
require_command find
require_command git
require_command python3
require_command sed
require_command tar
require_command uname
require_checksum_command

mkdir -p "$DIST_DIR"
DIST_DIR="$(cd "$DIST_DIR" && pwd)"

PACKAGE_NAME="$(manifest_value name)"
PACKAGE_VERSION="$(manifest_value version)"
if [[ -z "$PACKAGE_NAME" || -z "$PACKAGE_VERSION" ]]; then
    printf 'release candidate: could not read package name/version from Cargo.toml\n' >&2
    exit 1
fi

VERSION="${VERSION_OVERRIDE:-$PACKAGE_VERSION}"
OS_LABEL="$(platform_os)"
ARCH_LABEL="$(platform_arch)"
HOST_PLATFORM="$OS_LABEL-$ARCH_LABEL"
PLATFORM="${PLATFORM_OVERRIDE:-$HOST_PLATFORM}"
EXE_SUFFIX="$(exe_suffix "$PLATFORM")"
BINARY_NAME="$PACKAGE_NAME$EXE_SUFFIX"
BINARY_SRC="$ROOT/target/release/$BINARY_NAME"
RC_DIR="$DIST_DIR/release-candidate/$VERSION/$PLATFORM"
CHECKSUMS_PATH="$RC_DIR/checksums.txt"
MANIFEST_PATH="$RC_DIR/manifest.json"
RELEASE_NOTES_PATH="$RC_DIR/RELEASE_NOTES.md"
ARCHIVE_NAME="$PACKAGE_NAME-$VERSION-$PLATFORM.tar.gz"
ARCHIVE_PATH="$DIST_DIR/release-candidate/$ARCHIVE_NAME"
ZIP_ARCHIVE_NAME="$PACKAGE_NAME-$VERSION-$PLATFORM.zip"
ZIP_ARCHIVE_PATH="$DIST_DIR/release-candidate/$ZIP_ARCHIVE_NAME"
LATEST_PATH="$DIST_DIR/release-candidate/latest.json"
GENERATED_AT="$(date -u +"%Y-%m-%dT%H:%M:%SZ")"
COMMIT="$(git rev-parse --short HEAD 2>/dev/null || printf 'unknown')"
BRANCH="$(git branch --show-current 2>/dev/null || printf 'unknown')"
if [[ -z "$BRANCH" ]]; then
    BRANCH="detached"
fi
if [[ -n "$(git status --porcelain 2>/dev/null || true)" ]]; then
    GIT_DIRTY=true
else
    GIT_DIRTY=false
fi
RUSTC_VERSION="$(rustc --version)"
CARGO_VERSION="$(cargo --version)"
UNAME_VALUE="$(uname -a)"

if [[ "$REQUIRE_CLEAN" -eq 1 && "$GIT_DIRTY" == "true" ]]; then
    printf 'release candidate: git worktree has uncommitted changes\n' >&2
    exit 1
fi

if [[ "$RUN_GATE" -eq 1 ]]; then
    run bash "$ROOT/scripts/release_gate.sh"
    GATE_STATUS="passed"
else
    printf '\n==> skipping release gate (--skip-gate)\n'
    GATE_STATUS="skipped"
fi

run cargo build --release --bin "$PACKAGE_NAME"
if [[ ! -x "$BINARY_SRC" ]]; then
    printf 'release candidate: release binary not found or not executable: %s\n' "$BINARY_SRC" >&2
    exit 1
fi

rm -rf "$RC_DIR"
mkdir -p "$RC_DIR/bin" "$RC_DIR/docs" "$RC_DIR/scripts"

cp "$BINARY_SRC" "$RC_DIR/bin/$BINARY_NAME"
chmod 755 "$RC_DIR/bin/$BINARY_NAME"
cp README.md CHANGELOG.md ROADMAP.md LICENSE "$RC_DIR/"
cp install.sh "$RC_DIR/"
cp docs/INSTALL.md docs/RELEASE_TRUST.md docs/PACKAGE_CHANNELS.md "$RC_DIR/docs/"
cp scripts/release_gate.sh scripts/release_candidate.sh scripts/promote_release.sh "$RC_DIR/scripts/"
cp scripts/trust_release.sh scripts/trust_release.py scripts/publish_release.sh "$RC_DIR/scripts/"
cp scripts/package_channels.sh scripts/package_channels.py "$RC_DIR/scripts/"
cp scripts/assemble_matrix_release.sh scripts/assemble_matrix_release.py "$RC_DIR/scripts/"

write_release_notes
write_checksums
write_manifest

ARCHIVE_STAGE="$(mktemp -d "${TMPDIR:-/tmp}/cool-rc.XXXXXX")"
trap 'rm -rf "$ARCHIVE_STAGE"' EXIT
PAYLOAD_ROOT="$PACKAGE_NAME-$VERSION-$PLATFORM"
mkdir -p "$ARCHIVE_STAGE/$PAYLOAD_ROOT"
cp -R "$RC_DIR"/. "$ARCHIVE_STAGE/$PAYLOAD_ROOT"/
mkdir -p "$(dirname "$ARCHIVE_PATH")"
rm -f "$ARCHIVE_PATH"
run tar -czf "$ARCHIVE_PATH" -C "$ARCHIVE_STAGE" "$PAYLOAD_ROOT"
rm -f "$ZIP_ARCHIVE_PATH"
run python3 - "$ARCHIVE_STAGE" "$PAYLOAD_ROOT" "$ZIP_ARCHIVE_PATH" <<'PY'
import sys
import zipfile
from pathlib import Path

stage = Path(sys.argv[1])
payload = sys.argv[2]
output = Path(sys.argv[3])
root = stage / payload

with zipfile.ZipFile(output, "w", zipfile.ZIP_DEFLATED) as archive:
    for path in sorted(root.rglob("*")):
        if path.is_dir():
            continue
        rel = Path(payload) / path.relative_to(root)
        info = zipfile.ZipInfo(rel.as_posix())
        info.external_attr = (path.stat().st_mode & 0xFFFF) << 16
        with path.open("rb") as fh:
            archive.writestr(info, fh.read())
PY

write_latest

printf '\nrelease candidate: ok\n'
printf '  Payload   -> %s\n' "$(relative_to_root "$RC_DIR")"
printf '  Archive   -> %s\n' "$(relative_to_root "$ARCHIVE_PATH")"
printf '  Zip       -> %s\n' "$(relative_to_root "$ZIP_ARCHIVE_PATH")"
printf '  Manifest  -> %s\n' "$(relative_to_root "$MANIFEST_PATH")"
printf '  Checksums -> %s\n' "$(relative_to_root "$CHECKSUMS_PATH")"
printf '  Latest    -> %s\n' "$(relative_to_root "$LATEST_PATH")"
