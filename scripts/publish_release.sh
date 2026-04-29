#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

DIST_DIR="$ROOT/dist"
VERSION_OVERRIDE=""
PLATFORM_OVERRIDE=""
TAG_OVERRIDE=""
VERIFY_KEY=""
PUBLISH=0
DRAFT=1
PRERELEASE=0
ALLOW_EXISTING=1

usage() {
    cat <<'USAGE'
publish release v1.0 - verify and publish promoted Cool release assets

Usage:
  bash scripts/publish_release.sh [--version X.Y.Z] [--publish]

Options:
  --version X.Y.Z      Version to publish, defaults to Cargo.toml.
  --platform PLATFORM  Platform label, defaults to current host.
  --dist-dir DIR       Read promoted assets under DIR instead of ./dist.
  --tag TAG            GitHub Release tag, defaults to v<version>.
  --verify-key PATH    OpenSSL public key for signature verification.
  --publish            Create/update the GitHub Release with gh.
  --draft              Create a draft release when publishing. Default.
  --no-draft           Publish as a non-draft release.
  --prerelease         Mark the GitHub Release as a prerelease.
  --no-existing        Fail if the GitHub Release already exists.
USAGE
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

require_command() {
    if ! command -v "$1" >/dev/null 2>&1; then
        printf 'publish release: missing required command: %s\n' "$1" >&2
        exit 1
    fi
}

while [[ $# -gt 0 ]]; do
    case "$1" in
        --version)
            if [[ $# -lt 2 || -z "${2:-}" ]]; then
                printf 'publish release: --version requires a value\n' >&2
                exit 2
            fi
            VERSION_OVERRIDE="$2"
            shift 2
            ;;
        --version=*)
            VERSION_OVERRIDE="${1#--version=}"
            shift
            ;;
        --platform)
            if [[ $# -lt 2 || -z "${2:-}" ]]; then
                printf 'publish release: --platform requires a value\n' >&2
                exit 2
            fi
            PLATFORM_OVERRIDE="$2"
            shift 2
            ;;
        --platform=*)
            PLATFORM_OVERRIDE="${1#--platform=}"
            shift
            ;;
        --dist-dir)
            if [[ $# -lt 2 || -z "${2:-}" ]]; then
                printf 'publish release: --dist-dir requires a path\n' >&2
                exit 2
            fi
            DIST_DIR="$2"
            shift 2
            ;;
        --dist-dir=*)
            DIST_DIR="${1#--dist-dir=}"
            shift
            ;;
        --tag)
            if [[ $# -lt 2 || -z "${2:-}" ]]; then
                printf 'publish release: --tag requires a value\n' >&2
                exit 2
            fi
            TAG_OVERRIDE="$2"
            shift 2
            ;;
        --tag=*)
            TAG_OVERRIDE="${1#--tag=}"
            shift
            ;;
        --verify-key)
            if [[ $# -lt 2 || -z "${2:-}" ]]; then
                printf 'publish release: --verify-key requires a path\n' >&2
                exit 2
            fi
            VERIFY_KEY="$2"
            shift 2
            ;;
        --verify-key=*)
            VERIFY_KEY="${1#--verify-key=}"
            shift
            ;;
        --publish)
            PUBLISH=1
            shift
            ;;
        --draft)
            DRAFT=1
            shift
            ;;
        --no-draft)
            DRAFT=0
            shift
            ;;
        --prerelease)
            PRERELEASE=1
            shift
            ;;
        --no-existing)
            ALLOW_EXISTING=0
            shift
            ;;
        -h|--help|help)
            usage
            exit 0
            ;;
        *)
            printf 'publish release: unexpected argument: %s\n' "$1" >&2
            usage >&2
            exit 2
            ;;
    esac
done

require_command awk
require_command git
require_command python3
require_command uname

DIST_DIR="$(cd "$DIST_DIR" && pwd)"
VERSION="${VERSION_OVERRIDE:-$(manifest_value version)}"
PLATFORM="${PLATFORM_OVERRIDE:-$(platform_os)-$(platform_arch)}"
TAG="${TAG_OVERRIDE:-v$VERSION}"
RELEASE_DIR="$DIST_DIR/releases/$VERSION"
NOTES_PATH="$RELEASE_DIR/RELEASE.md"

if [[ ! -d "$RELEASE_DIR" ]]; then
    printf 'publish release: promoted release directory not found: %s\n' "$RELEASE_DIR" >&2
    exit 1
fi
if [[ ! -f "$NOTES_PATH" ]]; then
    printf 'publish release: release notes not found: %s\n' "$NOTES_PATH" >&2
    exit 1
fi

VERIFY_ARGS=(verify --version "$VERSION" --platform "$PLATFORM" --dist-dir "$DIST_DIR")
if [[ -n "$VERIFY_KEY" ]]; then
    VERIFY_ARGS+=(--verify-key "$VERIFY_KEY")
fi
bash scripts/trust_release.sh "${VERIFY_ARGS[@]}"

ASSETS=()
while IFS= read -r asset; do
    ASSETS+=("$asset")
done < <(find "$RELEASE_DIR" -maxdepth 1 -type f | LC_ALL=C sort)
if [[ "${#ASSETS[@]}" -eq 0 ]]; then
    printf 'publish release: no assets found under %s\n' "$RELEASE_DIR" >&2
    exit 1
fi

if [[ "$PUBLISH" -ne 1 ]]; then
    printf '\npublish release: dry run ok\n'
    printf '  Tag     -> %s\n' "$TAG"
    printf '  Assets  -> %s file(s)\n' "${#ASSETS[@]}"
    printf '  Release -> %s\n' "$RELEASE_DIR"
    printf '  Use --publish to create/update the GitHub Release.\n'
    exit 0
fi

require_command gh
TARGET_COMMIT="$(git rev-parse HEAD)"
if gh release view "$TAG" >/dev/null 2>&1; then
    if [[ "$ALLOW_EXISTING" -ne 1 ]]; then
        printf 'publish release: GitHub Release already exists: %s\n' "$TAG" >&2
        exit 1
    fi
    gh release upload "$TAG" "${ASSETS[@]}" --clobber
else
    CREATE_ARGS=(release create "$TAG" "${ASSETS[@]}" --title "Cool $VERSION" --notes-file "$NOTES_PATH" --target "$TARGET_COMMIT")
    if [[ "$DRAFT" -eq 1 ]]; then
        CREATE_ARGS+=(--draft)
    fi
    if [[ "$PRERELEASE" -eq 1 ]]; then
        CREATE_ARGS+=(--prerelease)
    fi
    gh "${CREATE_ARGS[@]}"
fi

printf '\npublish release: ok\n'
printf '  Tag    -> %s\n' "$TAG"
printf '  Assets -> %s file(s)\n' "${#ASSETS[@]}"
