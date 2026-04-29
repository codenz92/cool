#!/usr/bin/env python3
import argparse
import hashlib
import json
import shutil
import subprocess
import tarfile
import tempfile
import urllib.error
import urllib.parse
import urllib.request
from datetime import datetime, timezone
from pathlib import Path

import validate_release as local_validation


ROOT = Path(__file__).resolve().parents[1]


def fail(message):
    raise SystemExit(f"hosted release verification: {message}")


def write_json(path, data):
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(data, indent=2, sort_keys=True) + "\n", encoding="utf-8")


def display_path(path):
    try:
        return path.relative_to(ROOT).as_posix()
    except ValueError:
        return path.as_posix()


def safe_relpath(value):
    if not value:
        fail("empty asset path")
    rel = Path(value)
    if value.startswith("/") or ".." in rel.parts:
        fail(f"unsafe asset path: {value}")
    return rel.as_posix()


def quoted_relpath(value):
    return "/".join(urllib.parse.quote(part) for part in value.split("/"))


def asset_url(base_url, tag, rel):
    return f"{base_url.rstrip('/')}/{urllib.parse.quote(tag)}/{quoted_relpath(rel)}"


def is_http_url(url):
    return url.startswith("http://") or url.startswith("https://")


def sha256_bytes(data):
    return hashlib.sha256(data).hexdigest()


def download(url, output, required=True):
    output.parent.mkdir(parents=True, exist_ok=True)
    if output.exists():
        output.unlink()
    try:
        with urllib.request.urlopen(url) as response, output.open("wb") as fh:
            shutil.copyfileobj(response, fh)
    except urllib.error.HTTPError as exc:
        if not required and exc.code == 404:
            return None
        fail(f"download failed ({exc.code}) for {url}")
    except urllib.error.URLError as exc:
        if not required:
            return None
        fail(f"download failed for {url}: {exc.reason}")
    except OSError as exc:
        if not required:
            return None
        fail(f"download failed for {url}: {exc}")
    return output


def load_json(path):
    try:
        return json.loads(path.read_text(encoding="utf-8"))
    except json.JSONDecodeError as exc:
        fail(f"invalid JSON in {path}: {exc}")


def parse_sha256sums_text(text, source):
    entries = []
    for line_no, line in enumerate(text.splitlines(), 1):
        if not line.strip():
            continue
        parts = line.split(None, 1)
        if len(parts) != 2:
            fail(f"invalid checksum line in {source}: {line_no}")
        digest, rel = parts
        rel = rel.strip()
        if rel.startswith("*"):
            rel = rel[1:]
        safe_relpath(rel)
        if len(digest) != 64 or any(ch not in "0123456789abcdefABCDEF" for ch in digest):
            fail(f"invalid SHA-256 digest in {source}: {line_no}")
        entries.append((digest.lower(), rel))
    if not entries:
        fail(f"checksum file is empty: {source}")
    return entries


def verify_sha256sums(path, base):
    count = 0
    entries = parse_sha256sums_text(path.read_text(encoding="utf-8"), path.name)
    for expected, rel in entries:
        target = base / rel
        if not target.is_file():
            fail(f"checksum target missing: {rel}")
        actual = local_validation.sha256_path(target)
        if actual != expected:
            fail(f"checksum mismatch for {rel}: expected {expected}, got {actual}")
        count += 1
    return count


def verify_release_metadata(download_dir, release_json, latest_json, package_name, version, tag, platform_name, required_platforms):
    package = release_json.get("package", {})
    if package.get("name") != package_name or package.get("version") != version:
        fail("release.json package metadata mismatch")

    release = release_json.get("release", {})
    if release.get("tag") != tag:
        fail(f"release tag mismatch: expected {tag}, got {release.get('tag')}")
    if platform_name and release.get("platform") != platform_name:
        fail(f"release platform mismatch: expected {platform_name}, got {release.get('platform')}")

    assets = local_validation.scan_assets(download_dir, package_name, version)
    if not assets:
        fail("no hosted release archives were downloaded")

    release_platforms = set(release.get("platforms") or [release.get("platform")])
    for required in required_platforms:
        if required not in assets:
            fail(f"required hosted platform missing archive asset: {required}")
        if release.get("platform") == "multi" and required not in release_platforms:
            fail(f"required hosted platform missing from release.json: {required}")

    seen = set()
    for record in release_json.get("assets", []):
        rel = safe_relpath(record.get("path", ""))
        seen.add(rel)
        target = download_dir / rel
        if not target.is_file():
            fail(f"release.json asset was not downloaded: {rel}")
        if local_validation.sha256_path(target) != record.get("sha256"):
            fail(f"release.json asset hash mismatch: {rel}")
        if target.stat().st_size != record.get("bytes"):
            fail(f"release.json asset size mismatch: {rel}")
    if not seen:
        fail("release.json has no asset records")

    record = latest_json.get("release_json", {})
    release_json_path = download_dir / "release.json"
    if record.get("sha256") != local_validation.sha256_path(release_json_path):
        fail("latest.json release_json hash mismatch")
    if record.get("bytes") != release_json_path.stat().st_size:
        fail("latest.json release_json size mismatch")

    return assets


