#!/usr/bin/env python3
import argparse
import hashlib
import json
import os
import platform
import re
import subprocess
import sys
from datetime import datetime, timezone
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]


def fail(message):
    print(f"trust release: {message}", file=sys.stderr)
    raise SystemExit(1)


def sha256_path(path):
    h = hashlib.sha256()
    with open(path, "rb") as fh:
        for chunk in iter(lambda: fh.read(1024 * 1024), b""):
            h.update(chunk)
    return h.hexdigest()


def file_record(base, path):
    return {
        "path": path.relative_to(base).as_posix(),
        "sha256": sha256_path(path),
        "bytes": path.stat().st_size,
    }


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


def default_platform():
    system = platform.system().lower()
    machine = platform.machine().lower()
    if system == "darwin":
        system = "macos"
    if machine in ("aarch64", "arm64"):
        machine = "arm64"
    elif machine in ("amd64", "x86_64"):
        machine = "x86_64"
    return f"{system}-{machine}"


def git_output(*args):
    try:
        return subprocess.check_output(["git", *args], cwd=ROOT, text=True).strip()
    except subprocess.CalledProcessError:
        return "unknown"


def run_checked(args):
    try:
        subprocess.run(args, cwd=ROOT, check=True)
    except FileNotFoundError:
        fail(f"missing required command: {args[0]}")
    except subprocess.CalledProcessError as exc:
        raise SystemExit(exc.returncode)


def load_json(path):
    with open(path, "r", encoding="utf-8") as fh:
        return json.load(fh)


def write_json(path, data):
    path.write_text(json.dumps(data, indent=2, sort_keys=True) + "\n", encoding="utf-8")


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
        if not re.fullmatch(r"[0-9a-fA-F]{64}", digest):
            fail(f"invalid SHA-256 digest in {path}: {line_no}")
        entries.append((digest.lower(), rel))
    return entries


def verify_sha256sums(path, base):
    for expected, rel in parse_sha256sums(path):
        target = base / rel
        if not target.is_file():
            fail(f"checksum target missing: {rel}")
        actual = sha256_path(target)
        if actual != expected:
            fail(f"checksum mismatch for {rel}: expected {expected}, got {actual}")


def release_files(release_dir, include_signatures=True):
    files = []
    for path in release_dir.rglob("*"):
        if not path.is_file():
            continue
        rel = path.relative_to(release_dir).as_posix()
        if rel == "TRUST_SHA256SUMS":
            continue
        if rel.endswith(".tmp"):
            continue
        if not include_signatures and rel.endswith(".sig"):
            continue
        files.append(path)
    return sorted(files, key=lambda p: p.relative_to(release_dir).as_posix())


def write_trust_sums(release_dir):
    sums = release_dir / "TRUST_SHA256SUMS"
    lines = []
    for path in release_files(release_dir):
        rel = path.relative_to(release_dir).as_posix()
        lines.append(f"{sha256_path(path)}  {rel}")
    sums.write_text("\n".join(lines) + "\n", encoding="utf-8")
    return sums


def sanitize_spdx_id(text):
    clean = re.sub(r"[^A-Za-z0-9.-]", "-", text)
    return clean.strip("-") or "package"


def parse_cargo_lock():
    lock = ROOT / "Cargo.lock"
    if not lock.is_file():
        return []
    packages = []
    current = None
    for raw in lock.read_text(encoding="utf-8").splitlines():
        line = raw.strip()
        if line == "[[package]]":
            if current:
                packages.append(current)
            current = {}
            continue
        if current is None:
            continue
        for key in ("name", "version", "source", "checksum"):
            prefix = f"{key} = "
            if line.startswith(prefix):
                current[key] = line[len(prefix):].strip().strip('"')
                break
    if current:
        packages.append(current)
    return packages


