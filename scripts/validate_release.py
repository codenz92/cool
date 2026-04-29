#!/usr/bin/env python3
import argparse
import gzip
import hashlib
import io
import json
import platform
import re
import subprocess
import sys
import tarfile
import tempfile
import zipfile
from dataclasses import dataclass
from datetime import datetime, timezone
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]


def fail(message):
    raise SystemExit(f"release validation: {message}")


def sha256_path(path):
    h = hashlib.sha256()
    with open(path, "rb") as fh:
        for chunk in iter(lambda: fh.read(1024 * 1024), b""):
            h.update(chunk)
    return h.hexdigest()


def file_size(path):
    return path.stat().st_size


def load_json(path):
    try:
        with open(path, "r", encoding="utf-8") as fh:
            return json.load(fh)
    except json.JSONDecodeError as exc:
        fail(f"invalid JSON in {path}: {exc}")


def write_json(path, data):
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(data, indent=2, sort_keys=True) + "\n", encoding="utf-8")


def read_cargo_value(key):
    text = (ROOT / "Cargo.toml").read_text(encoding="utf-8")
    match = re.search(rf"(?m)^\s*{re.escape(key)}\s*=\s*\"([^\"]+)\"", text)
    if not match:
        fail(f"could not read {key} from Cargo.toml")
    return match.group(1)


def default_platform():
    system = platform.system().lower()
    machine = platform.machine().lower()
    if system == "darwin":
        system = "macos"
    elif system.startswith(("mingw", "msys", "cygwin")):
        system = "windows"
    if machine in ("aarch64", "arm64"):
        machine = "arm64"
    elif machine in ("amd64", "x86_64"):
        machine = "x86_64"
    return f"{system}-{machine}"


def display_path(path):
    try:
        return path.relative_to(ROOT).as_posix()
    except ValueError:
        return path.as_posix()


def parse_sha256sums(path):
    entries = []
    for line_no, line in enumerate(path.read_text(encoding="utf-8").splitlines(), 1):
        if not line.strip():
            continue
        parts = line.split(None, 1)
        if len(parts) != 2:
            fail(f"invalid checksum line in {path}: {line_no}")
        digest, rel = parts
        rel = rel.strip()
        if rel.startswith("*"):
            rel = rel[1:]
        if rel.startswith("/") or ".." in Path(rel).parts:
            fail(f"unsafe checksum path in {path}: {rel}")
        if not re.fullmatch(r"[0-9a-fA-F]{64}", digest):
            fail(f"invalid SHA-256 digest in {path}: {line_no}")
        entries.append((digest.lower(), rel))
    if not entries:
        fail(f"checksum file is empty: {path}")
    return entries


def verify_sha256sums(path, base):
    count = 0
    for expected, rel in parse_sha256sums(path):
        target = base / rel
        if not target.is_file():
            fail(f"checksum target missing: {rel}")
        actual = sha256_path(target)
        if actual != expected:
            fail(f"checksum mismatch for {rel}: expected {expected}, got {actual}")
        count += 1
    return count


def ensure_safe_member(name, archive):
    path = Path(name)
    if name.startswith("/") or ".." in path.parts:
        fail(f"unsafe archive member in {archive}: {name}")


@dataclass(frozen=True)
class Asset:
    path: Path
    platform: str
    kind: str
    sha256: str
    bytes: int

    @property
    def filename(self):
        return self.path.name


def scan_assets(release_dir, package_name, version):
    pattern = re.compile(rf"^{re.escape(package_name)}-{re.escape(version)}-(.+)\.(tar\.gz|zip)$")
    assets = {}
    for path in sorted(release_dir.iterdir()):
        if not path.is_file():
            continue
        match = pattern.match(path.name)
        if not match:
            continue
        platform_name, ext = match.groups()
        kind = "tar.gz" if ext == "tar.gz" else "zip"
        assets.setdefault(platform_name, {})[kind] = Asset(
            path=path,
            platform=platform_name,
            kind=kind,
            sha256=sha256_path(path),
            bytes=file_size(path),
        )
    return assets


def binary_name(platform_name):
    return "cool.exe" if platform_name.startswith("windows-") or platform_name.endswith("-windows") else "cool"


