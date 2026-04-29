#!/usr/bin/env python3
import argparse
import copy
import hashlib
import json
import os
import platform
import shutil
import subprocess
import tarfile
import tempfile
import zipfile
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
PLATFORMS = ["linux-x86_64", "macos-x86_64", "macos-arm64", "windows-x86_64"]


def fail(message):
    raise SystemExit(f"matrix smoke: {message}")


def read_cargo_value(key):
    import re

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


def sha256_path(path):
    h = hashlib.sha256()
    with open(path, "rb") as fh:
        for chunk in iter(lambda: fh.read(1024 * 1024), b""):
            h.update(chunk)
    return h.hexdigest()


def file_size(path):
    return path.stat().st_size


def safe_member(name, archive):
    rel = Path(name)
    if name.startswith("/") or ".." in rel.parts:
        fail(f"unsafe archive member in {archive}: {name}")


def list_payload_files(payload_dir):
    files = []
    for path in sorted(payload_dir.rglob("*")):
        if not path.is_file():
            continue
        rel = path.relative_to(payload_dir).as_posix()
        if rel in {"manifest.json", "checksums.txt"}:
            continue
        files.append(rel)
    return files


def write_checksums(payload_dir):
    lines = []
    for rel in list_payload_files(payload_dir):
        lines.append(f"{sha256_path(payload_dir / rel)}  {rel}")
    (payload_dir / "checksums.txt").write_text("\n".join(lines) + "\n", encoding="utf-8")


def write_manifest(payload_dir, package_name, version, target_platform, source_manifest):
    data = copy.deepcopy(source_manifest)
    data.setdefault("package", {})["name"] = package_name
    data.setdefault("package", {})["version"] = version
    data.setdefault("release_candidate", {})["platform"] = target_platform
    artifacts = []
    for rel in list_payload_files(payload_dir):
        path = payload_dir / rel
        artifacts.append({"path": rel, "sha256": sha256_path(path), "bytes": file_size(path)})
    checksums = payload_dir / "checksums.txt"
    artifacts.append({"path": "checksums.txt", "sha256": sha256_path(checksums), "bytes": file_size(checksums)})
    data["artifacts"] = artifacts
    (payload_dir / "manifest.json").write_text(json.dumps(data, indent=2, sort_keys=True) + "\n", encoding="utf-8")


def extract_source_payload(source_tar, expected_root, output_dir):
    with tarfile.open(source_tar, "r:gz") as archive:
        for member in archive.getmembers():
            safe_member(member.name, source_tar)
            if member.name == expected_root or member.name.startswith(f"{expected_root}/"):
                archive.extract(member, output_dir)
    payload = output_dir / expected_root
    if not payload.is_dir():
        fail(f"source payload root missing in {source_tar}: {expected_root}")
    return payload


def copy_payload(source_payload, target_payload, target_platform):
    if target_payload.exists():
        shutil.rmtree(target_payload)
    shutil.copytree(source_payload, target_payload, symlinks=False)
    bin_dir = target_payload / "bin"
    cool = bin_dir / "cool"
    cool_exe = bin_dir / "cool.exe"
    if target_platform.startswith("windows-"):
        if cool.exists():
            cool.rename(cool_exe)
    else:
        if cool_exe.exists():
            cool_exe.rename(cool)
    notes = target_payload / "RELEASE_NOTES.md"
    if notes.is_file():
        lines = []
        for line in notes.read_text(encoding="utf-8").splitlines():
            if line.startswith("Platform: "):
                lines.append(f"Platform: {target_platform}")
            else:
                lines.append(line)
        notes.write_text("\n".join(lines) + "\n", encoding="utf-8")


def write_tar(payload_parent, payload_name, output_path):
    with tarfile.open(output_path, "w:gz", format=tarfile.GNU_FORMAT) as archive:
        archive.add(payload_parent / payload_name, arcname=payload_name)


def write_zip(payload_parent, payload_name, output_path):
    root = payload_parent / payload_name
    with zipfile.ZipFile(output_path, "w", zipfile.ZIP_DEFLATED) as archive:
        for path in sorted(root.rglob("*")):
            if path.is_dir():
                continue
            rel = Path(payload_name) / path.relative_to(root)
            info = zipfile.ZipInfo(rel.as_posix())
            info.external_attr = (path.stat().st_mode & 0xFFFF) << 16
            with path.open("rb") as fh:
                archive.writestr(info, fh.read())


