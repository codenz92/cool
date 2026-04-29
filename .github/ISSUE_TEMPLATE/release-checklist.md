---
name: Release checklist
about: Track a Cool public release from candidate build through hosted verification.
title: "Release vX.Y.Z"
labels: release
assignees: ""
---

## Release

- Version:
- Tag:
- Release owner:
- Target date:

## Pre-Release Gate

- [ ] `bash scripts/release_gate.sh`
- [ ] `bash scripts/release_candidate.sh --require-clean --version <version>`
- [ ] `bash scripts/promote_release.sh --version <version>`
- [ ] `bash scripts/package_channels.sh generate --version <version>`
- [ ] `bash scripts/validate_release.sh --version <version> --require-trust --require-channels --install-smoke`

## Matrix And Publishing

- [ ] Dispatch `Release Matrix` for the version or push `v<version>`.
- [ ] Confirm Linux, macOS Intel, macOS Arm, and Windows artifacts are present.
- [ ] Confirm `release-validation.json` passed and was uploaded.
- [ ] Confirm package-channel archive is uploaded.
- [ ] Publish or promote the GitHub Release from draft.

## Post-Release Verification

- [ ] `Hosted Release Verify` workflow passed.
- [ ] `bash scripts/verify_hosted_release.sh --version <version> --platform multi --require-trust --check-channel-archive --install-smoke --install-smoke-platform linux-x86_64`
- [ ] Install docs and support matrix still match the uploaded assets.
- [ ] Release notes include hashes, trust metadata, and package-channel instructions.

## Rollback / Follow-Up

- [ ] If verification fails, mark the release as draft or delete the broken assets.
- [ ] Open a hotfix issue if the tag must be superseded.
- [ ] Record final links to the release, validation report, and hosted verification report.