def archive_payload_files(asset, package_name, version):
    root = f"{package_name}-{version}-{asset.platform}"
    files = {}
    dirs = set()
    if asset.kind == "tar.gz":
        with tarfile.open(asset.path, "r:gz") as archive:
            for member in archive.getmembers():
                ensure_safe_member(member.name, asset.path)
                if member.isdir():
                    dirs.add(member.name.rstrip("/"))
                    continue
                if not member.isfile():
                    continue
                fh = archive.extractfile(member)
                if fh is None:
                    fail(f"could not read archive member {member.name} in {asset.path}")
                files[member.name] = fh.read()
    else:
        with zipfile.ZipFile(asset.path) as archive:
            for info in archive.infolist():
                ensure_safe_member(info.filename, asset.path)
                if info.is_dir():
                    dirs.add(info.filename.rstrip("/"))
                    continue
                files[info.filename] = archive.read(info)

    if not any(name == root or name.startswith(f"{root}/") for name in [*dirs, *files.keys()]):
        fail(f"{asset.filename} does not contain expected payload root {root}")
    return root, files


def parse_payload_checksums(text):
    entries = []
    for line_no, line in enumerate(text.splitlines(), 1):
        if not line.strip():
            continue
        parts = line.split(None, 1)
        if len(parts) != 2:
            fail(f"invalid payload checksum line {line_no}")
        digest, rel = parts
        rel = rel.strip()
        if rel.startswith("*"):
            rel = rel[1:]
        if rel.startswith("/") or ".." in Path(rel).parts:
            fail(f"unsafe payload checksum path: {rel}")
        entries.append((digest.lower(), rel))
    if not entries:
        fail("payload checksums.txt is empty")
    return entries


def validate_archive(asset, package_name, version):
    root, files = archive_payload_files(asset, package_name, version)
    required = [
        f"{root}/bin/{binary_name(asset.platform)}",
        f"{root}/README.md",
        f"{root}/CHANGELOG.md",
        f"{root}/ROADMAP.md",
        f"{root}/LICENSE",
        f"{root}/install.sh",
        f"{root}/manifest.json",
        f"{root}/checksums.txt",
        f"{root}/docs/INSTALL.md",
        f"{root}/docs/RELEASE_TRUST.md",
        f"{root}/docs/PACKAGE_CHANNELS.md",
        f"{root}/docs/RELEASE_VALIDATION.md",
        f"{root}/scripts/release_candidate.sh",
        f"{root}/scripts/promote_release.sh",
        f"{root}/scripts/trust_release.py",
        f"{root}/scripts/publish_release.sh",
        f"{root}/scripts/package_channels.py",
        f"{root}/scripts/assemble_matrix_release.py",
        f"{root}/scripts/validate_release.py",
        f"{root}/scripts/smoke_matrix_release.py",
    ]
    for rel in required:
        if rel not in files:
            fail(f"{asset.filename} missing payload file {rel}")

    manifest = json.loads(files[f"{root}/manifest.json"].decode("utf-8"))
    if manifest.get("package", {}).get("name") != package_name:
        fail(f"{asset.filename} manifest package mismatch")
    if manifest.get("package", {}).get("version") != version:
        fail(f"{asset.filename} manifest version mismatch")
    if manifest.get("release_candidate", {}).get("platform") != asset.platform:
        fail(f"{asset.filename} manifest platform mismatch")

    checksums = parse_payload_checksums(files[f"{root}/checksums.txt"].decode("utf-8"))
    for expected, rel in checksums:
        member = f"{root}/{rel}"
        if member not in files:
            fail(f"{asset.filename} payload checksum target missing: {rel}")
        actual = hashlib.sha256(files[member]).hexdigest()
        if actual != expected:
            fail(f"{asset.filename} payload checksum mismatch for {rel}")
    return len(files)


