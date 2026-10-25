"""Writes THIRD-PARTY-LICENSES.md: every Rust crate that goes into Blink's two programs, its license, and the license texts.

    python scripts/third-party-licenses.py

The crates are the ones `cargo tree` lists for blink-native and blink-settings on Windows (what is linked in, not the tools used to
build). The texts are read from the crates' own folders in the Cargo registry, so run `cargo fetch` first on a fresh checkout.
Crates that have the same license text are listed together under it.
"""
import hashlib
import json
import os
import re
import subprocess
import sys

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
TARGET = 'x86_64-pc-windows-msvc'
OWN = {'blink-core', 'blink-native', 'blink-settings'}


def run(*args):
    result = subprocess.run(args, cwd=ROOT, capture_output=True, text=True, encoding='utf-8')
    if result.returncode != 0:
        sys.exit(result.stderr)
    return result.stdout


def crates():
    out = run('cargo', 'tree', '-p', 'blink-native', '-p', 'blink-settings', '--target', TARGET, '-e', 'normal', '--prefix', 'none', '--no-dedupe',
              '--format', '{p}|{l}|{r}')
    seen = {}
    for line in out.splitlines():
        parts = line.strip().split('|')
        if len(parts) < 3:
            continue
        head = parts[0].replace(' (*)', '')
        match = re.match(r'^(\S+) v(\S+)', head)
        if not match or match.group(1) in OWN:
            continue
        seen[(match.group(1), match.group(2))] = (parts[1] or 'unknown', parts[2])
    return seen


def manifests():
    meta = json.loads(run('cargo', 'metadata', '--format-version', '1', '--filter-platform', TARGET))
    return {(p['name'], p['version']): os.path.dirname(p['manifest_path']) for p in meta['packages']}


def license_files(folder):
    found = []
    for name in sorted(os.listdir(folder)):
        upper = name.upper()
        if upper.startswith(('LICENSE', 'LICENCE', 'COPYING', 'UNLICENSE', 'NOTICE')) and os.path.isfile(os.path.join(folder, name)):
            with open(os.path.join(folder, name), encoding='utf-8', errors='replace') as f:
                found.append((name, f.read().strip()))
    return found


def main():
    listed = crates()
    where = manifests()
    groups = {}
    missing = []
    for (name, version), (license_id, repo) in sorted(listed.items()):
        folder = where.get((name, version))
        files = license_files(folder) if folder else []
        if not files:
            missing.append(f'{name} {version} ({license_id})')
            continue
        for fname, text in files:
            key = hashlib.sha256(re.sub(r'\s+', ' ', text).encode()).hexdigest()
            groups.setdefault(key, {'text': text, 'crates': []})['crates'].append(f'{name} {version}')
    lines = [
        '# Third-party licenses',
        '',
        'Blink is MIT licensed (see `LICENSE`). It is built from the Rust crates below, each under the license shown. This file is made by',
        '`scripts/third-party-licenses.py`; the list is what is linked into `blink.exe` and `blink-settings.exe`.',
        '',
        f'{len(listed)} crates:',
        '',
        '| Crate | Version | License |',
        '|---|---|---|',
    ]
    for (name, version), (license_id, _repo) in sorted(listed.items()):
        lines.append(f'| {name} | {version} | {license_id.replace("|", "/")} |')
    lines += ['', '## License texts', '']
    for i, group in enumerate(sorted(groups.values(), key=lambda g: -len(g['crates'])), 1):
        names = group['crates']
        shown = ', '.join(names[:40]) + (f' and {len(names) - 40} more' if len(names) > 40 else '')
        lines += [f'### Text {i}', '', f'Used by: {shown}', '', '```text', group['text'], '```', '']
    if missing:
        lines += ['## Without a license file in the crate', '', 'Their license is as declared in the table above:', ''] + [f'- {m}' for m in missing] + ['']
    with open(os.path.join(ROOT, 'THIRD-PARTY-LICENSES.md'), 'w', encoding='utf-8', newline='\n') as f:
        f.write('\n'.join(lines))
    print(f'{len(listed)} crates, {len(groups)} distinct license texts, {len(missing)} without a file')
    if missing:
        print('without a license file:', ', '.join(missing))


if __name__ == '__main__':
    main()
