#!/usr/bin/env python3
"""Checks the 2.0 architecture boundaries and generated WebUI without modifying sources."""
import json
from pathlib import Path
import re
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parent.parent


def run(*command):
    result = subprocess.run(command, cwd=ROOT, check=True, capture_output=True, text=True)
    return result.stdout


def main():
    metadata=json.loads(run('cargo','metadata','--no-deps','--format-version','1','--locked'))
    packages={package['name']:package for package in metadata['packages']}
    assert 'nctool-gui' not in packages and 'nctool-core' not in packages
    tree = run('cargo', 'tree', '-p', 'nctool-cli', '--no-default-features', '--edges', 'normal', '--locked')
    assert not re.search(r'nctool-plugin-(nc|process|math)\b', tree), tree
    assert 'tauri' not in tree
    assert {d['name'] for d in packages['nctool-tpl']['dependencies'] if d['kind'] is None} == {'minijinja'}
    renderer = (ROOT / 'src/renderer.rs').read_text()
    assert 'with_v3_whitespace' not in renderer
    assert not re.search(r'add_filter\("(?:nc_|sin|cos|tan|sqrt)', renderer)
    assert not (ROOT / 'src/lint.rs').exists()
    assert 'nctool-plugin-nc' not in {d['name'] for d in packages['nctool-plugin-process']['dependencies']}
    assert (ROOT / 'sdk/python/nctool_plugin.py').read_bytes() == (ROOT / 'examples/plugins/python-report/nctool_plugin.py').read_bytes()
    run('node', 'scripts/build_ui.mjs', '--check')
    html = (ROOT / 'cli/ui/index.html').read_text()
    assert '/assets/' in html and 'type="module"' in html
    assert (ROOT/'ui/src/App.tsx').exists()
    source=(ROOT/'ui/src/lib/api.ts').read_text()
    assert '/api/v2/' in source and 'encode(body)' in source
    assert (ROOT/'cli/ui/assets.rs').exists()
    package = run('cargo', 'package', '-p', 'nctool-tpl', '--list', '--allow-dirty', '--offline')
    assert not any(name.startswith(('cli/', 'core/', 'plugins/', 'gui/', 'templates/', 'docs/', 'ui/', 'runtime/')) for name in package.splitlines())
    manifest = json.loads((ROOT / 'examples/plugins/python-report/plugin.json').read_text(encoding='utf-8'))
    assert manifest['descriptor']['requires'] == [{'id': 'template.render', 'version': 1}]
    print('PASS: pure template dependency tree, scoped engine, GUI exclusion, process service boundary, Python example, generated WebUI and package contents')


if __name__ == '__main__':
    main()