def validate_release_metadata(release_dir, dist_dir, package_name, version, platform_name, required_platforms, assets):
    release_json_path = release_dir / "release.json"
    latest_path = release_dir / "latest.json"
    global_latest_path = dist_dir / "releases" / "latest.json"
    sums_path = release_dir / "SHA256SUMS"
    for path in (release_json_path, latest_path, global_latest_path, sums_path):
        if not path.is_file():
            fail(f"required release metadata missing: {path}")

    release_json = load_json(release_json_path)
    package = release_json.get("package", {})
    if package.get("name") != package_name or package.get("version") != version:
        fail("release.json package metadata mismatch")
    release = release_json.get("release", {})
    if platform_name and release.get("platform") != platform_name:
        fail(f"release platform mismatch: expected {platform_name}, got {release.get('platform')}")

    release_platforms = set(release.get("platforms") or [release.get("platform")])
    for required in required_platforms:
        if required not in assets:
            fail(f"required release platform missing asset: {required}")
        if release.get("platform") == "multi" and required not in release_platforms:
            fail(f"required release platform missing from release.json: {required}")

    sums_count = verify_sha256sums(sums_path, release_dir)

    for record in release_json.get("assets", []):
        rel = record.get("path")
        if not rel:
            fail("release.json asset missing path")
        target = release_dir / rel
        if not target.is_file():
            fail(f"release.json asset missing on disk: {rel}")
        if sha256_path(target) != record.get("sha256"):
            fail(f"release.json asset hash mismatch: {rel}")
        if file_size(target) != record.get("bytes"):
            fail(f"release.json asset size mismatch: {rel}")

    for latest in (latest_path, global_latest_path):
        data = load_json(latest)
        record = data.get("release_json", {})
        if record.get("sha256") != sha256_path(release_json_path):
            fail(f"{latest} release_json hash mismatch")
        if record.get("bytes") != file_size(release_json_path):
            fail(f"{latest} release_json size mismatch")

    return release_json, sums_count


def validate_sidecars(release_dir, package_name, version, assets):
    for platform_name in sorted(assets):
        base = f"{package_name}-{version}-{platform_name}"
        manifest_path = release_dir / f"{base}.manifest.json"
        checksums_path = release_dir / f"{base}.checksums.txt"
        notes_path = release_dir / f"{base}.RC_NOTES.md"
        for path in (manifest_path, checksums_path, notes_path):
            if not path.is_file():
                fail(f"platform sidecar missing: {path.name}")
        manifest = load_json(manifest_path)
        if manifest.get("release_candidate", {}).get("platform") != platform_name:
            fail(f"{manifest_path.name} platform mismatch")


def validate_trust(release_dir, dist_dir, version, platform_name, verify_key):
    required = [
        release_dir / "sbom.spdx.json",
        release_dir / "provenance.intoto.json",
        release_dir / "trust.json",
        release_dir / "TRUST_SHA256SUMS",
    ]
    for path in required:
        if not path.is_file():
            fail(f"required trust artifact missing: {path.name}")
    trust_count = verify_sha256sums(release_dir / "TRUST_SHA256SUMS", release_dir)
    release_json = load_json(release_dir / "release.json")
    trust_json = load_json(release_dir / "trust.json")
    for path in (release_dir / "sbom.spdx.json", release_dir / "provenance.intoto.json"):
        load_json(path)
    if platform_name and trust_json.get("release", {}).get("platform") != platform_name:
        fail("trust.json platform mismatch")
    supply_chain = release_json.get("supply_chain", {})
    for key in ("sbom", "provenance"):
        record = supply_chain.get(key)
        if not record:
            fail(f"release.json missing supply_chain.{key}")
        target = release_dir / record["path"]
        if not target.is_file():
            fail(f"supply_chain.{key} target missing: {record['path']}")
        if sha256_path(target) != record.get("sha256"):
            fail(f"supply_chain.{key} hash mismatch")

    if verify_key:
        key_path = Path(verify_key).resolve()
        if not key_path.is_file():
            fail(f"verification key not found: {key_path}")
        for rel in ("SHA256SUMS.sig", "release.json.sig"):
            if not (release_dir / rel).is_file():
                fail(f"required signature missing: {rel}")
        for sig in sorted(release_dir.glob("*.sig")):
            target = release_dir / sig.name[:-4]
            if not target.is_file():
                fail(f"signature target missing: {target.name}")
            subprocess.run(
                ["openssl", "dgst", "-sha256", "-verify", str(key_path), "-signature", str(sig), str(target)],
                cwd=ROOT,
                check=True,
                stdout=subprocess.DEVNULL,
            )
    return trust_count


def parse_simple_yaml_fields(path):
    fields = {}
    for raw in path.read_text(encoding="utf-8").splitlines():
        line = raw.strip()
        if not line or line.startswith("#") or ":" not in line:
            continue
        key, value = line.split(":", 1)
        fields[key.strip()] = value.strip().strip('"')
    return fields


