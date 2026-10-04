#!/usr/bin/env python3
"""Archive the complete non-ignored Git working tree without changing the index."""
import argparse
import hashlib
import io
import json
from pathlib import Path
import subprocess
import tarfile
from datetime import datetime, timezone

ROOT = Path(__file__).resolve().parent.parent


def git(root, *args):
    return subprocess.run(
        ["git", "-C", str(root), *args], check=True, capture_output=True
    ).stdout


def digest(data):
    return hashlib.sha256(data).hexdigest()


def capture(root, destination):
    destination.mkdir(parents=True, exist_ok=False)
    evidence = git_evidence(root)
    head = git(root, "rev-parse", "HEAD").decode().strip()
    paths = sorted(set(git(root, "ls-files", "-z", "--cached", "--others",
                           "--exclude-standard").decode().split("\0")) - {""})
    entries = []
    archive_path = destination / "sources.tar.gz"
    with tarfile.open(archive_path, "w:gz") as archive:
        for name in paths:
            path = root / name
            if path.is_symlink():
                raise ValueError(f"Symlink requires separate review: {name}")
            if not path.exists():
                continue  # Deletions are preserved in the Git patches and status.
            if not path.is_file():
                raise ValueError(f"Unsupported working-tree entry: {name}")
            data = path.read_bytes()
            info = archive.gettarinfo(str(path), arcname=name)
            info.size = len(data)
            archive.addfile(info, io.BytesIO(data))
            entries.append({"path": name, "bytes": len(data), "sha256": digest(data)})
    if evidence != git_evidence(root) or head != git(root, "rev-parse", "HEAD").decode().strip():
        raise ValueError("Git state changed during capture; retry into a new directory")
    for entry in entries:
        path = root / entry["path"]
        if path.is_symlink() or not path.is_file() or digest(path.read_bytes()) != entry["sha256"]:
            raise ValueError(f"Source changed during capture: {entry['path']}")
    for name, data in evidence.items():
        (destination / name).write_bytes(data)
    manifest = {
        "format": "nctool-source-baseline", "version": 1,
        "created_at": datetime.now(timezone.utc).isoformat(),
        "head": head,
        "scope": "Existing tracked and non-ignored untracked files; ignored assets excluded",
        "archive_sha256": digest(archive_path.read_bytes()),
        "evidence": {name: digest(data) for name, data in evidence.items()},
        "files": entries,
    }
    (destination / "manifest.json").write_text(
        json.dumps(manifest, ensure_ascii=False, indent=2) + "\n", encoding="utf-8"
    )
    verify(destination)
    return manifest


def git_evidence(root):
    return {
        "status.txt": git(root, "status", "--porcelain=v1", "--untracked-files=all"),
        "staged.patch": git(root, "diff", "--cached", "--binary", "--full-index"),
        "unstaged.patch": git(root, "diff", "--binary", "--full-index"),
    }


def verify(destination):
    manifest = json.loads((destination / "manifest.json").read_text(encoding="utf-8"))
    if manifest.get("format") != "nctool-source-baseline" or manifest.get("version") != 1:
        raise ValueError("Unsupported baseline manifest")
    archive_path = destination / "sources.tar.gz"
    if digest(archive_path.read_bytes()) != manifest["archive_sha256"]:
        raise ValueError("Source archive checksum mismatch")
    for name, expected in manifest["evidence"].items():
        if digest((destination / name).read_bytes()) != expected:
            raise ValueError(f"Evidence checksum mismatch: {name}")
    expected = {entry["path"]: entry for entry in manifest["files"]}
    with tarfile.open(archive_path, "r:gz") as archive:
        seen = set()
        for member in archive:
            if not member.isfile() or member.name not in expected or member.name in seen:
                raise ValueError(f"Unexpected archive entry: {member.name}")
            stream = archive.extractfile(member)
            data = stream.read()
            entry = expected[member.name]
            if len(data) != entry["bytes"] or digest(data) != entry["sha256"]:
                raise ValueError(f"Source checksum mismatch: {member.name}")
            seen.add(member.name)
        if seen != set(expected):
            raise ValueError("Source archive is incomplete")
    return manifest


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("operation", choices=["capture", "verify"])
    parser.add_argument("directory", type=Path)
    args = parser.parse_args()
    destination = args.directory.resolve()
    if args.operation == "capture":
        # Prevent the archive from becoming part of its own source collection.
        if destination.is_relative_to(ROOT):
            relative = destination.relative_to(ROOT)
            if subprocess.run(["git", "-C", str(ROOT), "check-ignore", "-q", str(relative)]).returncode:
                parser.error("An in-repository destination must be Git-ignored")
        manifest = capture(ROOT, destination)
    else:
        manifest = verify(destination)
    print(f"PASS: {args.operation}, {len(manifest['files'])} files, HEAD {manifest['head']}")
    print(destination / "manifest.json")


if __name__ == "__main__":
    main()