def generate_sbom(version, release_dir, generated_at):
    package_name = read_cargo_value("name")
    packages = []
    for pkg in parse_cargo_lock():
        name = pkg.get("name", "unknown")
        pkg_version = pkg.get("version", "unknown")
        source = pkg.get("source", "NOASSERTION")
        record = {
            "SPDXID": f"SPDXRef-Package-{sanitize_spdx_id(name)}-{sanitize_spdx_id(pkg_version)}",
            "name": name,
            "versionInfo": pkg_version,
            "downloadLocation": source if source else "NOASSERTION",
            "filesAnalyzed": False,
        }
        if pkg.get("checksum"):
            record["checksums"] = [{"algorithm": "SHA256", "checksumValue": pkg["checksum"]}]
        if source.startswith("registry+"):
            record["externalRefs"] = [{
                "referenceCategory": "PACKAGE-MANAGER",
                "referenceType": "purl",
                "referenceLocator": f"pkg:cargo/{name}@{pkg_version}",
            }]
        packages.append(record)

    root_spdx = f"SPDXRef-Package-{sanitize_spdx_id(package_name)}-{sanitize_spdx_id(version)}"
    root_package = {
        "SPDXID": root_spdx,
        "name": package_name,
        "versionInfo": version,
        "downloadLocation": "NOASSERTION",
        "filesAnalyzed": False,
    }
    data = {
        "spdxVersion": "SPDX-2.3",
        "dataLicense": "CC0-1.0",
        "SPDXID": "SPDXRef-DOCUMENT",
        "name": f"{package_name}-{version}-sbom",
        "documentNamespace": f"https://github.com/codenz92/cool-lang/releases/{version}/sbom",
        "creationInfo": {
            "created": generated_at,
            "creators": ["Tool: cool-release-trust/1.0"],
        },
        "documentDescribes": [root_spdx],
        "packages": [root_package, *packages],
        "relationships": [
            {
                "spdxElementId": "SPDXRef-DOCUMENT",
                "relationshipType": "DESCRIBES",
                "relatedSpdxElement": root_spdx,
            }
        ],
    }
    output = release_dir / "sbom.spdx.json"
    write_json(output, data)
    return output


def release_subjects(release_dir):
    excluded = {
        "provenance.intoto.json",
        "trust.json",
        "TRUST_SHA256SUMS",
    }
    subjects = []
    for path in release_files(release_dir, include_signatures=False):
        rel = path.relative_to(release_dir).as_posix()
        if rel in excluded:
            continue
        subjects.append({"name": rel, "digest": {"sha256": sha256_path(path)}})
    return subjects


def generate_provenance(version, platform_name, release_dir, release_json, generated_at):
    release = release_json.get("release", {})
    repo = os.environ.get("GITHUB_REPOSITORY", "codenz92/cool-lang")
    server = os.environ.get("GITHUB_SERVER_URL", "https://github.com")
    workflow = os.environ.get("GITHUB_WORKFLOW", "local")
    workflow_ref = os.environ.get("GITHUB_WORKFLOW_REF")
    run_id = os.environ.get("GITHUB_RUN_ID", "local")
    run_attempt = os.environ.get("GITHUB_RUN_ATTEMPT", "1")
    source_uri = git_output("remote", "get-url", "origin")
    commit = git_output("rev-parse", "HEAD")
    cargo_lock = ROOT / "Cargo.lock"
    build_type = f"{server}/{workflow_ref}" if workflow_ref else f"{server}/{repo}/actions/workflows/{workflow}"
    statement = {
        "_type": "https://in-toto.io/Statement/v1",
        "subject": release_subjects(release_dir),
        "predicateType": "https://slsa.dev/provenance/v1",
        "predicate": {
            "buildDefinition": {
                "buildType": build_type,
                "externalParameters": {
                    "version": version,
                    "platform": platform_name,
                    "tag": release.get("tag", f"v{version}"),
                },
                "internalParameters": {
                    "release_gate": release.get("release_gate", "unknown"),
                    "promotion_tool": "scripts/promote_release.sh",
                    "trust_tool": "scripts/trust_release.py",
                },
                "resolvedDependencies": [
                    {
                        "uri": source_uri,
                        "digest": {"gitCommit": commit},
                    },
                    {
                        "uri": "Cargo.lock",
                        "digest": {"sha256": sha256_path(cargo_lock)} if cargo_lock.is_file() else {},
                    },
                ],
            },
            "runDetails": {
                "builder": {"id": f"{server}/{repo}/actions/workflows/{workflow}"},
                "metadata": {
                    "invocationId": f"{run_id}/{run_attempt}",
                    "startedOn": generated_at,
                    "finishedOn": generated_at,
                },
            },
        },
    }
    output = release_dir / "provenance.intoto.json"
    write_json(output, statement)
    return output


