---
name: Release hotfix
about: Track a regression or broken asset after a Cool public release.
title: "Hotfix vX.Y.Z: <summary>"
labels: release, hotfix
assignees: ""
---

## Impact

- Affected release:
- Affected platform(s):
- User-visible symptom:
- Workaround:

## Evidence

- Failing command:
- Failing workflow run:
- Broken asset or checksum:
- First known bad version:
- Last known good version:

## Fix Plan

- [ ] Reproduce from the hosted release URL.
- [ ] Add or update a regression test.
- [ ] Build and validate the hotfix candidate.
- [ ] Publish replacement assets or a superseding patch release.
- [ ] Run `Hosted Release Verify` against the final tag.

## Communication

- [ ] Update release notes with the hotfix status.
- [ ] Update install/support docs if the workaround changes.
- [ ] Close this issue with the fixed release link.
