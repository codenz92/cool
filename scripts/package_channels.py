#!/usr/bin/env python3
import argparse
import gzip
import hashlib
import io
import json
import os
import re
import tarfile
import tempfile
from dataclasses import dataclass
from datetime import datetime, timezone
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]


def fail(message):
    raise SystemExit(f"package channels: {message}")


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


def asset_url(base_url, tag, asset):
    return f"{base_url.rstrip('/')}/{tag}/{asset.filename}"


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


def write_json(path, data):
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(data, indent=2, sort_keys=True) + "\n", encoding="utf-8")


def create_channel_archive(dist_dir, channel_root, package_name, version):
    archive_path = dist_dir / "channels" / f"{package_name}-{version}-package-channels.tar.gz"
    payload_root = f"{package_name}-{version}-package-channels"
    with tarfile.open(archive_path, "w:gz", format=tarfile.GNU_FORMAT) as archive:
        for path in sorted(channel_root.rglob("*")):
            rel = Path(payload_root) / path.relative_to(channel_root)
            info = archive.gettarinfo(path, arcname=rel.as_posix())
            info.mtime = 0
            if path.is_file():
                with path.open("rb") as fh:
                    archive.addfile(info, fh)
            else:
                archive.addfile(info)
    return archive_path


def platform_arch(platform_name):
    if platform_name.endswith("arm64"):
        return "arm64"
    if platform_name.endswith("x86_64"):
        return "x64"
    return "unknown"


def homebrew_formula(package_name, version, assets, base_url, tag):
    lines = [
        "class Cool < Formula",
        '  desc "Cool programming language compiler and runtime"',
        '  homepage "https://github.com/codenz92/cool-lang"',
        f'  version "{version}"',
        '  license "MIT"',
        "",
    ]

    mac_assets = {k: v["tar.gz"] for k, v in assets.items() if k.startswith("macos-") and "tar.gz" in v}
    linux_assets = {k: v["tar.gz"] for k, v in assets.items() if k.startswith("linux-") and "tar.gz" in v}

    if mac_assets:
        lines += ["  on_macos do"]
        if "macos-arm64" in mac_assets:
            asset = mac_assets["macos-arm64"]
            lines += [
                "    if Hardware::CPU.arm?",
                f'      url "{asset_url(base_url, tag, asset)}"',
                f'      sha256 "{asset.sha256}"',
            ]
            if "macos-x86_64" in mac_assets:
                intel = mac_assets["macos-x86_64"]
                lines += [
                    "    else",
                    f'      url "{asset_url(base_url, tag, intel)}"',
                    f'      sha256 "{intel.sha256}"',
                    "    end",
                ]
            else:
                lines += ["    end"]
        elif "macos-x86_64" in mac_assets:
            asset = mac_assets["macos-x86_64"]
            lines += [
                f'    url "{asset_url(base_url, tag, asset)}"',
                f'    sha256 "{asset.sha256}"',
            ]
        lines += ["  end", ""]

    if linux_assets:
        asset = linux_assets.get("linux-x86_64") or next(iter(linux_assets.values()))
        lines += [
            "  on_linux do",
            f'    url "{asset_url(base_url, tag, asset)}"',
            f'    sha256 "{asset.sha256}"',
            "  end",
            "",
        ]

    lines += [
        "  def install",
        '    libexec.install Dir["*"]',
        '    bin.install_symlink libexec/"bin/cool" => "cool"',
        "  end",
        "",
        "  test do",
        '    system "#{bin}/cool", "help"',
        "  end",
        "end",
        "",
    ]
    return "\n".join(lines)


def write_winget_manifests(root, version, assets, base_url, tag):
    windows = assets.get("windows-x86_64", {})
    zip_asset = windows.get("zip")
    if not zip_asset:
        return None

    winget_root = root / "winget" / "Codenz.Cool" / version
    winget_root.mkdir(parents=True, exist_ok=True)
    nested_path = f"cool-{version}-windows-x86_64/bin/cool.exe"
    installer_url = asset_url(base_url, tag, zip_asset)
    installer_sha = zip_asset.sha256.upper()

    (winget_root / "Codenz.Cool.yaml").write_text(f"""# Created by scripts/package_channels.py
PackageIdentifier: Codenz.Cool
PackageVersion: {version}
DefaultLocale: en-US
ManifestType: version
ManifestVersion: 1.6.0
""", encoding="utf-8")

    (winget_root / "Codenz.Cool.locale.en-US.yaml").write_text(f"""# Created by scripts/package_channels.py
PackageIdentifier: Codenz.Cool
PackageVersion: {version}
PackageLocale: en-US
Publisher: Codenz
PackageName: Cool
License: MIT
ShortDescription: Cool programming language compiler and runtime.
PackageUrl: https://github.com/codenz92/cool-lang
ManifestType: defaultLocale
ManifestVersion: 1.6.0
""", encoding="utf-8")

    (winget_root / "Codenz.Cool.installer.yaml").write_text(f"""# Created by scripts/package_channels.py
PackageIdentifier: Codenz.Cool
PackageVersion: {version}
InstallerType: zip
NestedInstallerType: portable
NestedInstallerFiles:
  - RelativeFilePath: {nested_path}
    PortableCommandAlias: cool
Installers:
  - Architecture: x64
    InstallerUrl: {installer_url}
    InstallerSha256: {installer_sha}
ManifestType: installer
ManifestVersion: 1.6.0
""", encoding="utf-8")
    return winget_root