def verify_trust(download_dir, release_json, platform_name, verify_key):
    for rel in ("sbom.spdx.json", "provenance.intoto.json", "trust.json", "TRUST_SHA256SUMS"):
        if not (download_dir / rel).is_file():
            fail(f"required trust artifact missing: {rel}")
    trust_count = verify_sha256sums(download_dir / "TRUST_SHA256SUMS", download_dir)
    for rel in ("sbom.spdx.json", "provenance.intoto.json", "trust.json"):
        load_json(download_dir / rel)

    trust_json = load_json(download_dir / "trust.json")
    if platform_name and trust_json.get("release", {}).get("platform") != platform_name:
        fail("trust.json platform mismatch")

    supply_chain = release_json.get("supply_chain", {})
    for key in ("sbom", "provenance"):
        record = supply_chain.get(key)
        if not record:
            fail(f"release.json missing supply_chain.{key}")
        rel = safe_relpath(record.get("path", ""))
        target = download_dir / rel
        if not target.is_file():
            fail(f"supply_chain.{key} target missing: {rel}")
        if local_validation.sha256_path(target) != record.get("sha256"):
            fail(f"supply_chain.{key} hash mismatch")

    if verify_key:
        key_path = Path(verify_key).resolve()
        if not key_path.is_file():
            fail(f"verification key not found: {key_path}")
        for rel in ("SHA256SUMS.sig", "release.json.sig"):
            if not (download_dir / rel).is_file():
                fail(f"required signature missing: {rel}")
        for sig in sorted(download_dir.glob("*.sig")):
            target = download_dir / sig.name[:-4]
            if not target.is_file():
                fail(f"signature target missing: {target.name}")
            subprocess.run(
                ["openssl", "dgst", "-sha256", "-verify", str(key_path), "-signature", str(sig), str(target)],
                cwd=ROOT,
                check=True,
                stdout=subprocess.DEVNULL,
            )
    return trust_count


def validate_channel_archive(path, package_name, version, required_platforms):
    payload_root = f"{package_name}-{version}-package-channels"
    files = {}
    with tarfile.open(path, "r:gz") as archive:
        for member in archive.getmembers():
            local_validation.ensure_safe_member(member.name, path)
            if not member.isfile():
                continue
            if not (member.name == payload_root or member.name.startswith(f"{payload_root}/")):
                fail(f"channel archive has unexpected payload root: {member.name}")
            fh = archive.extractfile(member)
            if fh is None:
                fail(f"could not read channel archive member: {member.name}")
            rel = Path(member.name).relative_to(payload_root).as_posix()
            files[rel] = fh.read()

    for rel in ("channels.json", "CHANNEL_SHA256SUMS"):
        if rel not in files:
            fail(f"channel archive missing {rel}")

    channels = json.loads(files["channels.json"].decode("utf-8"))
    if channels.get("package", {}).get("name") != package_name or channels.get("package", {}).get("version") != version:
        fail("channels.json package metadata mismatch")

    channel_platforms = {record.get("platform") for record in channels.get("platforms", [])}
    for required in required_platforms:
        if required not in channel_platforms:
            fail(f"required platform missing from hosted channels.json: {required}")

    entries = parse_sha256sums_text(files["CHANNEL_SHA256SUMS"].decode("utf-8"), "CHANNEL_SHA256SUMS")
    for expected, rel in entries:
        if rel not in files:
            fail(f"channel checksum target missing: {rel}")
        actual = sha256_bytes(files[rel])
        if actual != expected:
            fail(f"channel checksum mismatch for {rel}: expected {expected}, got {actual}")
    return len(entries), len(channel_platforms)


