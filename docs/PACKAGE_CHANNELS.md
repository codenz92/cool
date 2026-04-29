# Package Channels

Cool package-channel metadata is generated from promoted release artifacts. The
generator scans `dist/releases/<version>/` for platform archives and writes
channel outputs under `dist/channels/<version>/`.

## Generate Channels

```bash
bash scripts/package_channels.sh generate --version 1.0.0
```

Generated outputs include:

- `channels.json` with platform assets, URLs, hashes, and channel paths
- `CHANNEL_SHA256SUMS`
- `homebrew/cool.rb`
- `winget/Codenz.Cool/<version>/...` when a Windows zip asset is present
- `apt/...` and a `.deb` package when a Linux x86_64 tarball is present
- `dist/channels/cool-<version>-package-channels.tar.gz`
- `dist/channels/latest.json`

## Required Platforms

Use required-platform checks in CI to prevent publishing partial channels:

```bash
bash scripts/package_channels.sh generate \
  --version 1.0.0 \
  --require-platform linux-x86_64 \
  --require-platform macos-x86_64 \
  --require-platform macos-arm64 \
  --require-platform windows-x86_64
```

## Matrix Assembly

The release matrix workflow builds one artifact set per platform, then assembles
them into a single release directory:

```bash
bash scripts/assemble_matrix_release.sh \
  --source-dir dist/matrix-input \
  --version 1.0.0
```

After assembly, run trust and channel generation:

```bash
bash scripts/trust_release.sh generate --version 1.0.0 --platform multi
bash scripts/trust_release.sh verify --version 1.0.0 --platform multi
bash scripts/package_channels.sh generate --version 1.0.0
```

Then validate the release and package channels before publishing:

```bash
bash scripts/validate_release.sh \
  --version 1.0.0 \
  --platform multi \
  --require-trust \
  --require-channels \
  --require-platform linux-x86_64 \
  --require-platform macos-x86_64 \
  --require-platform macos-arm64 \
  --require-platform windows-x86_64
```

## Channel Notes

Homebrew uses platform tarballs. Winget uses the Windows zip archive and the
portable `bin/cool.exe` nested installer path. Debian/apt metadata is generated
from the Linux x86_64 tarball into a simple `.deb` plus `Packages` indexes.

See `docs/RELEASE_VALIDATION.md` for the full pre-publish validation checklist
and synthetic matrix smoke test.
