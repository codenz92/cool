#!/usr/bin/env bash
set -euo pipefail

VERSION="${COOL_VERSION:-}"
PLATFORM="${COOL_PLATFORM:-}"
ARCHIVE="${COOL_ARCHIVE:-}"
ARCHIVE_URL="${COOL_ARCHIVE_URL:-}"
BASE_URL="${COOL_RELEASE_BASE_URL:-https://github.com/codenz92/cool-lang/releases/download}"
VERIFY_SHA256="${COOL_ARCHIVE_SHA256:-}"
CHECKSUMS_REF="${COOL_CHECKSUMS:-}"
CHECKSUMS_SIGNATURE_REF="${COOL_CHECKSUMS_SIGNATURE:-}"
VERIFY_KEY="${COOL_VERIFY_KEY:-}"
BIN_DIR="${COOL_BIN_DIR:-}"
DRY_RUN=0
NO_SMOKE=0
VERIFY_METADATA=0

if [[ -n "${COOL_PREFIX:-}" ]]; then
    PREFIX="$COOL_PREFIX"
elif [[ -n "${HOME:-}" ]]; then
    PREFIX="$HOME/.local"
else
    PREFIX="/usr/local"
fi

usage() {
    cat <<'USAGE'
install cool v1.0 — install a Cool release archive

Usage:
  bash install.sh --version X.Y.Z [--prefix DIR]
  bash install.sh --from PATH/TO/cool-X.Y.Z-platform.tar.gz [--prefix DIR]
  bash install.sh --url URL --version X.Y.Z [--prefix DIR]

Options:
  --version X.Y.Z       Release version to download or label.
  --platform PLATFORM   Platform label, defaults to the current host.
  --from ARCHIVE        Install from a local .tar.gz release archive.
  --url URL             Download this exact release archive URL.
  --base-url URL        Release download base, defaults to the GitHub Releases download endpoint.
  --prefix DIR          Install payloads under DIR/lib/cool, defaults to $HOME/.local.
  --bin-dir DIR         Symlink cool into DIR, defaults to <prefix>/bin.
  --verify-sha256 HEX   Verify the archive hash before installing.
  --verify-metadata     Verify the archive through SHA256SUMS metadata.
  --checksums PATH/URL  SHA256SUMS metadata file for archive verification.
  --checksums-signature PATH/URL  Detached SHA256SUMS signature.
  --verify-key PATH     OpenSSL public key for SHA256SUMS signature verification.
  --dry-run             Validate inputs and print the planned install paths.
  --no-smoke            Skip the post-install `cool help` smoke test.
USAGE
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

require_command() {
    if ! command -v "$1" >/dev/null 2>&1; then
        printf 'install cool: missing required command: %s\n' "$1" >&2
        exit 1
    fi
}

sha256_file() {
    if command -v shasum >/dev/null 2>&1; then
        shasum -a 256 "$1" | awk '{print $1}'
    elif command -v sha256sum >/dev/null 2>&1; then
        sha256sum "$1" | awk '{print $1}'
    else
        printf 'install cool: missing required command: shasum or sha256sum\n' >&2
        exit 1
    fi
}

download_archive() {
    local url="$1"
    local output="$2"
    if command -v curl >/dev/null 2>&1; then
        curl -fsSL "$url" -o "$output"
    elif command -v wget >/dev/null 2>&1; then
        wget -q -O "$output" "$url"
    else
        printf 'install cool: missing required command: curl or wget\n' >&2
        exit 1
    fi
}

is_url() {
    case "$1" in
        http://*|https://*) return 0 ;;
        *) return 1 ;;
    esac
}

materialize_reference() {
    local ref="$1"
    local output="$2"
    if is_url "$ref"; then
        download_archive "$ref" "$output"
    else
        if [[ ! -f "$ref" ]]; then
            printf 'install cool: metadata file not found: %s\n' "$ref" >&2
            exit 1
        fi
        cp "$ref" "$output"
    fi
}

verify_signature() {
    local target="$1"
    local signature="$2"
    local key="$3"
    require_command openssl
    openssl dgst -sha256 -verify "$key" -signature "$signature" "$target" >/dev/null
}

verify_archive_from_checksums() {
    local checksums="$1"
    local archive="$2"
    local archive_name expected actual
    archive_name="$(basename "$archive")"
    expected="$(awk -v name="$archive_name" '$2 == name || $2 == "*" name { print $1; found = 1 } END { if (!found) exit 1 }' "$checksums" || true)"
    if [[ -z "$expected" ]]; then
        printf 'install cool: archive %s was not listed in %s\n' "$archive_name" "$checksums" >&2
        exit 1
    fi
    actual="$(sha256_file "$archive")"
    if [[ "$actual" != "$expected" ]]; then
        printf 'install cool: SHA-256 mismatch for %s from metadata\n' "$archive_name" >&2
        printf '  expected: %s\n' "$expected" >&2
        printf '  actual:   %s\n' "$actual" >&2
        exit 1
    fi
}