def ar_members(path):
    data = path.read_bytes()
    if not data.startswith(b"!<arch>\n"):
        fail(f"invalid ar archive: {path}")
    offset = 8
    members = {}
    while offset + 60 <= len(data):
        header = data[offset:offset + 60]
        offset += 60
        name = header[:16].decode("ascii", errors="replace").strip()
        size_text = header[48:58].decode("ascii", errors="replace").strip()
        if header[58:60] != b"`\n" or not size_text.isdigit():
            fail(f"invalid ar member header in {path}")
        size = int(size_text)
        payload = data[offset:offset + size]
        offset += size
        if offset % 2:
            offset += 1
        members[name.rstrip("/")] = payload
    return members


def validate_debian(channel_root, channels, package_name, version):
    deb_rel = channels.get("debian_package")
    packages_rel = channels.get("debian_packages_index")
    if not deb_rel or not packages_rel:
        return 0
    deb_path = channel_root / deb_rel
    packages_path = channel_root / packages_rel
    packages_gz = packages_path.with_name("Packages.gz")
    for path in (deb_path, packages_path, packages_gz):
        if not path.is_file():
            fail(f"Debian channel file missing: {path}")
    members = ar_members(deb_path)
    if members.get("debian-binary") != b"2.0\n":
        fail("Debian package missing debian-binary 2.0 marker")
    for name in ("control.tar.gz", "data.tar.gz"):
        if name not in members:
            fail(f"Debian package missing {name}")
        with tarfile.open(fileobj=io.BytesIO(members[name]), mode="r:gz") as archive:
            for member in archive.getmembers():
                ensure_safe_member(member.name, deb_path)
    packages_text = packages_path.read_text(encoding="utf-8")
    if gzip.decompress(packages_gz.read_bytes()).decode("utf-8") != packages_text:
        fail("Packages.gz does not match Packages")
    fields = {}
    for line in packages_text.splitlines():
        if ":" in line:
            key, value = line.split(":", 1)
            fields[key] = value.strip()
    if fields.get("Package") != package_name or fields.get("Version") != version:
        fail("Debian Packages package metadata mismatch")
    if fields.get("Filename") != str(Path(deb_rel).relative_to("apt").as_posix()):
        fail("Debian Packages filename mismatch")
    if int(fields.get("Size", "0")) != file_size(deb_path):
        fail("Debian Packages size mismatch")
    if fields.get("SHA256") != sha256_path(deb_path):
        fail("Debian Packages SHA256 mismatch")
    return 1