def public_key_fingerprint(key_path, public_key=False):
    if not key_path:
        return None
    cmd = ["openssl", "pkey"]
    if public_key:
        cmd.append("-pubin")
    cmd += ["-in", str(key_path), "-pubout", "-outform", "DER"]
    try:
        data = subprocess.check_output(cmd)
    except (FileNotFoundError, subprocess.CalledProcessError):
        return None
    return hashlib.sha256(data).hexdigest()


def sign_file(path, key_path):
    signature = Path(str(path) + ".sig")
    run_checked([
        "openssl",
        "dgst",
        "-sha256",
        "-sign",
        str(key_path),
        "-out",
        str(signature),
        str(path),
    ])
    return signature


def verify_signature(path, signature, key_path):
    run_checked([
        "openssl",
        "dgst",
        "-sha256",
        "-verify",
        str(key_path),
        "-signature",
        str(signature),
        str(path),
    ])


def update_latest_json(release_dir, dist_dir, release_json_path):
    for latest in (release_dir / "latest.json", dist_dir / "releases" / "latest.json"):
        if not latest.is_file():
            continue
        data = load_json(latest)
        data["release_json"] = {
            "path": display_path(release_json_path),
            "sha256": sha256_path(release_json_path),
            "bytes": release_json_path.stat().st_size,
        }
        write_json(latest, data)


def write_trust_json(release_dir, version, platform_name, tag, generated_at, sign_key, signatures):
    files = [file_record(release_dir, path) for path in release_files(release_dir)]
    data = {
        "schema_version": 1,
        "package": {
            "name": read_cargo_value("name"),
            "version": version,
        },
        "release": {
            "tag": tag,
            "platform": platform_name,
            "generated_at": generated_at,
        },
        "git": {
            "commit": git_output("rev-parse", "HEAD"),
            "short_commit": git_output("rev-parse", "--short", "HEAD"),
            "branch": git_output("branch", "--show-current") or "detached",
            "dirty": bool(git_output("status", "--porcelain")),
        },
        "trust": {
            "hash_algorithm": "sha256",
            "signature_format": "openssl-dgst-sha256-detached",
            "signing_key_fingerprint": public_key_fingerprint(sign_key) if sign_key else None,
        },
        "files": files,
        "signatures": signatures,
    }
    output = release_dir / "trust.json"
    write_json(output, data)
    return output