while [[ $# -gt 0 ]]; do
    case "$1" in
        --version)
            if [[ $# -lt 2 || -z "${2:-}" ]]; then
                printf 'install cool: --version requires a value\n' >&2
                exit 2
            fi
            VERSION="$2"
            shift 2
            ;;
        --version=*)
            VERSION="${1#--version=}"
            shift
            ;;
        --platform)
            if [[ $# -lt 2 || -z "${2:-}" ]]; then
                printf 'install cool: --platform requires a value\n' >&2
                exit 2
            fi
            PLATFORM="$2"
            shift 2
            ;;
        --platform=*)
            PLATFORM="${1#--platform=}"
            shift
            ;;
        --from)
            if [[ $# -lt 2 || -z "${2:-}" ]]; then
                printf 'install cool: --from requires an archive path\n' >&2
                exit 2
            fi
            ARCHIVE="$2"
            shift 2
            ;;
        --from=*)
            ARCHIVE="${1#--from=}"
            shift
            ;;
        --url)
            if [[ $# -lt 2 || -z "${2:-}" ]]; then
                printf 'install cool: --url requires a URL\n' >&2
                exit 2
            fi
            ARCHIVE_URL="$2"
            shift 2
            ;;
        --url=*)
            ARCHIVE_URL="${1#--url=}"
            shift
            ;;
        --base-url)
            if [[ $# -lt 2 || -z "${2:-}" ]]; then
                printf 'install cool: --base-url requires a URL\n' >&2
                exit 2
            fi
            BASE_URL="$2"
            shift 2
            ;;
        --base-url=*)
            BASE_URL="${1#--base-url=}"
            shift
            ;;
        --prefix)
            if [[ $# -lt 2 || -z "${2:-}" ]]; then
                printf 'install cool: --prefix requires a directory\n' >&2
                exit 2
            fi
            PREFIX="$2"
            shift 2
            ;;
        --prefix=*)
            PREFIX="${1#--prefix=}"
            shift
            ;;
        --bin-dir)
            if [[ $# -lt 2 || -z "${2:-}" ]]; then
                printf 'install cool: --bin-dir requires a directory\n' >&2
                exit 2
            fi
            BIN_DIR="$2"
            shift 2
            ;;
        --bin-dir=*)
            BIN_DIR="${1#--bin-dir=}"
            shift
            ;;
        --verify-sha256)
            if [[ $# -lt 2 || -z "${2:-}" ]]; then
                printf 'install cool: --verify-sha256 requires a hash\n' >&2
                exit 2
            fi
            VERIFY_SHA256="$2"
            shift 2
            ;;
        --verify-sha256=*)
            VERIFY_SHA256="${1#--verify-sha256=}"
            shift
            ;;
        --verify-metadata)
            VERIFY_METADATA=1
            shift
            ;;
        --checksums)
            if [[ $# -lt 2 || -z "${2:-}" ]]; then
                printf 'install cool: --checksums requires a path or URL\n' >&2
                exit 2
            fi
            CHECKSUMS_REF="$2"
            VERIFY_METADATA=1
            shift 2
            ;;
        --checksums=*)
            CHECKSUMS_REF="${1#--checksums=}"
            VERIFY_METADATA=1
            shift
            ;;
        --checksums-signature)
            if [[ $# -lt 2 || -z "${2:-}" ]]; then
                printf 'install cool: --checksums-signature requires a path or URL\n' >&2
                exit 2
            fi
            CHECKSUMS_SIGNATURE_REF="$2"
            shift 2
            ;;
        --checksums-signature=*)
            CHECKSUMS_SIGNATURE_REF="${1#--checksums-signature=}"
            shift
            ;;
        --verify-key)
            if [[ $# -lt 2 || -z "${2:-}" ]]; then
                printf 'install cool: --verify-key requires a path\n' >&2
                exit 2
            fi
            VERIFY_KEY="$2"
            VERIFY_METADATA=1
            shift 2
            ;;
        --verify-key=*)
            VERIFY_KEY="${1#--verify-key=}"
            VERIFY_METADATA=1
            shift
            ;;
        --dry-run)
            DRY_RUN=1
            shift
            ;;
        --no-smoke)
            NO_SMOKE=1
            shift
            ;;
        -h|--help|help)
            usage
            exit 0
            ;;
        *)
            printf 'install cool: unexpected argument: %s\n' "$1" >&2
            usage >&2
            exit 2
            ;;
    esac
done

require_command awk
require_command find
require_command head
require_command mktemp
require_command tar
require_command uname

if [[ -z "$PLATFORM" ]]; then
    PLATFORM="$(platform_os)-$(platform_arch)"
fi
if [[ -z "$BIN_DIR" ]]; then
    BIN_DIR="$PREFIX/bin"
fi

TMP_DIR="$(mktemp -d "${TMPDIR:-/tmp}/cool-install.XXXXXX")"
cleanup() {
    rm -rf "$TMP_DIR"
}
trap cleanup EXIT