def tar_bytes(entries):
    raw = io.BytesIO()
    with tarfile.open(fileobj=raw, mode="w") as tf:
        for name, data, mode in entries:
            encoded = data.encode("utf-8")
            info = tarfile.TarInfo(name)
            info.size = len(encoded)
            info.mode = mode
            info.mtime = 0
            tf.addfile(info, io.BytesIO(encoded))
    return gzip.compress(raw.getvalue(), mtime=0)


def add_ar_member(out, name, data, mode="100644"):
    if len(name) > 15:
        name = name[:15]
    header = f"{name:<16}{0:<12}{0:<6}{0:<6}{mode:<8}{len(data):<10}`\n".encode("ascii")
    out.write(header)
    out.write(data)
    if len(data) % 2:
        out.write(b"\n")


def build_deb(root, package_name, version, asset, base_url, tag):
    deb_root = root / "apt" / "pool" / "main" / "c" / package_name
    deb_root.mkdir(parents=True, exist_ok=True)
    deb_path = deb_root / f"{package_name}_{version}_amd64.deb"
    payload_root = f"{package_name}-{version}-linux-x86_64"

    with tempfile.TemporaryDirectory() as tmp:
        data_tar = Path(tmp) / "data.tar.gz"
        md5sums = []
        with tarfile.open(asset.path, "r:gz") as source, tarfile.open(data_tar, "w:gz", format=tarfile.GNU_FORMAT) as data:
            for member in source.getmembers():
                rel = Path(member.name)
                parts = rel.parts
                if not parts or parts[0] != payload_root:
                    continue
                target_name = Path("usr/lib/cool") / rel
                member.name = target_name.as_posix()
                member.mtime = 0
                data.addfile(member, source.extractfile(member) if member.isfile() else None)
                if member.isfile():
                    extracted = source.extractfile(member)
                    if extracted is not None:
                        md5sums.append(f"{hashlib.md5(extracted.read()).hexdigest()}  {target_name.as_posix()}")
            link = tarfile.TarInfo("usr/bin/cool")
            link.type = tarfile.SYMTYPE
            link.linkname = f"../lib/cool/{payload_root}/bin/cool"
            link.mode = 0o777
            link.mtime = 0
            data.addfile(link)

        control = f"""Package: {package_name}
Version: {version}
Section: devel
Priority: optional
Architecture: amd64
Maintainer: Cool Project <noreply@github.com>
Homepage: https://github.com/codenz92/cool-lang
Description: Cool programming language compiler and runtime
 Cool is a compact programming language with interpreter, VM, LLVM backend,
 release tooling, and packaged standard library examples.
"""
        control_data = tar_bytes([
            ("control", control, 0o644),
            ("md5sums", "\n".join(md5sums) + "\n", 0o644),
        ])
        with deb_path.open("wb") as out:
            out.write(b"!<arch>\n")
            add_ar_member(out, "debian-binary", b"2.0\n")
            add_ar_member(out, "control.tar.gz", control_data)
            add_ar_member(out, "data.tar.gz", data_tar.read_bytes())

    packages_dir = root / "apt" / "dists" / "stable" / "main" / "binary-amd64"
    packages_dir.mkdir(parents=True, exist_ok=True)
    filename = deb_path.relative_to(root / "apt").as_posix()
    packages_text = f"""Package: {package_name}
Version: {version}
Architecture: amd64
Maintainer: Cool Project <noreply@github.com>
Filename: {filename}
Size: {file_size(deb_path)}
SHA256: {sha256_path(deb_path)}
Homepage: https://github.com/codenz92/cool-lang
Description: Cool programming language compiler and runtime
 Cool is a compact programming language with interpreter, VM, LLVM backend,
 release tooling, and packaged standard library examples.
"""
    (packages_dir / "Packages").write_text(packages_text, encoding="utf-8")
    (packages_dir / "Packages.gz").write_bytes(gzip.compress(packages_text.encode("utf-8"), mtime=0))
    return deb_path