def create_matrix_input(source_release_dir, package_name, version, source_platform, work_dir):
    source_root = f"{package_name}-{version}-{source_platform}"
    source_tar = source_release_dir / f"{source_root}.tar.gz"
    if not source_tar.is_file():
        fail(f"source tarball not found: {source_tar}")
    extracted = work_dir / "source"
    extracted.mkdir(parents=True)
    source_payload = extract_source_payload(source_tar, source_root, extracted)
    source_manifest = json.loads((source_payload / "manifest.json").read_text(encoding="utf-8"))

    input_dir = work_dir / "matrix-input"
    input_dir.mkdir()
    payload_parent = work_dir / "payloads"
    payload_parent.mkdir()
    for target_platform in PLATFORMS:
        payload_name = f"{package_name}-{version}-{target_platform}"
        payload = payload_parent / payload_name
        copy_payload(source_payload, payload, target_platform)
        write_checksums(payload)
        write_manifest(payload, package_name, version, target_platform, source_manifest)
        write_tar(payload_parent, payload_name, input_dir / f"{payload_name}.tar.gz")
        write_zip(payload_parent, payload_name, input_dir / f"{payload_name}.zip")
        shutil.copy2(payload / "manifest.json", input_dir / f"{payload_name}.manifest.json")
        shutil.copy2(payload / "checksums.txt", input_dir / f"{payload_name}.checksums.txt")
        shutil.copy2(payload / "RELEASE_NOTES.md", input_dir / f"{payload_name}.RC_NOTES.md")
    return input_dir


def run(args, env=None):
    subprocess.run(args, cwd=ROOT, check=True, env=env)


def main():
    parser = argparse.ArgumentParser(description="Run a synthetic four-platform release matrix validation.")
    parser.add_argument("--version")
    parser.add_argument("--source-platform", default=default_platform())
    parser.add_argument("--dist-dir", default=str(ROOT / "dist"))
    parser.add_argument("--work-dir")
    parser.add_argument("--keep", action="store_true")
    args = parser.parse_args()

    package_name = read_cargo_value("name")
    version = args.version or read_cargo_value("version")
    source_release_dir = Path(args.dist_dir).resolve() / "releases" / version
    if not source_release_dir.is_dir():
        fail(f"source release directory not found: {source_release_dir}")

    temp_ctx = None
    if args.work_dir:
        work_dir = Path(args.work_dir).resolve()
        if work_dir.exists():
            shutil.rmtree(work_dir)
        work_dir.mkdir(parents=True)
    else:
        temp_ctx = tempfile.TemporaryDirectory(prefix="cool-matrix-smoke.")
        work_dir = Path(temp_ctx.name)

    try:
        input_dir = create_matrix_input(source_release_dir, package_name, version, args.source_platform, work_dir)
        matrix_dist = work_dir / "dist"
        run(["bash", "scripts/assemble_matrix_release.sh", "--source-dir", str(input_dir), "--dist-dir", str(matrix_dist), "--version", version])
        run(["bash", "scripts/trust_release.sh", "generate", "--dist-dir", str(matrix_dist), "--version", version, "--platform", "multi"])
        run(["bash", "scripts/trust_release.sh", "verify", "--dist-dir", str(matrix_dist), "--version", version, "--platform", "multi"])
        channel_cmd = ["bash", "scripts/package_channels.sh", "generate", "--dist-dir", str(matrix_dist), "--version", version]
        for platform_name in PLATFORMS:
            channel_cmd.extend(["--require-platform", platform_name])
        run(channel_cmd)
        validate_cmd = [
            "bash",
            "scripts/validate_release.sh",
            "--dist-dir",
            str(matrix_dist),
            "--version",
            version,
            "--platform",
            "multi",
            "--require-trust",
            "--require-channels",
            "--report",
            str(matrix_dist / "validation" / version / "matrix-release-validation.json"),
        ]
        for platform_name in PLATFORMS:
            validate_cmd.extend(["--require-platform", platform_name])
        run(validate_cmd)
        print("matrix smoke: ok")
        print(f"  Work dir -> {work_dir}")
        print(f"  Dist     -> {matrix_dist}")
    finally:
        if temp_ctx and not args.keep:
            temp_ctx.cleanup()


if __name__ == "__main__":
    try:
        main()
    except subprocess.CalledProcessError as exc:
        fail(f"command failed with exit code {exc.returncode}: {' '.join(map(str, exc.cmd))}")
