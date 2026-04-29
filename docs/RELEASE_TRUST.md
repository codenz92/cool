# Release Trust

Cool release trust metadata is generated after a release candidate is promoted.
The trust layer is intentionally file-based so it can be uploaded to GitHub
Releases, mirrored, or verified offline.

## Generate

```bash
bash scripts/trust_release.sh generate --version 1.0.0
```

This validates the promoted `SHA256SUMS` file and writes:

- `sbom.spdx.json`
- `provenance.intoto.json`
- `trust.json`
- `TRUST_SHA256SUMS`

`scripts/promote_release.sh` runs this by default after promotion. Use
`--skip-trust` only when debugging the release layout.

## Sign

If an OpenSSL private key is available, sign the trust-critical metadata:

```bash
bash scripts/trust_release.sh generate \
  --version 1.0.0 \
  --sign-key release-signing-key.pem
```

The script emits detached signatures for:

- `SHA256SUMS`
- `release.json`
- `provenance.intoto.json`
- `sbom.spdx.json`
- `trust.json`

The GitHub publishing workflow can consume a base64-encoded private key from the
`COOL_RELEASE_SIGNING_KEY_B64` secret. If the secret is absent, the workflow
still publishes verifiable hash, SBOM, and provenance metadata, but no detached
signatures are emitted.

## Verify

Verify unsigned hash/provenance metadata:

```bash
bash scripts/trust_release.sh verify --version 1.0.0
```

Verify detached signatures as well:

```bash
bash scripts/trust_release.sh verify \
  --version 1.0.0 \
  --verify-key release-signing-public.pem
```

## Publish

Dry-run a publish:

```bash
bash scripts/publish_release.sh --version 1.0.0
```

Publish with the GitHub CLI:

```bash
bash scripts/publish_release.sh --version 1.0.0 --publish --no-draft
```

The script verifies the trust metadata before uploading assets with `gh`.