def choose_smoke_asset(assets, smoke_platform, smoke_kind):
    platform_name = smoke_platform or local_validation.default_platform()
    kinds = assets.get(platform_name)
    if not kinds:
        fail(f"install smoke platform asset missing: {platform_name}")
    if smoke_kind == "auto":
        preferred = "zip" if platform_name.startswith("windows-") else "tar.gz"
        kind = preferred if preferred in kinds else ("tar.gz" if "tar.gz" in kinds else "zip")
    else:
        kind = smoke_kind
    asset = kinds.get(kind)
    if not asset:
        fail(f"install smoke asset missing: {platform_name} {kind}")
    return platform_name, kind, asset


def run_install_smoke(download_dir, version, assets, asset_urls, base_url, tag, smoke_platform, smoke_kind, verify_key):
    platform_name, kind, asset = choose_smoke_asset(assets, smoke_platform, smoke_kind)
    with tempfile.TemporaryDirectory(prefix="cool-hosted-install.") as tmp:
        prefix = Path(tmp) / "prefix"
        asset_download_url = asset_urls.get(asset.filename)
        checksums_url = asset_url(base_url, tag, "SHA256SUMS")
        cmd = [
            "bash",
            str(ROOT / "install.sh"),
            "--version",
            version,
            "--platform",
            platform_name,
            "--prefix",
            str(prefix),
        ]
        if asset_download_url and is_http_url(asset_download_url) and is_http_url(checksums_url):
            cmd.extend(["--url", asset_download_url, "--checksums", checksums_url])
            if verify_key:
                cmd.extend(["--checksums-signature", asset_url(base_url, tag, "SHA256SUMS.sig"), "--verify-key", verify_key])
        else:
            cmd.extend(["--from", str(asset.path), "--checksums", str(download_dir / "SHA256SUMS")])
            if verify_key and (download_dir / "SHA256SUMS.sig").is_file():
                cmd.extend(["--checksums-signature", str(download_dir / "SHA256SUMS.sig"), "--verify-key", verify_key])
        subprocess.run(cmd, cwd=ROOT, check=True)
    return f"{platform_name}:{kind}"


def collect_release_assets(download_dir, base_url, tag, release_json, require_trust, verify_key):
    downloaded = {}

    def fetch(rel, required=True):
        rel = safe_relpath(rel)
        url = asset_url(base_url, tag, rel)
        path = download(url, download_dir / rel, required=required)
        if path:
            downloaded[rel] = url
        return path

    for rel in ("release.json", "latest.json", "SHA256SUMS"):
        fetch(rel)

    for record in release_json.get("assets", []):
        fetch(record.get("path", ""))

    if require_trust:
        for rel in ("sbom.spdx.json", "provenance.intoto.json", "trust.json", "TRUST_SHA256SUMS"):
            fetch(rel)
    for rel in ("SHA256SUMS.sig", "release.json.sig", "sbom.spdx.json.sig", "provenance.intoto.json.sig", "trust.json.sig"):
        fetch(rel, required=bool(verify_key and rel in {"SHA256SUMS.sig", "release.json.sig"}))
    return downloaded


