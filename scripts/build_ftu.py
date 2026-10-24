#!/usr/bin/env python3
"""Build and stage the independent plugin; never install or enable it automatically."""
import argparse
import hashlib
import json
from pathlib import Path
import shutil
import subprocess
import sys

root = Path(__file__).resolve().parent.parent
parser = argparse.ArgumentParser(description=__doc__)
parser.add_argument('--out', default='target/ftu-package')
parser.add_argument('--target-dir', default='target/engineering-ftu')
parser.add_argument('--release', action='store_true')
args = parser.parse_args()
out = (root / args.out).resolve()
if out.exists():
    parser.error('output must be a new directory; existing packages are never overwritten')
target = (root / args.target_dir).resolve()
cmd = ['cargo', 'build', '--manifest-path', str(root / 'external/ftu/Cargo.toml'), '--locked', '--target-dir', str(target)]
if args.release:
    cmd.append('--release')
subprocess.run(cmd, cwd=root, check=True)
suffix = '.exe' if sys.platform == 'win32' else ''
binary = target / ('release' if args.release else 'debug') / ('ftu-plugin' + suffix)
out.mkdir(parents=True)
manifest = json.loads((root / 'external/ftu/plugin.json').read_text(encoding='utf-8'))
manifest['command'] = ['./ftu-plugin' + suffix]
(out / 'plugin.json').write_text(json.dumps(manifest, ensure_ascii=False, indent=2) + '\n', encoding='utf-8')
for name in ['nctool.package.json', 'README.md']:
    source = root / 'external/ftu' / name
    if source.is_file():
        shutil.copy2(source, out / name)
shutil.copy2(binary, out / binary.name)
evidence = {'plugin': 'ftu', 'version': manifest['descriptor']['version'], 'platform': sys.platform, 'sha256': hashlib.sha256(binary.read_bytes()).hexdigest()}
(out / 'build.json').write_text(json.dumps(evidence, indent=2) + '\n', encoding='utf-8')
print(out)
