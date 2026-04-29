#!/usr/bin/env python3
import argparse
import hashlib
import json
import re
import shutil
import subprocess
from datetime import datetime, timezone
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]


def fail(message):
    raise SystemExit(f"assemble matrix release: {message}")


def sha256_path(path):
    h = hashlib.sha256()
    with open(path, "rb") as fh:
        for chunk in iter(lambda: fh.read(1024 * 1024), b""):
            h.update(chunk)
    return h.hexdigest()


def file_size(path):
    return path.stat().st_size


def display_path(path):
    try:
        return path.relative_to(ROOT).as_posix()
    except ValueError:
        return path.as_posix()


def read_cargo_value(key):
    text = (ROOT / "Cargo.toml").read_text(encoding="utf-8")
    match = re.search(rf"(?m)^\s*{re.escape(key)}\s*=\s*\"([^\"]+)\"", text)
    if not match:
        fail(f"could not read {key} from Cargo.toml")
    return match.group(1)


def git_output(*args):
    try:
        return subprocess.check_output(["git", *args], cwd=ROOT, text=True).strip()
    except subprocess.CalledProcessError:
        return "unknown"


def copy_unique(src, dst):
    dst.parent.mkdir(parents=True, exist_ok=True)
    if dst.exists():
        if sha256_path(src) != sha256_path(dst):
            fail(f"conflicting artifact for {dst.name}")
        return
    shutil.copy2(src, dst)


def asset_entries(release_dir):
    entries = []
    for path in sorted(release_dir.iterdir()):
        if not path.is_file():
            continue
        if path.name in {"release.json", "latest.json", "SHA256SUMS", "TRUST_SHA256SUMS"}:
            continue
        entries.append({
            "path": path.name,
            "sha256": sha256_path(path),
            "bytes": file_size(path),
        })
    return entries


def write_json(path, data):
    path.write_text(json.dumps(data, indent=2, sort_keys=True) + "\n", encoding="utf-8")


def write_sha256sums(release_dir):
    lines = []
    for path in sorted(release_dir.iterdir()):
        if path.is_file() and path.name not in {"release.json", "latest.json", "SHA256SUMS", "TRUST_SHA256SUMS"}:
            lines.append(f"{sha256_path(path)}  {path.name}")
    (release_dir / "SHA256SUMS").write_text("\n".join(lines) + "\n", encoding="utf-8")


def write_release_notes(release_dir, package_name, version, tag, platforms, generated_at):
    lines = [
        f"# Cool {version}",
        "",
        f"Tag: {tag}",
        f"Commit: {git_output('rev-parse', '--short', 'HEAD')}",
        f"Platforms: {', '.join(platforms)}",
        f"Promoted: {generated_at}",
        "",
        "## Assets",
        "",
    ]
    for path in sorted(release_dir.glob(f"{package_name}-{version}-*")):
        if path.is_file():
            lines.append(f"- `{path.name}` - SHA-256 `{sha256_path(path)}`")
    lines += [
        "",
        "## Install",
        "",
        "Use the root `install.sh` script with the matching platform archive, or use a package-channel manifest generated under `dist/channels/<version>/`.",
        "",
        "```bash",
        f"bash install.sh --version {version} --verify-metadata",
        "```",
        "",
        "This multi-platform release was assembled from per-platform release-candidate artifacts produced by the release matrix workflow.",
        "",
    ]
    (release_dir / "RELEASE.md").write_text("\n".join(lines), encoding="utf-8")


def main():
    parser = argparse.ArgumentParser(description="Assemble per-platform release artifacts into one release directory.")
    parser.add_argument("--source-dir", required=True)
    parser.add_argument("--version")
    parser.add_argument("--dist-dir", default=str(ROOT / "dist"))
    parser.add_argument("--tag")
    parser.add_argument("--base-url", default="https://github.com/codenz92/cool-lang/releases/download")
    args = parser.parse_args()

    package_name = read_cargo_value("name")
    version = args.version or read_cargo_value("version")
    tag = args.tag or f"v{version}"
    source_dir = Path(args.source_dir).resolve()
    dist_dir = Path(args.dist_dir).resolve()
    release_dir = dist_dir / "releases" / version
    if not source_dir.is_dir():
        fail(f"source directory not found: {source_dir}")

    if release_dir.exists():
        shutil.rmtree(release_dir)
    release_dir.mkdir(parents=True, exist_ok=True)

    artifact_pattern = re.compile(rf"^{re.escape(package_name)}-{re.escape(version)}-(.+)\.(tar\.gz|zip|manifest\.json|checksums\.txt|RC_NOTES\.md)$")
    platforms = set()
    copied = 0
    for src in sorted(source_dir.rglob("*")):
        if not src.is_file():
            continue
        match = artifact_pattern.match(src.name)
        if match:
            platforms.add(match.group(1))
            copy_unique(src, release_dir / src.name)
            copied += 1

    for rel in [
        "install.sh",
        "trust_release.sh",
        "trust_release.py",
        "publish_release.sh",
        "package_channels.sh",
        "package_channels.py",
        "validate_release.sh",
        "validate_release.py",
        "verify_hosted_release.sh",
        "verify_hosted_release.py",
    ]:
        src = ROOT / ("scripts" if rel != "install.sh" else "") / rel
        if not src.is_file():
            continue
        copy_unique(src, release_dir / rel)

    if not platforms:
        fail(f"no platform artifacts found below {source_dir}")

    generated_at = datetime.now(timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ")
    write_release_notes(release_dir, package_name, version, tag, sorted(platforms), generated_at)
    write_sha256sums(release_dir)

    release_json = {
        "schema_version": 1,
        "package": {"name": package_name, "version": version},
        "release": {
            "tag": tag,
            "platform": "multi",
            "platforms": sorted(platforms),
            "generated_at": generated_at,
            "release_gate": "passed",
        },
        "git": {
            "commit": git_output("rev-parse", "--short", "HEAD"),
            "branch": git_output("branch", "--show-current") or "detached",
            "dirty": bool(git_output("status", "--porcelain")),
        },
        "install": {
            "base_url": args.base_url,
            "script": "install.sh",
        },
        "assets": asset_entries(release_dir),
    }
    release_json_path = release_dir / "release.json"
    write_json(release_json_path, release_json)

    latest = {
        "schema_version": 1,
        "package": {"name": package_name, "version": version},
        "tag": tag,
        "platform": "multi",
        "platforms": sorted(platforms),
        "generated_at": generated_at,
        "release_json": {
            "path": display_path(release_json_path),
            "sha256": sha256_path(release_json_path),
            "bytes": file_size(release_json_path),
        },
    }
    write_json(release_dir / "latest.json", latest)
    (dist_dir / "releases").mkdir(parents=True, exist_ok=True)
    write_json(dist_dir / "releases" / "latest.json", latest)

    print("assemble matrix release: ok")
    print(f"  Release  -> {display_path(release_dir)}")
    print(f"  Platforms -> {', '.join(sorted(platforms))}")
    print(f"  Artifacts -> {copied}")


if __name__ == "__main__":
    main()
