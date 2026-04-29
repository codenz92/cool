#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

DIST_DIR="$ROOT/dist"
VERSION_OVERRIDE=""
PLATFORM_OVERRIDE=""
TAG_OVERRIDE=""
BASE_DOWNLOAD_URL="${COOL_RELEASE_BASE_URL:-https://github.com/codenz92/cool-lang/releases/download}"
ALLOW_DIRTY=0
ALLOW_RC_DIRTY=0
ALLOW_SKIPPED_GATE=0
ALLOW_COMMIT_MISMATCH=0
CREATE_TAG=0
DRY_RUN=0
FORCE_TAG=0
SKIP_TRUST=0
SIGN_KEY=""
VERIFY_KEY=""

usage() {
    cat <<'USAGE'
promote release v1.0 — validate and promote a Cool release-candidate artifact

Usage:
  bash scripts/promote_release.sh [--version X.Y.Z] [--platform PLATFORM]

Options:
  --version X.Y.Z          Version to promote, defaults to Cargo.toml.
  --platform PLATFORM      Platform label, defaults to the current host.
  --dist-dir DIR           Read/write artifacts under DIR instead of ./dist.
  --tag TAG                Release tag label, defaults to v<version>.
  --base-url URL           Download base recorded in release metadata.
  --create-tag             Create a local annotated git tag after validation.
  --force-tag              Replace an existing local tag when used with --create-tag.
  --allow-dirty            Allow promotion from a dirty working tree.
  --allow-rc-dirty         Allow promotion of an RC whose manifest has git.dirty=true.
  --allow-skipped-gate     Allow promotion of an RC whose release gate was skipped.
  --allow-commit-mismatch  Allow current HEAD to differ from the RC manifest commit.
  --sign-key PATH          Sign release trust metadata with an OpenSSL private key.
  --verify-key PATH        Verify generated release signatures with an OpenSSL public key.
  --skip-trust             Do not generate SBOM/provenance/trust metadata after promotion.
  --dry-run                Validate only; do not write promoted artifacts or tags.
USAGE
}

while [[ $# -gt 0 ]]; do
    case "$1" in
        --version)
            if [[ $# -lt 2 || -z "${2:-}" ]]; then
                printf 'promote release: --version requires a value\n' >&2
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
                printf 'promote release: --platform requires a value\n' >&2
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
                printf 'promote release: --dist-dir requires a path\n' >&2
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
                printf 'promote release: --tag requires a value\n' >&2
                exit 2
            fi
            TAG_OVERRIDE="$2"
            shift 2
            ;;
        --tag=*)
            TAG_OVERRIDE="${1#--tag=}"
            shift
            ;;
        --base-url)
            if [[ $# -lt 2 || -z "${2:-}" ]]; then
                printf 'promote release: --base-url requires a URL\n' >&2
                exit 2
            fi
            BASE_DOWNLOAD_URL="$2"
            shift 2
            ;;
        --base-url=*)
            BASE_DOWNLOAD_URL="${1#--base-url=}"
            shift
            ;;
        --create-tag)
            CREATE_TAG=1
            shift
            ;;
        --force-tag)
            FORCE_TAG=1
            shift
            ;;
        --allow-dirty)
            ALLOW_DIRTY=1
            shift
            ;;
        --allow-rc-dirty)
            ALLOW_RC_DIRTY=1
            shift
            ;;
        --allow-skipped-gate)
            ALLOW_SKIPPED_GATE=1
            shift
            ;;
        --allow-commit-mismatch)
            ALLOW_COMMIT_MISMATCH=1
            shift
            ;;
        --sign-key)
            if [[ $# -lt 2 || -z "${2:-}" ]]; then
                printf 'promote release: --sign-key requires a path\n' >&2
                exit 2
            fi
            SIGN_KEY="$2"
            shift 2
            ;;
        --sign-key=*)
            SIGN_KEY="${1#--sign-key=}"
            shift
            ;;
        --verify-key)
            if [[ $# -lt 2 || -z "${2:-}" ]]; then
                printf 'promote release: --verify-key requires a path\n' >&2
                exit 2
            fi
            VERIFY_KEY="$2"
            shift 2
            ;;
        --verify-key=*)
            VERIFY_KEY="${1#--verify-key=}"
            shift
            ;;
        --skip-trust)
            SKIP_TRUST=1
            shift
            ;;
        --dry-run)
            DRY_RUN=1
            shift
            ;;
        -h|--help|help)
            usage
            exit 0
            ;;
        *)
            printf 'promote release: unexpected argument: %s\n' "$1" >&2
            usage >&2
            exit 2
            ;;
    esac
