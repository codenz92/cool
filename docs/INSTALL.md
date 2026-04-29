# Installing Cool

Cool release artifacts are promoted from a validated release candidate. A
promoted release directory contains the platform tarball, `release.json`,
`SHA256SUMS`, release notes, and a copy of `install.sh`.

## Local Artifact

Use this path when testing a release before uploading it:

```bash
bash install.sh \
  --from dist/releases/1.0.0/cool-1.0.0-macos-arm64.tar.gz \
  --prefix "$HOME/.local"
```

The installer extracts the payload under `$HOME/.local/lib/cool/` and symlinks
`cool` into `$HOME/.local/bin/cool`.

## Hosted Release

After assets are uploaded to a GitHub release, install by version:

```bash
curl -fsSL https://raw.githubusercontent.com/codenz92/cool-lang/master/install.sh \
  | bash -s -- --version 1.0.0 --prefix "$HOME/.local"
```

The default download base is:

```text
https://github.com/codenz92/cool-lang/releases/download
```

Override it for mirrors or internal channels:

```bash
bash install.sh --version 1.0.0 --base-url https://example.invalid/cool/releases/download
```

## Checksum Verification

Every promoted release writes `SHA256SUMS`. Verify the archive before installing
or pass the expected archive hash to the installer:

```bash
shasum -a 256 -c SHA256SUMS
bash install.sh \
  --from cool-1.0.0-macos-arm64.tar.gz \
  --verify-sha256 "<archive-sha256>"
```

## Smoke Test

By default the installer runs:

```bash
cool help
```

Use `--no-smoke` only when installing into an environment where the binary cannot
be executed during setup.
