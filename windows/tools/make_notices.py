"""Collect locked Windows dependency licenses; no network or sibling sources."""
import argparse
import hashlib
import json
from pathlib import Path
import subprocess
import re

ROOT = Path(__file__).resolve().parents[1]
parser = argparse.ArgumentParser()
parser.add_argument('--output', type=Path, default=ROOT/'THIRD_PARTY_NOTICES.txt')
parser.add_argument('--online', action='store_true', help='Allow Cargo to obtain missing published crate metadata on a fresh build machine')
args = parser.parse_args()
metadata = json.loads(subprocess.check_output([
    'cargo', 'metadata', *([] if args.online else ['--offline']), '--locked', '--format-version', '1',
    '--filter-platform', 'x86_64-pc-windows-msvc',
], cwd=ROOT))
nodes = {node['id']: node for node in metadata['resolve']['nodes']}
reachable = set()
pending = [metadata['resolve']['root']]
while pending:
    key = pending.pop()
    if key in reachable:
        continue
    reachable.add(key)
    pending.extend(dep['pkg'] for dep in nodes[key]['deps'])
packages = sorted((p for p in metadata['packages'] if p['id'] in reachable and p['source']), key=lambda p:(p['name'],p['version']))
lines = ['WifiMap third-party notices', '', 'Generated from Cargo.lock for Windows x64. Dependency licenses remain their own.', '']
texts = {}
missing = []
origins = json.loads((ROOT/'third_party/origins.json').read_text(encoding='utf-8'))
for package in packages:
    label = f"{package['name']} {package['version']}"
    lines.append(f"{label}: {package.get('license') or 'see supplied license file'}")
    directory = Path(package['manifest_path']).parent
    files = []
    for file in directory.rglob('*'):
        if not file.is_file() or len(file.relative_to(directory).parts) > 3:
            continue
        name = file.name.lower()
        if (name.startswith(('license','copying','notice')) or 'license' in name
                or name in ('ufl.txt','ofl.txt','hack-regular.txt')) and file.suffix.lower() not in ('.rs','.toml','.json'):
            files.append(file)
    if package.get('license_file'):
        files.append(directory/package['license_file'])
    if not files:
        origin = next((o for o in origins if o['packages'].get(package['name']) == package['version']), None)
        if origin:
            vcs = json.loads((directory/'.cargo_vcs_info.json').read_text(encoding='utf-8'))
            assert '/'+vcs['git']['sha1']+'/' in origin['url'], f'License origin commit mismatch: {label}'
            file = ROOT/'third_party'/origin['file']
            files.append(file)
            lines.append('  Upstream license restored from: '+origin['url'])
            copyrights = set()
            for source in directory.rglob('*.rs'):
                for line in source.read_text(encoding='utf-8',errors='replace').splitlines()[:20]:
                    if re.match(r'\s*//\s*Copyright',line):
                        copyrights.add(line.strip().lstrip('/').strip())
            lines.extend('  '+notice for notice in sorted(copyrights))
        else:
            missing.append(label)
    for file in sorted(set(files)):
        content = file.read_text(encoding='utf-8', errors='replace').strip()
        if not content:
            continue
        digest = hashlib.sha256(content.encode()).hexdigest()
        if digest not in texts:
            texts[digest] = ([], content)
        name = file.relative_to(directory).as_posix() if file.is_relative_to(directory) else 'upstream/'+file.name
        texts[digest][0].append(f'{label}/{name}')
if missing:
    raise SystemExit('Missing license text; review before packaging: '+', '.join(missing))
for references, content in texts.values():
    lines.extend(['', '='*72, 'Used by: '+', '.join(references), '='*72, '', content])
args.output.parent.mkdir(parents=True, exist_ok=True)
args.output.write_text('\n'.join(lines)+'\n', encoding='utf-8')
print(f'Collected {len(packages)} dependency notices and {len(texts)} distinct license texts: {args.output.name}')