def generate(args):
    version = args.version or read_cargo_value("version")
    package_name = read_cargo_value("name")
    dist_dir = Path(args.dist_dir).resolve()
    release_dir = dist_dir / "releases" / version
    if not release_dir.is_dir():
        fail(f"release directory not found: {release_dir}")

    assets = scan_assets(release_dir, package_name, version)
    if not assets:
        fail(f"no release archives found under {release_dir}")
    for required in args.require_platform:
        if required not in assets:
            fail(f"required platform missing: {required}")

    tag = args.tag or f"v{version}"
    base_url = args.base_url
    channel_root = dist_dir / "channels" / version
    if channel_root.exists():
        for path in sorted(channel_root.rglob("*"), reverse=True):
            if path.is_file() or path.is_symlink():
                path.unlink()
            elif path.is_dir():
                path.rmdir()
    channel_root.mkdir(parents=True, exist_ok=True)

    formula_path = channel_root / "homebrew" / "cool.rb"
    formula_path.parent.mkdir(parents=True, exist_ok=True)
    formula_path.write_text(homebrew_formula(package_name, version, assets, base_url, tag), encoding="utf-8")

    winget_root = write_winget_manifests(channel_root, version, assets, base_url, tag)
    deb_path = None
    if "linux-x86_64" in assets and "tar.gz" in assets["linux-x86_64"]:
        deb_path = build_deb(channel_root, package_name, version, assets["linux-x86_64"]["tar.gz"], base_url, tag)

    platform_records = []
    for platform_name, kinds in sorted(assets.items()):
        record = {"platform": platform_name, "architecture": platform_arch(platform_name), "assets": {}}
        for kind, asset in sorted(kinds.items()):
            record["assets"][kind] = {
                "filename": asset.filename,
                "url": asset_url(base_url, tag, asset),
                "sha256": asset.sha256,
                "bytes": asset.bytes,
            }
        platform_records.append(record)

    channel_files = []
    for path in sorted(channel_root.rglob("*")):
        if path.is_file():
            channel_files.append({
                "path": path.relative_to(channel_root).as_posix(),
                "sha256": sha256_path(path),
                "bytes": file_size(path),
            })

    data = {
        "schema_version": 1,
        "package": {"name": package_name, "version": version},
        "generated_at": datetime.now(timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ"),
        "release": {"tag": tag, "base_url": base_url},
        "platforms": platform_records,
        "channels": {
            "homebrew_formula": "homebrew/cool.rb",
            "winget": winget_root.relative_to(channel_root).as_posix() if winget_root else None,
            "debian_package": deb_path.relative_to(channel_root).as_posix() if deb_path else None,
            "debian_packages_index": "apt/dists/stable/main/binary-amd64/Packages" if deb_path else None,
        },
        "files": channel_files,
    }
    channels_json = channel_root / "channels.json"
    write_json(channels_json, data)

    sums = channel_root / "CHANNEL_SHA256SUMS"
    lines = []
    for path in sorted(channel_root.rglob("*")):
        if path.is_file() and path.name != "CHANNEL_SHA256SUMS":
            lines.append(f"{sha256_path(path)}  {path.relative_to(channel_root).as_posix()}")
    sums.write_text("\n".join(lines) + "\n", encoding="utf-8")

    channel_archive = create_channel_archive(dist_dir, channel_root, package_name, version)
    latest = {
        "schema_version": 1,
        "package": {"name": package_name, "version": version},
        "generated_at": datetime.now(timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ"),
        "channels": {
            "path": display_path(channels_json),
            "sha256": sha256_path(channels_json),
            "bytes": file_size(channels_json),
        },
        "archive": {
            "path": display_path(channel_archive),
            "sha256": sha256_path(channel_archive),
            "bytes": file_size(channel_archive),
        },
    }
    write_json(dist_dir / "channels" / "latest.json", latest)

    print("package channels: ok")
    print(f"  Channels -> {display_path(channels_json)}")
    print(f"  Archive  -> {display_path(channel_archive)}")
    print(f"  Homebrew -> {display_path(formula_path)}")
    if winget_root:
        print(f"  Winget   -> {display_path(winget_root)}")
    if deb_path:
        print(f"  Debian   -> {display_path(deb_path)}")


def main():
    parser = argparse.ArgumentParser(description="Generate package-channel metadata from promoted Cool release assets.")
    parser.add_argument("generate", nargs="?")
    parser.add_argument("--version")
    parser.add_argument("--dist-dir", default=str(ROOT / "dist"))
    parser.add_argument("--tag")
    parser.add_argument("--base-url", default="https://github.com/codenz92/cool-lang/releases/download")
    parser.add_argument("--require-platform", action="append", default=[])
    args = parser.parse_args()
    generate(args)


if __name__ == "__main__":
    main()