def validate_channels(dist_dir, release_dir, package_name, version, required_platforms, assets):
    channel_root = dist_dir / "channels" / version
    channels_json = channel_root / "channels.json"
    sums_path = channel_root / "CHANNEL_SHA256SUMS"
    latest_path = dist_dir / "channels" / "latest.json"
    archive_path = dist_dir / "channels" / f"{package_name}-{version}-package-channels.tar.gz"
    for path in (channels_json, sums_path, latest_path, archive_path):
        if not path.is_file():
            fail(f"required channel artifact missing: {path}")
    channel_sums_count = verify_sha256sums(sums_path, channel_root)
    channels_data = load_json(channels_json)
    if channels_data.get("package", {}).get("name") != package_name:
        fail("channels.json package name mismatch")
    if channels_data.get("package", {}).get("version") != version:
        fail("channels.json package version mismatch")

    channel_platforms = {record.get("platform"): record for record in channels_data.get("platforms", [])}
    for required in required_platforms:
        if required not in channel_platforms:
            fail(f"required platform missing from channels.json: {required}")
    for platform_name, record in channel_platforms.items():
        if platform_name not in assets:
            fail(f"channels.json references missing release platform: {platform_name}")
        for kind, asset_record in record.get("assets", {}).items():
            asset = assets.get(platform_name, {}).get(kind)
            if not asset:
                fail(f"channels.json references missing release asset: {platform_name} {kind}")
            if asset_record.get("filename") != asset.filename:
                fail(f"channels.json filename mismatch for {platform_name} {kind}")
            if asset_record.get("sha256") != asset.sha256:
                fail(f"channels.json SHA256 mismatch for {platform_name} {kind}")
            if asset_record.get("bytes") != asset.bytes:
                fail(f"channels.json size mismatch for {platform_name} {kind}")
            if not str(asset_record.get("url", "")).endswith(f"/{asset.filename}"):
                fail(f"channels.json URL does not end with asset filename: {asset.filename}")

    for record in channels_data.get("files", []):
        rel = record.get("path")
        target = channel_root / rel
        if not target.is_file():
            fail(f"channels.json file record missing: {rel}")
        if sha256_path(target) != record.get("sha256"):
            fail(f"channels.json file hash mismatch: {rel}")
        if file_size(target) != record.get("bytes"):
            fail(f"channels.json file size mismatch: {rel}")

    formula_rel = channels_data.get("channels", {}).get("homebrew_formula")
    if formula_rel:
        formula = channel_root / formula_rel
        if not formula.is_file():
            fail("Homebrew formula missing")
        text = formula.read_text(encoding="utf-8")
        if "class Cool < Formula" not in text or "def install" not in text:
            fail("Homebrew formula missing required structure")
        for platform_name, kinds in assets.items():
            if (platform_name.startswith("macos-") or platform_name.startswith("linux-")) and "tar.gz" in kinds:
                asset = kinds["tar.gz"]
                if asset.sha256 not in text or asset.filename not in text:
                    fail(f"Homebrew formula missing asset hash or URL for {platform_name}")

    windows_zip = assets.get("windows-x86_64", {}).get("zip")
    winget_rel = channels_data.get("channels", {}).get("winget")
    if windows_zip:
        if not winget_rel:
            fail("Winget channel missing for Windows zip asset")
        winget_root = channel_root / winget_rel
        version_yaml = winget_root / "Codenz.Cool.yaml"
        locale_yaml = winget_root / "Codenz.Cool.locale.en-US.yaml"
        installer_yaml = winget_root / "Codenz.Cool.installer.yaml"
        for path in (version_yaml, locale_yaml, installer_yaml):
            if not path.is_file():
                fail(f"Winget manifest missing: {path}")
        version_fields = parse_simple_yaml_fields(version_yaml)
        installer_text = installer_yaml.read_text(encoding="utf-8")
        if version_fields.get("PackageIdentifier") != "Codenz.Cool":
            fail("Winget version manifest package identifier mismatch")
        if version_fields.get("PackageVersion") != version:
            fail("Winget version manifest version mismatch")
        if "InstallerType: zip" not in installer_text:
            fail("Winget installer manifest missing zip installer type")
        if "NestedInstallerType: portable" not in installer_text:
            fail("Winget installer manifest missing portable nested installer type")
        if f"RelativeFilePath: cool-{version}-windows-x86_64/bin/cool.exe" not in installer_text:
            fail("Winget installer manifest nested path mismatch")
        if windows_zip.sha256.upper() not in installer_text:
            fail("Winget installer manifest SHA256 mismatch")
        if windows_zip.filename not in installer_text:
            fail("Winget installer manifest URL missing Windows zip filename")

    deb_count = 0
    if "linux-x86_64" in assets and "tar.gz" in assets["linux-x86_64"]:
        deb_count = validate_debian(channel_root, channels_data.get("channels", {}), package_name, version)
        if deb_count == 0:
            fail("Debian channel missing for Linux x86_64 tarball")

    latest = load_json(latest_path)
    if latest.get("channels", {}).get("sha256") != sha256_path(channels_json):
        fail("channels latest.json channels hash mismatch")
    if latest.get("archive", {}).get("sha256") != sha256_path(archive_path):
        fail("channels latest.json archive hash mismatch")

    with tarfile.open(archive_path, "r:gz") as archive:
        names = []
        for member in archive.getmembers():
            ensure_safe_member(member.name, archive_path)
            names.append(member.name)
        payload_root = f"{package_name}-{version}-package-channels"
        for rel in ("channels.json", "CHANNEL_SHA256SUMS"):
            if f"{payload_root}/{rel}" not in names:
                fail(f"channel archive missing {rel}")
    return channel_sums_count, len(channel_platforms), deb_count


def run_install_smoke(release_dir, version, package_name, assets, smoke_platform, smoke_kind):
    platform_name = smoke_platform or default_platform()
    kinds = assets.get(platform_name)
    if not kinds:
        fail(f"install smoke platform asset missing: {platform_name}")
    if smoke_kind == "auto":
        kind = "zip" if platform_name.startswith("windows-") else "tar.gz"
        if kind not in kinds:
            kind = "tar.gz" if "tar.gz" in kinds else "zip"
    else:
        kind = smoke_kind
    asset = kinds.get(kind)
    if not asset:
        fail(f"install smoke asset missing: {platform_name} {kind}")
    with tempfile.TemporaryDirectory(prefix="cool-install-validate.") as tmp:
        prefix = Path(tmp) / "prefix"
        subprocess.run(
            [
                "bash",
                str(ROOT / "install.sh"),
                "--from",
                str(asset.path),
                "--prefix",
                str(prefix),
                "--checksums",
                str(release_dir / "SHA256SUMS"),
            ],
            cwd=ROOT,
            check=True,
        )
        subprocess.run([str(prefix / "bin" / "cool"), "help"], cwd=ROOT, check=True, stdout=subprocess.DEVNULL)
    return f"{platform_name}:{kind}"