def generate(args):
    version = args.version or read_cargo_value("version")
    platform_name = args.platform or default_platform()
    dist_dir = Path(args.dist_dir).resolve()
    release_dir = dist_dir / "releases" / version
    release_json_path = release_dir / "release.json"
    sha256sums_path = release_dir / "SHA256SUMS"

    if not release_dir.is_dir():
        fail(f"release directory not found: {release_dir}")
    if not release_json_path.is_file():
        fail(f"release.json not found: {release_json_path}")
    if not sha256sums_path.is_file():
        fail(f"SHA256SUMS not found: {sha256sums_path}")

    verify_sha256sums(sha256sums_path, release_dir)
    release_json = load_json(release_json_path)
    release = release_json.get("release", {})
    tag = release.get("tag", f"v{version}")
    generated_at = datetime.now(timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ")

    sbom_path = generate_sbom(version, release_dir, generated_at)
    provenance_path = generate_provenance(version, platform_name, release_dir, release_json, generated_at)

    release_json["supply_chain"] = {
        "sbom": file_record(release_dir, sbom_path),
        "provenance": file_record(release_dir, provenance_path),
        "trust_policy": {
            "hash_algorithm": "sha256",
            "required_signed_files": ["SHA256SUMS", "release.json"],
        },
    }
    write_json(release_json_path, release_json)
    update_latest_json(release_dir, dist_dir, release_json_path)

    signatures = []
    sign_key = Path(args.sign_key).resolve() if args.sign_key else None
    if sign_key:
        if not sign_key.is_file():
            fail(f"signing key not found: {sign_key}")
        for target in (sha256sums_path, release_json_path, provenance_path, sbom_path):
            sig = sign_file(target, sign_key)
            signatures.append({
                "target": target.relative_to(release_dir).as_posix(),
                "path": sig.relative_to(release_dir).as_posix(),
                "sha256": sha256_path(sig),
            })

    trust_json_path = write_trust_json(
        release_dir,
        version,
        platform_name,
        tag,
        generated_at,
        sign_key,
        signatures,
    )
    if sign_key:
        sign_file(trust_json_path, sign_key)

    write_trust_sums(release_dir)

    print("trust release: ok")
    print(f"  SBOM       -> {display_path(sbom_path)}")
    print(f"  Provenance -> {display_path(provenance_path)}")
    print(f"  Trust      -> {display_path(trust_json_path)}")
    print(f"  Sums       -> {display_path(release_dir / 'TRUST_SHA256SUMS')}")


def verify(args):
    version = args.version or read_cargo_value("version")
    platform_name = args.platform or default_platform()
    dist_dir = Path(args.dist_dir).resolve()
    release_dir = dist_dir / "releases" / version

    required = [
        release_dir / "release.json",
        release_dir / "SHA256SUMS",
        release_dir / "sbom.spdx.json",
        release_dir / "provenance.intoto.json",
        release_dir / "trust.json",
        release_dir / "TRUST_SHA256SUMS",
    ]
    for path in required:
        if not path.is_file():
            fail(f"required trust artifact missing: {path}")

    for path in (release_dir / "release.json", release_dir / "sbom.spdx.json", release_dir / "provenance.intoto.json", release_dir / "trust.json"):
        load_json(path)

    release_json = load_json(release_dir / "release.json")
    release = release_json.get("release", {})
    if release.get("platform") != platform_name:
        fail(f"release platform mismatch: expected {platform_name}, got {release.get('platform')}")

    verify_sha256sums(release_dir / "SHA256SUMS", release_dir)
    verify_sha256sums(release_dir / "TRUST_SHA256SUMS", release_dir)

    supply_chain = release_json.get("supply_chain", {})
    for key in ("sbom", "provenance"):
        record = supply_chain.get(key)
        if not record:
            fail(f"release.json missing supply_chain.{key}")
        target = release_dir / record["path"]
        if not target.is_file():
            fail(f"supply_chain.{key} target missing: {record['path']}")
        if sha256_path(target) != record["sha256"]:
            fail(f"supply_chain.{key} hash mismatch")

    verify_key = Path(args.verify_key).resolve() if args.verify_key else None
    if verify_key:
        if not verify_key.is_file():
            fail(f"verification key not found: {verify_key}")
        required_sigs = ["SHA256SUMS.sig", "release.json.sig"]
        for rel in required_sigs:
            if not (release_dir / rel).is_file():
                fail(f"required signature missing: {rel}")
        for sig in sorted(release_dir.glob("*.sig")):
            target = release_dir / sig.name[:-4]
            if not target.is_file():
                fail(f"signature target missing: {target.name}")
            verify_signature(target, sig, verify_key)

    print("trust release: verified")
    print(f"  Version  -> {version}")
    print(f"  Platform -> {platform_name}")
    print(f"  Release  -> {display_path(release_dir)}")


def main():
    parser = argparse.ArgumentParser(description="Generate or verify Cool release trust metadata.")
    sub = parser.add_subparsers(dest="command", required=True)

    def add_common(p):
        p.add_argument("--version")
        p.add_argument("--platform")
        p.add_argument("--dist-dir", default=str(ROOT / "dist"))

    gen = sub.add_parser("generate")
    add_common(gen)
    gen.add_argument("--sign-key", help="OpenSSL private key used for detached signatures.")

    ver = sub.add_parser("verify")
    add_common(ver)
    ver.add_argument("--verify-key", help="OpenSSL public key used to verify detached signatures.")

    args = parser.parse_args()
    if args.command == "generate":
        generate(args)
    elif args.command == "verify":
        verify(args)
    else:
        parser.error("unknown command")


if __name__ == "__main__":
    main()