done

require_command() {
    if ! command -v "$1" >/dev/null 2>&1; then
        printf 'promote release: missing required command: %s\n' "$1" >&2
        exit 1
    fi
}

require_checksum_command() {
    if command -v shasum >/dev/null 2>&1 || command -v sha256sum >/dev/null 2>&1; then
        return
    fi
    printf 'promote release: missing required command: shasum or sha256sum\n' >&2
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

relative_to_root() {
    local path_value="$1"
    case "$path_value" in
        "$ROOT"/*) printf '%s' "${path_value#"$ROOT"/}" ;;
        "$ROOT") printf '.' ;;
        *) printf '%s' "$path_value" ;;
    esac
}

read_manifest_fields() {
    python3 - "$MANIFEST_PATH" <<'PY'
import json
import sys

with open(sys.argv[1], "r", encoding="utf-8") as fh:
    data = json.load(fh)

package = data.get("package", {})
candidate = data.get("release_candidate", {})
git = data.get("git", {})
fields = [
    str(package.get("name", "")),
    str(package.get("version", "")),
    str(candidate.get("platform", "")),
    str(candidate.get("release_gate", "")),
    str(git.get("dirty", "")).lower(),
    str(git.get("commit", "")),
    str(git.get("branch", "")),
]
print("\t".join(fields))
PY
}

list_release_files_for_sums() {
    (
        cd "$RELEASE_DIR"
        find . -type f \
            ! -name release.json \
            ! -name latest.json \
            ! -name SHA256SUMS \
            | sed 's#^\./##' \
            | LC_ALL=C sort
    )
}

list_release_files_for_json() {
    (
        cd "$RELEASE_DIR"
        find . -type f \
            ! -name release.json \
            ! -name latest.json \
            | sed 's#^\./##' \
            | LC_ALL=C sort
    )
}

archive_entries() {
    tar -tzf "$1" | sed 's#\\#/#g; s#^\./##' | tr -d '\r'
}

archive_contains() {
    local archive_path="$1"
    local expected_path="$2"
    archive_entries "$archive_path" | grep -Fxq "$expected_path"
}

write_sha256sums() {
    : > "$RELEASE_SUMS_PATH"
    while IFS= read -r rel; do
        [[ -z "$rel" ]] && continue
        printf '%s  %s\n' "$(sha256_file "$RELEASE_DIR/$rel")" "$rel" >> "$RELEASE_SUMS_PATH"
    done < <(list_release_files_for_sums)
}

write_asset_entries() {
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
        json_string "$(sha256_file "$RELEASE_DIR/$rel")"
        printf ', "bytes": %s}' "$(file_size "$RELEASE_DIR/$rel")"
    done < <(list_release_files_for_json)
}

write_release_notes() {
    local archive_sha
    local zip_asset_line
    archive_sha="$(sha256_file "$PROMOTED_ARCHIVE_PATH")"
    zip_asset_line=""
    if [[ -f "$PROMOTED_ZIP_PATH" ]]; then
        zip_asset_line="- \`$ZIP_NAME\` — SHA-256 \`$(sha256_file "$PROMOTED_ZIP_PATH")\`"
    fi
    cat > "$RELEASE_NOTES_OUT" <<EOF
# Cool $VERSION

Tag: $TAG
Commit: $RC_COMMIT
Platform: $PLATFORM
Release gate: $RC_GATE
Promoted: $GENERATED_AT

## Assets

- \`$ARCHIVE_NAME\` — SHA-256 \`$archive_sha\`
$zip_asset_line
- \`$(basename "$PROMOTED_MANIFEST_PATH")\`
- \`$(basename "$PROMOTED_CHECKSUMS_PATH")\`
- \`$(basename "$PROMOTED_RC_NOTES_PATH")\`
- \`install.sh\`
- \`validate_release.sh\`
- \`validate_release.py\`
- \`verify_hosted_release.sh\`
- \`verify_hosted_release.py\`
- \`SHA256SUMS\`
- \`release.json\`
- \`sbom.spdx.json\`
- \`provenance.intoto.json\`
- \`trust.json\`
- \`TRUST_SHA256SUMS\`

## Install

\`\`\`bash
bash install.sh --from $ARCHIVE_NAME --verify-sha256 $archive_sha
\`\`\`

For hosted releases, the installer can download the matching asset directly:

\`\`\`bash
bash install.sh --version $VERSION --platform $PLATFORM --verify-sha256 $archive_sha
\`\`\`

Promotion validates the release-candidate manifest, checksums, archive layout,
git commit, and release-gate status before writing this directory.
EOF
}

write_release_json() {
    local archive_url
    local zip_archive_url
    archive_url="${BASE_DOWNLOAD_URL}/${TAG}/${ARCHIVE_NAME}"
    zip_archive_url="${BASE_DOWNLOAD_URL}/${TAG}/${ZIP_NAME}"
    cat > "$RELEASE_JSON_PATH" <<EOF
{
  "schema_version": 1,
  "package": {
    "name": $(json_string "$PACKAGE_NAME"),
    "version": $(json_string "$VERSION")
  },
  "release": {
    "tag": $(json_string "$TAG"),
    "platform": $(json_string "$PLATFORM"),
    "generated_at": $(json_string "$GENERATED_AT"),
    "release_gate": $(json_string "$RC_GATE")
  },
  "git": {
    "commit": $(json_string "$RC_COMMIT"),
    "branch": $(json_string "$RC_BRANCH"),
    "dirty": $RC_DIRTY
  },
  "source": {
    "candidate_manifest": $(json_string "$(relative_to_root "$MANIFEST_PATH")"),
    "candidate_archive": $(json_string "$(relative_to_root "$RC_ARCHIVE_PATH")")
  },
  "install": {
    "base_url": $(json_string "$BASE_DOWNLOAD_URL"),
    "archive_url": $(json_string "$archive_url"),
    "zip_archive_url": $(json_string "$zip_archive_url"),
    "script": "install.sh"
  },
  "assets": [
$(write_asset_entries)
  ]
}
EOF
}

write_latest_json() {
    local zip_block
    zip_block=""
    if [[ -f "$PROMOTED_ZIP_PATH" ]]; then
        zip_block=",
  \"zip_archive\": {
    \"path\": $(json_string "$(relative_to_root "$PROMOTED_ZIP_PATH")"),
    \"sha256\": $(json_string "$(sha256_file "$PROMOTED_ZIP_PATH")"),
    \"bytes\": $(file_size "$PROMOTED_ZIP_PATH")
  }"
    fi
    cat > "$LATEST_RELEASE_PATH" <<EOF
{
  "schema_version": 1,
  "package": {
    "name": $(json_string "$PACKAGE_NAME"),
    "version": $(json_string "$VERSION")
  },
  "tag": $(json_string "$TAG"),
  "platform": $(json_string "$PLATFORM"),
  "generated_at": $(json_string "$GENERATED_AT"),
  "release_json": {
    "path": $(json_string "$(relative_to_root "$RELEASE_JSON_PATH")"),
    "sha256": $(json_string "$(sha256_file "$RELEASE_JSON_PATH")"),
    "bytes": $(file_size "$RELEASE_JSON_PATH")
  },
  "archive": {
    "path": $(json_string "$(relative_to_root "$PROMOTED_ARCHIVE_PATH")"),
    "sha256": $(json_string "$(sha256_file "$PROMOTED_ARCHIVE_PATH")"),
    "bytes": $(file_size "$PROMOTED_ARCHIVE_PATH")
  }$zip_block
}
EOF
    cp "$LATEST_RELEASE_PATH" "$VERSION_LATEST_PATH"
}

maybe_create_tag() {
    if [[ "$CREATE_TAG" -eq 0 ]]; then
        return
    fi

    if git rev-parse -q --verify "refs/tags/$TAG" >/dev/null; then
        local existing
        existing="$(git rev-list -n 1 "$TAG")"
        local current
        current="$(git rev-parse HEAD)"
        if [[ "$existing" == "$current" ]]; then
            printf 'promote release: tag already points at HEAD: %s\n' "$TAG"
            return
        fi
        if [[ "$FORCE_TAG" -ne 1 ]]; then
            printf 'promote release: tag already exists and does not point at HEAD: %s\n' "$TAG" >&2
            exit 1
        fi
        git tag -f -a "$TAG" -m "Release $TAG"
    else
        git tag -a "$TAG" -m "Release $TAG"
    fi
}

require_command awk
require_command date
require_command find
require_command grep
require_command git
require_command python3
require_command sed
require_command tar
require_command tr
require_command uname
require_checksum_command

mkdir -p "$DIST_DIR"
DIST_DIR="$(cd "$DIST_DIR" && pwd)"

PACKAGE_NAME="$(manifest_value name)"
PACKAGE_VERSION="$(manifest_value version)"
if [[ -z "$PACKAGE_NAME" || -z "$PACKAGE_VERSION" ]]; then
    printf 'promote release: could not read package name/version from Cargo.toml\n' >&2
    exit 1
fi

VERSION="${VERSION_OVERRIDE:-$PACKAGE_VERSION}"
PLATFORM="${PLATFORM_OVERRIDE:-$(platform_os)-$(platform_arch)}"
TAG="${TAG_OVERRIDE:-v$VERSION}"
BINARY_NAME="$PACKAGE_NAME"
if [[ "$PLATFORM" == windows-* || "$PLATFORM" == *-windows* ]]; then
    BINARY_NAME="$PACKAGE_NAME.exe"
fi

RC_DIR="$DIST_DIR/release-candidate/$VERSION/$PLATFORM"
MANIFEST_PATH="$RC_DIR/manifest.json"
CHECKSUMS_PATH="$RC_DIR/checksums.txt"
RC_NOTES_PATH="$RC_DIR/RELEASE_NOTES.md"
RC_ARCHIVE_PATH="$DIST_DIR/release-candidate/$PACKAGE_NAME-$VERSION-$PLATFORM.tar.gz"
RC_ZIP_PATH="$DIST_DIR/release-candidate/$PACKAGE_NAME-$VERSION-$PLATFORM.zip"
PAYLOAD_ROOT="$PACKAGE_NAME-$VERSION-$PLATFORM"

RELEASE_DIR="$DIST_DIR/releases/$VERSION"
ARCHIVE_NAME="$PACKAGE_NAME-$VERSION-$PLATFORM.tar.gz"
ZIP_NAME="$PACKAGE_NAME-$VERSION-$PLATFORM.zip"
PROMOTED_ARCHIVE_PATH="$RELEASE_DIR/$ARCHIVE_NAME"
PROMOTED_ZIP_PATH="$RELEASE_DIR/$ZIP_NAME"
PROMOTED_MANIFEST_PATH="$RELEASE_DIR/$PACKAGE_NAME-$VERSION-$PLATFORM.manifest.json"
PROMOTED_CHECKSUMS_PATH="$RELEASE_DIR/$PACKAGE_NAME-$VERSION-$PLATFORM.checksums.txt"
PROMOTED_RC_NOTES_PATH="$RELEASE_DIR/$PACKAGE_NAME-$VERSION-$PLATFORM.RC_NOTES.md"
RELEASE_NOTES_OUT="$RELEASE_DIR/RELEASE.md"
RELEASE_SUMS_PATH="$RELEASE_DIR/SHA256SUMS"
RELEASE_JSON_PATH="$RELEASE_DIR/release.json"
VERSION_LATEST_PATH="$RELEASE_DIR/latest.json"
LATEST_RELEASE_PATH="$DIST_DIR/releases/latest.json"
GENERATED_AT="$(date -u +"%Y-%m-%dT%H:%M:%SZ")"
CURRENT_COMMIT="$(git rev-parse --short HEAD)"

if [[ -n "$(git status --porcelain)" && "$ALLOW_DIRTY" -ne 1 ]]; then
    printf 'promote release: git worktree has uncommitted changes; use --allow-dirty for local validation\n' >&2
    exit 1
fi

for required in "$MANIFEST_PATH" "$CHECKSUMS_PATH" "$RC_NOTES_PATH" "$RC_ARCHIVE_PATH" "$RC_DIR/bin/$BINARY_NAME"; do
    if [[ ! -e "$required" ]]; then
        printf 'promote release: required release-candidate artifact missing: %s\n' "$required" >&2
        exit 1
    fi
done

python3 -m json.tool "$MANIFEST_PATH" >/dev/null
MANIFEST_ROW="$(read_manifest_fields)"
IFS=$'\t' read -r RC_NAME RC_VERSION RC_PLATFORM RC_GATE RC_DIRTY RC_COMMIT RC_BRANCH <<< "$MANIFEST_ROW"

if [[ "$RC_NAME" != "$PACKAGE_NAME" ]]; then
    printf 'promote release: manifest package name mismatch: expected %s, got %s\n' "$PACKAGE_NAME" "$RC_NAME" >&2
    exit 1
fi
if [[ "$RC_VERSION" != "$VERSION" ]]; then
    printf 'promote release: manifest version mismatch: expected %s, got %s\n' "$VERSION" "$RC_VERSION" >&2
    exit 1
fi
if [[ "$RC_PLATFORM" != "$PLATFORM" ]]; then
    printf 'promote release: manifest platform mismatch: expected %s, got %s\n' "$PLATFORM" "$RC_PLATFORM" >&2
    exit 1
fi
if [[ "$RC_GATE" != "passed" && "$ALLOW_SKIPPED_GATE" -ne 1 ]]; then
    printf 'promote release: release candidate did not record a passed gate: %s\n' "$RC_GATE" >&2
    exit 1
fi
if [[ "$RC_DIRTY" == "true" && "$ALLOW_RC_DIRTY" -ne 1 ]]; then
    printf 'promote release: release-candidate manifest is dirty; rebuild from a clean tree or use --allow-rc-dirty for local validation\n' >&2
    exit 1
fi
if [[ "$RC_COMMIT" != "$CURRENT_COMMIT" && "$ALLOW_COMMIT_MISMATCH" -ne 1 ]]; then
    printf 'promote release: current HEAD %s does not match RC commit %s\n' "$CURRENT_COMMIT" "$RC_COMMIT" >&2
    exit 1
fi

(
    cd "$RC_DIR"
    if command -v shasum >/dev/null 2>&1; then
        shasum -a 256 -c checksums.txt
    else
        sha256sum -c checksums.txt
    fi
)

tar -tzf "$RC_ARCHIVE_PATH" >/dev/null
if ! archive_contains "$RC_ARCHIVE_PATH" "$PAYLOAD_ROOT/bin/$BINARY_NAME"; then
    printf 'promote release: archive does not contain expected binary path: %s/bin/%s\n' "$PAYLOAD_ROOT" "$BINARY_NAME" >&2
    exit 1
fi
if ! archive_contains "$RC_ARCHIVE_PATH" "$PAYLOAD_ROOT/manifest.json"; then
    printf 'promote release: archive does not contain expected manifest path: %s/manifest.json\n' "$PAYLOAD_ROOT" >&2
    exit 1
fi

if [[ "$DRY_RUN" -eq 1 ]]; then
    printf '\npromote release: dry run ok\n'
    printf '  Version  -> %s\n' "$VERSION"
    printf '  Platform -> %s\n' "$PLATFORM"
    printf '  Tag      -> %s\n' "$TAG"
    printf '  RC       -> %s\n' "$(relative_to_root "$RC_DIR")"
    exit 0
fi

rm -rf "$RELEASE_DIR"
mkdir -p "$RELEASE_DIR"
cp "$RC_ARCHIVE_PATH" "$PROMOTED_ARCHIVE_PATH"
if [[ -f "$RC_ZIP_PATH" ]]; then
    cp "$RC_ZIP_PATH" "$PROMOTED_ZIP_PATH"
fi
cp "$MANIFEST_PATH" "$PROMOTED_MANIFEST_PATH"
cp "$CHECKSUMS_PATH" "$PROMOTED_CHECKSUMS_PATH"
cp "$RC_NOTES_PATH" "$PROMOTED_RC_NOTES_PATH"
cp install.sh "$RELEASE_DIR/install.sh"
chmod 755 "$RELEASE_DIR/install.sh"
cp scripts/trust_release.sh scripts/trust_release.py scripts/publish_release.sh "$RELEASE_DIR/"
cp scripts/package_channels.sh scripts/package_channels.py "$RELEASE_DIR/"
cp scripts/validate_release.sh scripts/validate_release.py "$RELEASE_DIR/"
cp scripts/verify_hosted_release.sh scripts/verify_hosted_release.py "$RELEASE_DIR/"
chmod 755 "$RELEASE_DIR/trust_release.sh" "$RELEASE_DIR/trust_release.py" "$RELEASE_DIR/publish_release.sh"
chmod 755 "$RELEASE_DIR/package_channels.sh" "$RELEASE_DIR/package_channels.py"
chmod 755 "$RELEASE_DIR/validate_release.sh" "$RELEASE_DIR/validate_release.py"
chmod 755 "$RELEASE_DIR/verify_hosted_release.sh" "$RELEASE_DIR/verify_hosted_release.py"

write_release_notes
write_sha256sums
write_release_json
write_latest_json
maybe_create_tag

if [[ "$SKIP_TRUST" -ne 1 ]]; then
    TRUST_ARGS=(generate --version "$VERSION" --platform "$PLATFORM" --dist-dir "$DIST_DIR")
    if [[ -n "$SIGN_KEY" ]]; then
        TRUST_ARGS+=(--sign-key "$SIGN_KEY")
    fi
    bash scripts/trust_release.sh "${TRUST_ARGS[@]}"
    if [[ -n "$VERIFY_KEY" ]]; then
        bash scripts/trust_release.sh verify --version "$VERSION" --platform "$PLATFORM" --dist-dir "$DIST_DIR" --verify-key "$VERIFY_KEY"
    else
        bash scripts/trust_release.sh verify --version "$VERSION" --platform "$PLATFORM" --dist-dir "$DIST_DIR"
    fi
fi

printf '\npromote release: ok\n'
printf '  Release dir -> %s\n' "$(relative_to_root "$RELEASE_DIR")"
printf '  Archive     -> %s\n' "$(relative_to_root "$PROMOTED_ARCHIVE_PATH")"
printf '  Manifest    -> %s\n' "$(relative_to_root "$RELEASE_JSON_PATH")"
printf '  Checksums   -> %s\n' "$(relative_to_root "$RELEASE_SUMS_PATH")"
printf '  Latest      -> %s\n' "$(relative_to_root "$LATEST_RELEASE_PATH")"
if [[ "$CREATE_TAG" -eq 1 ]]; then
    printf '  Tag         -> %s\n' "$TAG"
else
    printf '  Tag         -> %s (not created; use --create-tag)\n' "$TAG"
fi