def main():
    parser = argparse.ArgumentParser(description="Verify a published Cool release from hosted download URLs.")
    parser.add_argument("--version")
    parser.add_argument("--tag")
    parser.add_argument("--platform", help="Expected release platform, for example macos-arm64 or multi.")
    parser.add_argument("--base-url", default="https://github.com/codenz92/cool-lang/releases/download")
    parser.add_argument("--download-dir", help="Directory used for downloaded assets. Defaults to a temporary directory.")
    parser.add_argument("--keep", action="store_true", help="Keep the temporary download directory.")
    parser.add_argument("--require-platform", action="append", default=[])
    parser.add_argument("--require-trust", action="store_true")
    parser.add_argument("--verify-key")
    parser.add_argument("--check-channel-archive", action="store_true")
    parser.add_argument("--install-smoke", action="store_true")
    parser.add_argument("--install-smoke-platform")
    parser.add_argument("--install-smoke-kind", choices=["auto", "tar.gz", "zip"], default="auto")
    parser.add_argument("--report", help="Write a JSON verification report to this path.")
    args = parser.parse_args()

    package_name = local_validation.read_cargo_value("name")
    version = args.version or local_validation.read_cargo_value("version")
    tag = args.tag or f"v{version}"

    if args.download_dir:
        temp_ctx = None
        download_dir = Path(args.download_dir).resolve()
        download_dir.mkdir(parents=True, exist_ok=True)
    elif args.keep:
        temp_ctx = None
        download_dir = Path(tempfile.mkdtemp(prefix="cool-hosted-release."))
    else:
        temp_ctx = tempfile.TemporaryDirectory(prefix="cool-hosted-release.")
        download_dir = Path(temp_ctx.name)

    try:
        for rel in ("release.json", "latest.json", "SHA256SUMS"):
            download(asset_url(args.base_url, tag, rel), download_dir / rel)
        release_json = load_json(download_dir / "release.json")
        latest_json = load_json(download_dir / "latest.json")
        asset_urls = collect_release_assets(download_dir, args.base_url, tag, release_json, args.require_trust, args.verify_key)

        sums_count = verify_sha256sums(download_dir / "SHA256SUMS", download_dir)
        assets = verify_release_metadata(
            download_dir,
            release_json,
            latest_json,
            package_name,
            version,
            tag,
            args.platform,
            args.require_platform,
        )
        local_validation.validate_sidecars(download_dir, package_name, version, assets)

        archive_file_count = 0
        for platform_assets in assets.values():
            for asset in platform_assets.values():
                archive_file_count += local_validation.validate_archive(asset, package_name, version)

        trust_count = None
        if args.require_trust:
            trust_count = verify_trust(download_dir, release_json, args.platform, args.verify_key)

        channel_summary = None
        if args.check_channel_archive:
            channel_name = f"{package_name}-{version}-package-channels.tar.gz"
            channel_path = download(asset_url(args.base_url, tag, channel_name), download_dir / channel_name)
            channel_summary = validate_channel_archive(channel_path, package_name, version, args.require_platform)

        smoke_result = None
        if args.install_smoke:
            smoke_result = run_install_smoke(
                download_dir,
                version,
                assets,
                asset_urls,
                args.base_url,
                tag,
                args.install_smoke_platform,
                args.install_smoke_kind,
                args.verify_key,
            )

        report = {
            "schema_version": 1,
            "generated_at": datetime.now(timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ"),
            "package": {"name": package_name, "version": version},
            "hosted_release": {
                "base_url": args.base_url,
                "tag": tag,
                "download_dir": display_path(download_dir),
                "platform": release_json.get("release", {}).get("platform"),
                "platforms": sorted(assets.keys()),
                "asset_count": sum(len(kinds) for kinds in assets.values()),
                "sha256sums_entries": sums_count,
                "archive_payload_files_checked": archive_file_count,
            },
            "trust": {"required": args.require_trust, "sha256sums_entries": trust_count},
            "channel_archive": {
                "required": args.check_channel_archive,
                "sha256sums_entries": channel_summary[0] if channel_summary else None,
                "platform_count": channel_summary[1] if channel_summary else None,
            },
            "install_smoke": smoke_result,
        }
        if args.report:
            write_json(Path(args.report), report)

        print("hosted release verification: ok")
        print(f"  Version  -> {version}")
        print(f"  Tag      -> {tag}")
        print(f"  Base URL -> {args.base_url.rstrip('/')}")
        print(f"  Platform -> {release_json.get('release', {}).get('platform')}")
        print(f"  Assets   -> {sum(len(kinds) for kinds in assets.values())} archive(s)")
        if args.check_channel_archive:
            print(f"  Channels -> {package_name}-{version}-package-channels.tar.gz")
        if smoke_result:
            print(f"  Install  -> {smoke_result}")
        if args.report:
            print(f"  Report   -> {display_path(Path(args.report).resolve())}")
        if args.keep or args.download_dir:
            print(f"  Download -> {display_path(download_dir)}")
    finally:
        if temp_ctx is not None:
            temp_ctx.cleanup()


if __name__ == "__main__":
    try:
        main()
    except subprocess.CalledProcessError as exc:
        fail(f"command failed with exit code {exc.returncode}: {' '.join(map(str, exc.cmd))}")