def main():
    parser = argparse.ArgumentParser(description="Validate promoted Cool release and package-channel artifacts.")
    parser.add_argument("--version")
    parser.add_argument("--platform", help="Expected release platform, for example macos-arm64 or multi.")
    parser.add_argument("--dist-dir", default=str(ROOT / "dist"))
    parser.add_argument("--require-platform", action="append", default=[])
    parser.add_argument("--require-trust", action="store_true")
    parser.add_argument("--require-channels", action="store_true")
    parser.add_argument("--verify-key")
    parser.add_argument("--install-smoke", action="store_true")
    parser.add_argument("--install-smoke-platform")
    parser.add_argument("--install-smoke-kind", choices=["auto", "tar.gz", "zip"], default="auto")
    parser.add_argument("--report", help="Write a JSON validation report to this path.")
    args = parser.parse_args()

    package_name = read_cargo_value("name")
    version = args.version or read_cargo_value("version")
    dist_dir = Path(args.dist_dir).resolve()
    release_dir = dist_dir / "releases" / version
    if not release_dir.is_dir():
        fail(f"release directory not found: {release_dir}")

    assets = scan_assets(release_dir, package_name, version)
    if not assets:
        fail(f"no release archives found under {release_dir}")

    release_json, sums_count = validate_release_metadata(
        release_dir,
        dist_dir,
        package_name,
        version,
        args.platform,
        args.require_platform,
        assets,
    )
    validate_sidecars(release_dir, package_name, version, assets)

    archive_file_count = 0
    for platform_assets in assets.values():
        for asset in platform_assets.values():
            archive_file_count += validate_archive(asset, package_name, version)

    trust_count = None
    if args.require_trust:
        trust_count = validate_trust(release_dir, dist_dir, version, args.platform, args.verify_key)

    channel_summary = None
    if args.require_channels:
        channel_summary = validate_channels(dist_dir, release_dir, package_name, version, args.require_platform, assets)

    smoke_result = None
    if args.install_smoke:
        smoke_result = run_install_smoke(
            release_dir,
            version,
            package_name,
            assets,
            args.install_smoke_platform,
            args.install_smoke_kind,
        )

    report = {
        "schema_version": 1,
        "generated_at": datetime.now(timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ"),
        "package": {"name": package_name, "version": version},
        "release": {
            "path": display_path(release_dir),
            "platform": release_json.get("release", {}).get("platform"),
            "platforms": sorted(assets.keys()),
            "asset_count": sum(len(kinds) for kinds in assets.values()),
            "sha256sums_entries": sums_count,
            "archive_payload_files_checked": archive_file_count,
        },
        "trust": {"required": args.require_trust, "sha256sums_entries": trust_count},
        "channels": {
            "required": args.require_channels,
            "sha256sums_entries": channel_summary[0] if channel_summary else None,
            "platform_count": channel_summary[1] if channel_summary else None,
            "debian_packages": channel_summary[2] if channel_summary else None,
        },
        "install_smoke": smoke_result,
    }
    if args.report:
        write_json(Path(args.report), report)

    print("release validation: ok")
    print(f"  Release  -> {display_path(release_dir)}")
    print(f"  Version  -> {version}")
    print(f"  Platform -> {release_json.get('release', {}).get('platform')}")
    print(f"  Assets   -> {sum(len(kinds) for kinds in assets.values())} archive(s)")
    if args.require_channels:
        print(f"  Channels -> {display_path(dist_dir / 'channels' / version / 'channels.json')}")
    if smoke_result:
        print(f"  Install  -> {smoke_result}")
    if args.report:
        print(f"  Report   -> {display_path(Path(args.report).resolve())}")


if __name__ == "__main__":
    try:
        main()
    except subprocess.CalledProcessError as exc:
        fail(f"command failed with exit code {exc.returncode}: {' '.join(map(str, exc.cmd))}")
