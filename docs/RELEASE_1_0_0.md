# Cool 1.0.0 Release Record

Release date: 2026-04-30

## Published Artifact

- Git tag: `v1.0.0`
- Tag object: `9eddd4ebfc2c3f0beb6a1a5a6cbf3fa7ea40d7da`
- Target commit: `03d66a6d0fdfe80bd17acb2775aa0d5ec1252753`
- GitHub Release: https://github.com/codenz92/cool-lang/releases/tag/v1.0.0
- Release state: public, non-draft, non-prerelease

## Release Execution Evidence

| Check | Result | Evidence |
| ----- | ------ | -------- |
| Branch release gate | Passed | https://github.com/codenz92/cool-lang/actions/runs/25146479767 |
| Branch release validation | Passed | https://github.com/codenz92/cool-lang/actions/runs/25146479790 |
| Non-publishing release matrix dry-run | Passed | https://github.com/codenz92/cool-lang/actions/runs/25146596781 |
| Publishing release matrix | Passed | https://github.com/codenz92/cool-lang/actions/runs/25147009306 |
| Tag release candidate workflow | Passed | https://github.com/codenz92/cool-lang/actions/runs/25147009316 |
| Tag release promotion workflow | Passed | https://github.com/codenz92/cool-lang/actions/runs/25147009304 |
| Hosted public release verification | Passed | `dist/hosted-validation/1.0.0/public-hosted-release-validation.json` |
| Public installer audit | Passed | `install.sh --version 1.0.0 --platform macos-arm64 --verify-metadata` and installed `cool help` |

## Platform Matrix

| Platform | Publishing Job |
| -------- | -------------- |
| Linux x86_64 | https://github.com/codenz92/cool-lang/actions/runs/25147009306/job/73708956027 |
| macOS x86_64 | https://github.com/codenz92/cool-lang/actions/runs/25147009306/job/73708956017 |
| macOS arm64 | https://github.com/codenz92/cool-lang/actions/runs/25147009306/job/73708956020 |
| Windows x86_64 | https://github.com/codenz92/cool-lang/actions/runs/25147009306/job/73708956016 |
| Multi-platform assemble/publish | https://github.com/codenz92/cool-lang/actions/runs/25147009306/job/73710219376 |

## Final Release Fixes

- Removed the macOS Intel LLVM backend crash by routing dynamic Cool method and closure calls through generated argv dispatch wrappers instead of ABI-sensitive fixed C function pointer casts.
- Added native regression coverage for Phase 6 pass3 user-module imports and entrypoints.
- Added native regression coverage for dynamic method, closure, and constructor calls with omitted default arguments.
- Updated built-in `collections` method tables to use the same argv dispatch ABI as generated classes.
- Preserved Darwin/Mach-O safety by rejecting x86 port I/O builtins on Darwin targets with explicit diagnostics.

## Hosted Verification Command

```bash
bash scripts/verify_hosted_release.sh \
  --version 1.0.0 \
  --platform multi \
  --require-trust \
  --require-platform linux-x86_64 \
  --require-platform macos-x86_64 \
  --require-platform macos-arm64 \
  --require-platform windows-x86_64 \
  --check-channel-archive \
  --install-smoke \
  --install-smoke-platform macos-arm64 \
  --report dist/hosted-validation/1.0.0/public-hosted-release-validation.json
```

## Notes

Several earlier `v1.0.0` tag matrix attempts were cancelled or failed while hardening the release workflow and LLVM backend across Linux, Windows, macOS arm64, and macOS Intel. The final published tag points to `03d66a6d0fdfe80bd17acb2775aa0d5ec1252753`; both a non-publishing matrix dry-run and the tag-triggered publishing matrix passed on that commit.