if [[ -z "$ARCHIVE" ]]; then
    if [[ -z "$ARCHIVE_URL" ]]; then
        if [[ -z "$VERSION" ]]; then
            printf 'install cool: --version is required when --from or --url is not provided\n' >&2
            exit 2
        fi
        ARCHIVE_URL="${BASE_URL}/v${VERSION}/cool-${VERSION}-${PLATFORM}.tar.gz"
    fi
    ARCHIVE="$TMP_DIR/cool-release.tar.gz"
    printf 'Downloading %s\n' "$ARCHIVE_URL"
    download_archive "$ARCHIVE_URL" "$ARCHIVE"
fi

if [[ ! -f "$ARCHIVE" ]]; then
    printf 'install cool: archive not found: %s\n' "$ARCHIVE" >&2
    exit 1
fi

if [[ -n "$VERIFY_SHA256" ]]; then
    ACTUAL_SHA256="$(sha256_file "$ARCHIVE")"
    if [[ "$ACTUAL_SHA256" != "$VERIFY_SHA256" ]]; then
        printf 'install cool: SHA-256 mismatch for %s\n' "$ARCHIVE" >&2
        printf '  expected: %s\n' "$VERIFY_SHA256" >&2
        printf '  actual:   %s\n' "$ACTUAL_SHA256" >&2
        exit 1
    fi
fi

if [[ "$VERIFY_METADATA" -eq 1 ]]; then
    if [[ -z "$CHECKSUMS_REF" ]]; then
        if [[ -z "$VERSION" ]]; then
            printf 'install cool: --version is required to infer SHA256SUMS metadata\n' >&2
            exit 2
        fi
        CHECKSUMS_REF="${BASE_URL}/v${VERSION}/SHA256SUMS"
    fi
    CHECKSUMS_PATH="$TMP_DIR/SHA256SUMS"
    materialize_reference "$CHECKSUMS_REF" "$CHECKSUMS_PATH"
    if [[ -n "$VERIFY_KEY" ]]; then
        if [[ -z "$CHECKSUMS_SIGNATURE_REF" ]]; then
            CHECKSUMS_SIGNATURE_REF="${CHECKSUMS_REF}.sig"
        fi
        CHECKSUMS_SIGNATURE_PATH="$TMP_DIR/SHA256SUMS.sig"
        materialize_reference "$CHECKSUMS_SIGNATURE_REF" "$CHECKSUMS_SIGNATURE_PATH"
        verify_signature "$CHECKSUMS_PATH" "$CHECKSUMS_SIGNATURE_PATH" "$VERIFY_KEY"
    fi
    verify_archive_from_checksums "$CHECKSUMS_PATH" "$ARCHIVE"
fi

tar -xzf "$ARCHIVE" -C "$TMP_DIR"
PAYLOAD_DIR="$(find "$TMP_DIR" -mindepth 1 -maxdepth 1 -type d | head -n 1)"
if [[ -z "$PAYLOAD_DIR" ]]; then
    printf 'install cool: archive did not contain a payload directory\n' >&2
    exit 1
fi

PAYLOAD_NAME="$(basename "$PAYLOAD_DIR")"
BINARY_NAME="cool"
if [[ "$PLATFORM" == windows-* || "$PLATFORM" == *-windows* ]]; then
    BINARY_NAME="cool.exe"
fi
PAYLOAD_BIN="$PAYLOAD_DIR/bin/$BINARY_NAME"
if [[ ! -x "$PAYLOAD_BIN" ]]; then
    printf 'install cool: archive payload does not contain an executable bin/%s\n' "$BINARY_NAME" >&2
    exit 1
fi

INSTALL_DIR="$PREFIX/lib/cool/$PAYLOAD_NAME"

if [[ "$DRY_RUN" -eq 1 ]]; then
    printf 'install cool: dry run\n'
    printf '  Archive     -> %s\n' "$ARCHIVE"
    printf '  Payload     -> %s\n' "$PAYLOAD_NAME"
    printf '  Install dir -> %s\n' "$INSTALL_DIR"
    printf '  Binary link -> %s/cool\n' "$BIN_DIR"
    exit 0
fi

mkdir -p "$PREFIX/lib/cool" "$BIN_DIR"
rm -rf "$INSTALL_DIR"
cp -R "$PAYLOAD_DIR" "$INSTALL_DIR"
chmod 755 "$INSTALL_DIR/bin/$BINARY_NAME"
ln -sf "$INSTALL_DIR/bin/$BINARY_NAME" "$BIN_DIR/cool"

if [[ "$NO_SMOKE" -eq 0 ]]; then
    "$BIN_DIR/cool" help >/dev/null
fi

printf 'install cool: ok\n'
printf '  Installed -> %s\n' "$INSTALL_DIR"
printf '  Binary    -> %s/cool\n' "$BIN_DIR"
case ":${PATH:-}:" in
    *":$BIN_DIR:"*) ;;
    *) printf '  PATH      -> add %s to PATH if cool is not found by your shell\n' "$BIN_DIR" ;;
esac
