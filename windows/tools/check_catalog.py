"""Validate all shipped locales and literal UI/error keys without a GUI."""
import json
from pathlib import Path
import re
root = Path(__file__).resolve().parents[1]
catalog = json.loads((root/'locales/catalog.json').read_text(encoding='utf-8'))
keys = set()
for file in (root/'src').glob('*.rs'):
    source = file.read_text(encoding='utf-8')
    for expression in re.findall(r'\b(?:t|text)\.get\(([^)]*)\)',source):
        keys.update(re.findall(r'"([a-z_]+)"',expression))
    keys.update(re.findall(r'AppError::(?:new|detail)\(\s*"([a-z_]+)"',source))
missing = keys-catalog.keys()
assert not missing, f'Missing keys: {sorted(missing)}'
for key, row in catalog.items():
    assert len(row) == 6 and all(isinstance(s,str) and s.strip() for s in row), key
print(f'{len(catalog)} complete six-language entries; {len(keys)} literal UI/error keys checked.')
